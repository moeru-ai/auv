//! Real X11 integration probe. Run only in an isolated 800x600 Xvfb session.
#![cfg(target_os = "linux")]

use auv_driver_common::{
  CaptureOptions, Click, ClickModifiers, Driver, InputPolicy, InputTarget, KeyPressOptions, MoveMouseRequest, Point, Rect, Scroll,
  TextSubmit, TypeTextOptions, input::MouseButton,
};
use auv_driver_linux_x11::X11Driver;
use std::{
  fs,
  path::PathBuf,
  process::{Child, Command},
  time::{Duration, Instant},
};

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
