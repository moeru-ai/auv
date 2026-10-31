//! Window-bound input admission, native submission receipts, and foreground lifetime.

#[cfg(target_os = "windows")]
mod native {
  use std::cell::{Cell, RefCell};
  use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
  use std::sync::atomic::{AtomicBool, Ordering};
  use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, mpsc};
  use std::thread::{self, JoinHandle};
  use std::time::{Duration, Instant};

  use auv_driver_common::input::{DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath};
  use auv_driver_common::window::Window;
  use auv_driver_common::{DriverError, DriverResult, InputActionProgress};
  use windows::Win32::Foundation::{HANDLE, HMODULE, HWND, LPARAM, WAIT_TIMEOUT, WPARAM};
  use windows::Win32::System::Threading::{
    GetCurrentThreadId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
  };
  use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
  use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LBUTTON, VK_LWIN, VK_MBUTTON, VK_MENU, VK_RBUTTON, VK_RWIN, VK_SHIFT, VK_XBUTTON1, VK_XBUTTON2,
  };
  use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EVENT_OBJECT_DESTROY, EVENT_SYSTEM_FOREGROUND, GetForegroundWindow, GetMessageW, IsWindowVisible, MSG, PM_NOREMOVE,
    PeekMessageW, PostThreadMessageW, TranslateMessage, WINEVENT_OUTOFCONTEXT, WM_QUIT,
  };

  use crate::error::{backend, invalid_input};
  use crate::window::{Identity, probe_window, window_handle};

  static SUBMISSIONS: Mutex<()> = Mutex::new(());
  thread_local! {
    static CURRENT: RefCell<Option<Arc<State>>> = const { RefCell::new(None) };
    static WATCHED: RefCell<Option<Arc<State>>> = const { RefCell::new(None) };
    static RESERVED: Cell<bool> = const { Cell::new(false) };
    static PREPARING: Cell<bool> = const { Cell::new(false) };
    static SCROLL_STEPS: Cell<bool> = const { Cell::new(false) };
  }

  fn current() -> Option<Arc<State>> {
    CURRENT.with(|value| value.borrow().clone())
  }

  pub(in super::super) struct Reservation {
    _gate: MutexGuard<'static, ()>,
  }
  impl Reservation {
    fn acquire(preparing: bool) -> DriverResult<Self> {
      if RESERVED.with(|value| value.replace(true)) {
        return Err(invalid_input("nested input preparation is unsupported"));
      }
      let gate = SUBMISSIONS.lock().unwrap_or_else(PoisonError::into_inner);
      PREPARING.with(|value| value.set(preparing));
      Ok(Self { _gate: gate })
    }
  }
  impl Drop for Reservation {
    fn drop(&mut self) {
      PREPARING.with(|value| value.set(false));
      RESERVED.with(|value| value.set(false));
    }
  }

  struct State {
    hwnd: isize,
    identity: Identity,
    process: OwnedHandle,
    lost: Mutex<Option<String>>,
    changed: Condvar,
    stop: AtomicBool,
    // Posting and worker exit share this lock, so a recycled thread id is never messaged.
    watcher_id: Mutex<u32>,
    /// The action template remains available before the first native call.
    receipt: Mutex<(InputActionResult, InputActionProgress)>,
  }

  impl State {
    fn lose(&self, message: String) {
      let mut lost = self.lost.lock().unwrap_or_else(PoisonError::into_inner);
      if lost.is_none() {
        *lost = Some(message);
      }
      self.changed.notify_all();
    }

    fn error(&self, message: impl Into<String>) -> DriverError {
      DriverError::TargetLost {
        target: format!("window:{}", self.hwnd),
        message: message.into(),
      }
    }

    fn check(&self) -> DriverResult<()> {
      if auv_driver_common::input_cancellation::current_input_cancellation().is_some_and(|signal| signal.is_cancelled()) {
        return Err(invalid_input("window input cancelled before further delivery"));
      }
      if let Some(message) = self.lost.lock().unwrap_or_else(PoisonError::into_inner).clone() {
        return Err(self.error(message));
      }
      let probe = probe_window(self.hwnd);
      // SAFETY: OwnedHandle retains this exact process object; the zero-timeout wait never blocks.
      let process_alive = unsafe { WaitForSingleObject(HANDLE(self.process.as_raw_handle()), 0) == WAIT_TIMEOUT };
      let problem = if !process_alive {
        Some("original process exited".into())
      } else if let Some(reason) = self.identity.lost_reason(&probe) {
        Some(format!("window identity changed: {reason:?}"))
      } else if probe.minimized {
        Some("window is minimized".into())
      } else if probe.cloaked {
        Some("window is cloaked".into())
      }
      // SAFETY: read-only queries of a handle whose ownership is revalidated above.
      else if unsafe { !IsWindowVisible(HWND(self.hwnd as _)).as_bool() } {
        Some("window is hidden".into())
      } else if unsafe { GetForegroundWindow() }.0 as isize != self.hwnd {
        Some("foreground no longer belongs to the admitted window".into())
      } else {
        None
      };
      if let Some(problem) = problem {
        self.lose(problem.clone());
        return Err(self.error(problem));
      }
      // A callback can invalidate the guard while the synchronous queries run.
      if let Some(message) = self.lost.lock().unwrap_or_else(PoisonError::into_inner).clone() {
        return Err(self.error(message));
      }
      Ok(())
    }

    fn stamp(&self, cause: DriverError) -> DriverError {
      if matches!(cause, DriverError::InputAction { .. }) {
        return cause;
      }
      DriverError::InputAction {
        cause: Box::new(cause),
        progress: self.receipt.lock().unwrap_or_else(PoisonError::into_inner).1.clone(),
      }
    }
  }

  unsafe extern "system" fn event(_hook: HWINEVENTHOOK, kind: u32, hwnd: HWND, object: i32, child: i32, _thread: u32, _time: u32) {
    // Clone before any native query: no RefCell borrow or mutex spans a reentrant OS call.
    let state = WATCHED.with(|value| value.borrow().clone());
    let Some(state) = state else { return };
    if kind == EVENT_OBJECT_DESTROY && hwnd.0 as isize == state.hwnd && object == 0 && child == 0 {
      state.lose("admitted root window was destroyed".into());
    } else if kind == EVENT_SYSTEM_FOREGROUND {
      // SAFETY: read-only query; the event's named HWND is never dereferenced.
      let actual = unsafe { GetForegroundWindow() }.0 as isize;
      if hwnd.0 as isize != state.hwnd || actual != state.hwnd {
        state.lose(format!("foreground history invalidated the recipient (named={}, current={actual})", hwnd.0 as isize));
      }
    }
  }

  /// OS registration boundary; handles stay on their registering worker.
  trait Hooks: Send + 'static {
    fn prepare(&self) -> Result<(), String>;
    fn register(&self, event_id: u32) -> HWINEVENTHOOK;
    fn remove(&self, hook: HWINEVENTHOOK) -> bool;
  }

  struct NativeHooks;

  impl Hooks for NativeHooks {
    fn prepare(&self) -> Result<(), String> {
      crate::desktop::try_ensure_input_desktop().map_err(|error| error.to_string())
    }

    fn register(&self, event_id: u32) -> HWINEVENTHOOK {
      // SAFETY: callback has the required ABI. WATCHED retains state on this
      // registering thread until the registrations are removed and it exits.
      unsafe { SetWinEventHook(event_id, event_id, HMODULE::default(), Some(event), 0, 0, WINEVENT_OUTOFCONTEXT) }
    }

    fn remove(&self, hook: HWINEVENTHOOK) -> bool {
      // SAFETY: both live registrations are removed on their registering thread.
      unsafe { UnhookWinEvent(hook).as_bool() }
    }
  }

  /// Registration and unhooking stay on this worker; its owner always joins it.
  struct Watcher {
    state: Arc<State>,
    thread: Option<JoinHandle<Result<(), String>>>,
  }

  impl Watcher {
    fn start(state: Arc<State>, hooks: impl Hooks) -> DriverResult<Self> {
      let (ready, receiver) = mpsc::channel();
      let worker_state = state.clone();
      let thread = thread::Builder::new()
        .name("auv-keyboard-recipient".into())
        .spawn(move || {
          let result = (|| -> Result<(), String> {
            hooks.prepare()?;
            WATCHED.with(|value| *value.borrow_mut() = Some(worker_state.clone()));
            let foreground = hooks.register(EVENT_SYSTEM_FOREGROUND);
            if foreground.0.is_null() {
              return Err("SetWinEventHook foreground failed".into());
            }
            // Only root-window destruction is accepted by the callback.
            let destroy = hooks.register(EVENT_OBJECT_DESTROY);
            if destroy.0.is_null() {
              // Release the first hook on its registering thread before reporting failure.
              if !hooks.remove(foreground) {
                return Err("SetWinEventHook destruction failed; foreground cleanup failure".into());
              }
              return Err("SetWinEventHook destruction failed".into());
            }
            let mut message = MSG::default();
            // SAFETY: initialize this thread's queue before publishing its id to the shutdown path.
            unsafe {
              let _ = PeekMessageW(&mut message, None, 0, 0, PM_NOREMOVE);
            }
            let id = unsafe { GetCurrentThreadId() };
            *worker_state.watcher_id.lock().unwrap_or_else(PoisonError::into_inner) = id;
            let listening = ready.send(Ok(id)).is_ok();
            let mut loop_error = None;
            while listening && !worker_state.stop.load(Ordering::Acquire) {
              // SAFETY: writable message and this thread's own queue.
              let received = unsafe { GetMessageW(&mut message, None, 0, 0) }.0;
              if received <= 0 {
                if received < 0 || !worker_state.stop.load(Ordering::Acquire) {
                  loop_error = Some("recipient message loop ended before batch shutdown".to_string());
                }
                break;
              }
              unsafe {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
              }
            }
            // Both calls must run even when the first reports a cleanup failure.
            let unhooked = hooks.remove(destroy) & hooks.remove(foreground);
            if !unhooked {
              return Err("recipient hook cleanup failed".into());
            }
            if let Some(error) = loop_error {
              return Err(error);
            }
            Ok(())
          })();
          if let Err(error) = &result {
            worker_state.lose(error.clone());
            let _ = ready.send(Err(error.clone()));
          }
          *worker_state.watcher_id.lock().unwrap_or_else(PoisonError::into_inner) = 0;
          WATCHED.with(|value| value.borrow_mut().take());
          result
        })
        .map_err(|error| backend(format!("start recipient watcher: {error}")))?;
      let watcher = Self {
        state,
        thread: Some(thread),
      };
      match receiver.recv_timeout(Duration::from_secs(5)) {
        Ok(Ok(_)) => Ok(watcher),
        Ok(Err(error)) => Err(backend(error)),
        Err(error) => Err(backend(format!("recipient watcher did not become ready: {error}"))),
      }
      // On every failed start `watcher` drops here, stopping and joining even a late installer.
    }
  }

  impl Drop for Watcher {
    fn drop(&mut self) {
      self.state.stop.store(true, Ordering::Release);
      {
        let id = self.state.watcher_id.lock().unwrap_or_else(PoisonError::into_inner);
        if *id != 0 {
          unsafe {
            let _ = PostThreadMessageW(*id, WM_QUIT, WPARAM(0), LPARAM(0));
          }
        }
      }
      if let Some(thread) = self.thread.take() {
        match thread.join() {
          Ok(Ok(())) => {}
          // NOTICE: the joined worker has exited. Windows also removes its hooks
          // on thread exit; see the remarks for `UnhookWinEvent`:
          // https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-unhookwinevent
          // TODO(windows-keyboard-recipient): structured cleanup reporting needs
          // an owner-approved run-record consumer; InputActionResult covers submissions.
          result => eprintln!("recipient watcher ended with a cleanup failure: {result:?}"),
        }
      }
    }
  }

  pub(in super::super) struct Guard {
    state: Arc<State>,
    _watcher: Watcher,
    // The gate outlives watcher shutdown and is never detached from the input worker.
    _reservation: Reservation,
  }

  impl Guard {
    pub(in super::super) fn open(window: &Window) -> DriverResult<Self> {
      if current().is_some() {
        return Err(invalid_input("nested window input batches are unsupported"));
      }
      let reservation = Reservation::acquire(true)?;
      crate::desktop::try_ensure_input_desktop().map_err(|error| backend(error.to_string()))?;
      let hwnd = window_handle(window)?.0 as isize;
      let probe = probe_window(hwnd);
      // TODO(windows-input-reference): Window carries the observed PID, not a window-generation
      // receipt. Same-owner reuse before admission needs a versioned producer/consumer contract;
      // this guard latches root destruction and rechecks identity only during its own batch.
      // A foreign held modifier must not silently affect a new protected batch. Explicit
      // cross-call keyboard holds remain outside this recipient contract.
      if [
        VK_SHIFT,
        VK_CONTROL,
        VK_MENU,
        VK_LWIN,
        VK_RWIN,
        VK_LBUTTON,
        VK_RBUTTON,
        VK_MBUTTON,
        VK_XBUTTON1,
        VK_XBUTTON2,
      ]
      .iter()
      .any(|key| unsafe { GetAsyncKeyState(i32::from(key.0)) < 0 })
      {
        return Err(invalid_input("release held modifiers and mouse buttons before a window input batch"));
      }
      if !probe.exists
        || window.process_id != Some(probe.process_id)
        || probe.process_id == 0
        || probe.thread_id == 0
        || probe.class_name.is_empty()
      {
        return Err(DriverError::TargetLost {
          target: format!("window:{hwnd}"),
          message: "observed owner does not match a live window".into(),
        });
      }
      if probe.minimized || probe.cloaked || unsafe { !IsWindowVisible(HWND(hwnd as _)).as_bool() } {
        return Err(DriverError::TargetLost {
          target: format!("window:{hwnd}"),
          message: "window is minimized, hidden, or cloaked".into(),
        });
      }
      // SAFETY: OpenProcess returns one owned kernel handle, transferred exactly once to OwnedHandle.
      let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, probe.process_id) }
        .map_err(|error| backend(format!("retain recipient process: {error}")))?;
      let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
      let state = Arc::new(State {
        hwnd,
        identity: Identity::from_probe(&probe),
        process,
        lost: Mutex::new(None),
        changed: Condvar::new(),
        stop: AtomicBool::new(false),
        watcher_id: Mutex::new(0),
        receipt: Mutex::new((
          super::super::foreground_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None),
          InputActionProgress::NotDelivered,
        )),
      });
      if auv_driver_common::input_cancellation::current_input_cancellation().is_some_and(|signal| signal.is_cancelled()) {
        return Err(invalid_input("window input cancelled before activation"));
      }
      if unsafe { GetForegroundWindow() }.0 as isize != hwnd {
        crate::window::activate_window(window)?;
      }
      let watcher = Watcher::start(state.clone(), NativeHooks)?;
      let guard = Self {
        state,
        _watcher: watcher,
        _reservation: reservation,
      };
      guard.state.check()?;
      CURRENT.with(|value| *value.borrow_mut() = Some(guard.state.clone()));
      PREPARING.with(|value| value.set(false));
      Ok(guard)
    }

    pub(in super::super) fn begin_action(&self, mouse: DisturbanceLevel, clipboard: DisturbanceLevel) {
      let mut action = super::super::foreground_result(mouse, DisturbanceLevel::Unknown, clipboard);
      if clipboard == DisturbanceLevel::Temporary {
        action.selected_path = InputDeliveryPath::ClipboardPaste;
        action.attempts = vec![InputAttempt::success(InputDeliveryPath::ClipboardPaste)];
      }
      *self.state.receipt.lock().unwrap_or_else(PoisonError::into_inner) = (action, InputActionProgress::NotDelivered);
    }
    pub(in super::super) fn stamp(&self, cause: DriverError) -> DriverError {
      self.state.stamp(cause)
    }
    pub(in super::super) fn check(&self) -> DriverResult<()> {
      self.state.check()
    }
  }

  impl Drop for Guard {
    fn drop(&mut self) {
      PREPARING.with(|value| value.set(true));
      CURRENT.with(|value| value.borrow_mut().take());
    }
  }

  /// A complete scroll alone may reuse its recipient for its own samples.
  /// Other nested input batches continue to require a new admission and fail.
  pub(in super::super) fn with_scroll_steps<T>(action: impl FnOnce() -> T) -> T {
    struct RestoreScrollSteps(bool);
    impl Drop for RestoreScrollSteps {
      fn drop(&mut self) {
        SCROLL_STEPS.with(|value| value.set(self.0));
      }
    }
    debug_assert!(current().is_some());
    let _restore = RestoreScrollSteps(SCROLL_STEPS.with(|value| value.replace(true)));
    action()
  }
  pub(in super::super) fn scroll_batch_active() -> bool {
    SCROLL_STEPS.with(Cell::get)
  }
  pub(in super::super) fn reuse_scroll_recipient(window: &Window) -> DriverResult<bool> {
    if !scroll_batch_active() {
      return Ok(false);
    }
    let state = current().ok_or_else(|| invalid_input("scroll batch has no admitted recipient"))?;
    if window.reference.id.parse::<isize>().ok() != Some(state.hwnd) || window.process_id != Some(state.identity.process_id) {
      return Err(state.stamp(invalid_input("scroll sample cannot change its admitted recipient")));
    }
    state.check().map_err(|cause| state.stamp(cause))?;
    Ok(true)
  }
  pub(in super::super) fn stamp_current(cause: DriverError) -> DriverError {
    match current() {
      Some(state) => state.stamp(cause),
      None => cause,
    }
  }

  /// Other foreground primitives wait until a protected batch releases its input worker.
  pub(in super::super) fn reserve_submission() -> DriverResult<Option<Reservation>> {
    if RESERVED.with(Cell::get) {
      if PREPARING.with(Cell::get) {
        return Err(invalid_input("native input refused during recipient preparation or shutdown"));
      }
      return Ok(None);
    }
    Reservation::acquire(false).map(Some)
  }
  pub(in super::super) fn check() -> DriverResult<()> {
    if auv_driver_common::input_cancellation::current_input_cancellation().is_some_and(|signal| signal.is_cancelled()) {
      return Err(invalid_input("input cancelled before native submission"));
    }
    current().map_or(Ok(()), |state| state.check())
  }
  pub(in super::super) fn before_submission() {
    if let Some(state) = current() {
      let mut receipt = state.receipt.lock().unwrap_or_else(PoisonError::into_inner);
      let mut action = receipt.0.clone();
      action.attempts = vec![InputAttempt::failure(
        action.selected_path,
        "native submission may have partial effects",
      )];
      receipt.1 = InputActionProgress::DeliveryUncertain { action };
    }
  }
  pub(in super::super) fn after_submission(accepted: bool) {
    if accepted && let Some(state) = current() {
      let mut receipt = state.receipt.lock().unwrap_or_else(PoisonError::into_inner);
      let mut action = receipt.0.clone();
      action.attempts[0].message = Some("native input accepted; this may be only a prefix of the requested action".into());
      receipt.1 = InputActionProgress::Submitted { action };
    }
  }
  pub(in super::super) fn wait(duration: Duration) -> DriverResult<()> {
    let deadline =
      Instant::now().checked_add(duration).ok_or_else(|| super::super::invalid_input("input wait exceeds the native clock range"))?;
    let Some(state) = current() else {
      if !duration.is_zero() {
        thread::sleep(duration);
      }
      return Ok(());
    };
    loop {
      state.check()?;
      let remaining = deadline.saturating_duration_since(Instant::now());
      if remaining.is_zero() {
        return Ok(());
      }
      let lost = state.lost.lock().unwrap_or_else(PoisonError::into_inner);
      // The condition variable wakes immediately on recipient loss. Poll only transport
      // cancellation, whose existing wake registrations are private to auv-driver-common.
      let _ = state.changed.wait_timeout(lost, remaining.min(Duration::from_millis(25))).unwrap_or_else(PoisonError::into_inner);
    }
  }

  #[cfg(test)]
  mod tests {
    use super::*;

    #[derive(Default)]
    struct Calls {
      entries: Mutex<Vec<(&'static str, u32, thread::ThreadId)>>,
      dropped: AtomicBool,
    }

    struct FakeHooks {
      calls: Arc<Calls>,
      fail_register: u32,
      fail_remove: u32,
    }

    impl Hooks for FakeHooks {
      fn prepare(&self) -> Result<(), String> {
        // No desktop switch or real event registration is needed by this model.
        Ok(())
      }

      fn register(&self, event_id: u32) -> HWINEVENTHOOK {
        self.calls.entries.lock().unwrap().push(("register", event_id, thread::current().id()));
        if event_id == self.fail_register {
          HWINEVENTHOOK::default()
        } else {
          // Opaque fixture IDs are consumed only by FakeHooks::remove.
          HWINEVENTHOOK(event_id as usize as *mut _)
        }
      }

      fn remove(&self, hook: HWINEVENTHOOK) -> bool {
        let event_id = hook.0 as usize as u32;
        self.calls.entries.lock().unwrap().push(("remove", event_id, thread::current().id()));
        event_id != self.fail_remove
      }
    }

    impl Drop for FakeHooks {
      fn drop(&mut self) {
        self.calls.dropped.store(true, Ordering::Release);
      }
    }

    fn fixture(fail_register: u32, fail_remove: u32) -> (Arc<State>, FakeHooks, Arc<Calls>) {
      // State owns a real handle to this process, without an application window.
      // The hook model never calls State::check or installs a native callback.
      let process_id = std::process::id();
      // SAFETY: one owned process handle is transferred exactly once to OwnedHandle.
      let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, false, process_id) }.unwrap();
      let process = unsafe { OwnedHandle::from_raw_handle(process.0) };
      let state = Arc::new(State {
        hwnd: 0,
        identity: Identity {
          process_id,
          thread_id: 0,
          class_name: String::new(),
        },
        process,
        lost: Mutex::new(None),
        changed: Condvar::new(),
        stop: AtomicBool::new(false),
        watcher_id: Mutex::new(0),
        receipt: Mutex::new((
          crate::input::foreground_result(DisturbanceLevel::None, DisturbanceLevel::Unknown, DisturbanceLevel::None),
          InputActionProgress::NotDelivered,
        )),
      });
      let calls = Arc::new(Calls::default());
      let hooks = FakeHooks {
        calls: calls.clone(),
        fail_register,
        fail_remove,
      };
      (state, hooks, calls)
    }

    fn joined_calls(state: &State, calls: &Calls) -> Vec<(&'static str, u32)> {
      assert!(state.stop.load(Ordering::Acquire));
      assert_eq!(*state.watcher_id.lock().unwrap(), 0);
      assert!(calls.dropped.load(Ordering::Acquire));
      let entries = calls.entries.lock().unwrap();
      let owner = entries[0].2;
      assert_ne!(owner, thread::current().id());
      assert!(entries.iter().all(|entry| entry.2 == owner));
      entries.iter().map(|entry| (entry.0, entry.1)).collect()
    }

    #[test]
    fn foreground_registration_failure_joins_worker() {
      let (state, hooks, calls) = fixture(EVENT_SYSTEM_FOREGROUND, 0);
      let error = Watcher::start(state.clone(), hooks).err().expect("registration must fail");
      assert!(error.to_string().contains("SetWinEventHook foreground failed"));
      assert_eq!(joined_calls(&state, &calls), [("register", EVENT_SYSTEM_FOREGROUND)]);
    }

    #[test]
    fn destroy_registration_failure_releases_first_hook_and_joins() {
      let (state, hooks, calls) = fixture(EVENT_OBJECT_DESTROY, 0);
      let error = Watcher::start(state.clone(), hooks).err().expect("registration must fail");
      assert!(error.to_string().contains("SetWinEventHook destruction failed"));
      assert_eq!(
        joined_calls(&state, &calls),
        [
          ("register", EVENT_SYSTEM_FOREGROUND),
          ("register", EVENT_OBJECT_DESTROY),
          ("remove", EVENT_SYSTEM_FOREGROUND),
        ]
      );
    }

    #[test]
    fn partial_start_cleanup_failure_is_retained_and_joins() {
      let (state, hooks, calls) = fixture(EVENT_OBJECT_DESTROY, EVENT_SYSTEM_FOREGROUND);
      let error = Watcher::start(state.clone(), hooks).err().expect("registration must fail");
      assert!(error.to_string().contains("foreground cleanup failure"));
      assert!(state.lost.lock().unwrap().as_ref().unwrap().contains("foreground cleanup failure"));
      assert_eq!(
        joined_calls(&state, &calls),
        [
          ("register", EVENT_SYSTEM_FOREGROUND),
          ("register", EVENT_OBJECT_DESTROY),
          ("remove", EVENT_SYSTEM_FOREGROUND),
        ]
      );
    }

    #[test]
    fn cleanup_failure_attempts_both_hooks_on_owner_and_joins() {
      let (state, hooks, calls) = fixture(0, EVENT_OBJECT_DESTROY);
      let watcher = Watcher::start(state.clone(), hooks).expect("registration succeeds");
      drop(watcher);
      assert!(state.lost.lock().unwrap().as_ref().unwrap().contains("recipient hook cleanup failed"));
      assert_eq!(
        joined_calls(&state, &calls),
        [
          ("register", EVENT_SYSTEM_FOREGROUND),
          ("register", EVENT_OBJECT_DESTROY),
          ("remove", EVENT_OBJECT_DESTROY),
          ("remove", EVENT_SYSTEM_FOREGROUND),
        ]
      );
    }

    #[test]
    fn normal_shutdown_removes_both_hooks_on_owner_and_joins() {
      let (state, hooks, calls) = fixture(0, 0);
      let watcher = Watcher::start(state.clone(), hooks).expect("registration succeeds");
      drop(watcher);
      assert!(state.lost.lock().unwrap().is_none());
      assert_eq!(
        joined_calls(&state, &calls),
        [
          ("register", EVENT_SYSTEM_FOREGROUND),
          ("register", EVENT_OBJECT_DESTROY),
          ("remove", EVENT_OBJECT_DESTROY),
          ("remove", EVENT_SYSTEM_FOREGROUND),
        ]
      );
    }
  }
}

#[cfg(target_os = "windows")]
pub(super) use native::{
  Guard, after_submission, before_submission, check, reserve_submission, reuse_scroll_recipient, scroll_batch_active, stamp_current, wait,
  with_scroll_steps,
};

#[cfg(not(target_os = "windows"))]
pub(super) struct Guard;
#[cfg(not(target_os = "windows"))]
impl Guard {
  pub(super) fn open(_window: &auv_driver_common::window::Window) -> auv_driver_common::DriverResult<Self> {
    Err(auv_driver_common::DriverError::unsupported("Windows window input recipient"))
  }
  pub(super) fn begin_action(
    &self,
    _mouse: auv_driver_common::input::DisturbanceLevel,
    _clipboard: auv_driver_common::input::DisturbanceLevel,
  ) {
  }
  pub(super) fn stamp(&self, cause: auv_driver_common::DriverError) -> auv_driver_common::DriverError {
    cause
  }
  pub(super) fn check(&self) -> auv_driver_common::DriverResult<()> {
    Err(auv_driver_common::DriverError::unsupported("Windows window input recipient"))
  }
}
#[cfg(not(target_os = "windows"))]
pub(super) fn with_scroll_steps<T>(action: impl FnOnce() -> T) -> T {
  action()
}
#[cfg(not(target_os = "windows"))]
pub(super) fn scroll_batch_active() -> bool {
  false
}
#[cfg(not(target_os = "windows"))]
pub(super) fn reuse_scroll_recipient(_window: &auv_driver_common::Window) -> auv_driver_common::DriverResult<bool> {
  Ok(false)
}
#[cfg(not(target_os = "windows"))]
pub(super) fn stamp_current(cause: auv_driver_common::DriverError) -> auv_driver_common::DriverError {
  cause
}
#[cfg(not(target_os = "windows"))]
pub(super) fn check() -> auv_driver_common::DriverResult<()> {
  Err(auv_driver_common::DriverError::unsupported("Windows window input recipient"))
}
#[cfg(not(target_os = "windows"))]
pub(super) fn wait(duration: std::time::Duration) -> auv_driver_common::DriverResult<()> {
  std::thread::sleep(duration);
  Ok(())
}
#[cfg(not(target_os = "windows"))]
pub(super) fn reserve_submission() -> auv_driver_common::DriverResult<Option<()>> {
  Ok(None)
}
