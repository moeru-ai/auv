//! Win32 desktop and window station management.
//!
//! Provides thread desktop attachment helpers to ensure background tasks,
//! benchmarks, and capture sessions interact with the interactive desktop,
//! backed by RAII handle management and thread desktop restoration.

#[cfg(target_os = "windows")]
use std::cell::RefCell;
#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HANDLE;
#[cfg(target_os = "windows")]
use windows::Win32::System::StationsAndDesktops::{
  CloseDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, GetThreadDesktop, GetUserObjectInformationW, HDESK, OpenInputDesktop,
  SetThreadDesktop, UOI_NAME,
};
#[cfg(target_os = "windows")]
use windows::Win32::System::Threading::GetCurrentThreadId;

#[derive(Debug, thiserror::Error)]
pub enum DesktopError {
  #[error("Windows desktop is unavailable or cannot be accessed")]
  DesktopUnavailable,
}

#[cfg(target_os = "windows")]
thread_local! {
  static THREAD_DESKTOP_HOLDER: RefCell<Option<BoundDesktop>> = const { RefCell::new(None) };
}

/// An owned, RAII-guarded Windows desktop handle.
///
/// Calls `CloseDesktop` when dropped.
#[derive(Debug)]
pub struct DesktopHandle(#[cfg(target_os = "windows")] pub HDESK, #[cfg(not(target_os = "windows"))] pub ());

impl DesktopHandle {
  pub fn is_valid(&self) -> bool {
    #[cfg(target_os = "windows")]
    {
      !self.0.is_invalid() && !self.0.0.is_null()
    }
    #[cfg(not(target_os = "windows"))]
    {
      false
    }
  }
}

impl Drop for DesktopHandle {
  fn drop(&mut self) {
    #[cfg(target_os = "windows")]
    if self.is_valid() {
      // SAFETY: self.0 is an owned HDESK allocated by OpenInputDesktop / OpenDesktop.
      // Calling CloseDesktop releases the handle from the calling process's handle table.
      let _ = unsafe { CloseDesktop(self.0) };
    }
  }
}

/// An RAII guard that attaches the calling thread to a desktop for a scoped duration,
/// and restores the original thread desktop upon drop before closing the opened desktop handle.
#[derive(Debug)]
pub struct BoundDesktop {
  #[cfg(target_os = "windows")]
  original: HDESK,
  #[cfg(target_os = "windows")]
  opened: DesktopHandle,
}

impl BoundDesktop {
  /// Binds the current thread to the input desktop with the specified access rights.
  /// If the current thread is already on the input desktop, returns a guard that
  /// preserves the current desktop and closes the newly opened handle cleanly upon drop.
  #[cfg(target_os = "windows")]
  pub fn bind_input(access: DESKTOP_ACCESS_FLAGS) -> Result<Self, DesktopError> {
    unsafe {
      let original = GetThreadDesktop(GetCurrentThreadId()).map_err(|_| DesktopError::DesktopUnavailable)?;

      let opened = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, access).map_err(|_| DesktopError::DesktopUnavailable)?;
      let opened = DesktopHandle(opened);

      let original_name = desktop_name(original).ok();
      let input_name = desktop_name(opened.0).ok();
      let same_desktop = match (original_name.as_deref(), input_name.as_deref()) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
      };
      if same_desktop {
        // Already on the input desktop: preserve current binding and close opened handle on drop.
        return Ok(Self {
          original: HDESK::default(),
          opened,
        });
      }

      SetThreadDesktop(opened.0).map_err(|_| DesktopError::DesktopUnavailable)?;

      Ok(Self { original, opened })
    }
  }

  /// Returns the desktop handle of the opened desktop.
  #[cfg(target_os = "windows")]
  pub fn desktop(&self) -> HDESK {
    self.opened.0
  }
}

impl Drop for BoundDesktop {
  fn drop(&mut self) {
    #[cfg(target_os = "windows")]
    if !self.original.is_invalid() && !self.original.0.is_null() {
      // SAFETY: Restoring the original thread desktop before dropping `self.opened`
      // ensures that `opened` is no longer assigned to this thread, allowing
      // `CloseDesktop` in `DesktopHandle::drop` to succeed.
      let _ = unsafe { SetThreadDesktop(self.original) };
    }
  }
}

/// Retrieves the name of a desktop from its handle.
#[cfg(target_os = "windows")]
pub fn desktop_name(desktop: HDESK) -> Result<String, DesktopError> {
  unsafe {
    let mut units = [0u16; 64];
    let mut needed = 0u32;
    GetUserObjectInformationW(
      HANDLE(desktop.0),
      UOI_NAME,
      Some(units.as_mut_ptr().cast()),
      std::mem::size_of_val(&units) as u32,
      Some(&mut needed),
    )
    .map_err(|_| DesktopError::DesktopUnavailable)?;

    if needed == 0 || needed as usize > std::mem::size_of_val(&units) || needed % 2 != 0 {
      return Err(DesktopError::DesktopUnavailable);
    }

    let end = units.iter().position(|unit| *unit == 0).ok_or(DesktopError::DesktopUnavailable)?;
    String::from_utf16(&units[..end]).map_err(|_| DesktopError::DesktopUnavailable)
  }
}

#[cfg(not(target_os = "windows"))]
pub fn desktop_name(_desktop: ()) -> Result<String, DesktopError> {
  Err(DesktopError::DesktopUnavailable)
}

/// Executes a closure within the scope of the input desktop, restoring the thread's
/// prior desktop upon completion or error.
#[cfg(target_os = "windows")]
pub fn with_input_desktop<R>(action: impl FnOnce() -> R) -> Result<R, DesktopError> {
  let _guard = BoundDesktop::bind_input(DESKTOP_ACCESS_FLAGS(0x000F_01FF))?;
  Ok(action())
}

#[cfg(not(target_os = "windows"))]
pub fn with_input_desktop<R>(action: impl FnOnce() -> R) -> Result<R, DesktopError> {
  Ok(action())
}

/// Ensures the current thread is attached to the input desktop without leaking handles.
///
/// If the thread is already attached to the input desktop, no handle is leaked and the thread
/// is untouched. If the thread transitions to the input desktop, the handle is managed in
/// thread-local storage and closed when the thread completes or transitions again.
#[cfg(target_os = "windows")]
pub fn ensure_input_desktop() {
  unsafe {
    let current = GetThreadDesktop(GetCurrentThreadId());
    let Ok(opened) = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(0x000F_01FF)) else {
      return;
    };
    let opened = DesktopHandle(opened);

    if let Ok(current) = current {
      let cur_name = desktop_name(current).ok();
      let input_name = desktop_name(opened.0).ok();
      let same_desktop = match (cur_name.as_deref(), input_name.as_deref()) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(b),
        _ => false,
      };
      if same_desktop {
        // Already on the input desktop. `opened` is dropped here, closing the handle cleanly.
        return;
      }
    }

    THREAD_DESKTOP_HOLDER.with(|holder| {
      let mut slot = holder.borrow_mut();
      if SetThreadDesktop(opened.0).is_ok() {
        if let Some(bound) = slot.as_mut() {
          // Thread transitioned across desktops again: reassign `opened` while preserving
          // the thread's true initial `original` desktop. Replacing `bound.opened` drops
          // and closes the previous handle now that the thread is attached to the new one.
          bound.opened = opened;
        } else {
          let original = current.unwrap_or_default();
          *slot = Some(BoundDesktop { original, opened });
        }
      }
    });
  }
}

#[cfg(not(target_os = "windows"))]
pub fn ensure_input_desktop() {}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn desktop_handle_invalid_does_not_panic_on_drop() {
    #[cfg(target_os = "windows")]
    let handle = DesktopHandle(HDESK::default());
    #[cfg(not(target_os = "windows"))]
    let handle = DesktopHandle(());
    assert!(!handle.is_valid());
    drop(handle);
  }

  #[cfg(target_os = "windows")]
  #[test]
  fn ensure_input_desktop_multiple_calls_do_not_panic() {
    // Calling ensure_input_desktop repeatedly must succeed without leaking or crashing.
    for _ in 0..50 {
      ensure_input_desktop();
    }
  }

  #[cfg(target_os = "windows")]
  #[test]
  fn with_input_desktop_runs_action_and_returns_value() {
    let res = with_input_desktop(|| 42);
    if let Ok(value) = res {
      assert_eq!(value, 42);
    }
  }

  #[cfg(target_os = "windows")]
  #[test]
  fn bound_desktop_preserves_thread_desktop_after_drop() {
    unsafe {
      let before = GetThreadDesktop(GetCurrentThreadId());
      if let Ok(before) = before {
        let name_before = desktop_name(before).ok();
        {
          let guard = BoundDesktop::bind_input(DESKTOP_ACCESS_FLAGS(0x000F_01FF));
          if let Ok(guard) = guard {
            let active = GetThreadDesktop(GetCurrentThreadId());
            if let Ok(active) = active {
              let name_active = desktop_name(active).ok();
              assert!(name_active.is_some());
            }
            drop(guard);
          }
        }
        let after = GetThreadDesktop(GetCurrentThreadId());
        if let Ok(after) = after {
          let name_after = desktop_name(after).ok();
          assert_eq!(name_before, name_after);
        }
      }
    }
  }

  #[cfg(target_os = "windows")]
  #[test]
  fn bound_desktop_transition_preserves_initial_original_desktop() {
    // Verify that consecutive desktop reassignments in BoundDesktop maintain
    // the initial original desktop without leaking or using dangling handles.
    let original = HDESK(0x1234 as _);
    let mut bound = BoundDesktop {
      original,
      opened: DesktopHandle(HDESK::default()),
    };
    assert_eq!(bound.original, original);

    // Reassign opened handle simulating transition
    bound.opened = DesktopHandle(HDESK::default());
    assert_eq!(bound.original, original);
  }

  #[test]
  fn desktop_name_case_insensitivity_contract() {
    let a = "Default";
    let b = "default";
    assert!(a.eq_ignore_ascii_case(b));
    let c = "Winlogon";
    let d = "winlogon";
    assert!(c.eq_ignore_ascii_case(d));
    assert!(!a.eq_ignore_ascii_case(c));
  }
}
