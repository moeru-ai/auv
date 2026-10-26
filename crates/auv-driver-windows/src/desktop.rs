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

/// Errors arising from desktop attachment or lifecycle operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DesktopError {
  #[error("Failed to query thread desktop")]
  QueryFailed,
  #[error("Failed to open input desktop")]
  OpenFailed,
  #[error("Failed to switch thread desktop")]
  SwitchFailed,
  #[error("Failed to restore original thread desktop")]
  RestorationFailed,
  #[error("Failed to close desktop handle")]
  CloseFailed,
}

pub(crate) trait DesktopApi {
  fn get_thread_desktop(&self) -> Result<u64, DesktopError>;
  fn open_input_desktop(&self) -> Result<u64, DesktopError>;
  fn desktop_name(&self, handle: u64) -> Result<String, DesktopError>;
  fn set_thread_desktop(&self, handle: u64) -> Result<(), DesktopError>;
  fn close_desktop(&self, handle: u64) -> Result<(), DesktopError>;
}

#[cfg(target_os = "windows")]
struct Win32DesktopApi;

#[cfg(target_os = "windows")]
impl DesktopApi for Win32DesktopApi {
  fn get_thread_desktop(&self) -> Result<u64, DesktopError> {
    unsafe {
      let hdesk = GetThreadDesktop(GetCurrentThreadId()).map_err(|_| DesktopError::QueryFailed)?;
      if hdesk.is_invalid() || hdesk.0.is_null() {
        Err(DesktopError::QueryFailed)
      } else {
        Ok(hdesk.0 as usize as u64)
      }
    }
  }

  fn open_input_desktop(&self) -> Result<u64, DesktopError> {
    unsafe {
      let hdesk =
        OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(0x000F_01FF)).map_err(|_| DesktopError::OpenFailed)?;
      if hdesk.is_invalid() || hdesk.0.is_null() {
        Err(DesktopError::OpenFailed)
      } else {
        Ok(hdesk.0 as usize as u64)
      }
    }
  }

  fn desktop_name(&self, handle: u64) -> Result<String, DesktopError> {
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
      .map_err(|_| DesktopError::QueryFailed)?;

      if needed == 0 || needed as usize > std::mem::size_of_val(&units) || needed % 2 != 0 {
        return Err(DesktopError::QueryFailed);
      }

      let end = units.iter().position(|unit| *unit == 0).ok_or(DesktopError::QueryFailed)?;
      String::from_utf16(&units[..end]).map_err(|_| DesktopError::QueryFailed)
    }
  }

  fn set_thread_desktop(&self, handle: u64) -> Result<(), DesktopError> {
    unsafe { SetThreadDesktop(HDESK(handle as usize as *mut _)).map_err(|_| DesktopError::SwitchFailed) }
  }

  fn close_desktop(&self, handle: u64) -> Result<(), DesktopError> {
    unsafe { CloseDesktop(HDESK(handle as usize as *mut _)).map_err(|_| DesktopError::CloseFailed) }
  }
}

/// Internal state machine tracking thread desktop attachments and lifecycle.
#[derive(Debug, Default)]
pub(crate) struct DesktopState {
  original: Option<u64>,
  bound: Option<u64>,
  unclosed: Vec<u64>,
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

  #[cfg(test)]
  pub(crate) fn unclosed_handles(&self) -> &[u64] {
    &self.unclosed
  }

  /// Explicitly transfers ownership of any unclosed handle IDs out of the state machine.
  #[allow(dead_code)]
  pub(crate) fn take_unclosed_handles(&mut self) -> Vec<u64> {
    std::mem::take(&mut self.unclosed)
  }

  /// Retries cleanup of thread desktop and all tracked desktop handles.
  #[allow(dead_code)]
  pub(crate) fn retry_cleanup(&mut self, api: &impl DesktopApi) -> Result<(), DesktopError> {
    self.cleanup(api)
  }

  pub(crate) fn attach(&mut self, api: &impl DesktopApi) -> Result<(), DesktopError> {
    // Opportunistically retry closing any lingering unclosed handles from previous transition errors
    self.unclosed.retain(|&h| api.close_desktop(h).is_err());

    let opened = api.open_input_desktop()?;

    let current = match api.get_thread_desktop() {
      Ok(cur) => cur,
      Err(err) => {
        // Cannot query calling thread's current desktop. Without an identifiable original,
        // switching would create an un-restorable thread state. Fail closed and close opened handle.
        if let Err(_) = api.close_desktop(opened) {
          self.unclosed.push(opened);
        }
        return Err(err);
      }
    };

    let same_desktop = match (api.desktop_name(current), api.desktop_name(opened)) {
      (Ok(cur_name), Ok(input_name)) => cur_name.eq_ignore_ascii_case(&input_name),
      _ => false,
    };

    if same_desktop {
      // Already on the input desktop. `opened` was never bound to the thread,
      // so close it immediately to prevent handle leaks.
      if let Err(err) = api.close_desktop(opened) {
        self.unclosed.push(opened);
        return Err(err);
      }
      return Ok(());
    }

    // Need to switch the calling thread to the input desktop.
    if let Err(err) = api.set_thread_desktop(opened) {
      // Switch failed. Thread desktop was unchanged; close opened handle immediately.
      if let Err(_) = api.close_desktop(opened) {
        self.unclosed.push(opened);
      }
      return Err(err);
    }

    // Switch succeeded.
    if let Some(prev) = self.bound.replace(opened) {
      // Thread was previously assigned to `prev`, but is now on `opened`.
      // `prev` is no longer in use by the thread and can be safely closed.
      if let Err(err) = api.close_desktop(prev) {
        // Closing previous handle failed. Retain ownership in unclosed so it can be retried or transferred.
        self.unclosed.push(prev);
        return Err(err);
      }
    } else {
      // First desktop transition on this thread: preserve initial desktop for restoration.
      self.original = Some(current);
    }

    Ok(())
  }

  pub(crate) fn cleanup(&mut self, api: &impl DesktopApi) -> Result<(), DesktopError> {
    if let Some(&bound) = self.bound.as_ref() {
      if let Some(&orig) = self.original.as_ref() {
        if api.set_thread_desktop(orig).is_err() {
          self.restoration_failed = true;
          return Err(DesktopError::RestorationFailed);
        }
        self.restoration_failed = false;

        // Successfully restored thread to original desktop.
        // Now close the bound desktop handle. If closing fails, retain ownership in bound for retry.
        if let Err(err) = api.close_desktop(bound) {
          return Err(err);
        }
        self.bound = None;

        // Close any lingering unclosed handles from previous transition close failures
        let mut still_unclosed = Vec::new();
        let mut close_err = None;
        for &h in &self.unclosed {
          if let Err(err) = api.close_desktop(h) {
            still_unclosed.push(h);
            if close_err.is_none() {
              close_err = Some(err);
            }
          }
        }
        self.unclosed = still_unclosed;

        if let Some(err) = close_err {
          return Err(err);
        }

        self.original = None;
        Ok(())
      } else {
        self.restoration_failed = true;
        Err(DesktopError::RestorationFailed)
      }
    } else {
      Ok(())
    }
  }
}

#[cfg(target_os = "windows")]
impl Drop for DesktopState {
  fn drop(&mut self) {
    let _ = self.cleanup(&Win32DesktopApi);
  }
}

#[cfg(target_os = "windows")]
thread_local! {
  static THREAD_DESKTOP_STATE: RefCell<DesktopState> = const { RefCell::new(DesktopState {
    original: None,
    bound: None,
    unclosed: Vec::new(),
    restoration_failed: false,
  }) };
}

/// Attempts to attach the current thread to the input desktop, returning typed error details if failed.
#[cfg(target_os = "windows")]
pub fn try_ensure_input_desktop() -> Result<(), DesktopError> {
  THREAD_DESKTOP_STATE.with(|state| {
    let mut state = state.borrow_mut();
    state.attach(&Win32DesktopApi)
  })
}

/// No-op fallback on non-Windows platforms.
#[cfg(not(target_os = "windows"))]
pub fn try_ensure_input_desktop() -> Result<(), DesktopError> {
  Ok(())
}

/// Ensures the current thread is attached to the input desktop without leaking handles.
/// Returns true if attached (or already on input desktop), false on error.
#[cfg(target_os = "windows")]
pub fn ensure_input_desktop() -> bool {
  try_ensure_input_desktop().is_ok()
}

/// No-op fallback on non-Windows platforms.
#[cfg(not(target_os = "windows"))]
pub fn ensure_input_desktop() -> bool {
  true
}

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
    close_desktop_should_fail_for: RefCell<HashSet<u64>>,
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
        close_desktop_should_fail_for: RefCell::new(HashSet::new()),
        closed_handles: RefCell::new(Vec::new()),
        set_desktop_calls: RefCell::new(Vec::new()),
      }
    }
  }

  impl DesktopApi for MockDesktopApi {
    fn get_thread_desktop(&self) -> Result<u64, DesktopError> {
      if self.get_current_should_fail {
        Err(DesktopError::QueryFailed)
      } else {
        self.current_desktop.borrow().ok_or(DesktopError::QueryFailed)
      }
    }

    fn open_input_desktop(&self) -> Result<u64, DesktopError> {
      if self.open_should_fail {
        Err(DesktopError::OpenFailed)
      } else {
        let handle = *self.next_input_handle.borrow();
        *self.next_input_handle.borrow_mut() += 1;
        self.desktop_names.borrow_mut().insert(handle, self.input_desktop_name.borrow().clone());
        Ok(handle)
      }
    }

    fn desktop_name(&self, handle: u64) -> Result<String, DesktopError> {
      self.desktop_names.borrow().get(&handle).cloned().ok_or(DesktopError::QueryFailed)
    }

    fn set_thread_desktop(&self, handle: u64) -> Result<(), DesktopError> {
      self.set_desktop_calls.borrow_mut().push(handle);
      if self.set_desktop_should_fail_for.borrow().contains(&handle) {
        Err(DesktopError::SwitchFailed)
      } else {
        *self.current_desktop.borrow_mut() = Some(handle);
        Ok(())
      }
    }

    fn close_desktop(&self, handle: u64) -> Result<(), DesktopError> {
      // Assert Win32 invariant: CloseDesktop must never be called on a handle currently assigned to calling thread!
      if *self.current_desktop.borrow() == Some(handle) {
        panic!("Win32 ERROR_BUSY violation: Attempted CloseDesktop on handle {handle} currently assigned to calling thread");
      }
      if self.close_desktop_should_fail_for.borrow().contains(&handle) {
        return Err(DesktopError::CloseFailed);
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
    assert!(state.cleanup(&mock).is_ok());
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
    assert!(state.cleanup(&mock).is_ok());
    assert_eq!(mock.set_desktop_calls.borrow().as_slice(), &[200, 100]);
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
    assert!(!state.is_restoration_failed());
    assert_eq!(state.bound_handle(), None);
    assert_eq!(state.original_handle(), None);
  }

  #[test]
  fn test_restoration_failure_retains_handle_ownership_and_enables_retry_recovery() {
    let mock = MockDesktopApi::new(Some(100), "Winlogon", "Default");
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_ok());
    assert_eq!(state.bound_handle(), Some(200));
    assert_eq!(state.original_handle(), Some(100));

    // Simulate windows or hooks created on thread: restoring to 100 fails
    mock.set_desktop_should_fail_for.borrow_mut().insert(100);

    let res = state.cleanup(&mock);
    assert_eq!(res, Err(DesktopError::RestorationFailed));

    // Invariant: restoration failure MUST retain handle identities and ownership
    assert!(state.is_restoration_failed());
    assert_eq!(state.bound_handle(), Some(200), "must retain bound handle identity upon restoration failure");
    assert_eq!(state.original_handle(), Some(100), "must retain original handle identity upon restoration failure");
    // Handle 200 was NOT closed, preventing Win32 ERROR_BUSY
    assert!(mock.closed_handles.borrow().is_empty());

    // Now simulate thread unregisters its hooks/windows, allowing retry recovery:
    mock.set_desktop_should_fail_for.borrow_mut().remove(&100);

    let retry_res = state.cleanup(&mock);
    assert_eq!(retry_res, Ok(()), "recovery cleanup must succeed once restoration succeeds");
    assert!(!state.is_restoration_failed());
    assert_eq!(state.bound_handle(), None);
    assert_eq!(state.original_handle(), None);
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
  }

  #[test]
  fn test_same_desktop_probe_close_failure_reports_error() {
    let mock = MockDesktopApi::new(Some(100), "Default", "Default");
    mock.close_desktop_should_fail_for.borrow_mut().insert(200);
    let mut state = DesktopState::new();

    let res = state.attach(&mock);
    assert_eq!(res, Err(DesktopError::CloseFailed));
  }

  #[test]
  fn test_intermediate_handle_close_failure_retains_ownership_and_enables_retry() {
    let mock = MockDesktopApi::new(Some(100), "Winlogon", "Default");
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_ok());
    assert_eq!(state.bound_handle(), Some(200));

    // Switch to Screensaver (201), but closing previous 200 fails
    *mock.input_desktop_name.borrow_mut() = "Screensaver".to_string();
    mock.close_desktop_should_fail_for.borrow_mut().insert(200);

    let res = state.attach(&mock);
    assert_eq!(res, Err(DesktopError::CloseFailed));
    // Ownership of unclosed handle 200 is retained in state.unclosed, NOT lost!
    assert_eq!(state.bound_handle(), Some(201));
    assert_eq!(state.unclosed_handles(), &[200]);
    assert_eq!(state.original_handle(), Some(100));

    // Unblock 200: next transition to 202 opportunistically cleans up 200
    mock.close_desktop_should_fail_for.borrow_mut().remove(&200);
    *mock.input_desktop_name.borrow_mut() = "Admin".to_string();
    assert!(state.attach(&mock).is_ok());
    assert_eq!(state.bound_handle(), Some(202));
    assert!(state.unclosed_handles().is_empty());
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200, 201]);

    // Final cleanup restores back to 100 and closes 202
    assert!(state.cleanup(&mock).is_ok());
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200, 201, 202]);
    assert_eq!(state.bound_handle(), None);
    assert_eq!(state.original_handle(), None);
  }

  #[test]
  fn test_initial_set_thread_desktop_failure_closes_opened_handle() {
    let mock = MockDesktopApi::new(Some(100), "Winlogon", "Default");
    mock.set_desktop_should_fail_for.borrow_mut().insert(200);
    let mut state = DesktopState::new();

    assert_eq!(state.attach(&mock), Err(DesktopError::SwitchFailed));

    // Opened handle was closed immediately upon switch failure
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
    assert_eq!(state.bound_handle(), None);
    assert_eq!(state.original_handle(), None);
  }

  #[test]
  fn test_cleanup_close_failure_retains_handle_ownership_and_enables_retry() {
    let mock = MockDesktopApi::new(Some(100), "Winlogon", "Default");
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_ok());
    assert_eq!(state.bound_handle(), Some(200));
    assert_eq!(state.original_handle(), Some(100));

    // Inject close failure for 200 during cleanup
    mock.close_desktop_should_fail_for.borrow_mut().insert(200);

    let res = state.cleanup(&mock);
    assert_eq!(res, Err(DesktopError::CloseFailed));

    // Crucial: Handle 200 and original 100 MUST NOT be cleared on close failure
    assert_eq!(state.bound_handle(), Some(200));
    assert_eq!(state.original_handle(), Some(100));

    // Unblock 200 and retry cleanup
    mock.close_desktop_should_fail_for.borrow_mut().remove(&200);
    assert!(state.retry_cleanup(&mock).is_ok());

    assert_eq!(state.bound_handle(), None);
    assert_eq!(state.original_handle(), None);
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[200]);
  }

  #[test]
  fn test_take_unclosed_handles_transfers_ownership() {
    let mock = MockDesktopApi::new(Some(100), "Winlogon", "Default");
    let mut state = DesktopState::new();

    assert!(state.attach(&mock).is_ok());

    // Transition to 201 with 200 close failing
    *mock.input_desktop_name.borrow_mut() = "Screensaver".to_string();
    mock.close_desktop_should_fail_for.borrow_mut().insert(200);
    assert_eq!(state.attach(&mock), Err(DesktopError::CloseFailed));
    assert_eq!(state.unclosed_handles(), &[200]);

    // Explicit transfer of ownership
    let unclosed = state.take_unclosed_handles();
    assert_eq!(unclosed, vec![200]);
    assert!(state.unclosed_handles().is_empty());

    // Cleanup now only manages 201 and 100
    assert!(state.cleanup(&mock).is_ok());
    assert_eq!(mock.closed_handles.borrow().as_slice(), &[201]);

    // Caller explicitly closes the transferred handle
    mock.close_desktop_should_fail_for.borrow_mut().remove(&200);
    assert!(mock.close_desktop(unclosed[0]).is_ok());
  }

  #[test]
  fn test_get_thread_desktop_failure_closes_opened_and_aborts() {
    let mut mock = MockDesktopApi::new(None, "", "Default");
    mock.get_current_should_fail = true;
    let mut state = DesktopState::new();

    assert_eq!(state.attach(&mock), Err(DesktopError::QueryFailed));

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

    assert_eq!(state.attach(&mock), Err(DesktopError::OpenFailed));
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
    assert!(state.cleanup(&mock).is_ok());
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
    assert!(ensure_input_desktop());
  }
}
