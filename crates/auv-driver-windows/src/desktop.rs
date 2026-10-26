//! Win32 desktop and window station management.
//!
//! Provides thread desktop attachment helpers to ensure background tasks,
//! benchmarks, and capture sessions interact with the interactive desktop,
//! backed by deterministic handle ownership and safe thread desktop restoration.

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

#[cfg(target_os = "windows")]
pub(crate) trait DesktopApi {
  fn get_thread_desktop(&self) -> Result<u64, ()>;
  fn open_input_desktop(&self) -> Result<u64, ()>;
  fn desktop_name(&self, handle: u64) -> Result<String, ()>;
  fn set_thread_desktop(&self, handle: u64) -> Result<(), ()>;
  fn close_desktop(&self, handle: u64) -> Result<(), ()>;
}

#[cfg(target_os = "windows")]
struct Win32DesktopApi;

#[cfg(target_os = "windows")]
impl DesktopApi for Win32DesktopApi {
  fn get_thread_desktop(&self) -> Result<u64, ()> {
    unsafe {
      let hdesk = GetThreadDesktop(GetCurrentThreadId()).map_err(|_| ())?;
      if hdesk.is_invalid() || hdesk.0.is_null() {
        Err(())
      } else {
        Ok(hdesk.0 as usize as u64)
      }
    }
  }

  fn open_input_desktop(&self) -> Result<u64, ()> {
    unsafe {
      let hdesk = OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(0x000F_01FF)).map_err(|_| ())?;
      if hdesk.is_invalid() || hdesk.0.is_null() {
        Err(())
      } else {
        Ok(hdesk.0 as usize as u64)
      }
    }
  }

  fn desktop_name(&self, handle: u64) -> Result<String, ()> {
    unsafe {
      let mut units = [0u16; 64];
      let mut needed = 0u32;
      GetUserObjectInformationW(
        HANDLE(handle as usize as *mut _),
        UOI_NAME,
        Some(units.as_mut_ptr().cast()),
        std::mem::size_of_val(&units) as u32,
        Some(&mut needed),
      )
      .map_err(|_| ())?;

      if needed == 0 || needed as usize > std::mem::size_of_val(&units) || needed % 2 != 0 {
        return Err(());
      }

      let end = units.iter().position(|unit| *unit == 0).ok_or(())?;
      String::from_utf16(&units[..end]).map_err(|_| ())
    }
  }

  fn set_thread_desktop(&self, handle: u64) -> Result<(), ()> {
    unsafe { SetThreadDesktop(HDESK(handle as usize as *mut _)).map_err(|_| ()) }
  }

  fn close_desktop(&self, handle: u64) -> Result<(), ()> {
    unsafe { CloseDesktop(HDESK(handle as usize as *mut _)).map_err(|_| ()) }
  }
}

/// Internal state machine tracking thread desktop attachments and lifecycle.
#[derive(Debug, Default)]
pub(crate) struct DesktopState {
  original: Option<u64>,
  bound: Option<u64>,
  restoration_failed: bool,
}

impl DesktopState {
  #[cfg(test)]
  pub(crate) fn new() -> Self {
    Self::default()
  }

  #[cfg(test)]
  pub(crate) fn bound_handle(&self) -> Option<u64> {
    self.bound
  }

  #[cfg(test)]
  pub(crate) fn original_handle(&self) -> Option<u64> {
    self.original
  }

  #[cfg(test)]
  pub(crate) fn is_restoration_failed(&self) -> bool {
    self.restoration_failed
  }

  pub(crate) fn attach(&mut self, api: &impl DesktopApi) -> Result<(), ()> {
    let Ok(opened) = api.open_input_desktop() else {
      return Err(());
    };

    let Some(current) = api.get_thread_desktop().ok() else {
      // Cannot query calling thread's current desktop. Without an identifiable original,
      // switching would create an un-restorable thread state. Fail closed and close opened handle.
      let _ = api.close_desktop(opened);
      return Err(());
    };

    let same_desktop = match (api.desktop_name(current), api.desktop_name(opened)) {
      (Ok(cur_name), Ok(input_name)) => cur_name.eq_ignore_ascii_case(&input_name),
      _ => false,
    };

    if same_desktop {
      // Already on the input desktop. `opened` was never bound to the thread,
      // so close it immediately to prevent handle leaks.
      let _ = api.close_desktop(opened);
      return Ok(());
    }

    // Need to switch the calling thread to the input desktop.
    if api.set_thread_desktop(opened).is_err() {
      // Switch failed. Thread desktop was unchanged; close opened handle immediately.
      let _ = api.close_desktop(opened);
      return Err(());
    }

    // Switch succeeded.
    if let Some(prev) = self.bound.replace(opened) {
      // Thread was previously assigned to `prev`, but is now on `opened`.
      // `prev` is no longer in use by the thread and can be safely closed.
      let _ = api.close_desktop(prev);
    } else {
      // First desktop transition on this thread: preserve initial desktop for restoration.
      self.original = Some(current);
    }

    Ok(())
  }

  pub(crate) fn cleanup(&mut self, api: &impl DesktopApi) {
    if let Some(bound) = self.bound.take() {
      if let Some(orig) = self.original.take() {
        if api.set_thread_desktop(orig).is_ok() {
          // Successfully restored thread to original desktop.
          // `bound` is no longer assigned to this thread, so CloseDesktop is valid.
          let _ = api.close_desktop(bound);
        } else {
          // SetThreadDesktop(orig) failed (e.g. windows or hooks created on the thread).
          // Because `bound` is still assigned to the thread, Win32 CloseDesktop would fail with ERROR_BUSY (170).
          // We MUST NOT call close_desktop; preserve ownership and flag restoration failure.
          self.restoration_failed = true;
        }
      } else {
        // No original desktop was recorded; thread remains on `bound`.
        self.restoration_failed = true;
      }
    }
  }
}

#[cfg(target_os = "windows")]
impl Drop for DesktopState {
  fn drop(&mut self) {
    self.cleanup(&Win32DesktopApi);
  }
}

#[cfg(target_os = "windows")]
thread_local! {
  static THREAD_DESKTOP_STATE: RefCell<DesktopState> = const { RefCell::new(DesktopState {
    original: None,
    bound: None,
    restoration_failed: false,
  }) };
}

/// Ensures the current thread is attached to the input desktop without leaking handles.
///
/// If the thread is already attached to the input desktop, no handle is leaked and the thread
/// is untouched. If the thread transitions to the input desktop, the handle is managed in
/// thread-local storage and closed when the thread completes or transitions again.
#[cfg(target_os = "windows")]
pub fn ensure_input_desktop() {
  THREAD_DESKTOP_STATE.with(|state| {
    let mut state = state.borrow_mut();
    let _ = state.attach(&Win32DesktopApi);
  });
}

#[cfg(not(target_os = "windows"))]
pub fn ensure_input_desktop() {}

#[cfg(test)]
mod tests {
  use super::*;
  use std::cell::RefCell;
  use std::collections::{HashMap, HashSet};

  struct MockDesktopApi {
    current_desktop: RefCell<Option<u64>>,
    next_input_handle: RefCell<u64>,
    input_desktop_name: RefCell<String>,
    desktop_names: RefCell<HashMap<u64, String>>,
    open_should_fail: bool,
    get_current_should_fail: bool,
    set_desktop_should_fail_for: RefCell<HashSet<u64>>,
    closed_handles: RefCell<Vec<u64>>,
    set_desktop_calls: RefCell<Vec<u64>>,
  }

  impl MockDesktopApi {
    fn new(current: Option<u64>, current_name: &str, input_name: &str) -> Self {
      let mut names = HashMap::new();
      if let Some(cur) = current {
        names.insert(cur, current_name.to_string());
      }
      Self {
        current_desktop: RefCell::new(current),
        next_input_handle: RefCell::new(200),
        input_desktop_name: RefCell::new(input_name.to_string()),
        desktop_names: RefCell::new(names),
        open_should_fail: false,
        get_current_should_fail: false,
        set_desktop_should_fail_for: RefCell::new(HashSet::new()),
        closed_handles: RefCell::new(Vec::new()),
        set_desktop_calls: RefCell::new(Vec::new()),
      }
    }
  }

  impl DesktopApi for MockDesktopApi {
    fn get_thread_desktop(&self) -> Result<u64, ()> {
      if self.get_current_should_fail {
        Err(())
      } else {
        self.current_desktop.borrow().ok_or(())
      }
    }

    fn open_input_desktop(&self) -> Result<u64, ()> {
      if self.open_should_fail {
        Err(())
      } else {
        let handle = *self.next_input_handle.borrow();
        *self.next_input_handle.borrow_mut() += 1;
        self.desktop_names.borrow_mut().insert(handle, self.input_desktop_name.borrow().clone());
        Ok(handle)
      }
    }

    fn desktop_name(&self, handle: u64) -> Result<String, ()> {
      self.desktop_names.borrow().get(&handle).cloned().ok_or(())
    }

    fn set_thread_desktop(&self, handle: u64) -> Result<(), ()> {
      self.set_desktop_calls.borrow_mut().push(handle);
      if self.set_desktop_should_fail_for.borrow().contains(&handle) {
        Err(())
      } else {
        *self.current_desktop.borrow_mut() = Some(handle);
        Ok(())
      }
    }

    fn close_desktop(&self, handle: u64) -> Result<(), ()> {
      // Assert Win32 invariant: CloseDesktop must never be called on a handle currently assigned to calling thread!
      if *self.current_desktop.borrow() == Some(handle) {
        panic!("Win32 ERROR_BUSY violation: Attempted CloseDesktop on handle {handle} currently assigned to calling thread");
      }
      self.closed_handles.borrow_mut().push(handle);
      Ok(())
    }
  }

  #[test]
  fn test_already_on_input_desktop_closes_opened_immediately_without_switch() {
    let mock = MockDesktopApi::new(Some(100), "Default", "Default");
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_ok());

    // Never called SetThreadDesktop
    assert!(mock.set_desktop_calls.borrow().is_empty());
    // Immediately closed opened probe handle
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
    // State remains unattached
    assert_eq!(state.bound_handle(), None);
    assert_eq!(state.original_handle(), None);

    // Cleanup is a no-op
    state.cleanup(&mock);
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
    assert!(mock.set_desktop_calls.borrow().is_empty());
  }

  #[test]
  fn test_different_desktop_switches_and_restores_on_cleanup() {
    let mock = MockDesktopApi::new(Some(100), "Winlogon", "Default");
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_ok());

    // Switched thread to opened handle 200
    assert_eq!(mock.set_desktop_calls.borrow().as_slice(), &[200]);
    // Handle 200 is NOT closed while active on thread
    assert!(mock.closed_handles.borrow().is_empty());
    assert_eq!(state.bound_handle(), Some(200));
    assert_eq!(state.original_handle(), Some(100));

    // Cleanup restores to original 100 and closes 200
    state.cleanup(&mock);
    assert_eq!(mock.set_desktop_calls.borrow().as_slice(), &[200, 100]);
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
    assert!(!state.is_restoration_failed());
  }

  #[test]
  fn test_restoration_failure_does_not_close_in_use_handle() {
    let mock = MockDesktopApi::new(Some(100), "Winlogon", "Default");
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_ok());

    // Simulate windows or hooks created on thread: restoring to 100 fails
    mock.set_desktop_should_fail_for.borrow_mut().insert(100);

    state.cleanup(&mock);

    // Restoration failed flagged
    assert!(state.is_restoration_failed());
    // Handle 200 was NOT closed, preventing Win32 ERROR_BUSY
    assert!(mock.closed_handles.borrow().is_empty());
  }

  #[test]
  fn test_initial_set_thread_desktop_failure_closes_opened_handle() {
    let mock = MockDesktopApi::new(Some(100), "Winlogon", "Default");
    mock.set_desktop_should_fail_for.borrow_mut().insert(200);
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_err());

    // Opened handle was closed immediately upon switch failure
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
    assert_eq!(state.bound_handle(), None);
    assert_eq!(state.original_handle(), None);
  }

  #[test]
  fn test_get_thread_desktop_failure_closes_opened_and_aborts() {
    let mut mock = MockDesktopApi::new(None, "", "Default");
    mock.get_current_should_fail = true;
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_err());

    // Probe handle closed immediately
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
    assert!(mock.set_desktop_calls.borrow().is_empty());
    assert_eq!(state.bound_handle(), None);
  }

  #[test]
  fn test_open_input_desktop_failure_fails_closed() {
    let mut mock = MockDesktopApi::new(Some(100), "Default", "Default");
    mock.open_should_fail = true;
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_err());
    assert!(mock.closed_handles.borrow().is_empty());
    assert!(mock.set_desktop_calls.borrow().is_empty());
    assert_eq!(state.bound_handle(), None);
  }

  #[test]
  fn test_successive_transitions_preserve_initial_original_and_close_intermediate() {
    let mock = MockDesktopApi::new(Some(100), "Winlogon", "Default");
    let mut state = DesktopState::new();

    // First switch: Winlogon -> Default (handle 200)
    assert!(state.attach(&mock).is_ok());
    assert_eq!(state.original_handle(), Some(100));
    assert_eq!(state.bound_handle(), Some(200));

    // Input desktop transitions to Screensaver (handle 201)
    *mock.input_desktop_name.borrow_mut() = "Screensaver".to_string();
    assert!(state.attach(&mock).is_ok());

    // Previous handle 200 was closed once thread moved to 201
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
    // Initial original handle 100 remains preserved
    assert_eq!(state.original_handle(), Some(100));
    assert_eq!(state.bound_handle(), Some(201));

    // Cleanup restores back to initial 100 and closes 201
    state.cleanup(&mock);
    assert_eq!(mock.set_desktop_calls.borrow().as_slice(), &[200, 201, 100]);
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200, 201]);
    assert!(!state.is_restoration_failed());
  }

  #[test]
  fn test_case_insensitive_desktop_name_matching() {
    let mock = MockDesktopApi::new(Some(100), "DEFAULT", "default");
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_ok());

    // Treated as same desktop: no switch, opened handle closed
    assert!(mock.set_desktop_calls.borrow().is_empty());
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
  }

  #[cfg(target_os = "windows")]
  #[test]
  #[ignore = "touches real host desktop"]
  fn real_win32_ensure_input_desktop_smoke() {
    ensure_input_desktop();
  }
}
