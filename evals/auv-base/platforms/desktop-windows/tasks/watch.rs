//! Independent witness of what happens to the user's desktop while an evaluation runs.
//!
//! The driver's own result says what it *believes* it did. This watches the OS instead:
//!
//! - **Injected input.** Low-level mouse and keyboard hooks see every event system-wide
//!   with an `INJECTED` flag set when it came from `SendInput`, `mouse_event` or `keybd_event`
//!   rather than hardware. A background delivery must leave this count at zero. Posted window
//!   messages (`PostMessage`) are not input events and never appear here. A cursor moved with
//!   `SetCursorPos` raises no hook event either; the cursor comparison below catches it.
//! - **Physical input.** The same hooks count events with the flag clear, which are the user's
//!   hands. A change in the foreground window or the cursor that the user's own input explains
//!   makes an operation inconclusive instead of failed.
//! - **Foreground changes.** A WinEvent hook records every `EVENT_SYSTEM_FOREGROUND` with the
//!   window the event names, the window that really is foreground when the hook runs, and when.
//!   The two differ: the event also fires when a window merely activates inside its own input
//!   queue without becoming the system foreground (observed for a detached application that
//!   handles a posted click). So the event alone is not proof of a change; the real foreground
//!   window is. A change that happens and reverts between two polls, or that lands between
//!   operations, is still seen.
//!
//! The hooks only ever count and timestamp. They keep no key codes, no characters and no
//! coordinates, and pass every event on untouched. For injected events they also keep the
//! injector's own tag (`dwExtraInfo`), whether it ran at a lower integrity level, and for a key
//! only its kind (text, modifier, function, media and a few more) and whether it was a release, so
//! a finding can tell one source of injected input from another without revealing what was typed.

use std::sync::atomic::{AtomicIsize, AtomicU64, AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc;
use std::sync::{Mutex, OnceLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Result, anyhow, bail};
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::WindowsAndMessaging::{
  CallNextHookEx, DispatchMessageW, EVENT_SYSTEM_FOREGROUND, GetCursorPos, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
  HC_ACTION, HHOOK, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLKHF_LOWER_IL_INJECTED, LLKHF_UP, LLMHF_INJECTED, LLMHF_LOWER_IL_INJECTED, MSG,
  MSLLHOOKSTRUCT, PostThreadMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL,
  WINEVENT_OUTOFCONTEXT, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
};

/// How long before a foreground change a physical click or key still explains it.
const USER_CAUSE_BEFORE_MS: u64 = 2000;
/// Slack after a change, for an event that is reported a moment late.
const USER_CAUSE_AFTER_MS: u64 = 150;
/// The OS runs the hooks and then moves the cursor, so a user's last mouse event can land its
/// cursor update just after a snapshot that already counted it. Physical activity this close
/// before a window opens still explains a cursor change inside it.
const CURSOR_GUARD_MS: u64 = 120;

static INJECTED_MOUSE: AtomicU64 = AtomicU64::new(0);
static INJECTED_KEYS: AtomicU64 = AtomicU64::new(0);
static PHYSICAL_MOUSE: AtomicU64 = AtomicU64::new(0);
static PHYSICAL_KEYS: AtomicU64 = AtomicU64::new(0);
static FOREGROUND_CHANGES: AtomicU64 = AtomicU64::new(0);
static LAST_PHYSICAL_MS: AtomicU64 = AtomicU64::new(0);
static MOUSE_HOOK: AtomicIsize = AtomicIsize::new(0);
static KEY_HOOK: AtomicIsize = AtomicIsize::new(0);
static STARTED: OnceLock<Instant> = OnceLock::new();
static RUNNING: Mutex<bool> = Mutex::new(false);

/// Counts input observed by the active witness without reading or moving the desktop.
pub(super) fn physical_input_counts() -> (u64, u64) {
  (PHYSICAL_MOUSE.load(Relaxed), PHYSICAL_KEYS.load(Relaxed))
}

const RING: usize = 256;
/// Times (ms since start, plus one so zero means empty) of physical clicks and key presses.
static DISCRETE: [AtomicU64; RING] = [const { AtomicU64::new(0) }; RING];
static DISCRETE_NEXT: AtomicUsize = AtomicUsize::new(0);
/// Foreground changes: when, and which window became foreground.
static FG_TIME: [AtomicU64; RING] = [const { AtomicU64::new(0) }; RING];
static FG_WINDOW: [AtomicIsize; RING] = [const { AtomicIsize::new(0) }; RING];
static FG_ACTUAL: [AtomicIsize; RING] = [const { AtomicIsize::new(0) }; RING];
/// The process that owned the real foreground window when the hook ran.
static FG_PROCESS: [AtomicU64; RING] = [const { AtomicU64::new(0) }; RING];
static FG_NEXT: AtomicUsize = AtomicUsize::new(0);
/// Injected input events: when, the injector's tag, and [`INJECTED_KEYBOARD`] /
/// [`INJECTED_LOWER_INTEGRITY`] / [`INJECTED_KEY_UP`] bits with the key's kind above them.
static INJ_TIME: [AtomicU64; RING] = [const { AtomicU64::new(0) }; RING];
static INJ_TAG: [AtomicU64; RING] = [const { AtomicU64::new(0) }; RING];
static INJ_BITS: [AtomicU64; RING] = [const { AtomicU64::new(0) }; RING];
static INJ_NEXT: AtomicUsize = AtomicUsize::new(0);
const INJECTED_KEYBOARD: u64 = 1;
const INJECTED_LOWER_INTEGRITY: u64 = 2;
const INJECTED_KEY_UP: u64 = 4;
const KEY_KIND_SHIFT: u32 = 3;
const KEY_KIND_MASK: u64 = 7;
const KEY_KINDS: [&str; 8] = [
  "other",
  "modifier",
  "input_method",
  "unassigned",
  "text",
  "editing_navigation",
  "function",
  "media",
];

/// The kind of a virtual key, an index into [`KEY_KINDS`]. Letters, digits, space, the numeric
/// keypad and punctuation all fall under "text", so the kind never says what was typed. NOTICE:
/// "unassigned" is what hotkey tools send to stop a released Alt or Win key from opening a menu
/// (AutoHotkey's default mask key is `vkE8`); "input_method" includes `VK_PACKET`, which carries a
/// character the hook does not read; "function" includes F13 to F24, which keep-awake tools send
/// because no keyboard has them; "media" covers the browser, volume, media and launch keys.
fn key_kind(vk: u32) -> u64 {
  match vk {
    0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5 => 1,
    0x15..=0x1A | 0x1C..=0x1F | 0xE5 | 0xE7 => 2,
    0x07 | 0x0E | 0x0F | 0x3A..=0x40 | 0x88..=0x8F | 0x97..=0x9F | 0xD8..=0xDA | 0xE8 | 0xFF => 3,
    0x20 | 0x30..=0x39 | 0x41..=0x5A | 0x60..=0x6F | 0xBA..=0xC0 | 0xDB..=0xDF | 0xE2 => 4,
    0x08 | 0x09 | 0x0D | 0x1B | 0x21..=0x28 | 0x2D | 0x2E => 5,
    0x70..=0x87 => 6,
    0xA6..=0xB7 => 7,
    _ => 0,
  }
}

fn now_ms() -> u64 {
  STARTED.get_or_init(Instant::now).elapsed().as_millis() as u64 + 1
}

/// What the desktop looked like at one instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
  pub taken_ms: u64,
  pub foreground: isize,
  pub cursor: (i32, i32),
  pub injected_mouse: u64,
  pub injected_keys: u64,
  pub physical_mouse: u64,
  pub physical_keys: u64,
  pub foreground_changes: u64,
  pub last_physical_ms: u64,
}

/// One `EVENT_SYSTEM_FOREGROUND`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ForegroundEvent {
  pub at: u64,
  /// The window the event names.
  pub window: isize,
  /// The window that really was foreground when the hook ran.
  pub actual: isize,
  /// The process that owned `actual`.
  pub process: u32,
}

/// One injected mouse or keyboard event. Which key or where is not kept.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectedEvent {
  pub at: u64,
  pub keyboard: bool,
  /// The injector's `dwExtraInfo`, a value it chooses; some tools tag their input with it.
  pub tag: u64,
  /// The injector ran at a lower integrity level than the hook.
  pub lower_integrity: bool,
  /// For a key: its kind (see [`key_kind`]) and whether it was released.
  pub key: Option<(&'static str, bool)>,
}

/// The foreground events and physical clicks and key presses around an operation.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Timeline {
  pub foreground_events: Vec<ForegroundEvent>,
  /// Times of the user's own clicks and key presses.
  pub discrete_input: Vec<u64>,
  /// Injected events, which no background operation sends.
  pub injected_input: Vec<InjectedEvent>,
}

/// How an operation affected the desktop.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "verdict", rename_all = "snake_case")]
pub enum Verdict {
  /// Nothing the driver could have caused changed: no injected input, no foreground change,
  /// same cursor.
  Held,
  /// Something changed that the user's own input does not explain.
  Violated { reasons: Vec<&'static str> },
  /// A foreground or cursor change happened, but the user's own input explains it. Injected
  /// input is still checked.
  Inconclusive { reasons: Vec<&'static str> },
}

impl Verdict {
  pub fn is_violation(&self) -> bool {
    matches!(self, Self::Violated { .. })
  }
}

impl Snapshot {
  /// Compares the desktop around one operation. `timeline` covers the span from shortly before
  /// `before` to `after`.
  pub fn judge(before: &Snapshot, after: &Snapshot, timeline: &Timeline) -> Verdict {
    let mut violations = Vec::new();
    let mut excused = Vec::new();
    if after.injected_mouse != before.injected_mouse {
      violations.push("injected_mouse_input");
    }
    if after.injected_keys != before.injected_keys {
      violations.push("injected_key_input");
    }

    let user_caused =
      |at: u64| timeline.discrete_input.iter().any(|&input| input + USER_CAUSE_BEFORE_MS >= at && input <= at + USER_CAUSE_AFTER_MS);

    // A real change is a different window being foreground, at the end or at any event. An
    // event whose window never became foreground is only noted (see the module docs).
    let real: Vec<_> = timeline.foreground_events.iter().filter(|event| event.actual != before.foreground).collect();
    if after.foreground != before.foreground || !real.is_empty() {
      let mut moments: Vec<u64> = real.iter().map(|event| event.at).collect();
      if after.foreground != before.foreground {
        moments.push(after.taken_ms);
      }
      if moments.iter().all(|&at| user_caused(at)) {
        excused.push("foreground_window_changed");
      } else {
        violations.push("foreground_window_changed");
      }
    }

    if after.cursor != before.cursor {
      let user_active = after.last_physical_ms + CURSOR_GUARD_MS >= before.taken_ms;
      if user_active {
        excused.push("cursor_moved")
      } else {
        violations.push("cursor_moved")
      }
    }

    if !violations.is_empty() {
      Verdict::Violated {
        reasons: violations,
      }
    } else if !excused.is_empty() {
      Verdict::Inconclusive { reasons: excused }
    } else {
      Verdict::Held
    }
  }
}

pub struct DesktopWatch {
  /// The hook thread's id, so `drop` can end its message loop with `WM_QUIT`.
  thread_id: u32,
  thread: Option<JoinHandle<()>>,
}

fn is_discrete(message: u32) -> bool {
  matches!(message, WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_XBUTTONDOWN | WM_MOUSEWHEEL)
}

/// `key`: the virtual key and whether it was released, for a keyboard event.
fn note_injected(key: Option<(u32, bool)>, tag: usize, lower_integrity: bool) {
  let slot = INJ_NEXT.fetch_add(1, Relaxed) % RING;
  INJ_TAG[slot].store(tag as u64, Relaxed);
  let mut bits = if lower_integrity {
    INJECTED_LOWER_INTEGRITY
  } else {
    0
  };
  if let Some((vk, up)) = key {
    bits |= INJECTED_KEYBOARD | key_kind(vk) << KEY_KIND_SHIFT;
    if up {
      bits |= INJECTED_KEY_UP;
    }
  }
  INJ_BITS[slot].store(bits, Relaxed);
  INJ_TIME[slot].store(now_ms(), Relaxed);
}

fn note_physical(discrete: bool) {
  let at = now_ms();
  LAST_PHYSICAL_MS.store(at, Relaxed);
  if discrete {
    DISCRETE[DISCRETE_NEXT.fetch_add(1, Relaxed) % RING].store(at, Relaxed);
  }
}

unsafe extern "system" fn mouse_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
  if code == HC_ACTION as i32 {
    // SAFETY: for `WH_MOUSE_LL` Windows passes a valid `MSLLHOOKSTRUCT` pointer in `lparam`.
    let event = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
    if event.flags & LLMHF_INJECTED != 0 {
      INJECTED_MOUSE.fetch_add(1, Relaxed);
      note_injected(None, event.dwExtraInfo, event.flags & LLMHF_LOWER_IL_INJECTED != 0);
    } else {
      PHYSICAL_MOUSE.fetch_add(1, Relaxed);
      note_physical(is_discrete(wparam.0 as u32));
    }
  }
  let hook = HHOOK(MOUSE_HOOK.load(Relaxed) as *mut core::ffi::c_void);
  unsafe { CallNextHookEx(hook, code, wparam, lparam) }
}

unsafe extern "system" fn key_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
  if code == HC_ACTION as i32 {
    // SAFETY: for `WH_KEYBOARD_LL` Windows passes a valid `KBDLLHOOKSTRUCT` pointer in `lparam`.
    // For injected keys only the key's kind is kept, never the key code or a character.
    let event = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
    if event.flags.0 & LLKHF_INJECTED.0 != 0 {
      INJECTED_KEYS.fetch_add(1, Relaxed);
      let up = event.flags.0 & LLKHF_UP.0 != 0;
      note_injected(Some((event.vkCode, up)), event.dwExtraInfo, event.flags.0 & LLKHF_LOWER_IL_INJECTED.0 != 0);
    } else {
      PHYSICAL_KEYS.fetch_add(1, Relaxed);
      note_physical(true);
    }
  }
  let hook = HHOOK(KEY_HOOK.load(Relaxed) as *mut core::ffi::c_void);
  unsafe { CallNextHookEx(hook, code, wparam, lparam) }
}

unsafe extern "system" fn foreground_event(
  _hook: HWINEVENTHOOK,
  _event: u32,
  hwnd: HWND,
  _object: i32,
  _child: i32,
  _thread: u32,
  _time: u32,
) {
  let slot = FG_NEXT.fetch_add(1, Relaxed) % RING;
  FG_WINDOW[slot].store(hwnd.0 as isize, Relaxed);
  let mut process = 0u32;
  // SAFETY: read-only queries with a writable out parameter.
  let actual = unsafe {
    let actual = GetForegroundWindow();
    GetWindowThreadProcessId(actual, Some(&mut process));
    actual
  };
  FG_ACTUAL[slot].store(actual.0 as isize, Relaxed);
  FG_PROCESS[slot].store(u64::from(process), Relaxed);
  FG_TIME[slot].store(now_ms(), Relaxed);
  FOREGROUND_CHANGES.fetch_add(1, Relaxed);
}

impl DesktopWatch {
  /// Installs the hooks on a thread of their own.
  pub fn start() -> Result<Self> {
    {
      let mut running = RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
      if *running {
        bail!("a desktop watch is already running in this process");
      }
      *running = true;
    }
    let _ = now_ms();
    for counter in [
      &INJECTED_MOUSE,
      &INJECTED_KEYS,
      &PHYSICAL_MOUSE,
      &PHYSICAL_KEYS,
      &FOREGROUND_CHANGES,
      &LAST_PHYSICAL_MS,
    ] {
      counter.store(0, Relaxed);
    }
    for slot in DISCRETE.iter().chain(FG_TIME.iter()).chain(INJ_TIME.iter()) {
      slot.store(0, Relaxed);
    }
    DISCRETE_NEXT.store(0, Relaxed);
    FG_NEXT.store(0, Relaxed);
    INJ_NEXT.store(0, Relaxed);

    let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
    let thread = std::thread::Builder::new().name("desktop-watch".into()).spawn(move || {
      auv_driver_windows::desktop::ensure_input_desktop();
      // SAFETY: hooks are installed and removed on this thread, which pumps messages.
      unsafe {
        let module: HMODULE = GetModuleHandleW(None).unwrap_or_default();
        let mouse = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
        let keys = SetWindowsHookExW(WH_KEYBOARD_LL, Some(key_hook), module, 0);
        let foreground =
          SetWinEventHook(EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND, None, Some(foreground_event), 0, 0, WINEVENT_OUTOFCONTEXT);
        let (Ok(mouse), Ok(keys)) = (mouse, keys) else {
          let _ = ready_tx.send(Err("could not install the low-level input hooks".into()));
          return;
        };
        if foreground.0.is_null() {
          let _ = ready_tx.send(Err("could not install the foreground WinEvent hook".into()));
          return;
        }
        MOUSE_HOOK.store(mouse.0 as isize, Relaxed);
        KEY_HOOK.store(keys.0 as isize, Relaxed);
        let _ = ready_tx.send(Ok(GetCurrentThreadId()));

        // NOTICE: a blocking `GetMessageW` loop, not a sleep-and-poll one. Windows holds every
        // mouse and keyboard event system-wide until a low-level hook returns, and it delivers the
        // call by sending a message to this thread. A thread that sleeps between pumps (a Windows
        // sleep can last a whole timer tick, ~15 ms) would make the user's pointer and keys lag
        // for as long as the watch runs. `GetMessageW` services the call the moment it arrives.
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
          let _ = TranslateMessage(&message);
          DispatchMessageW(&message);
        }
        let _ = UnhookWindowsHookEx(mouse);
        let _ = UnhookWindowsHookEx(keys);
        let _ = UnhookWinEvent(foreground);
      }
    })?;
    let release = || *RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = false;
    match ready_rx.recv_timeout(Duration::from_secs(5)) {
      Ok(Ok(thread_id)) => Ok(Self {
        thread_id,
        thread: Some(thread),
      }),
      Ok(Err(message)) => {
        release();
        Err(anyhow!(message))
      }
      Err(_) => {
        release();
        Err(anyhow!("the desktop watch thread did not start"))
      }
    }
  }

  pub fn snapshot(&self) -> Snapshot {
    let mut cursor = POINT::default();
    // SAFETY: read-only queries with a writable out parameter.
    let foreground = unsafe {
      let _ = GetCursorPos(&mut cursor);
      GetForegroundWindow()
    };
    Snapshot {
      taken_ms: now_ms(),
      foreground: foreground.0 as isize,
      cursor: (cursor.x, cursor.y),
      injected_mouse: INJECTED_MOUSE.load(Relaxed),
      injected_keys: INJECTED_KEYS.load(Relaxed),
      physical_mouse: PHYSICAL_MOUSE.load(Relaxed),
      physical_keys: PHYSICAL_KEYS.load(Relaxed),
      foreground_changes: FOREGROUND_CHANGES.load(Relaxed),
      last_physical_ms: LAST_PHYSICAL_MS.load(Relaxed),
    }
  }

  /// Foreground changes and the user's physical clicks and key presses between `from_ms` and
  /// `to_ms`, widened backwards so a click that caused a change is included.
  pub fn timeline(&self, from_ms: u64, to_ms: u64) -> Timeline {
    let mut timeline = Timeline::default();
    for slot in 0..RING {
      let at = FG_TIME[slot].load(Relaxed);
      if at != 0 && at >= from_ms && at <= to_ms + 1 {
        timeline.foreground_events.push(ForegroundEvent {
          at,
          window: FG_WINDOW[slot].load(Relaxed),
          actual: FG_ACTUAL[slot].load(Relaxed),
          process: FG_PROCESS[slot].load(Relaxed) as u32,
        });
      }
      let at = INJ_TIME[slot].load(Relaxed);
      if at != 0 && at >= from_ms && at <= to_ms + 1 {
        let bits = INJ_BITS[slot].load(Relaxed);
        timeline.injected_input.push(InjectedEvent {
          at,
          keyboard: bits & INJECTED_KEYBOARD != 0,
          tag: INJ_TAG[slot].load(Relaxed),
          lower_integrity: bits & INJECTED_LOWER_INTEGRITY != 0,
          key: (bits & INJECTED_KEYBOARD != 0)
            .then(|| (KEY_KINDS[(bits >> KEY_KIND_SHIFT & KEY_KIND_MASK) as usize], bits & INJECTED_KEY_UP != 0)),
        });
      }
      let input = DISCRETE[slot].load(Relaxed);
      if input != 0 && input + USER_CAUSE_BEFORE_MS >= from_ms && input <= to_ms + USER_CAUSE_AFTER_MS {
        timeline.discrete_input.push(input);
      }
    }
    timeline.foreground_events.sort_unstable_by_key(|event| event.at);
    timeline.discrete_input.sort_unstable();
    timeline.injected_input.sort_unstable_by_key(|event| event.at);
    timeline
  }

  /// Judges the desktop around one operation.
  pub fn judge(&self, before: &Snapshot, after: &Snapshot) -> (Verdict, Timeline) {
    let timeline = self.timeline(before.taken_ms, after.taken_ms);
    (Snapshot::judge(before, after, &timeline), timeline)
  }
}

impl Drop for DesktopWatch {
  fn drop(&mut self) {
    // SAFETY: posting `WM_QUIT` to the hook thread, which ends its message loop and unhooks.
    unsafe {
      let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
    }
    if let Some(thread) = self.thread.take() {
      let _ = thread.join();
    }
    *RUNNING.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = false;
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn at(taken_ms: u64) -> Snapshot {
    Snapshot {
      taken_ms,
      foreground: 100,
      cursor: (10, 20),
      injected_mouse: 0,
      injected_keys: 0,
      physical_mouse: 0,
      physical_keys: 0,
      foreground_changes: 0,
      last_physical_ms: 0,
    }
  }

  fn quiet() -> Timeline {
    Timeline::default()
  }

  #[test]
  fn an_untouched_desktop_holds() {
    assert_eq!(Snapshot::judge(&at(1000), &at(1010), &quiet()), Verdict::Held);
  }

  #[test]
  fn injected_input_is_a_violation_even_when_the_user_is_active() {
    let mut after = at(1010);
    after.injected_mouse = 1;
    after.physical_mouse = 40;
    after.last_physical_ms = 1005;
    assert_eq!(
      Snapshot::judge(&at(1000), &after, &quiet()),
      Verdict::Violated {
        reasons: vec!["injected_mouse_input"]
      }
    );
  }

  fn event(at: u64, window: isize, actual: isize) -> ForegroundEvent {
    ForegroundEvent {
      at,
      window,
      actual,
      process: 0,
    }
  }

  #[test]
  fn a_foreground_change_with_no_user_input_is_a_violation() {
    let mut after = at(1010);
    after.foreground = 200;
    let timeline = Timeline {
      foreground_events: vec![event(1005, 200, 200)],
      discrete_input: vec![],
      injected_input: vec![],
    };
    assert_eq!(
      Snapshot::judge(&at(1000), &after, &timeline),
      Verdict::Violated {
        reasons: vec!["foreground_window_changed"]
      }
    );
  }

  #[test]
  fn an_event_whose_window_never_became_foreground_is_not_a_change() {
    // Observed: a detached application that handles a posted click fires the event for itself
    // while the system foreground stays with the user's window.
    let timeline = Timeline {
      foreground_events: vec![event(1005, 200, 100)],
      discrete_input: vec![],
      injected_input: vec![],
    };
    assert_eq!(Snapshot::judge(&at(1000), &at(1010), &timeline), Verdict::Held);
  }

  #[test]
  fn a_foreground_change_right_after_a_user_click_is_inconclusive() {
    let mut after = at(5010);
    after.foreground = 200;
    let timeline = Timeline {
      foreground_events: vec![event(5005, 200, 200)],
      discrete_input: vec![5000],
      injected_input: vec![],
    };
    assert_eq!(
      Snapshot::judge(&at(5000), &after, &timeline),
      Verdict::Inconclusive {
        reasons: vec!["foreground_window_changed"]
      }
    );
  }

  #[test]
  fn a_foreground_flicker_that_reverts_still_needs_a_cause() {
    // The window handle is the same before and after, but another window really was foreground
    // in between.
    let timeline = Timeline {
      foreground_events: vec![event(1004, 200, 200), event(1006, 100, 100)],
      discrete_input: vec![],
      injected_input: vec![],
    };
    assert_eq!(
      Snapshot::judge(&at(1000), &at(1010), &timeline),
      Verdict::Violated {
        reasons: vec!["foreground_window_changed"]
      }
    );
  }

  #[test]
  fn a_click_long_before_does_not_excuse_a_later_foreground_change() {
    let mut after = at(9010);
    after.foreground = 200;
    let timeline = Timeline {
      foreground_events: vec![event(9005, 200, 200)],
      discrete_input: vec![5000],
      injected_input: vec![],
    };
    assert!(Snapshot::judge(&at(9000), &after, &timeline).is_violation());
  }

  #[test]
  fn a_cursor_move_with_no_recent_physical_input_is_a_violation() {
    let mut after = at(1010);
    after.cursor = (500, 600);
    assert_eq!(
      Snapshot::judge(&at(1000), &after, &quiet()),
      Verdict::Violated {
        reasons: vec!["cursor_moved"]
      }
    );
  }

  #[test]
  fn a_cursor_move_just_after_the_users_last_mouse_event_is_inconclusive() {
    // The hook counted the user's event before `before` was taken, but the OS moved the cursor
    // only afterwards: physical activity shortly before the window still explains it.
    let mut before = at(1000);
    before.last_physical_ms = 960;
    let mut after = at(1010);
    after.cursor = (500, 600);
    after.last_physical_ms = 960;
    assert_eq!(
      Snapshot::judge(&before, &after, &quiet()),
      Verdict::Inconclusive {
        reasons: vec!["cursor_moved"]
      }
    );
  }

  #[test]
  fn an_injected_key_is_kept_only_as_a_kind_that_never_names_a_typed_character() {
    // The witness records injected keys to tell their sources apart, and must not record text.
    // Letters, digits, space, a keypad digit and a punctuation key.
    for vk in [
      u32::from(b'A'),
      u32::from(b'Z'),
      u32::from(b'0'),
      0x20,
      0x65,
      0xBA,
    ] {
      assert_eq!(KEY_KINDS[key_kind(vk) as usize], "text", "vk {vk:#x}");
    }
    assert_eq!(KEY_KINDS[key_kind(0x0D) as usize], "editing_navigation");
    assert_eq!(KEY_KINDS[key_kind(0x26) as usize], "editing_navigation");
    assert_eq!(KEY_KINDS[key_kind(0x70) as usize], "function");
    assert_eq!(KEY_KINDS[key_kind(0x7E) as usize], "function");
    assert_eq!(KEY_KINDS[key_kind(0xAD) as usize], "media");
    assert_eq!(KEY_KINDS[key_kind(0x2C) as usize], "other");
    assert_eq!(KEY_KINDS[key_kind(0xA4) as usize], "modifier");
    assert_eq!(KEY_KINDS[key_kind(0xE7) as usize], "input_method");
    assert_eq!(KEY_KINDS[key_kind(0xE8) as usize], "unassigned");
  }
}
