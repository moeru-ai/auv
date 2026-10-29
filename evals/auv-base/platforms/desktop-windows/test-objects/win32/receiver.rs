//! Native Win32 receiver for the Windows background-session evaluations.
//!
//! One top-level window in a process of its own, built from the classic controls real
//! Win32 applications use: a push button, a read-only multiline edit with a vertical
//! scrollbar, and a label. It reports what the *receiving application* processed, not what
//! the driver claims it delivered:
//!
//! - `clicks` counts `BN_CLICKED` notifications, so the button's own press logic ran.
//! - `first_line` is the edit control's first visible line, so a wheel message really
//!   scrolled it.
//! - `raw` counts messages as they arrive at each control, before the control acts on them.
//! - `activated`, `mouse_activate` and `foreground` show whether the window was ever
//!   activated or foregrounded, which a background delivery must never cause.
//! - `foreground_taken` counts the times the window became the foreground window anyway, and
//!   `foreground_yielded` the times it handed the foreground straight back (see below).
//! - A strip of sixteen cells along the bottom paints the low eight bits of `clicks` and of
//!   `first_line` (white is 1, most significant bit first). A capture of the window can be decoded
//!   and compared with the numbers above, which proves the frame shows the window's *latest* state
//!   and not an older one.
//!
//! The parent drives it over JSON lines: it prints one `ready` line, then answers one
//! reply per request line. Requests run on the window's own thread, behind every message
//! already posted to it, so a `state` reply includes the effect of any input the parent
//! delivered before asking. The lines travel over stdin and stdout, or, with `--connect`, over
//! a loopback socket the receiver dials: that lets the parent start it detached from its own
//! process tree, the way an independently launched application would run. Either channel
//! closing closes the window.
//!
//! The window is shown without activation and, with `--sink`, placed at the bottom of the
//! Z-order so ordinary windows cover it. It never asks for focus and never touches another
//! window, with one exception that protects the user: if the window becomes the foreground
//! window, it hands the foreground back at once to the window it took it from (or, when Windows
//! does not say, to the foreground window at its start), and with `--sink` goes back to the
//! bottom of the Z-order, where becoming the foreground window raised it from. Windows can let a
//! freshly started process take the foreground on its first activation; whether it does is
//! Windows' decision, made from how and when the process was started, and was seen both with and
//! without user input since the start. The hand-back keeps that from leaving the user's desktop
//! changed, and the counters keep it visible.
//!
//! For the evaluations of foreground commands, which act on the screen and not on a window,
//! `--foreground-input` adds a writable text box that logs the key messages it receives (`typing`), and a pad on the
//! frame that records where a drag went down and up (`pad`). In that mode, after a `keep_foreground`
//! request, a window that real input made the foreground window keeps the foreground: the
//! evaluation hands it back itself when it is done. `--topmost` creates the window above ordinary
//! windows, so its text can be read off the screen and real input aimed at it lands on it.
//!
//! An explicitly armed `reject_typing_focus` request redirects received text-box focus to this
//! receiver's own button. Its counters and GetFocus state expose a real post-delivery refusal.

#[cfg(not(windows))]
fn main() {
  eprintln!("win32-receiver requires Windows");
  std::process::exit(2);
}

#[cfg(windows)]
fn main() {
  native::run();
}

#[cfg(windows)]
mod native {
  use std::io::BufRead;
  use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, Ordering::Relaxed};

  use clap::Parser;
  use serde_json::{Value, json};
  use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
  use windows::Win32::Graphics::Gdi::{
    BLACK_BRUSH, BeginPaint, ClientToScreen, CreateSolidBrush, EndPaint, FillRect, GetStockObject, HBRUSH, HDC, InvalidateRect, PAINTSTRUCT,
    UpdateWindow, WHITE_BRUSH,
  };
  use windows::Win32::System::LibraryLoader::GetModuleHandleW;
  use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext};
  use windows::Win32::UI::Input::Ime::{HIMC, ImmAssociateContext};
  use windows::Win32::UI::Input::KeyboardAndMouse::{GetFocus, ReleaseCapture, SetCapture, SetFocus};
  use windows::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, BS_PUSHBUTTON, CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL,
    ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, GWLP_WNDPROC, GetClientRect, GetForegroundWindow, GetMessageW, GetParent, GetWindowRect,
    GetWindowTextLengthW, GetWindowTextW, HMENU, HWND_BOTTOM, IsIconic, MSG, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW,
    SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetForegroundWindow,
    SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow, TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE, WM_APP,
    WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_ERASEBKGND, WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETFOCUS, WM_SIZE, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WM_VSCROLL, WNDCLASSEXW, WNDPROC, WS_BORDER, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW,
    WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
  };
  use windows::core::{HSTRING, PCWSTR, w};

  /// Message the stdin reader posts to the window thread to run one request there.
  const WM_REQUEST: u32 = WM_APP + 1;
  /// Posted to itself when the window became the foreground window: hand it back to the window in
  /// `wparam`, once the activation that caused it has finished.
  const WM_YIELD_FOREGROUND: u32 = WM_APP + 2;
  const ID_BUTTON: usize = 1001;
  const ID_LABEL: usize = 1002;
  const ID_EDIT: usize = 1003;
  const ID_TYPING: usize = 1004;
  const KEY_LOG_LIMIT: usize = 256;
  /// In foreground-input mode the scrolling edit is this tall, and the pad takes the client area
  /// from `PAD_TOP` down to just above the strip.
  const SHORT_EDIT_HEIGHT: i32 = 100;
  const PAD_TOP: i32 = 206;
  /// `MK_LBUTTON` in a mouse message's `wparam`: the left button is down.
  const MK_LBUTTON: usize = 0x0001;
  /// Edit-control messages live in the Controls module, which this crate does not enable.
  const EM_GETSEL: u32 = 0x00B0;
  const EM_GETFIRSTVISIBLELINE: u32 = 0x00CE;
  const LINE_COUNT: usize = 400;
  /// The state strip: sixteen cells of `STRIP_CELL_WIDTH` by `STRIP_CELL_HEIGHT`, `STRIP_LEFT` from the
  /// left edge of the client area and `STRIP_BOTTOM` above its bottom edge.
  const STRIP_CELLS: i32 = 16;
  const STRIP_CELL_WIDTH: i32 = 12;
  const STRIP_CELL_HEIGHT: i32 = 16;
  const STRIP_LEFT: i32 = 16;
  const STRIP_BOTTOM: i32 = 22;

  #[derive(Parser)]
  #[command(about = "Native Win32 receiver for background-session evaluations")]
  struct Options {
    #[arg(long, default_value = "AUV Win32 receiver")]
    title: String,
    /// Position and size of the whole window (frame included), in screen pixels.
    #[arg(long, default_value_t = 100)]
    left: i32,
    #[arg(long, default_value_t = 100)]
    top: i32,
    #[arg(long, default_value_t = 380)]
    width: i32,
    #[arg(long, default_value_t = 300)]
    height: i32,
    /// Background color as RRGGBB, so a capture can tell which receiver it shows.
    #[arg(long, default_value = "2E6F9E")]
    color: String,
    /// Caption for the push button.
    #[arg(long, default_value = "Activate Target")]
    button_caption: String,
    /// Initial text in the foreground-input text box, for OCR-driven workflow evaluations.
    #[arg(long, default_value = "")]
    input_caption: String,
    /// Nonzero capture calibration before any input is sent; not a delivered click.
    #[arg(long, default_value_t = 0)]
    initial_clicks: u64,
    /// Shown in the edit text so each receiver's content differs.
    #[arg(long, default_value_t = 0)]
    index: u32,
    /// Place the window at the bottom of the Z-order so ordinary windows cover it.
    #[arg(long)]
    sink: bool,
    /// Talk to the parent over this loopback address (`127.0.0.1:port`) instead of stdin and
    /// stdout, so the receiver can be started detached from the parent's process tree.
    #[arg(long)]
    connect: Option<String>,
    /// Give the window `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`. Off by default: an ordinary
    /// top-level window is the realistic target, and one that can never activate would
    /// hide an activation a real application would suffer.
    #[arg(long)]
    no_activate_style: bool,
    /// Create the window topmost, above ordinary windows, so that text drawn on it can be read off
    /// the screen and real input aimed at it lands on it. It is created so: Windows ignores a later
    /// `HWND_TOPMOST` for a window whose process holds no foreground right, from this process or
    /// from the harness (seen on 2026-10-12).
    #[arg(long)]
    topmost: bool,
    /// Foreground-input mode: adds a writable text box that logs the keys it receives and a pad
    /// that records a drag, and keeps the foreground when real input gives it to the window
    /// instead of handing it back.
    #[arg(long)]
    foreground_input: bool,
    /// Leave the text box with the thread's input method. By default it has none, so a key press
    /// reaches it as that key: an IME such as Microsoft Pinyin turns key presses into
    /// `VK_PROCESSKEY` and composes them, and no character arrives until it commits.
    #[arg(long)]
    keep_ime: bool,
  }

  struct Counters {
    clicks: AtomicU64,
    button_down: AtomicU64,
    button_up: AtomicU64,
    button_move: AtomicU64,
    button_wheel: AtomicU64,
    edit_down: AtomicU64,
    edit_wheel: AtomicU64,
    edit_hwheel: AtomicU64,
    edit_move: AtomicU64,
    frame_down: AtomicU64,
    activated: AtomicU64,
    mouse_activate: AtomicU64,
    focus_set: AtomicU64,
    foreground_taken: AtomicU64,
    foreground_yielded: AtomicU64,
    typing_chars: AtomicU64,
    typing_down: AtomicU64,
    typing_focus: AtomicU64,
    typing_focus_rejected: AtomicU64,
  }

  static COUNTERS: Counters = Counters {
    clicks: AtomicU64::new(0),
    button_down: AtomicU64::new(0),
    button_up: AtomicU64::new(0),
    button_move: AtomicU64::new(0),
    button_wheel: AtomicU64::new(0),
    edit_down: AtomicU64::new(0),
    edit_wheel: AtomicU64::new(0),
    edit_hwheel: AtomicU64::new(0),
    edit_move: AtomicU64::new(0),
    frame_down: AtomicU64::new(0),
    activated: AtomicU64::new(0),
    mouse_activate: AtomicU64::new(0),
    focus_set: AtomicU64::new(0),
    foreground_taken: AtomicU64::new(0),
    foreground_yielded: AtomicU64::new(0),
    typing_chars: AtomicU64::new(0),
    typing_down: AtomicU64::new(0),
    typing_focus: AtomicU64::new(0),
    typing_focus_rejected: AtomicU64::new(0),
  };

  // The window thread is the only writer of these handles, set during `WM_CREATE`.
  static BUTTON: AtomicIsize = AtomicIsize::new(0);
  static LABEL: AtomicIsize = AtomicIsize::new(0);
  static EDIT: AtomicIsize = AtomicIsize::new(0);
  static BUTTON_PROC: AtomicIsize = AtomicIsize::new(0);
  static EDIT_PROC: AtomicIsize = AtomicIsize::new(0);
  static BACKGROUND: AtomicIsize = AtomicIsize::new(0);
  static INDEX: AtomicU64 = AtomicU64::new(0);
  /// The foreground window when the receiver started: where the foreground goes back to when
  /// Windows does not name the window it came from.
  static HOME_FOREGROUND: AtomicIsize = AtomicIsize::new(0);
  /// Started with `--sink`: the window belongs under ordinary windows.
  static SINK: AtomicBool = AtomicBool::new(false);
  /// Started with `--foreground-input`: the text box and the pad exist.
  static FOREGROUND_INPUT: AtomicBool = AtomicBool::new(false);
  /// Set by the `keep_foreground` request in foreground-input mode: the foreground is no longer
  /// handed back, because the evaluation is about to give it to this window with a real click.
  static KEEP_FOREGROUND: AtomicBool = AtomicBool::new(false);
  /// Started with `--keep-ime`: the text box keeps the thread's input method.
  static KEEP_IME: AtomicBool = AtomicBool::new(false);
  /// Explicit fault injection only: reject text focus by focusing our own button.
  static REJECT_TYPING_FOCUS: AtomicBool = AtomicBool::new(false);
  static TYPING: AtomicIsize = AtomicIsize::new(0);
  static TYPING_PROC: AtomicIsize = AtomicIsize::new(0);
  /// Key messages the text box received, oldest first, at most `KEY_LOG_LIMIT`. They can only be
  /// keys that reached this window, so nothing typed elsewhere on the desktop is kept.
  static KEY_LOG: std::sync::Mutex<Vec<Value>> = std::sync::Mutex::new(Vec::new());
  /// What the pad saw of the last drag over it, in screen coordinates.
  static PAD: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);
  /// Armed only by the keyboard-recipient harness. Consumed after the first key release.
  static KEYBOARD_FAULT: std::sync::Mutex<Option<(String, isize)>> = std::sync::Mutex::new(None);
  static KEYBOARD_FAULT_RECEIPT: std::sync::Mutex<Option<Value>> = std::sync::Mutex::new(None);
  static STARTED: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
  static BASE_TITLE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
  static BUTTON_CAPTION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
  static INPUT_CAPTION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
  /// Text the application submitted when its button actually received BN_CLICKED.
  static SUBMITTED_TEXT: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());
  /// Where replies go: stdout, or the socket to the parent. Written from the window thread only.
  static OUT: std::sync::Mutex<Option<Box<dyn std::io::Write + Send>>> = std::sync::Mutex::new(None);

  fn emit(value: &Value) {
    use std::io::Write;
    if let Ok(mut out) = OUT.lock()
      && let Some(out) = out.as_mut()
    {
      let _ = writeln!(out, "{value}");
      let _ = out.flush();
    }
  }

  fn handle(slot: &AtomicIsize) -> HWND {
    HWND(slot.load(Relaxed) as *mut core::ffi::c_void)
  }

  fn count(counter: &AtomicU64) {
    counter.fetch_add(1, Relaxed);
  }

  fn parse_color(hex: &str) -> COLORREF {
    let value = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x2E6F9E);
    let (red, green, blue) = ((value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF);
    COLORREF(red | (green << 8) | (blue << 16))
  }

  pub fn run() {
    auv_driver_windows::desktop::ensure_input_desktop();
    let options = Options::parse();
    // SAFETY: process-wide setting made once before any window exists.
    unsafe {
      let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    INDEX.store(u64::from(options.index), Relaxed);
    COUNTERS.clicks.store(options.initial_clicks, Relaxed);
    // SAFETY: a read-only query.
    HOME_FOREGROUND.store(unsafe { GetForegroundWindow() }.0 as isize, Relaxed);
    SINK.store(options.sink, Relaxed);
    FOREGROUND_INPUT.store(options.foreground_input, Relaxed);
    KEEP_IME.store(options.keep_ime, Relaxed);
    let _ = STARTED.set(std::time::Instant::now());
    let _ = BASE_TITLE.set(options.title.clone());
    let _ = BUTTON_CAPTION.set(options.button_caption.clone());
    let _ = INPUT_CAPTION.set(options.input_caption.clone());
    let reader: Box<dyn BufRead + Send> = match &options.connect {
      Some(address) => {
        let stream = std::net::TcpStream::connect(address).expect("connect to the parent");
        let _ = stream.set_nodelay(true);
        *OUT.lock().unwrap() = Some(Box::new(stream.try_clone().expect("clone socket")));
        Box::new(std::io::BufReader::new(stream))
      }
      None => {
        *OUT.lock().unwrap() = Some(Box::new(std::io::stdout()));
        Box::new(std::io::BufReader::new(std::io::stdin()))
      }
    };
    // SAFETY: the brush lives for the whole process and is only used by the window thread.
    BACKGROUND.store(unsafe { CreateSolidBrush(parse_color(&options.color)) }.0 as isize, Relaxed);

    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.expect("module handle").into();
    let class = WNDCLASSEXW {
      cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
      style: windows::Win32::UI::WindowsAndMessaging::CS_DBLCLKS,
      lpfnWndProc: Some(frame_proc),
      hInstance: instance,
      lpszClassName: w!("AuvEvalWin32Receiver"),
      ..Default::default()
    };
    // SAFETY: static class name and procedure outlive the window.
    assert_ne!(unsafe { RegisterClassExW(&class) }, 0, "register class");

    let mut extended = if options.no_activate_style {
      WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW
    } else {
      WINDOW_EX_STYLE(0)
    };
    if options.topmost {
      extended |= WS_EX_TOPMOST;
    }
    let title = HSTRING::from(options.title.as_str());
    // SAFETY: valid class and live strings; the window is created on this thread, which
    // then runs its message loop.
    let hwnd = unsafe {
      CreateWindowExW(
        extended,
        w!("AuvEvalWin32Receiver"),
        &title,
        WS_OVERLAPPEDWINDOW,
        options.left,
        options.top,
        options.width,
        options.height,
        None,
        None,
        instance,
        None,
      )
    }
    .expect("create receiver window");

    // SAFETY: `hwnd` is live. Without activation the window never becomes foreground.
    unsafe {
      let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
      if options.sink {
        let _ = SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
      }
    }

    emit(&{
      let mut ready = layout(hwnd);
      ready["event"] = json!("ready");
      ready["title"] = json!(options.title);
      ready
    });

    spawn_request_reader(hwnd.0 as isize, reader);

    let mut message = MSG::default();
    // SAFETY: standard message loop on the thread that owns the window.
    unsafe {
      while GetMessageW(&mut message, None, 0, 0).as_bool() {
        let _ = TranslateMessage(&message);
        DispatchMessageW(&message);
      }
    }
  }

  fn spawn_request_reader(raw: isize, reader: Box<dyn BufRead + Send>) {
    std::thread::spawn(move || {
      let hwnd = HWND(raw as *mut core::ffi::c_void);
      for line in reader.lines() {
        let Ok(line) = line else { break };
        let boxed = Box::into_raw(Box::new(line));
        // SAFETY: ownership of the box moves to the window thread when the post succeeds.
        if unsafe { PostMessageW(hwnd, WM_REQUEST, WPARAM(0), LPARAM(boxed as isize)) }.is_err() {
          // SAFETY: the post failed, so nothing else owns the box.
          drop(unsafe { Box::from_raw(boxed) });
          return;
        }
      }
      // A closed channel means the parent is gone or finished with this receiver.
      // SAFETY: posting to a window that may already be gone is harmless.
      unsafe {
        let _ = PostMessageW(hwnd, WM_CLOSE, WPARAM(0), LPARAM(0));
      }
    });
  }

  /// Mirrors the counters into the window title, so a receiver started without pipes can still
  /// be read from outside with `GetWindowTextW`.
  fn publish_title(hwnd: HWND) {
    let base = BASE_TITLE.get().map(String::as_str).unwrap_or("AUV Win32 receiver");
    let c = &COUNTERS;
    let title =
      format!("{base} | clicks={} activated={} focus={}", c.clicks.load(Relaxed), c.activated.load(Relaxed), c.focus_set.load(Relaxed));
    // SAFETY: live top-level window owned by this thread.
    unsafe {
      let _ = SetWindowTextW(hwnd, &HSTRING::from(title));
    }
  }

  fn create_children(frame: HWND) {
    let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }.expect("module handle").into();
    let index = INDEX.load(Relaxed);
    let child = |class: PCWSTR, text: PCWSTR, style: WINDOW_STYLE, id: usize| -> HWND {
      // SAFETY: valid built-in classes; the child belongs to `frame` on this thread.
      unsafe {
        CreateWindowExW(
          WINDOW_EX_STYLE(0),
          class,
          text,
          WS_CHILD | WS_VISIBLE | style,
          0,
          0,
          10,
          10,
          frame,
          HMENU(id as *mut core::ffi::c_void),
          instance,
          None,
        )
      }
      .expect("create child window")
    };
    let button_caption = BUTTON_CAPTION.get().map(String::as_str).unwrap_or("Activate Target");
    let button_caption_hstring = HSTRING::from(button_caption);
    let button = child(w!("BUTTON"), PCWSTR(button_caption_hstring.as_ptr()), WINDOW_STYLE(BS_PUSHBUTTON as u32), ID_BUTTON);
    let initial_label = HSTRING::from(format!("clicks {}", COUNTERS.clicks.load(Relaxed)));
    let label = child(w!("STATIC"), PCWSTR(initial_label.as_ptr()), WINDOW_STYLE(0), ID_LABEL);
    let edit =
      child(w!("EDIT"), w!(""), WS_VSCROLL | WS_BORDER | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL | ES_READONLY) as u32), ID_EDIT);

    let text: String =
      (0..LINE_COUNT).map(|line| format!("line {line:03}  receiver {index}  the quick brown fox jumps over the lazy dog\r\n")).collect();
    // SAFETY: live control and a live UTF-16 string.
    unsafe {
      let _ = SetWindowTextW(edit, &HSTRING::from(text.as_str()));
      BUTTON_PROC.store(SetWindowLongPtrW(button, GWLP_WNDPROC, button_proc as *const () as usize as isize), Relaxed);
      EDIT_PROC.store(SetWindowLongPtrW(edit, GWLP_WNDPROC, edit_proc as *const () as usize as isize), Relaxed);
    }
    BUTTON.store(button.0 as isize, Relaxed);
    LABEL.store(label.0 as isize, Relaxed);
    EDIT.store(edit.0 as isize, Relaxed);
    if FOREGROUND_INPUT.load(Relaxed) {
      let typing = child(w!("EDIT"), w!(""), WS_BORDER | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32), ID_TYPING);
      // SAFETY: live control; its original procedure is kept for `call_original`. Associating no
      // input context turns the IME off for this control only.
      unsafe {
        let _ = SetWindowTextW(typing, &HSTRING::from(INPUT_CAPTION.get().map(String::as_str).unwrap_or("")));
        TYPING_PROC.store(SetWindowLongPtrW(typing, GWLP_WNDPROC, typing_proc as *const () as usize as isize), Relaxed);
        if !KEEP_IME.load(Relaxed) {
          let _ = ImmAssociateContext(typing, HIMC::default());
        }
      }
      TYPING.store(typing.0 as isize, Relaxed);
    }
  }

  fn arrange_children(frame: HWND) {
    let mut client = RECT::default();
    // SAFETY: live window and writable rectangle.
    unsafe {
      let _ = GetClientRect(frame, &mut client);
      let (width, height) = (client.right - client.left, client.bottom - client.top);
      let _ = MoveWindow(handle(&BUTTON), 16, 16, 180, 32, true);
      let _ = MoveWindow(handle(&LABEL), 206, 22, (width - 222).max(40), 24, true);
      if FOREGROUND_INPUT.load(Relaxed) {
        // Text box, a shorter scrolling edit, then the pad down to the strip.
        let _ = MoveWindow(handle(&TYPING), 16, 60, (width - 32).max(40), 26, true);
        let _ = MoveWindow(handle(&EDIT), 16, 96, (width - 32).max(40), SHORT_EDIT_HEIGHT, true);
      } else {
        let _ = MoveWindow(handle(&EDIT), 16, 64, (width - 32).max(40), (height - 80 - STRIP_BOTTOM - STRIP_CELL_HEIGHT + 16).max(40), true);
      }
    }
  }

  /// The pad, in client coordinates: where a drag over the frame itself is recorded.
  fn pad_rect(frame: HWND) -> RECT {
    let mut client = RECT::default();
    // SAFETY: live window and writable rectangle.
    unsafe {
      let _ = GetClientRect(frame, &mut client);
    }
    RECT {
      left: 16,
      top: PAD_TOP,
      right: client.right - 16,
      bottom: client.bottom - STRIP_BOTTOM - 6,
    }
  }

  /// A point from a mouse message's `lparam`, in `window`'s client coordinates, as a screen point.
  fn screen_point(window: HWND, lparam: LPARAM) -> POINT {
    let mut point = POINT {
      x: i32::from(lparam.0 as u16 as i16),
      y: i32::from((lparam.0 >> 16) as u16 as i16),
    };
    // SAFETY: live window and a writable point.
    unsafe {
      let _ = ClientToScreen(window, &mut point);
    }
    point
  }

  fn since_start_ms() -> u64 {
    STARTED.get().map(|started| started.elapsed().as_millis() as u64).unwrap_or(0)
  }

  fn log_key(vk: usize, up: bool, system: bool) {
    if let Ok(mut log) = KEY_LOG.lock()
      && log.len() < KEY_LOG_LIMIT
    {
      log.push(json!({"vk": vk, "up": up, "system": system, "ms": since_start_ms()}));
    }
  }

  extern "system" fn typing_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
      WM_KEYDOWN | WM_SYSKEYDOWN => log_key(wparam.0, false, message == WM_SYSKEYDOWN),
      WM_KEYUP | WM_SYSKEYUP => log_key(wparam.0, true, message == WM_SYSKEYUP),
      WM_CHAR => count(&COUNTERS.typing_chars),
      WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => count(&COUNTERS.typing_down),
      WM_SETFOCUS => {
        count(&COUNTERS.focus_set);
        count(&COUNTERS.typing_focus);
        publish_title(unsafe { GetParent(hwnd) }.unwrap_or_default());
      }
      _ => {}
    }
    let result = call_original(&TYPING_PROC, hwnd, message, wparam, lparam);
    if matches!(message, WM_KEYUP | WM_SYSKEYUP) {
      let fault = KEYBOARD_FAULT.lock().ok().and_then(|mut fault| fault.take());
      if let Some((kind, target)) = fault {
        // The caller supplied another owned fixture. No input is synthesized here: this models
        // an application changing foreground after it has processed the first key release.
        let owner = unsafe { GetParent(hwnd) }.unwrap_or_default();
        let target = HWND(target as *mut core::ffi::c_void);
        // SAFETY: native foreground/window requests on the harness's owned receiver pair.
        // The fault mutex was released before these potentially reentrant OS calls.
        let (reached_target, returned_owner) = unsafe {
          let requested = SetForegroundWindow(target).as_bool();
          let reached = requested && GetForegroundWindow() == target;
          let returned = match kind.as_str() {
            "aba" => {
              std::thread::sleep(std::time::Duration::from_millis(50));
              SetForegroundWindow(owner).as_bool() && GetForegroundWindow() == owner
            }
            "minimize" => {
              let _ = ShowWindow(owner, SW_SHOWMINNOACTIVE);
              false
            }
            "close" => {
              let _ = PostMessageW(owner, WM_CLOSE, WPARAM(0), LPARAM(0));
              false
            }
            _ => false,
          };
          (reached, returned)
        };
        if let Ok(mut receipt) = KEYBOARD_FAULT_RECEIPT.lock() {
          *receipt = Some(json!({"kind": kind, "reached_target": reached_target, "returned_owner": returned_owner}));
        }
      }
    }
    if message == WM_SETFOCUS && REJECT_TYPING_FOCUS.load(Relaxed) {
      let button = handle(&BUTTON);
      // SAFETY: this request is armed after both controls are created. Both
      // controls belong to this GUI thread; no mutex or mutable borrow spans
      // the reentrant focus callbacks. Other processes' windows are untouched.
      unsafe {
        let _ = SetFocus(button);
        if GetFocus() == button {
          count(&COUNTERS.typing_focus_rejected);
        }
      }
    }
    result
  }

  /// Records a drag over the pad: where the button went down and up on the screen, and how many
  /// moves arrived with it held. The window captures the mouse while the button is down, so a
  /// real drag that leaves the pad is still followed.
  fn pad_message(frame: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) {
    let point = screen_point(frame, lparam);
    let client = POINT {
      x: i32::from(lparam.0 as u16 as i16),
      y: i32::from((lparam.0 >> 16) as u16 as i16),
    };
    let pad = pad_rect(frame);
    let Ok(mut state) = PAD.lock() else { return };
    match message {
      WM_LBUTTONDOWN if client.x >= pad.left && client.x < pad.right && client.y >= pad.top && client.y < pad.bottom => {
        *state = Some(json!({"down": [point.x, point.y], "down_ms": since_start_ms(), "moves": 0, "up": null, "up_ms": null}));
        // SAFETY: capturing for this thread's own window while its button is down.
        unsafe {
          let _ = SetCapture(frame);
        }
      }
      WM_MOUSEMOVE if wparam.0 & MK_LBUTTON != 0 => {
        if let Some(drag) = state.as_mut()
          && drag["up"].is_null()
        {
          drag["moves"] = json!(drag["moves"].as_u64().unwrap_or(0) + 1);
          drag["last"] = json!([point.x, point.y]);
        }
      }
      WM_LBUTTONUP => {
        if let Some(drag) = state.as_mut()
          && drag["up"].is_null()
        {
          drag["up"] = json!([point.x, point.y]);
          drag["up_ms"] = json!(since_start_ms());
          // SAFETY: releasing a capture this thread took.
          unsafe {
            let _ = ReleaseCapture();
          }
        }
      }
      _ => {}
    }
  }

  fn call_original(slot: &AtomicIsize, hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // SAFETY: the slot holds the control's original window procedure stored at creation.
    let original: WNDPROC = unsafe { std::mem::transmute::<isize, WNDPROC>(slot.load(Relaxed)) };
    unsafe { CallWindowProcW(original, hwnd, message, wparam, lparam) }
  }

  extern "system" fn button_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
      WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => count(&COUNTERS.button_down),
      WM_LBUTTONUP => count(&COUNTERS.button_up),
      WM_MOUSEMOVE => count(&COUNTERS.button_move),
      WM_MOUSEWHEEL | WM_MOUSEHWHEEL => count(&COUNTERS.button_wheel),
      WM_MOUSEACTIVATE => count(&COUNTERS.mouse_activate),
      WM_SETFOCUS => {
        count(&COUNTERS.focus_set);
        publish_title(unsafe { GetParent(hwnd) }.unwrap_or_default());
      }
      _ => {}
    }
    call_original(&BUTTON_PROC, hwnd, message, wparam, lparam)
  }

  extern "system" fn edit_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
      WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => count(&COUNTERS.edit_down),
      WM_MOUSEWHEEL => count(&COUNTERS.edit_wheel),
      WM_MOUSEHWHEEL => count(&COUNTERS.edit_hwheel),
      WM_MOUSEMOVE => count(&COUNTERS.edit_move),
      WM_MOUSEACTIVATE => count(&COUNTERS.mouse_activate),
      WM_SETFOCUS => {
        count(&COUNTERS.focus_set);
        publish_title(unsafe { GetParent(hwnd) }.unwrap_or_default());
      }
      _ => {}
    }
    let result = call_original(&EDIT_PROC, hwnd, message, wparam, lparam);
    if matches!(message, WM_MOUSEWHEEL | WM_MOUSEHWHEEL | WM_VSCROLL) {
      // SAFETY: the edit's parent is the live frame window on this thread.
      refresh_strip(unsafe { GetParent(hwnd) }.unwrap_or_default());
    }
    result
  }

  fn first_visible_line() -> i64 {
    // SAFETY: a read-only query of this thread's live edit control.
    unsafe { SendMessageW(handle(&EDIT), EM_GETFIRSTVISIBLELINE, WPARAM(0), LPARAM(0)).0 as i64 }
  }

  fn strip_rect(frame: HWND) -> RECT {
    let mut client = RECT::default();
    // SAFETY: live window and writable rectangle.
    unsafe {
      let _ = GetClientRect(frame, &mut client);
    }
    let top = client.bottom - STRIP_BOTTOM;
    RECT {
      left: STRIP_LEFT,
      top,
      right: STRIP_LEFT + STRIP_CELLS * STRIP_CELL_WIDTH,
      bottom: top + STRIP_CELL_HEIGHT,
    }
  }

  fn draw_strip(frame: HWND, dc: HDC) {
    let strip = strip_rect(frame);
    let value = (((COUNTERS.clicks.load(Relaxed) & 0xFF) << 8) | (first_visible_line() as u64 & 0xFF)) as u16;
    for cell in 0..STRIP_CELLS {
      let bit = (value >> (STRIP_CELLS - 1 - cell)) & 1;
      let rect = RECT {
        left: strip.left + cell * STRIP_CELL_WIDTH,
        top: strip.top,
        right: strip.left + (cell + 1) * STRIP_CELL_WIDTH,
        bottom: strip.bottom,
      };
      // SAFETY: stock brushes need no cleanup; `dc` is the paint context.
      unsafe {
        let brush = GetStockObject(if bit == 1 { WHITE_BRUSH } else { BLACK_BRUSH });
        FillRect(dc, &rect, HBRUSH(brush.0));
      }
    }
  }

  /// Repaints the strip now, so the compositor has the new state as soon as the message is handled.
  fn refresh_strip(frame: HWND) {
    let strip = strip_rect(frame);
    // SAFETY: live window; a synchronous repaint on the window's own thread.
    unsafe {
      let _ = InvalidateRect(frame, Some(&strip), false);
      let _ = UpdateWindow(frame);
    }
  }

  extern "system" fn frame_proc(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
      WM_CREATE => {
        create_children(hwnd);
        LRESULT(0)
      }
      WM_SIZE => {
        if handle(&EDIT).0.is_null() {
          return LRESULT(0);
        }
        arrange_children(hwnd);
        LRESULT(0)
      }
      WM_PAINT => {
        let mut paint = PAINTSTRUCT::default();
        // SAFETY: the standard begin/end pairing on this thread's own window.
        unsafe {
          let dc = BeginPaint(hwnd, &mut paint);
          draw_strip(hwnd, dc);
          let _ = EndPaint(hwnd, &paint);
        }
        LRESULT(0)
      }
      WM_ERASEBKGND => {
        let mut client = RECT::default();
        // SAFETY: `wparam` is the device context Windows passes with this message.
        unsafe {
          let _ = GetClientRect(hwnd, &mut client);
          FillRect(
            windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void),
            &client,
            HBRUSH(BACKGROUND.load(Relaxed) as *mut _),
          );
        }
        LRESULT(1)
      }
      WM_COMMAND => {
        let notification = (wparam.0 >> 16) as u32;
        let id = wparam.0 & 0xFFFF;
        if id == ID_BUTTON && notification == BN_CLICKED {
          let clicks = COUNTERS.clicks.fetch_add(1, Relaxed) + 1;
          if FOREGROUND_INPUT.load(Relaxed)
            && let Ok(mut submitted) = SUBMITTED_TEXT.lock()
          {
            *submitted = window_text(handle(&TYPING));
          }
          // SAFETY: live label control.
          unsafe {
            let _ = SetWindowTextW(handle(&LABEL), &HSTRING::from(format!("clicks {clicks}")));
          }
          publish_title(hwnd);
          refresh_strip(hwnd);
        }
        LRESULT(0)
      }
      WM_ACTIVATE => {
        if wparam.0 & 0xFFFF != 0 {
          count(&COUNTERS.activated);
          publish_title(hwnd);
          // SAFETY: a read-only query, and a message posted to this thread's own window.
          unsafe {
            if GetForegroundWindow() == hwnd && KEEP_FOREGROUND.load(Relaxed) {
              // Real input gave it the foreground on purpose; the evaluation hands it back itself.
              count(&COUNTERS.foreground_taken);
            } else if GetForegroundWindow() == hwnd {
              count(&COUNTERS.foreground_taken);
              // `lparam` is the window being deactivated, when Windows says.
              let to = if lparam.0 != 0 {
                lparam.0
              } else {
                HOME_FOREGROUND.load(Relaxed)
              };
              let _ = PostMessageW(hwnd, WM_YIELD_FOREGROUND, WPARAM(to as usize), LPARAM(0));
            }
          }
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
      }
      WM_YIELD_FOREGROUND => {
        // SAFETY: this process holds the foreground, so it may hand it to another window.
        if unsafe { GetForegroundWindow() == hwnd && SetForegroundWindow(HWND(wparam.0 as *mut core::ffi::c_void)).as_bool() } {
          count(&COUNTERS.foreground_yielded);
          if SINK.load(Relaxed) {
            // SAFETY: Z-order only, on this thread's own window.
            unsafe {
              let _ = SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
          }
        }
        LRESULT(0)
      }
      WM_MOUSEACTIVATE => {
        count(&COUNTERS.mouse_activate);
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
      }
      WM_LBUTTONDOWN => {
        count(&COUNTERS.frame_down);
        if FOREGROUND_INPUT.load(Relaxed) {
          pad_message(hwnd, message, wparam, lparam);
        }
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
      }
      WM_MOUSEMOVE | WM_LBUTTONUP if FOREGROUND_INPUT.load(Relaxed) => {
        pad_message(hwnd, message, wparam, lparam);
        unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
      }
      WM_REQUEST => {
        // SAFETY: the reader thread leaked this box into the message; take it back once.
        let line = *unsafe { Box::from_raw(lparam.0 as *mut String) };
        let reply = handle_request(hwnd, &line);
        emit(&reply);
        LRESULT(0)
      }
      WM_CLOSE => {
        // SAFETY: closing this thread's own window.
        unsafe {
          let _ = DestroyWindow(hwnd);
        }
        LRESULT(0)
      }
      WM_DESTROY => {
        // SAFETY: ends this thread's message loop.
        unsafe { PostQuitMessage(0) };
        LRESULT(0)
      }
      _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
  }

  /// Screen position of the middle of `control`, for the parent to aim input at.
  fn center(control: HWND) -> Value {
    let mut rect = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: live control, writable out parameters.
    unsafe {
      let _ = GetClientRect(control, &mut rect);
      origin.x = (rect.right - rect.left) / 2;
      origin.y = (rect.bottom - rect.top) / 2;
      let _ = ClientToScreen(control, &mut origin);
    }
    json!([origin.x, origin.y])
  }

  /// Screen position of the strip's top-left corner.
  fn strip_origin(frame: HWND) -> Value {
    let strip = strip_rect(frame);
    let mut origin = POINT {
      x: strip.left,
      y: strip.top,
    };
    // SAFETY: live window and a writable point.
    unsafe {
      let _ = ClientToScreen(frame, &mut origin);
    }
    json!([origin.x, origin.y])
  }

  fn layout(hwnd: HWND) -> Value {
    let mut rect = RECT::default();
    // SAFETY: live window and writable rectangle.
    unsafe {
      let _ = GetWindowRect(hwnd, &mut rect);
    }
    let mut layout = json!({
      "hwnd": (hwnd.0 as isize).to_string(),
      "pid": std::process::id(),
      "window_rect": [rect.left, rect.top, rect.right, rect.bottom],
      "button_hwnd": (handle(&BUTTON).0 as isize).to_string(),
      "edit_hwnd": (handle(&EDIT).0 as isize).to_string(),
      "strip": {
        "origin": strip_origin(hwnd),
        "cell": [STRIP_CELL_WIDTH, STRIP_CELL_HEIGHT],
        "cells": STRIP_CELLS,
      },
      "button_center": center(handle(&BUTTON)),
      "edit_center": center(handle(&EDIT)),
      "label_center": center(handle(&LABEL)),
      "button_rect": screen_rect(handle(&BUTTON)),
      "label_rect": screen_rect(handle(&LABEL)),
      "edit_rect": screen_rect(handle(&EDIT)),
    });
    if FOREGROUND_INPUT.load(Relaxed) {
      let pad = pad_rect(hwnd);
      let mut corners = [
        POINT {
          x: pad.left,
          y: pad.top,
        },
        POINT {
          x: pad.right,
          y: pad.bottom,
        },
      ];
      // SAFETY: live window and writable points.
      unsafe {
        for corner in &mut corners {
          let _ = ClientToScreen(hwnd, corner);
        }
      }
      layout["typing_rect"] = screen_rect(handle(&TYPING));
      layout["typing_center"] = center(handle(&TYPING));
      layout["pad_rect"] = json!([corners[0].x, corners[0].y, corners[1].x, corners[1].y]);
    }
    layout
  }

  fn window_text(window: HWND) -> String {
    // SAFETY: live window; the buffer is sized from the reported length plus the terminator.
    unsafe {
      let length = GetWindowTextLengthW(window).max(0) as usize;
      let mut buffer = vec![0u16; length + 1];
      let copied = GetWindowTextW(window, &mut buffer).max(0) as usize;
      String::from_utf16_lossy(&buffer[..copied])
    }
  }

  /// A control's rectangle on the screen as `[left, top, right, bottom]`: the truth a text
  /// recognized in a capture of the window can be placed against.
  fn screen_rect(control: HWND) -> Value {
    let mut rect = RECT::default();
    // SAFETY: live control and writable rectangle.
    unsafe {
      let _ = GetWindowRect(control, &mut rect);
    }
    json!([rect.left, rect.top, rect.right, rect.bottom])
  }

  fn state(hwnd: HWND) -> Value {
    // SAFETY: live window and control; read-only queries.
    let (first_line, minimized, foreground) = unsafe {
      (SendMessageW(handle(&EDIT), EM_GETFIRSTVISIBLELINE, WPARAM(0), LPARAM(0)).0, IsIconic(hwnd).as_bool(), GetForegroundWindow() == hwnd)
    };
    let c = &COUNTERS;
    let mut state = json!({
      "clicks": c.clicks.load(Relaxed),
      "first_line": first_line,
      "raw": {
        "button_down": c.button_down.load(Relaxed),
        "button_up": c.button_up.load(Relaxed),
        "button_move": c.button_move.load(Relaxed),
        "button_wheel": c.button_wheel.load(Relaxed),
        "edit_down": c.edit_down.load(Relaxed),
        "edit_wheel": c.edit_wheel.load(Relaxed),
        "edit_hwheel": c.edit_hwheel.load(Relaxed),
        "edit_move": c.edit_move.load(Relaxed),
        "frame_down": c.frame_down.load(Relaxed),
      },
      "activated": c.activated.load(Relaxed),
      "mouse_activate": c.mouse_activate.load(Relaxed),
      "focus_set": c.focus_set.load(Relaxed),
      "foreground_taken": c.foreground_taken.load(Relaxed),
      "foreground_yielded": c.foreground_yielded.load(Relaxed),
      "minimized": minimized,
      "foreground": foreground,
      "button_focused": unsafe { GetFocus() } == handle(&BUTTON),
    });
    if FOREGROUND_INPUT.load(Relaxed) {
      let typing = handle(&TYPING);
      let mut selection_start = 0_u32;
      let mut selection_end = 0_u32;
      // SAFETY: the edit belongs to this thread, and both DWORD outputs live through the call.
      unsafe {
        let _ = SendMessageW(
          typing,
          EM_GETSEL,
          WPARAM((&mut selection_start as *mut u32) as usize),
          LPARAM((&mut selection_end as *mut u32) as isize),
        );
      }
      state["submitted_text"] = json!(SUBMITTED_TEXT.lock().map(|text| text.clone()).unwrap_or_default());
      state["typing"] = json!({
        "text": window_text(typing),
        "selection": [selection_start, selection_end],
        // The request runs on the window's own thread, so this is the control focused in it.
        "focused": unsafe { GetFocus() } == typing,
        "chars": c.typing_chars.load(Relaxed),
        "button_down": c.typing_down.load(Relaxed),
        "focus_received": c.typing_focus.load(Relaxed),
        "focus_rejected": c.typing_focus_rejected.load(Relaxed),
        "keys": KEY_LOG.lock().map(|log| log.clone()).unwrap_or_default(),
      });
      state["pad"] = PAD.lock().ok().and_then(|pad| pad.clone()).unwrap_or(Value::Null);
    }
    state
  }

  fn handle_request(hwnd: HWND, line: &str) -> Value {
    let request: Value = match serde_json::from_str(line) {
      Ok(request) => request,
      Err(error) => return json!({"ok": false, "error": format!("bad request: {error}")}),
    };
    let int = |key: &str| request[key].as_i64().map(|value| value as i32);
    let mut reply = match request["cmd"].as_str() {
      Some("state") => state(hwnd),
      Some("layout") => layout(hwnd),
      // The receiver moves, resizes and minimizes itself, so the evaluation can inject
      // those faults without the driver or the runner touching the window.
      Some("minimize") => {
        // SAFETY: live window; minimizing without activation.
        unsafe {
          let _ = ShowWindow(hwnd, SW_SHOWMINNOACTIVE);
        }
        json!({})
      }
      // Undoes `minimize` without activating the window, and puts it back under ordinary windows.
      Some("restore") => {
        // SAFETY: live window; restoring without activation, then back to the bottom of the Z-order.
        unsafe {
          let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
          let _ = SetWindowPos(hwnd, HWND_BOTTOM, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
        json!({})
      }
      Some("move") => {
        if let (Some(left), Some(top)) = (int("left"), int("top")) {
          // SAFETY: live window; position only, no activation or Z-order change.
          unsafe {
            let _ = SetWindowPos(hwnd, None, left, top, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
          }
          layout(hwnd)
        } else {
          json!({"ok": false, "error": "move needs left and top"})
        }
      }
      Some("resize") => {
        if let (Some(width), Some(height)) = (int("width"), int("height")) {
          // SAFETY: live window; size only, no activation or Z-order change.
          unsafe {
            let _ = SetWindowPos(hwnd, None, 0, 0, width, height, SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE);
          }
          layout(hwnd)
        } else {
          json!({"ok": false, "error": "resize needs width and height"})
        }
      }
      Some("keep_foreground") if FOREGROUND_INPUT.load(Relaxed) => {
        KEEP_FOREGROUND.store(true, Relaxed);
        json!({})
      }
      Some("prepare_keyboard") if FOREGROUND_INPUT.load(Relaxed) => {
        // Fixture initialization only: start with a blank first responder on its own GUI thread.
        let typing = handle(&TYPING);
        // SAFETY: both operations affect only this receiver's own live child control.
        unsafe {
          let _ = SetWindowTextW(typing, w!(""));
          let _ = SetFocus(typing);
        }
        // SAFETY: GetFocus queries only this thread's input queue.
        json!({"focused": unsafe { GetFocus() } == typing})
      }
      Some("arm_keyboard_fault") if FOREGROUND_INPUT.load(Relaxed) => {
        let kind = request["kind"].as_str().unwrap_or("");
        let target = request["target"].as_str().and_then(|value| value.parse::<isize>().ok()).unwrap_or(0);
        if ["stay", "aba", "minimize", "close"].contains(&kind) && target != 0 {
          *KEYBOARD_FAULT.lock().unwrap() = Some((kind.into(), target));
          *KEYBOARD_FAULT_RECEIPT.lock().unwrap() = None;
          json!({"armed": true})
        } else {
          json!({"ok": false, "error": "keyboard fault needs a known kind and target HWND"})
        }
      }
      Some("keyboard_fault_receipt") => json!({"receipt": KEYBOARD_FAULT_RECEIPT.lock().ok().and_then(|value| value.clone())}),
      Some("reject_typing_focus") if FOREGROUND_INPUT.load(Relaxed) => {
        if let Some(enabled) = request["enabled"].as_bool() {
          REJECT_TYPING_FOCUS.store(enabled, Relaxed);
          json!({"enabled": enabled})
        } else {
          json!({"ok": false, "error": "reject_typing_focus needs enabled"})
        }
      }
      // Forgets the logged keys and the last drag, so the next step is read on its own.
      Some("reset_log") => {
        if let Ok(mut log) = KEY_LOG.lock() {
          log.clear();
        }
        if let Ok(mut pad) = PAD.lock() {
          *pad = None;
        }
        json!({})
      }
      // Hands the foreground back to `to`: harness cleanup after a deliberately diagnosed steal.
      Some("yield_foreground") => {
        let to = request["to"].as_str().and_then(|value| value.parse::<isize>().ok()).unwrap_or(0);
        // SAFETY: a handle query and a foreground request; this process holds the foreground.
        let handed = unsafe { SetForegroundWindow(HWND(to as *mut core::ffi::c_void)) }.as_bool();
        json!({"handed": handed})
      }
      Some("close") => {
        // The reply is written before the window goes, then the loop ends.
        emit(&json!({"ok": true, "cmd": "close"}));
        // SAFETY: closing this thread's own window.
        unsafe {
          let _ = DestroyWindow(hwnd);
        }
        return json!({"event": "closed"});
      }
      other => return json!({"ok": false, "error": format!("unknown command {other:?}")}),
    };
    if reply.get("ok").is_none() {
      reply["ok"] = json!(true);
    }
    reply["cmd"] = request["cmd"].clone();
    reply
  }
}
