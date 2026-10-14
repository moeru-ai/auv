//! Real X11 integration probe. Run only in an isolated 800x600 Xvfb session.
#![cfg(target_os = "linux")]

use auv_driver_common::{
  App, CaptureOptions, Click, ClickModifiers, ClickOptions, DisturbanceLevel, Driver, DriverError, InputDeliveryPath, InputPolicy,
  InputTarget, KeyPressOptions, KeyboardInput, MoveMouseRequest, PasteTextOptions, Point, PressKeysOptions, Rect, ScreenPoint, Scroll,
  ScrollOptions, TextSubmit, TypeTextOptions, Window, WindowPoint, WindowSelector, input::MouseButton,
};
use auv_driver_linux_x11::X11Driver;
use std::{
  fs,
  io::Write,
  path::{Path, PathBuf},
  process::{Child, Command, Stdio},
  sync::Mutex,
  time::{Duration, Instant},
};
use x11rb::{
  connection::Connection,
  protocol::xproto::{ConnectionExt as _, CreateWindowAux, WindowClass},
};

// Both tests drive the same display; run them one at a time.
static DESKTOP: Mutex<()> = Mutex::new(());

struct Fixture {
  child: Child,
  directory: PathBuf,
}
impl Drop for Fixture {
  fn drop(&mut self) {
    let _ = self.child.kill();
    let _ = self.child.wait();
    let _ = fs::remove_dir_all(&self.directory);
  }
}

fn wait_for(mut condition: impl FnMut() -> bool) {
  let started = Instant::now();
  while !condition() {
    assert!(started.elapsed() < Duration::from_secs(10), "X11 fixture timed out");
    std::thread::sleep(Duration::from_millis(30));
  }
}

#[test]
#[ignore = "requires isolated Xvfb and python3-tk; manipulates the selected desktop"]
fn capture_and_input_change_real_application_state() {
  let _desktop = DESKTOP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
  let directory = std::env::temp_dir().join(format!("auv-x11-test-{}", std::process::id()));
  fs::create_dir(&directory).unwrap();
  let child = Command::new("python3")
    .arg("-c")
    .arg(
      r#"
import pathlib, sys, tkinter as tk
p = pathlib.Path(sys.argv[1])
root = tk.Tk()
root.overrideredirect(True)
root.geometry('800x600+0+0')
root.configure(background='white')
entry = tk.Entry(root)
entry.place(x=20, y=20, width=300, height=40)
# NOTICE: Tk's Linux default Ctrl+A means beginning-of-line. This fixture
# explicitly models editors with select-all, the driver's replacement contract.
# Keep this binding while the fixture uses Tk instead of a desktop editor.
def select_all(event):
    entry.selection_range(0, tk.END)
    return 'break'
entry.bind('<Control-a>', select_all)
canvas = tk.Canvas(root, background='#ff0000', highlightthickness=0)
canvas.place(x=200, y=150, width=100, height=100)
listbox = tk.Listbox(root)
listbox.place(x=400, y=300, width=200, height=200)
for i in range(100):
    listbox.insert(tk.END, 'item '+str(i))
def record_scroll(event):
    root.after(50, lambda: (p/'scroll_position').write_text(str(listbox.yview()[0])))
listbox.bind('<Button-5>', record_scroll, add='+')
def submit(event):
    (p/'text').write_text(entry.get())
    canvas.configure(background='#0000ff')
entry.bind('<Return>', submit)
def event(e):
    with (p/'events').open('a') as f:
        f.write(str(e.num)+':'+str(e.state & 1)+'\n')
root.bind('<ButtonPress>', event)
def transition(kind):
    def record(e):
        with (p/'transitions').open('a') as f:
            f.write(kind+':'+str(e.keysym if kind.startswith('key') else e.num)+'\n')
    return record
root.bind('<ButtonPress>', transition('button-down'), add='+')
root.bind('<ButtonRelease>', transition('button-up'))
root.bind('<KeyPress>', transition('key-down'))
root.bind('<KeyRelease>', transition('key-up'))
root.update()
(p/'ready').touch()
root.mainloop()
"#,
    )
    .arg(&directory)
    .spawn()
    .unwrap();
  let fixture = Fixture { child, directory };
  wait_for(|| fixture.directory.join("ready").exists());
  let session = X11Driver.open_local().unwrap();
  let display = session.display().capture(CaptureOptions::default()).unwrap();
  assert_eq!((display.capture.image.width(), display.capture.image.height()), (800, 600));
  assert_eq!(display.capture.image.get_pixel(220, 170).0, [255, 0, 0, 255]);
  let region = session
    .display()
    .capture_region(CaptureOptions {
      region: Some(Rect::new(210.0, 160.0, 20.0, 20.0)),
      ..Default::default()
    })
    .unwrap();
  assert_eq!(region.capture.image.get_pixel(5, 5).0, [255, 0, 0, 255]);
  assert_eq!(region.capture.bounds.origin, Point::new(210.0, 160.0));
  assert!(
    session
      .display()
      .capture_region(CaptureOptions {
        region: Some(Rect::new(790.0, 590.0, 20.0, 20.0)),
        ..Default::default()
      })
      .is_err()
  );

  let input = session.input();
  input.click_at(Point::new(50.0, 40.0), Click::Single, ClickModifiers::default()).unwrap();
  input.type_text("discard", TypeTextOptions::default()).unwrap();
  input
    .type_text(
      "AUV Ω",
      TypeTextOptions {
        replace_existing: true,
        submit: TextSubmit::Return,
        ..Default::default()
      },
    )
    .unwrap();
  wait_for(|| fixture.directory.join("text").exists());
  assert_eq!(fs::read_to_string(fixture.directory.join("text")).unwrap(), "AUV Ω");
  wait_for(|| session.display().capture(CaptureOptions::default()).unwrap().capture.image.get_pixel(220, 170).0 == [0, 0, 255, 255]);
  input.click_button_at(Point::new(450.0, 450.0), MouseButton::Right, Click::Single, ClickModifiers::default()).unwrap();
  input.scroll_at(Point::new(450.0, 450.0), Scroll::new(120.0, 120.0), Duration::ZERO).unwrap();
  wait_for(|| {
    fs::read_to_string(fixture.directory.join("scroll_position"))
      .ok()
      .and_then(|position| position.parse::<f64>().ok())
      .is_some_and(|position| position > 0.0)
  });
  input.drag(Point::new(450.0, 450.0), Point::new(500.0, 500.0), MouseButton::Left).unwrap();
  assert_eq!(input.current_position().unwrap(), Point::new(500.0, 500.0));
  let (point, _) = input.move_mouse(MoveMouseRequest::direct(Point::new(520.0, 520.0)), |_| true).unwrap();
  assert_eq!(point, Point::new(520.0, 520.0));
  let (point, _) = input.drag_mouse(MoveMouseRequest::direct(Point::new(540.0, 540.0)), MouseButton::Left).unwrap();
  assert_eq!(point, Point::new(540.0, 540.0));

  let mouse = input.create_mouse().unwrap();
  input.mouse_down(&InputTarget::Foreground, mouse, Point::new(560.0, 560.0), MouseButton::Left, Duration::from_secs(1)).unwrap();
  input.move_mouse_to(mouse, Point::new(580.0, 580.0)).unwrap();
  input.mouse_up(mouse).unwrap();
  input.remove_mouse(mouse).unwrap();

  input.click_at(Point::new(50.0, 40.0), Click::Single, ClickModifiers::default()).unwrap();
  let hold =
    input.key_down(&InputTarget::Foreground, vec!["Shift".into()], InputPolicy::ForegroundPreferred, Duration::from_secs(1)).unwrap();
  input.key_up(hold.into_id()).unwrap();
  let control = input
    .key_down(&InputTarget::Foreground, vec!["ctrl".into()], InputPolicy::ForegroundPreferred, Duration::from_secs(2))
    .unwrap()
    .into_id();
  let shift = input
    .key_down(&InputTarget::Foreground, vec!["shift".into()], InputPolicy::ForegroundPreferred, Duration::from_secs(2))
    .unwrap()
    .into_id();
  // Both names resolve to the same X keysym; a second press must be refused
  // before delivery rather than releasing the first owner's modifier.
  assert!(
    input.key_down(&InputTarget::Foreground, vec!["control".into()], InputPolicy::ForegroundPreferred, Duration::from_secs(2)).is_err()
  );
  input
    .press_key(KeyPressOptions {
      key: "p".into(),
      ..Default::default()
    })
    .unwrap();
  input.key_up(shift).unwrap();
  input.key_up(control).unwrap();
  input
    .press_key(KeyPressOptions {
      key: "Escape".into(),
      ..Default::default()
    })
    .unwrap();
  // NOTICE: Tk 8.6 translates X11 button 7 to button 5 + Shift. Assert the
  // toolkit-level observation until this fixture uses raw X11 events instead.
  // Source: https://github.com/tcltk/tk/blob/core-8-6-13/generic/tkEvent.c#L1135-L1140
  wait_for(|| {
    fs::read_to_string(fixture.directory.join("events"))
      .is_ok_and(|events| events.contains("3:0\n") && events.contains("5:0\n") && events.contains("5:1\n"))
  });
  wait_for(|| {
    fs::read_to_string(fixture.directory.join("transitions")).is_ok_and(|events| {
      events.contains("button-down:1\n")
        && events.contains("button-up:1\n")
        && events.contains("key-down:Shift_L\n")
        && events.contains("key-up:Shift_L\n")
        && events.contains("key-down:Control_L\nkey-down:Shift_L\n")
        && events.contains("key-up:Shift_L\nkey-up:Control_L\n")
    })
  });
}

fn spawn_paste_fixture(directory: &Path) -> Child {
  Command::new("python3")
    .arg("-c")
    .arg(
      r#"
import pathlib, sys, tkinter as tk
p = pathlib.Path(sys.argv[1])
root = tk.Tk()
root.overrideredirect(True)
root.geometry('800x600+0+0')
entry = tk.Entry(root)
entry.place(x=20, y=20, width=300, height=40)
def submit(event):
    (p/'text.tmp').write_text(entry.get())
    (p/'text.tmp').rename(p/'text')
    entry.delete(0, tk.END)
entry.bind('<Return>', submit)
def dump_clipboard(event):
    try:
        value = root.clipboard_get()
    except tk.TclError as error:
        value = 'ERR:' + str(error)
    (p/'clipboard.tmp').write_text(value)
    (p/'clipboard.tmp').rename(p/'clipboard')
root.bind_all('<F5>', dump_clipboard)
root.bind_all('<F6>', lambda event: root.focus_set())
root.update()
(p/'ready').touch()
root.mainloop()
"#,
    )
    .arg(directory)
    .spawn()
    .unwrap()
}

fn take_file(path: &Path) -> String {
  wait_for(|| path.exists());
  let value = fs::read_to_string(path).unwrap();
  fs::remove_file(path).unwrap();
  value
}

#[test]
#[ignore = "requires isolated Xvfb, python3-tk and xclip; manipulates the selected desktop"]
fn clipboard_paste_delivers_text_and_restores_the_previous_clipboard() {
  let _desktop = DESKTOP.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
  let directory = std::env::temp_dir().join(format!("auv-x11-paste-test-{}", std::process::id()));
  fs::create_dir(&directory).unwrap();
  let child = spawn_paste_fixture(&directory);
  let fixture = Fixture { child, directory };
  wait_for(|| fixture.directory.join("ready").exists());

  // An independent client observes CLIPBOARD ownership.
  let (observer, screen) = x11rb::connect(None).unwrap();
  let root = observer.setup().roots[screen].root;
  let clipboard = observer.intern_atom(false, b"CLIPBOARD").unwrap().reply().unwrap().atom;
  let owner = || observer.get_selection_owner(clipboard).unwrap().reply().unwrap().owner;
  observer.set_selection_owner(x11rb::NONE, clipboard, x11rb::CURRENT_TIME).unwrap();
  assert_eq!(owner(), x11rb::NONE);

  let session = X11Driver.open_local().unwrap();
  let input = session.input();
  let text_file = fixture.directory.join("text");
  let clipboard_file = fixture.directory.join("clipboard");
  let submit = |text: &str| PasteTextOptions {
    text: text.into(),
    submit: TextSubmit::Return,
    ..Default::default()
  };
  input.click_at(Point::new(50.0, 40.0), Click::Single, ClickModifiers::default()).unwrap();

  // 1. Empty clipboard: the paste lands and CLIPBOARD is released again.
  let result = input.paste_text(submit("héllo 世界")).unwrap();
  assert_eq!(take_file(&text_file), "héllo 世界");
  assert_eq!(result.selected_path, InputDeliveryPath::ClipboardPaste);
  assert_eq!(result.clipboard_disturbance, DisturbanceLevel::Temporary);
  assert!(!result.verified);
  assert_eq!(owner(), x11rb::NONE);

  // 2. Text owned by another client: the paste lands and an independent
  // reader gets the previous text back afterwards.
  let mut xclip = Command::new("xclip").args(["-selection", "clipboard", "-quiet"]).stdin(Stdio::piped()).spawn().unwrap();
  xclip.stdin.take().unwrap().write_all("before ü".as_bytes()).unwrap();
  wait_for(|| owner() != x11rb::NONE);
  let result = input.paste_text(submit("AUV Ω")).unwrap();
  assert_eq!(take_file(&text_file), "AUV Ω");
  assert_eq!(result.clipboard_disturbance, DisturbanceLevel::Temporary, "{result:?}");
  // xclip exits after losing CLIPBOARD to the paste.
  let _ = xclip.wait();
  input
    .press_key(KeyPressOptions {
      key: "F5".into(),
      ..Default::default()
    })
    .unwrap();
  assert_eq!(take_file(&clipboard_file), "before ü");
  assert_eq!(session.clipboard().snapshot().unwrap(), "before ü");

  // 3. Another client takes CLIPBOARD during the settle period: it keeps it.
  let thief = observer.generate_id().unwrap();
  observer
    .create_window(0, thief, root, 0, 0, 1, 1, 0, WindowClass::INPUT_ONLY, x11rb::COPY_FROM_PARENT, &CreateWindowAux::new())
    .unwrap()
    .check()
    .unwrap();
  let result = std::thread::scope(|scope| {
    let paste = scope.spawn(|| {
      input.paste_text(PasteTextOptions {
        settle: Duration::from_millis(1500),
        ..submit("stolen")
      })
    });
    std::thread::sleep(Duration::from_millis(700));
    observer.set_selection_owner(thief, clipboard, x11rb::CURRENT_TIME).unwrap();
    observer.flush().unwrap();
    paste.join().unwrap()
  })
  .unwrap();
  assert_eq!(take_file(&text_file), "stolen");
  assert_eq!(result.clipboard_disturbance, DisturbanceLevel::Unknown);
  assert!(result.attempts[0].message.as_deref().is_some_and(|message| message.contains("another client")), "{result:?}");
  assert_eq!(owner(), thief);

  // 4. The thief never answers conversions: paste fails before any key.
  let error = input.paste_text(submit("never")).unwrap_err();
  assert!(error.to_string().contains("did not answer"), "{error}");
  assert_eq!(owner(), thief);
  assert!(!text_file.exists());

  // 5. Nothing requests the text: the paste fails and CLIPBOARD is empty
  // again, as it was before the paste.
  observer.set_selection_owner(x11rb::NONE, clipboard, x11rb::CURRENT_TIME).unwrap();
  observer.flush().unwrap();
  input
    .press_key(KeyPressOptions {
      key: "F6".into(),
      ..Default::default()
    })
    .unwrap();
  let error = input.paste_text(submit("ignored")).unwrap_err();
  assert!(error.to_string().contains("did not request CLIPBOARD"), "{error}");
  assert_eq!(owner(), x11rb::NONE);

  // 6. ROOT CAUSE: GNOME's mutter reads every new CLIPBOARD owner to mirror
  // it. Before the fix, that read counted as the paste consumer, so a paste
  // into a window that ignores Ctrl+V (here the focused Tk toplevel) reported
  // success while nothing was pasted. Only the focused client's process
  // counts now; xclip in another process plays mutter.
  let mirror = std::thread::spawn(move || {
    let (watcher, _) = x11rb::connect(None).unwrap();
    let started = Instant::now();
    while watcher.get_selection_owner(clipboard).unwrap().reply().unwrap().owner == x11rb::NONE {
      assert!(started.elapsed() < Duration::from_secs(5), "the paste never took CLIPBOARD");
      std::thread::sleep(Duration::from_millis(1));
    }
    Command::new("xclip").args(["-o", "-selection", "clipboard"]).output().unwrap().stdout
  });
  let error = input.paste_text(submit("mirrored")).unwrap_err();
  assert_eq!(String::from_utf8(mirror.join().unwrap()).unwrap(), "mirrored");
  assert!(error.to_string().contains("did not request CLIPBOARD"), "{error}");
  assert!(error.to_string().contains("only other clients read it"), "{error}");
  assert_eq!(owner(), x11rb::NONE);
  assert!(!text_file.exists());

  // 7. ROOT CAUSE: a just-started Chrome in a loaded OSWorld VM asked for
  // TARGETS at once but for the text more than a second later. The paste
  // timed out after one second and restored the clipboard, so nothing was
  // pasted. A focused application that has started now gets five seconds.
  // This thread is that application: it focuses its own window, asks for
  // TARGETS at once and for the text two seconds later.
  let (ready, focused) = std::sync::mpsc::channel();
  let slow = std::thread::spawn(move || {
    use x11rb::protocol::{Event, xproto::InputFocus};
    let (app, screen) = x11rb::connect(None).unwrap();
    let root = app.setup().roots[screen].root;
    let atom = |name: &[u8]| app.intern_atom(false, name).unwrap().reply().unwrap().atom;
    let (targets, utf8, property) = (atom(b"TARGETS"), atom(b"UTF8_STRING"), atom(b"_AUV_TEST_PASTE"));
    let window = app.generate_id().unwrap();
    app
      .create_window(x11rb::COPY_DEPTH_FROM_PARENT, window, root, 0, 0, 10, 10, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new())
      .unwrap();
    app.map_window(window).unwrap();
    app.flush().unwrap();
    wait_for(|| app.get_window_attributes(window).unwrap().reply().unwrap().map_state == x11rb::protocol::xproto::MapState::VIEWABLE);
    app.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME).unwrap();
    wait_for(|| app.get_input_focus().unwrap().reply().unwrap().focus == window);
    ready.send(()).unwrap();
    wait_for(|| app.get_selection_owner(clipboard).unwrap().reply().unwrap().owner != x11rb::NONE);
    let convert = |target| {
      app.convert_selection(window, clipboard, target, property, x11rb::CURRENT_TIME).unwrap();
      app.flush().unwrap();
      while !matches!(app.wait_for_event().unwrap(), Event::SelectionNotify(_)) {}
      app.get_property(true, window, property, x11rb::protocol::xproto::AtomEnum::ANY, 0, 1024).unwrap().reply().unwrap().value
    };
    convert(targets);
    std::thread::sleep(Duration::from_secs(2));
    convert(utf8)
  });
  focused.recv().unwrap();
  let result = input.paste_text(submit("slow")).unwrap();
  assert_eq!(String::from_utf8(slow.join().unwrap()).unwrap(), "slow");
  assert_eq!(result.selected_path, InputDeliveryPath::ClipboardPaste);
  assert_eq!(owner(), x11rb::NONE);

  // 8. ROOT CAUSE: Chrome 130 reads CLIPBOARD in several rounds per Ctrl+V
  // and inserts a later one. The paste restored the clipboard right after the
  // first text read (settle 0), so Chrome inserted the old clipboard while
  // the result said the text was read. A consumer's later rounds within the
  // quiet window now get the pasted text. This thread reads twice, 100 ms
  // apart, from its focused window.
  let (ready, focused) = std::sync::mpsc::channel();
  let rounds = std::thread::spawn(move || {
    use x11rb::protocol::{Event, xproto::InputFocus};
    let (app, screen) = x11rb::connect(None).unwrap();
    let root = app.setup().roots[screen].root;
    let atom = |name: &[u8]| app.intern_atom(false, name).unwrap().reply().unwrap().atom;
    let (utf8, property) = (atom(b"UTF8_STRING"), atom(b"_AUV_TEST_PASTE"));
    let window = app.generate_id().unwrap();
    app
      .create_window(x11rb::COPY_DEPTH_FROM_PARENT, window, root, 0, 0, 10, 10, 0, WindowClass::INPUT_OUTPUT, 0, &CreateWindowAux::new())
      .unwrap();
    app.map_window(window).unwrap();
    app.flush().unwrap();
    wait_for(|| app.get_window_attributes(window).unwrap().reply().unwrap().map_state == x11rb::protocol::xproto::MapState::VIEWABLE);
    app.set_input_focus(InputFocus::PARENT, window, x11rb::CURRENT_TIME).unwrap();
    wait_for(|| app.get_input_focus().unwrap().reply().unwrap().focus == window);
    ready.send(()).unwrap();
    wait_for(|| app.get_selection_owner(clipboard).unwrap().reply().unwrap().owner != x11rb::NONE);
    let read = || {
      app.convert_selection(window, clipboard, utf8, property, x11rb::CURRENT_TIME).unwrap();
      app.flush().unwrap();
      while !matches!(app.wait_for_event().unwrap(), Event::SelectionNotify(_)) {}
      String::from_utf8(
        app.get_property(true, window, property, x11rb::protocol::xproto::AtomEnum::ANY, 0, 1024).unwrap().reply().unwrap().value,
      )
      .unwrap()
    };
    let first = read();
    std::thread::sleep(Duration::from_millis(100));
    (first, read())
  });
  focused.recv().unwrap();
  input.paste_text(submit("rounds")).unwrap();
  assert_eq!(rounds.join().unwrap(), ("rounds".to_string(), "rounds".to_string()));
  assert_eq!(owner(), x11rb::NONE);
}

/// One managed Tk window whose Entry reports submitted text and whose Listbox
/// reports its scroll position and left clicks. `pid` publishes `_NET_WM_PID` through xprop
/// because Tk does not set it.
fn spawn_managed_fixture(directory: &std::path::Path, class: &str, title: &str, geometry: &str, pid: bool) -> Child {
  Command::new("python3")
    .arg("-c")
    .arg(
      r#"
import os, pathlib, subprocess, sys, tkinter as tk
p, class_name, title, geometry, pid = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5] == '1'
root = tk.Tk(className=class_name)
root.title(title)
root.geometry(geometry)
entry = tk.Entry(root)
entry.pack(fill=tk.X)
listbox = tk.Listbox(root)
listbox.pack(fill=tk.BOTH, expand=True)
for i in range(200):
    listbox.insert(tk.END, 'item '+str(i))
def submit(event):
    (p/(class_name+'.text')).write_text(entry.get())
entry.bind('<Return>', submit)
def record_scroll(event):
    root.after(50, lambda: (p/(class_name+'.scroll')).write_text(str(listbox.yview()[0])))
listbox.bind('<Button-5>', record_scroll, add='+')
listbox.bind('<Button-1>', lambda event: (p/(class_name+'.click')).write_text('clicked'), add='+')
entry.focus_set()
root.update()
if pid:
    # The WM-managed client is Tk's wrapper, the parent of the root widget's
    # window; `wm frame` would name the window manager's decoration frame.
    tree = subprocess.run(['xwininfo', '-tree', '-id', str(root.winfo_id())], check=True, capture_output=True, text=True).stdout
    wrapper = next(line.split()[3] for line in tree.splitlines() if line.strip().startswith('Parent window id:'))
    subprocess.run(['xprop', '-id', wrapper, '-f', '_NET_WM_PID', '32c', '-set', '_NET_WM_PID', str(os.getpid())], check=True)
(p/(class_name+'.ready')).touch()
root.mainloop()
"#,
    )
    .arg(directory)
    .args([class, title, geometry, if pid { "1" } else { "0" }])
    .spawn()
    .unwrap()
}

fn read_file(directory: &std::path::Path, name: &str) -> Option<String> {
  fs::read_to_string(directory.join(name)).ok()
}

#[test]
#[ignore = "requires isolated Xvfb, an EWMH window manager (openbox), xprop, and python3-tk; manipulates the selected desktop"]
fn ewmh_windows_resolve_activate_and_receive_targeted_input() {
  let directory = std::env::temp_dir().join(format!("auv-x11-ewmh-test-{}", std::process::id()));
  fs::create_dir(&directory).unwrap();
  let alpha = spawn_managed_fixture(&directory, "AuvAlpha", "AUV Alpha", "400x300+20+20", true);
  let alpha_pid = alpha.id();
  let _alpha = Fixture {
    child: alpha,
    directory: directory.clone(),
  };
  wait_for(|| directory.join("AuvAlpha.ready").exists());
  // Beta maps last, overlaps Alpha's lower-right corner, and becomes active.
  let beta = spawn_managed_fixture(&directory, "AuvBeta", "AUV Beta", "400x300+300+200", false);
  let _beta = Fixture {
    child: beta,
    directory: directory.join("unused"),
  };
  wait_for(|| directory.join("AuvBeta.ready").exists());

  let session = X11Driver.open_local().unwrap();
  let windows = session.window();
  let find = |title: &str| -> Option<Window> { windows.list().unwrap().into_iter().find(|window| window.title.as_deref() == Some(title)) };
  wait_for(|| find("AUV Alpha").is_some() && find("AUV Beta").is_some());

  // window.list: EWMH identity, owner, and decorated frame.
  let alpha_window = find("AUV Alpha").unwrap();
  assert!(alpha_window.reference.id.starts_with("x11:0x"), "{alpha_window:?}");
  // Tk derives WM_CLASS ("auvAlpha", "Auvalpha") from className.
  assert_eq!(alpha_window.app_name.as_deref(), Some("Auvalpha"));
  assert_eq!(alpha_window.application_id.as_deref(), Some("Auvalpha"));
  assert_eq!(alpha_window.process_id, Some(alpha_pid));
  assert!(alpha_window.is_visible && alpha_window.is_main);
  assert!(alpha_window.frame.size.width >= 400.0 && alpha_window.frame.size.height >= 300.0, "{alpha_window:?}");
  let beta_window = find("AUV Beta").unwrap();
  assert_eq!(beta_window.process_id, None);
  assert_eq!(windows.list().unwrap().first().map(|window| window.reference.clone()), Some(beta_window.reference.clone()));

  // window.resolve: case-insensitive application ids, titles, frontmost.
  let resolved = windows.resolve(WindowSelector::default().owned_by(App::id("AuvAlpha"))).unwrap();
  assert_eq!(resolved.reference, alpha_window.reference);
  let resolved = windows.resolve(WindowSelector::default().title_contains("beta")).unwrap();
  assert_eq!(resolved.reference, beta_window.reference);
  let frontmost = WindowSelector {
    main_visible: true,
    ..WindowSelector::default().owned_by(App::frontmost())
  };
  assert_eq!(windows.resolve(frontmost.clone()).unwrap().reference, beta_window.reference);
  assert!(matches!(windows.resolve(WindowSelector::default().owned_by(App::id("missing"))), Err(DriverError::NotFound { .. })));

  let window_point = WindowPoint::new(5.0, 7.0);
  let screen = windows.to_screen_point(&alpha_window, window_point).unwrap();
  assert_eq!(screen, ScreenPoint::new(alpha_window.frame.origin.x + 5.0, alpha_window.frame.origin.y + 7.0));
  assert_eq!(windows.to_window_point(&alpha_window, screen).unwrap(), window_point);

  let input = session.input();
  let text = |text: &str| KeyboardInput::TypeText {
    text: text.into(),
    options: TypeTextOptions {
      policy: InputPolicy::ForegroundPreferred,
      submit: TextSubmit::Return,
      ..Default::default()
    },
  };

  // Validation and dry-run bind the recipient without activating it.
  let missing = InputTarget::Application {
    id: "missing".into(),
  };
  assert!(matches!(input.input_keyboard(&missing, vec![text("lost")], true).unwrap_err().cause, DriverError::NotFound { .. }));
  let alpha_app = InputTarget::Application {
    id: "auvalpha".into(),
  };
  assert!(input.input_keyboard(&alpha_app, vec![text("dry")], true).unwrap().is_none());
  let background = KeyboardInput::PressKeys {
    options: PressKeysOptions {
      keys: vec!["a".into()],
      ..Default::default()
    },
    policy: InputPolicy::BackgroundOnly,
  };
  assert!(matches!(input.input_keyboard(&alpha_app, vec![background], false).unwrap_err().cause, DriverError::Unsupported { .. }));
  assert_eq!(windows.resolve(frontmost.clone()).unwrap().reference, beta_window.reference);

  // Application target: activate Alpha, confirm focus, deliver foreground XTEST.
  let results = input.input_keyboard(&alpha_app, vec![text("alpha via x11")], false).unwrap().unwrap();
  wait_for(|| read_file(&directory, "AuvAlpha.text").as_deref() == Some("alpha via x11"));
  assert_eq!(windows.resolve(frontmost.clone()).unwrap().reference, alpha_window.reference);
  let result = &results[0];
  assert_eq!(result.selected_path, InputDeliveryPath::ForegroundSystemEvents);
  assert_eq!(result.attempts[0].path, InputDeliveryPath::WindowTargetedKeyboard);
  assert!(result.fallback_reason().is_some_and(|reason| reason.contains("X11 has no window-bound keyboard delivery")), "{result:?}");
  assert!(result.fallback_reason().is_some_and(|reason| reason.contains(&format!("(pid {alpha_pid})"))), "{result:?}");
  assert_eq!(result.focus_disturbance, DisturbanceLevel::Foreground);
  assert!(!result.verified);
  assert!(read_file(&directory, "AuvBeta.text").is_none());

  // Window target: the exact observed window, including one without a pid.
  let results = input.input_keyboard(&InputTarget::Window(beta_window.clone()), vec![text("beta via x11")], false).unwrap().unwrap();
  wait_for(|| read_file(&directory, "AuvBeta.text").as_deref() == Some("beta via x11"));
  assert!(results[0].fallback_reason().is_some_and(|reason| reason.contains("(pid unknown)")));
  assert_eq!(read_file(&directory, "AuvAlpha.text").as_deref(), Some("alpha via x11"));

  // A stale owner is rejected before activation or delivery.
  let stale = Window {
    process_id: Some(alpha_pid + 100_000),
    ..alpha_window.clone()
  };
  assert!(matches!(
    input.input_keyboard(&InputTarget::Window(stale), vec![text("stale")], false).unwrap_err().cause,
    DriverError::InvalidInput { .. }
  ));
  assert_eq!(windows.resolve(frontmost).unwrap().reference, beta_window.reference);

  // Window-relative scroll: Beta covers this point of Alpha's listbox, so the
  // wheel only reaches Alpha after activation raises it.
  let alpha_window = find("AUV Alpha").unwrap();
  let point = WindowPoint::new(alpha_window.frame.size.width - 40.0, alpha_window.frame.size.height - 40.0);
  let covered = windows.to_screen_point(&alpha_window, point).unwrap().point();
  assert!(beta_window.frame.origin.x < covered.x && beta_window.frame.origin.y < covered.y, "{covered:?} {beta_window:?}");
  let result = windows.scroll(&alpha_window, point, Scroll::new(0.0, 360.0), ScrollOptions::default()).unwrap();
  wait_for(|| read_file(&directory, "AuvAlpha.scroll").and_then(|value| value.parse::<f64>().ok()).is_some_and(|value| value > 0.0));
  assert!(read_file(&directory, "AuvBeta.scroll").is_none());
  assert_eq!(result.attempts[0].path, InputDeliveryPath::WindowTargetedWheel);
  assert!(result.fallback_reason().is_some_and(|reason| reason.contains("X11 has no window-bound wheel delivery")), "{result:?}");
  assert_eq!(result.focus_disturbance, DisturbanceLevel::Foreground);
  assert_eq!(input.current_position().unwrap(), Point::new(covered.x.round(), covered.y.round()));
  assert!(windows.scroll(&alpha_window, WindowPoint::new(-1.0, 0.0), Scroll::new(0.0, 120.0), ScrollOptions::default()).is_err());

  // Window-relative click: Alpha is active now and covers this point of
  // Beta's listbox, so the click only reaches Beta after activation raises it.
  let beta_window = find("AUV Beta").unwrap();
  let point = WindowPoint::new(40.0, 80.0);
  let covered = windows.to_screen_point(&beta_window, point).unwrap().point();
  let alpha_frame = find("AUV Alpha").unwrap().frame;
  assert!(covered.x < alpha_frame.origin.x + alpha_frame.size.width && covered.y < alpha_frame.origin.y + alpha_frame.size.height);
  let result = windows.click(&beta_window, point, ClickOptions::default()).unwrap();
  wait_for(|| read_file(&directory, "AuvBeta.click").is_some());
  assert!(read_file(&directory, "AuvAlpha.click").is_none());
  assert_eq!(result.attempts[0].path, InputDeliveryPath::WindowTargetedMouse);
  assert!(result.fallback_reason().is_some_and(|reason| reason.contains("X11 has no window-bound pointer delivery")), "{result:?}");
  assert_eq!(result.focus_disturbance, DisturbanceLevel::Foreground);
  assert_eq!(input.current_position().unwrap(), Point::new(covered.x.round(), covered.y.round()));
  let background = ClickOptions {
    policy: InputPolicy::BackgroundOnly,
    ..ClickOptions::default()
  };
  assert!(matches!(windows.click(&beta_window, point, background), Err(DriverError::InvalidInput { .. })));

  // Window drag: Beta is active and covers Alpha's lower-right corner again;
  // a zero-length drag presses there only after activation raises Alpha.
  let alpha_window = find("AUV Alpha").unwrap();
  let point = WindowPoint::new(alpha_window.frame.size.width - 40.0, alpha_window.frame.size.height - 40.0);
  let covered = windows.to_screen_point(&alpha_window, point).unwrap().point();
  let (end, result) =
    windows.drag(&alpha_window, MoveMouseRequest::direct(covered), MouseButton::Left, InputPolicy::ForegroundPreferred).unwrap();
  wait_for(|| read_file(&directory, "AuvAlpha.click").is_some());
  assert_eq!(end, covered);
  assert_eq!(result.attempts[0].path, InputDeliveryPath::WindowTargetedMouse);
  assert_eq!(result.focus_disturbance, DisturbanceLevel::Foreground);

  // ROOT CAUSE: OSWorld agents ran `input.drag --target window:…`, which
  // the Runner sends through the logical-mouse path; X11 rejected every
  // window target there ("window-targeted mouse input is not supported").
  // A window target now activates the window and drives foreground XTEST.
  fs::remove_file(directory.join("AuvBeta.click")).unwrap();
  let beta_window = find("AUV Beta").unwrap();
  let point = windows.to_screen_point(&beta_window, WindowPoint::new(40.0, 80.0)).unwrap().point();
  let request = MoveMouseRequest {
    target: Some(InputTarget::Window(beta_window.clone())),
    ..MoveMouseRequest::direct(point)
  };
  let (_, result) = input.drag_mouse(request, MouseButton::Left).unwrap();
  wait_for(|| read_file(&directory, "AuvBeta.click").is_some());
  assert_eq!(result.attempts[0].path, InputDeliveryPath::WindowTargetedMouse);
  assert_eq!(result.focus_disturbance, DisturbanceLevel::Foreground);

  // ROOT CAUSE: `window.capture` was unsupported on X11, so window.findText,
  // waitForText, and clickText failed in OSWorld. It now crops the window's
  // frame from its monitor and binds the crop to the window.
  let capture = windows.capture(&beta_window).unwrap();
  assert_eq!(capture.bounds, beta_window.frame);
  assert_eq!((capture.image.width(), capture.image.height()), (beta_window.frame.size.width as u32, beta_window.frame.size.height as u32));
  assert_eq!(capture.origin, Some(auv_driver_common::Position::in_window(&beta_window.reference, WindowPoint::new(0.0, 0.0))));
}

// app.activate on X11 resolves the application's window, activates it through
// EWMH `_NET_ACTIVE_WINDOW`, and verifies the active window afterwards.
#[test]
#[ignore = "requires isolated Xvfb, an EWMH window manager (openbox), xprop, and python3-tk; manipulates the selected desktop"]
fn ewmh_application_activation_foregrounds_and_verifies() {
  let directory = std::env::temp_dir().join(format!("auv-x11-activate-test-{}", std::process::id()));
  fs::create_dir(&directory).unwrap();
  let alpha = spawn_managed_fixture(&directory, "AuvAlpha", "AUV Alpha", "400x300+20+20", true);
  let _alpha = Fixture {
    child: alpha,
    directory: directory.clone(),
  };
  wait_for(|| directory.join("AuvAlpha.ready").exists());
  let beta = spawn_managed_fixture(&directory, "AuvBeta", "AUV Beta", "400x300+300+200", false);
  let _beta = Fixture {
    child: beta,
    directory: directory.join("unused"),
  };
  wait_for(|| directory.join("AuvBeta.ready").exists());

  let session = X11Driver.open_local().unwrap();
  let windows = session.window();
  let find = |title: &str| -> Option<Window> { windows.list().unwrap().into_iter().find(|window| window.title.as_deref() == Some(title)) };
  wait_for(|| find("AUV Alpha").is_some() && find("AUV Beta").is_some());

  for (application, title) in [
    ("AuvAlpha", "AUV Alpha"),
    ("auvbeta", "AUV Beta"),
    ("AuvAlpha", "AUV Alpha"),
  ] {
    let result = windows.activate_application(application, std::time::Duration::from_millis(100)).unwrap();
    assert_eq!(result.method, auv_driver_common::ApplicationActivationMethod::EwmhActiveWindow);
    assert_eq!(result.activated_window, Some(find(title).unwrap().reference), "{application}");
    assert!(
      matches!(result.verification, auv_driver_common::ApplicationActivationVerification::VerifiedForeground { .. }),
      "{application}: {:?}",
      result.verification
    );
    assert_eq!(windows.list().unwrap().first().and_then(|window| window.title.clone()).as_deref(), Some(title));
  }
  assert!(matches!(windows.activate_application("missing", std::time::Duration::ZERO), Err(DriverError::NotFound { .. })));
}
