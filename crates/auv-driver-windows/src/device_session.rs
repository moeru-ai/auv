//! Windows physical-console session observation and locked-session input.
//!
//! WTS session IDs can be reused after logoff. Keep the logon time and account
//! with the selector so a delayed unlock cannot target a different login.

/// Lock state reported by WTS for one existing console login.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsoleLockState {
  Locked,
  Usable,
  Unknown,
}

/// One current login on the physical console. This is not an RDP session.
// TODO(device-unlock-windows): RDP session selection is deferred until the
// owner approves a worker and readback path for non-console desktops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsoleSession {
  pub session_id: u32,
  pub logon_time: i64,
  /// Stable OS account identity, read from the selected session's token.
  pub account_sid: String,
  pub domain: String,
  pub user: String,
  pub lock_state: ConsoleLockState,
}

impl ConsoleSession {
  /// Opaque selector for this login instance, revalidated before delivery.
  pub fn selector(&self) -> String {
    format!("windows-console:{}:{}", self.session_id, self.logon_time)
  }

  /// Checks that a fresh observation still names this login and account.
  pub fn same_login(&self, current: &Self) -> bool {
    self.session_id == current.session_id && self.logon_time == current.logon_time && self.account_sid == current.account_sid
  }

  /// Display account name in `DOMAIN\user` form, or `user` without a domain.
  /// It is not an authority key; compare `account_sid` for identity.
  pub fn account_name(&self) -> String {
    if self.domain.is_empty() {
      self.user.clone()
    } else {
      format!(r"{}\{}", self.domain, self.user)
    }
  }
}

/// Failure to read a reliable physical-console session state.
#[derive(Debug, thiserror::Error)]
pub enum ConsoleSessionError {
  #[error("the physical console is changing sessions")]
  ConsoleTransition,
  #[error("Windows session observation failed: {0}")]
  QueryFailed(String),
  #[error("Windows returned an inconsistent console session record")]
  InconsistentRecord,
  #[error("the console account SID could not be verified under this host identity")]
  IdentityUnverified,
  #[error("physical-console observation requires Windows")]
  UnsupportedPlatform,
}

/// Reads one physical-console login, or `None` if nobody is logged in there.
///
/// The caller must re-read and compare [`ConsoleSession::same_login`] before
/// posting input and again when verifying the unlock. A `None` result is not
/// permission to sign in from the greeter in the locked-session release.
pub fn observe_console() -> Result<Option<ConsoleSession>, ConsoleSessionError> {
  native::observe_console()
}

/// Outcome of one credential delivery to a locked physical-console login.
///
/// The caller must run this inside a LocalSystem worker placed in the selected
/// console session. The credential must have been retrieved on that machine;
/// it must not arrive through a remote request, command line, or environment.
// NOTICE(device-unlock-windows-host): The installed LocalSystem host owns the
// target-local vault and starts this operation in the selected console session.
pub fn unlock_existing_session(target: &ConsoleSession, credential: &str) -> Result<ConsoleSession, ConsoleUnlockError> {
  native::unlock_existing_session(target, credential)
}

/// Request a lock from a LocalSystem worker placed on this login's interactive
/// Default desktop, then observe the same physical-console login as locked.
/// A successful `LockWorkStation` call alone is only an initiation receipt.
pub fn lock_existing_session(target: &ConsoleSession) -> Result<ConsoleSession, ConsoleLockError> {
  native::lock_existing_session(target)
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ConsoleLockError {
  #[error("the selected console login changed")]
  StaleSession,
  #[error("the selected console is already locked")]
  AlreadyLocked,
  #[error("the selected console lock state is unknown")]
  UnknownState,
  #[error("a console-session LocalSystem worker is required")]
  WorkerIdentity,
  #[error("the interactive Default desktop is unavailable")]
  DesktopUnavailable,
  #[error("Windows rejected the lock request")]
  LockRejected,
  #[error("the console lock outcome could not be verified")]
  Unverified,
  #[error("physical-console lock requires Windows")]
  UnsupportedPlatform,
}

fn require_same_usable_login(target: &ConsoleSession, current: &ConsoleSession) -> Result<(), ConsoleLockError> {
  if !target.same_login(current) {
    return Err(ConsoleLockError::StaleSession);
  }

  if target.lock_state != ConsoleLockState::Usable {
    return Err(ConsoleLockError::StaleSession);
  }

  match current.lock_state {
    ConsoleLockState::Usable => Ok(()),
    ConsoleLockState::Locked => Err(ConsoleLockError::AlreadyLocked),
    ConsoleLockState::Unknown => Err(ConsoleLockError::UnknownState),
  }
}

/// Errors are intentionally independent of the credential and its length.
#[derive(Debug, thiserror::Error)]
pub enum ConsoleUnlockError {
  #[error("the selected console login changed")]
  StaleSession,
  #[error("the selected console is not locked")]
  NotLocked,
  #[error("the selected console lock state is unknown")]
  UnknownState,
  #[error("a console-session LocalSystem worker is required")]
  WorkerIdentity,
  #[error("the locked desktop is unavailable")]
  DesktopUnavailable,
  #[error("Windows rejected the input event")]
  InputRejected,
  #[error("the console unlock outcome could not be verified")]
  Unverified,
  #[error("the local credential is not usable for this input path")]
  InvalidCredential,
  #[error("physical-console unlock requires Windows")]
  UnsupportedPlatform,
}

/// Check that this process is LocalSystem in the specified Windows session.
/// The Session 0 host, its protected storage, and its selected-session worker
/// use the same identity gate.
#[cfg(target_os = "windows")]
pub fn verify_local_system_process_in_session(session_id: u32) -> Result<(), ConsoleUnlockError> {
  native::verify_local_system_process_in_session(session_id)
}

#[cfg(target_os = "windows")]
mod native {
  use std::ffi::c_void;
  use std::mem::{align_of, size_of, size_of_val};
  use std::thread;
  use std::time::{Duration, Instant};

  use windows::Win32::Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree};
  use windows::Win32::Security::Authorization::ConvertSidToStringSidW;
  use windows::Win32::Security::{GetTokenInformation, IsWellKnownSid, TOKEN_QUERY, TOKEN_USER, TokenUser, WinLocalSystemSid};
  use windows::Win32::System::RemoteDesktop::{
    ProcessIdToSessionId, WTS_CURRENT_SERVER_HANDLE, WTS_SESSIONSTATE_LOCK, WTS_SESSIONSTATE_UNLOCK, WTSActive, WTSDomainName,
    WTSFreeMemory, WTSGetActiveConsoleSessionId, WTSINFOEXW, WTSQuerySessionInformationW, WTSQueryUserToken, WTSSessionInfoEx, WTSUserName,
  };
  use windows::Win32::System::Shutdown::LockWorkStation;
  use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS, DESKTOP_WRITEOBJECTS, GetThreadDesktop,
    GetUserObjectInformationW, HDESK, OpenInputDesktop, SetThreadDesktop, UOI_NAME,
  };
  use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, GetCurrentThreadId, OpenProcessToken};
  use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, SendInput, VIRTUAL_KEY, VK_RETURN,
  };
  use windows::core::PWSTR;
  use zeroize::Zeroizing;

  use super::{ConsoleLockError, ConsoleLockState, ConsoleSession, ConsoleSessionError, ConsoleUnlockError, require_same_usable_login};

  const LIMITED_DESKTOP_ACCESS: DESKTOP_ACCESS_FLAGS = DESKTOP_ACCESS_FLAGS(DESKTOP_READOBJECTS.0 | DESKTOP_WRITEOBJECTS.0);
  const INPUT_DELIVERY_DESKTOP_ACCESS: DESKTOP_ACCESS_FLAGS = DESKTOP_ACCESS_FLAGS(0x000F_01FF);

  struct QueryBuffer(PWSTR);

  impl Drop for QueryBuffer {
    fn drop(&mut self) {
      if !self.0.is_null() {
        // SAFETY: WTSQuerySessionInformationW allocated this pointer and the
        // buffer guard is its unique owner. WTSFreeMemory accepts that allocation.
        unsafe { WTSFreeMemory(self.0.0.cast::<c_void>()) };
      }
    }
  }

  struct TokenHandle(HANDLE);

  impl Drop for TokenHandle {
    fn drop(&mut self) {
      // SAFETY: WTSQueryUserToken returned this owned kernel handle once.
      let _ = unsafe { CloseHandle(self.0) };
    }
  }

  struct DesktopHandle(HDESK);

  impl Drop for DesktopHandle {
    fn drop(&mut self) {
      // SAFETY: OpenInputDesktop returned this owned desktop handle. It is
      // closed on the same short-lived thread that bound it, after input.
      let _ = unsafe { CloseDesktop(self.0) };
    }
  }

  struct BoundDesktop {
    original: HDESK,
    _opened: DesktopHandle,
  }

  impl Drop for BoundDesktop {
    fn drop(&mut self) {
      // SAFETY: This guard is created and dropped on the same fresh thread.
      // It restores that thread's original desktop before `opened` is closed.
      let _ = unsafe { SetThreadDesktop(self.original) };
    }
  }

  pub(super) fn observe_console() -> Result<Option<ConsoleSession>, ConsoleSessionError> {
    // NOTICE: 0xFFFFFFFF is documented for console attach/detach. Retrying
    // after that transition belongs to the Device host, not this snapshot.
    // SAFETY: This Win32 call takes no pointers and returns a value snapshot.
    let session_id = unsafe { WTSGetActiveConsoleSessionId() };

    if session_id == u32::MAX {
      return Err(ConsoleSessionError::ConsoleTransition);
    }

    let mut buffer = QueryBuffer(PWSTR::null());
    let mut bytes = 0u32;
    // SAFETY: The out-pointers live through the call; the returned buffer is
    // owned by QueryBuffer, including on subsequent validation failures.
    unsafe { WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE, session_id, WTSSessionInfoEx, &mut buffer.0, &mut bytes) }
      .map_err(|error| ConsoleSessionError::QueryFailed(error.to_string()))?;

    if buffer.0.is_null() || (bytes as usize) < size_of::<WTSINFOEXW>() {
      return Err(ConsoleSessionError::InconsistentRecord);
    }

    // WTS owns the returned allocation. Copy its fixed-size record before the
    // buffer guard frees it; the union's level-1 payload is valid only at 1.
    // SAFETY: The non-null buffer has at least size_of::<WTSINFOEXW>() bytes.
    // read_unaligned avoids assuming the WTS allocator's alignment here.
    let info = unsafe { buffer.0.0.cast::<WTSINFOEXW>().read_unaligned() };

    if info.Level != 1 {
      return Err(ConsoleSessionError::InconsistentRecord);
    }

    // SAFETY: WTSINFOEXW.Level == 1 selects WTSInfoExLevel1 in the union.
    let level = unsafe { info.Data.WTSInfoExLevel1 };

    if level.SessionId != session_id || level.SessionState != WTSActive {
      return Err(ConsoleSessionError::InconsistentRecord);
    }

    let user = query_text(session_id, WTSUserName)?;

    if user.is_empty() {
      return Ok(None);
    }

    if level.LogonTime == 0 {
      return Err(ConsoleSessionError::InconsistentRecord);
    }

    let lock_state = match level.SessionFlags as u32 {
      WTS_SESSIONSTATE_LOCK => ConsoleLockState::Locked,
      WTS_SESSIONSTATE_UNLOCK => ConsoleLockState::Usable,
      _ => ConsoleLockState::Unknown,
    };

    Ok(Some(ConsoleSession {
      session_id,
      logon_time: level.LogonTime,
      account_sid: query_account_sid(session_id)?,
      domain: query_text(session_id, WTSDomainName)?,
      user,
      lock_state,
    }))
  }

  fn query_text(session_id: u32, class: windows::Win32::System::RemoteDesktop::WTS_INFO_CLASS) -> Result<String, ConsoleSessionError> {
    let mut buffer = QueryBuffer(PWSTR::null());
    let mut bytes = 0u32;
    // SAFETY: The out-pointers live through the call and QueryBuffer frees the
    // WTS allocation after decoding.
    unsafe { WTSQuerySessionInformationW(WTS_CURRENT_SERVER_HANDLE, session_id, class, &mut buffer.0, &mut bytes) }
      .map_err(|error| ConsoleSessionError::QueryFailed(error.to_string()))?;

    if buffer.0.is_null() || bytes == 0 || bytes % 2 != 0 {
      return Err(ConsoleSessionError::InconsistentRecord);
    }

    // SAFETY: WTS returned `bytes` initialized bytes. u16 alignment is not
    // assumed, so each code unit is read unaligned within the allocation.
    let units = (0..bytes as usize / 2).map(|index| unsafe { buffer.0.0.add(index).read_unaligned() }).collect::<Vec<_>>();
    let Some(end) = units.iter().position(|unit| *unit == 0) else {
      return Err(ConsoleSessionError::InconsistentRecord);
    };

    String::from_utf16(&units[..end]).map_err(|_| ConsoleSessionError::InconsistentRecord)
  }

  pub(super) fn unlock_existing_session(target: &ConsoleSession, credential: &str) -> Result<ConsoleSession, ConsoleUnlockError> {
    verify_worker(target)?;
    require_locked_target(target)?;

    if credential.is_empty() || credential.chars().any(char::is_control) {
      return Err(ConsoleUnlockError::InvalidCredential);
    }

    let units = Zeroizing::new(credential.encode_utf16().collect::<Vec<_>>());
    // NOTICE: Cap one-shot SendInput to a small credential batch. Longer
    // credentials need a separately validated chunking and partial-failure
    // policy before this bound can be raised.
    if units.len() > 128 {
      return Err(ConsoleUnlockError::InvalidCredential);
    }

    match input_desktop_name()?.as_str() {
      "Default" => {
        with_bound_desktop(target, "Default", send_return)?;
        let deadline = Instant::now() + Duration::from_secs(4);

        loop {
          require_locked_target(target)?;

          if input_desktop_name()?.eq_ignore_ascii_case("Winlogon") {
            break;
          }

          if Instant::now() >= deadline {
            return Err(ConsoleUnlockError::DesktopUnavailable);
          }

          thread::sleep(Duration::from_millis(100));
        }
      }
      name if name.eq_ignore_ascii_case("Winlogon") => {}
      _ => return Err(ConsoleUnlockError::DesktopUnavailable),
    }

    // The credential is only borrowed by a fresh console-session SYSTEM
    // thread. Neither the driver error nor its caller-facing result contains
    // the secret or its length. A different login at this point is rejected.
    with_bound_desktop(target, "Winlogon", || send_credential(&units))?;

    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
      let current = observe_console().map_err(|_| ConsoleUnlockError::Unverified)?.ok_or(ConsoleUnlockError::StaleSession)?;

      if !target.same_login(&current) {
        return Err(ConsoleUnlockError::StaleSession);
      }

      if current.lock_state == ConsoleLockState::Usable {
        return Ok(current);
      }

      if Instant::now() >= deadline {
        return Err(ConsoleUnlockError::Unverified);
      }

      thread::sleep(Duration::from_millis(100));
    }
  }

  pub(super) fn lock_existing_session(target: &ConsoleSession) -> Result<ConsoleSession, ConsoleLockError> {
    verify_local_system_process_in_session(target.session_id).map_err(|_| ConsoleLockError::WorkerIdentity)?;
    let current = observe_console().map_err(|_| ConsoleLockError::Unverified)?.ok_or(ConsoleLockError::StaleSession)?;
    require_same_usable_login(target, &current)?;
    // LockWorkStation requires the calling process to run on the interactive
    // desktop. The worker is launched on winsta0\\default; also check both
    // the current input desktop and this thread's bound desktop before input.
    if input_desktop_name().map_err(|_| ConsoleLockError::DesktopUnavailable)? != "Default" {
      return Err(ConsoleLockError::DesktopUnavailable);
    }

    // SAFETY: GetThreadDesktop only reads the desktop bound to this thread.
    let thread_desktop = unsafe { GetThreadDesktop(GetCurrentThreadId()) }.map_err(|_| ConsoleLockError::DesktopUnavailable)?;

    if desktop_name(thread_desktop).map_err(|_| ConsoleLockError::DesktopUnavailable)? != "Default" {
      return Err(ConsoleLockError::DesktopUnavailable);
    }

    let current = observe_console().map_err(|_| ConsoleLockError::Unverified)?.ok_or(ConsoleLockError::StaleSession)?;
    require_same_usable_login(target, &current)?;
    // NOTICE(device-lock-windows-receipt): Microsoft documents this as an
    // asynchronous initiation receipt, so require the same-login WTS readback.
    // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-lockworkstation
    // SAFETY: This process is the verified worker in the selected interactive
    // session and Default desktop.
    unsafe { LockWorkStation() }.map_err(|_| ConsoleLockError::LockRejected)?;
    let deadline = Instant::now() + Duration::from_secs(5);

    loop {
      let current = observe_console().map_err(|_| ConsoleLockError::Unverified)?.ok_or(ConsoleLockError::StaleSession)?;

      if !target.same_login(&current) {
        return Err(ConsoleLockError::StaleSession);
      }

      if current.lock_state == ConsoleLockState::Locked {
        return Ok(current);
      }

      if Instant::now() >= deadline {
        return Err(ConsoleLockError::Unverified);
      }

      thread::sleep(Duration::from_millis(100));
    }
  }

  fn verify_worker(target: &ConsoleSession) -> Result<(), ConsoleUnlockError> {
    if target.session_id == 0 {
      return Err(ConsoleUnlockError::WorkerIdentity);
    }

    verify_local_system_process_in_session(target.session_id)
  }

  pub(super) fn verify_local_system_process_in_session(session_id: u32) -> Result<(), ConsoleUnlockError> {
    let mut process_session = 0u32;
    // SAFETY: The out-pointer remains valid for this call.
    unsafe { ProcessIdToSessionId(GetCurrentProcessId(), &mut process_session) }.map_err(|_| ConsoleUnlockError::WorkerIdentity)?;

    if process_session != session_id {
      return Err(ConsoleUnlockError::WorkerIdentity);
    }

    let mut raw_token = HANDLE::default();
    // SAFETY: GetCurrentProcess is a valid pseudo handle; the returned token
    // is an owned kernel handle closed by TokenHandle.
    unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw_token) }.map_err(|_| ConsoleUnlockError::WorkerIdentity)?;
    let token = TokenHandle(raw_token);
    let is_system = with_token_user_sid(token.0, |sid| {
      // SAFETY: The SID lives in the token information buffer for this call.
      Ok(unsafe { IsWellKnownSid(sid, WinLocalSystemSid) }.as_bool())
    })
    .map_err(|_| ConsoleUnlockError::WorkerIdentity)?;

    if !is_system {
      return Err(ConsoleUnlockError::WorkerIdentity);
    }

    Ok(())
  }

  fn require_locked_target(target: &ConsoleSession) -> Result<(), ConsoleUnlockError> {
    let current = observe_console().map_err(|_| ConsoleUnlockError::Unverified)?.ok_or(ConsoleUnlockError::StaleSession)?;

    if !target.same_login(&current) {
      return Err(ConsoleUnlockError::StaleSession);
    }

    match current.lock_state {
      ConsoleLockState::Locked => Ok(()),
      ConsoleLockState::Usable => Err(ConsoleUnlockError::NotLocked),
      ConsoleLockState::Unknown => Err(ConsoleUnlockError::UnknownState),
    }
  }

  fn input_desktop_name() -> Result<String, ConsoleUnlockError> {
    // SAFETY: The current thread opens the current input desktop. This call
    // does not change its desktop; DesktopHandle owns the returned handle.
    let desktop = DesktopHandle(
      unsafe { OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, LIMITED_DESKTOP_ACCESS) }
        .map_err(|_| ConsoleUnlockError::DesktopUnavailable)?,
    );
    desktop_name(desktop.0)
  }

  fn desktop_name(desktop: HDESK) -> Result<String, ConsoleUnlockError> {
    let mut units = [0u16; 64];
    let mut needed = 0u32;
    // SAFETY: The buffer is live and aligned for UTF-16; its byte size is
    // passed to Win32, which writes at most that many bytes.
    unsafe {
      GetUserObjectInformationW(HANDLE(desktop.0), UOI_NAME, Some(units.as_mut_ptr().cast()), size_of_val(&units) as u32, Some(&mut needed))
    }
    .map_err(|_| ConsoleUnlockError::DesktopUnavailable)?;

    if needed == 0 || needed as usize > size_of_val(&units) || needed % 2 != 0 {
      return Err(ConsoleUnlockError::DesktopUnavailable);
    }

    let end = units.iter().position(|unit| *unit == 0).ok_or(ConsoleUnlockError::DesktopUnavailable)?;
    String::from_utf16(&units[..end]).map_err(|_| ConsoleUnlockError::DesktopUnavailable)
  }

  fn with_bound_desktop<T: Send>(
    target: &ConsoleSession,
    expected: &'static str,
    action: impl FnOnce() -> Result<T, ConsoleUnlockError> + Send,
  ) -> Result<T, ConsoleUnlockError> {
    // NOTICE: `docs/notes/neko/auv-windows-system-pin-rebind-worker.ps1`
    // used this access mask for successful locked-console input. The same
    // login rejected Return with 0x81 and accepted it with 0x000F01FF in the
    // reviewed A/B gate (`docs/ai/references/session-api/2026-09-28-windows-locked-session-host-handoff.md`).
    // Keep read-only observation narrow; reduce this delivery mask only after
    // a narrower mask passes a locked-console SendInput receipt gate.
    with_bound_desktop_access(target, expected, INPUT_DELIVERY_DESKTOP_ACCESS, action)
  }

  fn with_bound_desktop_access<T: Send>(
    target: &ConsoleSession,
    expected: &'static str,
    access: DESKTOP_ACCESS_FLAGS,
    action: impl FnOnce() -> Result<T, ConsoleUnlockError> + Send,
  ) -> Result<T, ConsoleUnlockError> {
    // SetThreadDesktop may fail after a thread has created any window or hook.
    // Each phase therefore gets its own short-lived thread; Return can change
    // the input desktop before the credential phase begins.
    thread::scope(|scope| {
      scope
        .spawn(move || {
          let desktop = DesktopHandle(
            unsafe {
              // SAFETY: This fresh thread opens and owns the current input
              // desktop; the guard closes it after the binding is restored.
              OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, access)
            }
            .map_err(|_| ConsoleUnlockError::DesktopUnavailable)?,
          );

          if !desktop_name(desktop.0)?.eq_ignore_ascii_case(expected) {
            return Err(ConsoleUnlockError::DesktopUnavailable);
          }

          // SAFETY: GetThreadDesktop reads this new thread's current desktop.
          let original = unsafe { GetThreadDesktop(GetCurrentThreadId()) }.map_err(|_| ConsoleUnlockError::DesktopUnavailable)?;
          // SAFETY: This thread has not created windows or hooks. The opened
          // handle remains live until it is restored and dropped.
          unsafe { SetThreadDesktop(desktop.0) }.map_err(|_| ConsoleUnlockError::DesktopUnavailable)?;
          let _binding = BoundDesktop {
            original,
            _opened: desktop,
          };
          require_locked_target(target)?;
          action()
        })
        .join()
        .map_err(|_| ConsoleUnlockError::Unverified)?
    })
  }

  fn send_return() -> Result<(), ConsoleUnlockError> {
    let mut events = vec![return_event(false), return_event(true)];
    send_and_clear(&mut events)
  }

  fn return_event(up: bool) -> INPUT {
    INPUT {
      r#type: INPUT_KEYBOARD,
      Anonymous: INPUT_0 {
        ki: KEYBDINPUT {
          wVk: VK_RETURN,
          wScan: 0,
          dwFlags: if up {
            KEYEVENTF_KEYUP
          } else {
            Default::default()
          },
          time: 0,
          dwExtraInfo: 0,
        },
      },
    }
  }

  fn send_credential(units: &[u16]) -> Result<(), ConsoleUnlockError> {
    let mut events = Vec::with_capacity(units.len() * 2 + 2);

    for unit in units {
      let event = |up| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
          ki: KEYBDINPUT {
            wVk: VIRTUAL_KEY(0),
            wScan: *unit,
            dwFlags: KEYEVENTF_UNICODE
              | if up {
                KEYEVENTF_KEYUP
              } else {
                Default::default()
              },
            time: 0,
            dwExtraInfo: 0,
          },
        },
      };

      events.push(event(false));
      events.push(event(true));
    }

    events.push(INPUT {
      r#type: INPUT_KEYBOARD,
      Anonymous: INPUT_0 {
        ki: KEYBDINPUT {
          wVk: VK_RETURN,
          wScan: 0,
          dwFlags: Default::default(),
          time: 0,
          dwExtraInfo: 0,
        },
      },
    });
    events.push(INPUT {
      r#type: INPUT_KEYBOARD,
      Anonymous: INPUT_0 {
        ki: KEYBDINPUT {
          wVk: VK_RETURN,
          wScan: 0,
          dwFlags: KEYEVENTF_KEYUP,
          time: 0,
          dwExtraInfo: 0,
        },
      },
    });
    send_and_clear(&mut events)
  }

  fn send_and_clear(events: &mut Vec<INPUT>) -> Result<(), ConsoleUnlockError> {
    // SAFETY: SendInput reads the initialized contiguous INPUT slice for the
    // call duration. A short count is not evidence of credential rejection.
    let sent = unsafe { SendInput(events, size_of::<INPUT>() as i32) } as usize;
    // INPUT has no destructor. Wipe event copies of credential code units
    // before releasing the allocation, even when SendInput rejects a batch.
    // SAFETY: The Vec owns this writable allocation. Volatile byte writes
    // clear even any padding without forming a reference to uninitialized
    // padding bytes, and cannot be optimized out as a dead store.
    let bytes = size_of_val(events.as_slice());
    let pointer = events.as_mut_ptr().cast::<u8>();

    for offset in 0..bytes {
      unsafe { pointer.add(offset).write_volatile(0) };
    }

    if sent == events.len() {
      Ok(())
    } else {
      Err(ConsoleUnlockError::InputRejected)
    }
  }

  fn query_account_sid(session_id: u32) -> Result<String, ConsoleSessionError> {
    let mut raw_token = HANDLE::default();
    // SAFETY: The out-pointer lives through the call. A successful call gives
    // us one owned handle, closed by TokenHandle.
    unsafe { WTSQueryUserToken(session_id, &mut raw_token) }.map_err(|_| ConsoleSessionError::IdentityUnverified)?;
    let token = TokenHandle(raw_token);

    with_token_user_sid(token.0, |sid| {
      let mut raw_sid = PWSTR::null();
      // SAFETY: The SID pointer is valid while with_token_user_sid holds its
      // GetTokenInformation buffer; this call allocates raw_sid with LocalAlloc.
      unsafe { ConvertSidToStringSidW(sid, &mut raw_sid) }.map_err(|_| ConsoleSessionError::IdentityUnverified)?;

      if raw_sid.is_null() {
        return Err(ConsoleSessionError::IdentityUnverified);
      }

      // SAFETY: ConvertSidToStringSidW returned a null-terminated UTF-16 string.
      let sid = unsafe { raw_sid.to_string() }.map_err(|_| ConsoleSessionError::IdentityUnverified);
      // SAFETY: LocalFree consumes the returned allocation exactly once.
      unsafe { LocalFree(HLOCAL(raw_sid.0.cast::<c_void>())) };
      sid
    })
  }

  fn with_token_user_sid<T>(
    token: HANDLE,
    use_sid: impl FnOnce(windows::Win32::Security::PSID) -> Result<T, ConsoleSessionError>,
  ) -> Result<T, ConsoleSessionError> {
    let mut bytes = 0u32;
    // SAFETY: A null output buffer requests the required TOKEN_USER size.
    let _ = unsafe { GetTokenInformation(token, TokenUser, None, 0, &mut bytes) };

    if bytes < size_of::<TOKEN_USER>() as u32 {
      return Err(ConsoleSessionError::IdentityUnverified);
    }

    // GetTokenInformation writes a TOKEN_USER header before the variable SID.
    // A byte Vec only promises alignment 1, so allocate pointer-sized words.
    if align_of::<TOKEN_USER>() > align_of::<usize>() {
      return Err(ConsoleSessionError::IdentityUnverified);
    }

    let mut data = vec![0usize; (bytes as usize).div_ceil(size_of::<usize>())];
    // SAFETY: The word buffer is aligned for TOKEN_USER and has at least the
    // queried byte capacity. Its embedded SID pointer remains live until drop.
    unsafe { GetTokenInformation(token, TokenUser, Some(data.as_mut_ptr().cast()), bytes, &mut bytes) }
      .map_err(|_| ConsoleSessionError::IdentityUnverified)?;

    if (bytes as usize) < size_of::<TOKEN_USER>() {
      return Err(ConsoleSessionError::IdentityUnverified);
    }

    // SAFETY: The buffer contains a complete, aligned TOKEN_USER and the SID
    // pointer is only used while this allocation is alive.
    let token_user = unsafe { data.as_ptr().cast::<TOKEN_USER>().read() };

    if token_user.User.Sid.0.is_null() {
      return Err(ConsoleSessionError::IdentityUnverified);
    }

    use_sid(token_user.User.Sid)
  }
}

#[cfg(not(target_os = "windows"))]
mod native {
  use super::{ConsoleLockError, ConsoleSession, ConsoleSessionError, ConsoleUnlockError};

  pub(super) fn observe_console() -> Result<Option<ConsoleSession>, ConsoleSessionError> {
    Err(ConsoleSessionError::UnsupportedPlatform)
  }

  pub(super) fn unlock_existing_session(_: &ConsoleSession, _: &str) -> Result<ConsoleSession, ConsoleUnlockError> {
    Err(ConsoleUnlockError::UnsupportedPlatform)
  }

  pub(super) fn lock_existing_session(_: &ConsoleSession) -> Result<ConsoleSession, ConsoleLockError> {
    Err(ConsoleLockError::UnsupportedPlatform)
  }
}

#[cfg(test)]
mod tests {
  use super::{ConsoleLockError, ConsoleLockState, ConsoleSession, require_same_usable_login};

  #[test]
  fn lock_requires_the_same_usable_login() {
    let target = ConsoleSession {
      session_id: 5,
      logon_time: 123,
      account_sid: "S-1-5-21-100".into(),
      domain: "PC".into(),
      user: "owner".into(),
      lock_state: ConsoleLockState::Usable,
    };

    assert_eq!(require_same_usable_login(&target, &target), Ok(()));
    assert_eq!(
      require_same_usable_login(
        &target,
        &ConsoleSession {
          logon_time: 124,
          ..target.clone()
        }
      ),
      Err(ConsoleLockError::StaleSession)
    );
    assert_eq!(
      require_same_usable_login(
        &target,
        &ConsoleSession {
          lock_state: ConsoleLockState::Locked,
          ..target.clone()
        }
      ),
      Err(ConsoleLockError::AlreadyLocked)
    );
    assert_eq!(
      require_same_usable_login(
        &target,
        &ConsoleSession {
          lock_state: ConsoleLockState::Unknown,
          ..target.clone()
        }
      ),
      Err(ConsoleLockError::UnknownState)
    );
  }

  #[test]
  fn selector_and_login_identity_reject_reused_session_id() {
    let session = ConsoleSession {
      session_id: 1,
      logon_time: 42,
      account_sid: "S-1-5-21-1".into(),
      domain: "HOST".into(),
      user: "alice".into(),
      lock_state: ConsoleLockState::Locked,
    };

    assert_eq!(session.selector(), "windows-console:1:42");

    let mut current = session.clone();
    current.lock_state = ConsoleLockState::Usable;

    assert!(session.same_login(&current));

    current.logon_time += 1;

    assert!(!session.same_login(&current));

    current.logon_time -= 1;
    current.account_sid = "S-1-5-21-2".into();

    assert!(!session.same_login(&current));
  }
}
