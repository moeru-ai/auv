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
  use windows::Win32::UI::WindowsAndMessaging::{
    BN_CLICKED, BS_PUSHBUTTON, CallWindowProcW, CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, ES_AUTOVSCROLL,
    ES_MULTILINE, ES_READONLY, GWLP_WNDPROC, GetClientRect, GetForegroundWindow, GetMessageW, GetParent, GetWindowRect, HMENU, HWND_BOTTOM,
    IsIconic, MSG, MoveWindow, PostMessageW, PostQuitMessage, RegisterClassExW, SW_SHOWMINNOACTIVE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, SendMessageW, SetForegroundWindow, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, ShowWindow,
    TranslateMessage, WINDOW_EX_STYLE, WINDOW_STYLE, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY, WM_ERASEBKGND,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEHWHEEL, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_PAINT, WM_SETFOCUS,
    WM_SIZE, WM_VSCROLL, WNDCLASSEXW, WNDPROC, WS_BORDER, WS_CHILD, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
    WS_VSCROLL,
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
  /// `EM_GETFIRSTVISIBLELINE` lives in the Controls module, which this crate does not enable.
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
  static BASE_TITLE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
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
    let options = Options::parse();
    // SAFETY: process-wide setting made once before any window exists.
    unsafe {
      let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    INDEX.store(u64::from(options.index), Relaxed);
    // SAFETY: a read-only query.
    HOME_FOREGROUND.store(unsafe { GetForegroundWindow() }.0 as isize, Relaxed);
    SINK.store(options.sink, Relaxed);
    let _ = BASE_TITLE.set(options.title.clone());
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

    let extended = if options.no_activate_style {
      WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW
    } else {
      WINDOW_EX_STYLE(0)
    };
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
    let button = child(w!("BUTTON"), w!("Press"), WINDOW_STYLE(BS_PUSHBUTTON as u32), ID_BUTTON);
    let label = child(w!("STATIC"), w!("clicks 0"), WINDOW_STYLE(0), ID_LABEL);
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
  }

  fn arrange_children(frame: HWND) {
    let mut client = RECT::default();
    // SAFETY: live window and writable rectangle.
    unsafe {
      let _ = GetClientRect(frame, &mut client);
      let (width, height) = (client.right - client.left, client.bottom - client.top);
      let _ = MoveWindow(handle(&BUTTON), 16, 16, 110, 32, true);
      let _ = MoveWindow(handle(&LABEL), 140, 22, (width - 156).max(40), 24, true);
      let _ = MoveWindow(handle(&EDIT), 16, 64, (width - 32).max(40), (height - 80 - STRIP_BOTTOM - STRIP_CELL_HEIGHT + 16).max(40), true);
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
            if GetForegroundWindow() == hwnd {
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
    json!({
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
    })
  }

  fn state(hwnd: HWND) -> Value {
    // SAFETY: live window and control; read-only queries.
    let (first_line, minimized, foreground) = unsafe {
      (SendMessageW(handle(&EDIT), EM_GETFIRSTVISIBLELINE, WPARAM(0), LPARAM(0)).0, IsIconic(hwnd).as_bool(), GetForegroundWindow() == hwnd)
    };
    let c = &COUNTERS;
    json!({
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
    })
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
