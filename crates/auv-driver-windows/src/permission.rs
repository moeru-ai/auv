//! Process-level automation readiness probe: UAC elevation, UIPI/UIAccess, and
//! interactive-session isolation.
//!
//! Windows has no single "automation permission" switch like macOS. Instead,
//! synthetic input (`SendInput`) and cross-process window driving can be
//! silently blocked by process-level conditions, so this probe surfaces the
//! three that matter for an automation driver. Each signal reuses the shared
//! [`PermissionStatus`] vocabulary: `Granted` when the condition holds,
//! `Missing` when it does not, and `Unknown` when the underlying query failed
//! (or the platform cannot determine it).
// TODO(windows-readiness-assessment): a target-aware readiness assessment that
// combines this probe with window/frontmost/frame-drift checks (mirroring the
// macOS `assess_readiness`) is deferred until an owner-approved slice; it needs
// Windows equivalents for the macOS app-bundle/frontmost concepts first.

use auv_driver::permission::PermissionStatus;

/// Windows-specific automation readiness signals.
///
/// - `elevated`: this process runs with an elevated (administrator) token. A
///   non-elevated process cannot drive or send input to windows owned by
///   elevated processes (UIPI).
/// - `ui_access`: this process holds the UIAccess privilege, which lets it
///   bypass UIPI and drive higher-integrity windows without being elevated.
/// - `interactive_session`: this process runs in an interactive session (not
///   Session 0). Session 0 services have no access to the user desktop, so
///   input and capture cannot reach it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WindowsPermissionProbe {
  pub elevated: PermissionStatus,
  pub ui_access: PermissionStatus,
  pub interactive_session: PermissionStatus,
}

/// Probes the current process's automation readiness signals.
///
/// This never hard-fails: a signal that cannot be determined is reported as
/// [`PermissionStatus::Unknown`] rather than erroring, so callers always get a
/// complete report.
pub fn probe() -> WindowsPermissionProbe {
  native::probe()
}

#[cfg(target_os = "windows")]
mod native {
  use std::ffi::c_void;
  use std::mem::size_of;

  use auv_driver::permission::PermissionStatus;
  use windows::Win32::Foundation::{CloseHandle, HANDLE};
  use windows::Win32::Security::{
    GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation, TokenUIAccess,
  };
  use windows::Win32::System::RemoteDesktop::ProcessIdToSessionId;
  use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentProcessId, OpenProcessToken,
  };
  use windows::core::Result as WindowsResult;

  use super::WindowsPermissionProbe;

  /// Closes the process token handle opened for the probe, since the handle
  /// returned by `OpenProcessToken` is a real handle that must be released
  /// (unlike the `GetCurrentProcess` pseudo-handle).
  struct TokenGuard(HANDLE);

  impl Drop for TokenGuard {
    fn drop(&mut self) {
      let _ = unsafe { CloseHandle(self.0) };
    }
  }

  pub(super) fn probe() -> WindowsPermissionProbe {
    let token = open_process_token();
    let (elevated, ui_access) = match &token {
      Ok(guard) => (
        status_from(query_elevation(guard.0)),
        status_from(query_ui_access(guard.0)),
      ),
      Err(_) => (PermissionStatus::Unknown, PermissionStatus::Unknown),
    };
    WindowsPermissionProbe {
      elevated,
      ui_access,
      interactive_session: status_from(query_interactive_session()),
    }
  }

  fn open_process_token() -> WindowsResult<TokenGuard> {
    let mut handle = HANDLE::default();
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut handle) }?;
    Ok(TokenGuard(handle))
  }

  fn query_elevation(token: HANDLE) -> WindowsResult<bool> {
    let mut elevation = TOKEN_ELEVATION::default();
    let mut return_length = 0u32;
    unsafe {
      GetTokenInformation(
        token,
        TokenElevation,
        Some(&mut elevation as *mut TOKEN_ELEVATION as *mut c_void),
        size_of::<TOKEN_ELEVATION>() as u32,
        &mut return_length,
      )
    }?;
    Ok(elevation.TokenIsElevated != 0)
  }

  fn query_ui_access(token: HANDLE) -> WindowsResult<bool> {
    // TokenUIAccess fills a single DWORD that is nonzero when the token has the
    // UIAccess privilege.
    let mut ui_access = 0u32;
    let mut return_length = 0u32;
    unsafe {
      GetTokenInformation(
        token,
        TokenUIAccess,
        Some(&mut ui_access as *mut u32 as *mut c_void),
        size_of::<u32>() as u32,
        &mut return_length,
      )
    }?;
    Ok(ui_access != 0)
  }

  fn query_interactive_session() -> WindowsResult<bool> {
    let mut session_id = 0u32;
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut session_id) }?;
    // Session 0 is the non-interactive services session; any other session is
    // an interactive user desktop.
    Ok(session_id != 0)
  }

  fn status_from(result: WindowsResult<bool>) -> PermissionStatus {
    match result {
      Ok(true) => PermissionStatus::Granted,
      Ok(false) => PermissionStatus::Missing,
      Err(_) => PermissionStatus::Unknown,
    }
  }
}

#[cfg(not(target_os = "windows"))]
mod native {
  use super::WindowsPermissionProbe;

  pub(super) fn probe() -> WindowsPermissionProbe {
    // No Windows token/session model on other targets; every signal is Unknown,
    // which is the default for `PermissionStatus`.
    WindowsPermissionProbe::default()
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::permission::PermissionStatus;

  use super::*;

  #[test]
  fn default_probe_is_all_unknown() {
    let probe = WindowsPermissionProbe::default();
    assert_eq!(probe.elevated, PermissionStatus::Unknown);
    assert_eq!(probe.ui_access, PermissionStatus::Unknown);
    assert_eq!(probe.interactive_session, PermissionStatus::Unknown);
  }

  // Live smoke test: the token and session queries must succeed (resolve to a
  // concrete Granted/Missing) when run as a normal interactive process, proving
  // the FFI calls are wired correctly. The environment (admin vs not) is not
  // asserted, only that each signal was determinable.
  #[cfg(target_os = "windows")]
  #[test]
  fn probe_resolves_signals_on_windows() {
    let probe = probe();
    assert_ne!(probe.elevated, PermissionStatus::Unknown);
    assert_ne!(probe.ui_access, PermissionStatus::Unknown);
    assert_ne!(probe.interactive_session, PermissionStatus::Unknown);
  }
}
