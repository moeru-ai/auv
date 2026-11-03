use auv_driver_common::geometry::{CoordinateSpace, Rect};
use auv_driver_common::selector::{App, TextMatcher, Window as WindowQuery, WindowSelector};
use auv_driver_common::window::{Window, WindowRef};

use super::*;

fn window(id: &str, title: Option<&str>, app: Option<&str>, pid: u32, frame: Rect) -> Window {
  Window {
    reference: WindowRef { id: id.to_string() },
    title: title.map(str::to_string),
    app_name: app.map(str::to_string),
    application_id: None,
    process_id: (pid != 0).then_some(pid),
    frame,
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: true,
    state: Default::default(),
  }
}

#[test]
fn resolve_returns_single_title_match() {
  let windows = vec![
    window("1", Some("Editor"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 100.0, 100.0)),
    window("2", Some("Browser"), Some("web.exe"), 20, Rect::new(0.0, 0.0, 100.0, 100.0)),
  ];

  let selector = WindowQuery::title_contains("Brow");
  let resolved = resolve_from_windows(&windows, &selector).expect("one window matches");

  assert_eq!(resolved.reference.id, "2");
}

#[test]
fn resolve_reports_ambiguous_match() {
  let windows = vec![
    window("1", Some("Doc"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 100.0, 100.0)),
    window("2", Some("Doc"), Some("app.exe"), 11, Rect::new(0.0, 0.0, 100.0, 100.0)),
  ];

  let selector = WindowQuery::title_exact("Doc");

  assert!(resolve_from_windows(&windows, &selector).is_err());
}

#[test]
fn resolve_reports_not_found_when_nothing_matches() {
  let windows = vec![window(
    "1",
    Some("Doc"),
    Some("app.exe"),
    10,
    Rect::new(0.0, 0.0, 100.0, 100.0),
  )];

  let selector = WindowQuery::title_exact("Missing");

  assert!(resolve_from_windows(&windows, &selector).is_err());
}

#[test]
fn main_visible_prefers_foreground_then_largest() {
  let mut foreground = window("fg", Some("Front"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 50.0, 50.0));
  foreground.is_main = true;
  let big = window("big", Some("Big"), Some("app.exe"), 11, Rect::new(0.0, 0.0, 800.0, 600.0));
  let windows = vec![big, foreground];

  let resolved = resolve_from_windows(&windows, &WindowQuery::main_visible()).expect("a main visible window resolves");

  assert_eq!(resolved.reference.id, "fg");
}

#[test]
fn main_visible_falls_back_to_largest_without_foreground() {
  let windows = vec![
    window("small", Some("Small"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 50.0, 50.0)),
    window("big", Some("Big"), Some("app.exe"), 11, Rect::new(0.0, 0.0, 800.0, 600.0)),
  ];

  let resolved = resolve_from_windows(&windows, &WindowQuery::main_visible()).expect("largest window resolves");

  assert_eq!(resolved.reference.id, "big");
}

#[test]
fn app_selector_matches_by_name_and_pid() {
  let windows = vec![
    window("1", Some("A"), Some("editor.exe"), 10, Rect::new(0.0, 0.0, 100.0, 100.0)),
    window("2", Some("B"), Some("browser.exe"), 20, Rect::new(0.0, 0.0, 100.0, 100.0)),
  ];

  let by_name = WindowSelector::default().owned_by(App::name("browser.exe"));
  assert_eq!(resolve_from_windows(&windows, &by_name).unwrap().reference.id, "2");

  let by_pid = WindowSelector::default().owned_by(App::pid(10));
  assert_eq!(resolve_from_windows(&windows, &by_pid).unwrap().reference.id, "1");
}

// An executable file name is case-insensitive on Windows: `Notepad.exe` is the same program as
// `notepad.exe`. Titles stay case-sensitive, as in the macOS resolver.
#[test]
fn app_name_matches_an_executable_name_ignoring_case() {
  let windows = vec![
    window("1", Some("A"), Some("Notepad.exe"), 10, Rect::new(0.0, 0.0, 100.0, 100.0)),
    window("2", Some("B"), Some("msedge.exe"), 20, Rect::new(0.0, 0.0, 100.0, 100.0)),
  ];

  let lower = WindowSelector::default().owned_by(App::name("notepad.exe"));
  assert_eq!(resolve_from_windows(&windows, &lower).unwrap().reference.id, "1");

  let upper = WindowSelector::default().owned_by(App::name("MSEDGE.EXE"));
  assert_eq!(resolve_from_windows(&windows, &upper).unwrap().reference.id, "2");

  let partial = WindowSelector::default().owned_by(AppSelector {
    name: Some(TextMatcher::Contains("PAD".to_string())),
    ..AppSelector::default()
  });
  assert_eq!(resolve_from_windows(&windows, &partial).unwrap().reference.id, "1");

  let title = WindowSelector::default().title_exact("a");
  assert!(resolve_from_windows(&windows, &title).is_err());
}

#[test]
fn invisible_windows_are_never_matched() {
  let mut hidden = window("1", Some("Doc"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 100.0, 100.0));
  hidden.is_visible = false;

  let selector = WindowQuery::title_exact("Doc");

  assert!(resolve_from_windows(&[hidden], &selector).is_err());
}

#[test]
fn application_id_that_is_not_the_executable_is_rejected() {
  let windows = vec![window(
    "1",
    Some("Doc"),
    Some("app.exe"),
    10,
    Rect::new(0.0, 0.0, 100.0, 100.0),
  )];

  let selector = WindowSelector::default().owned_by(AppSelector {
    id: Some(TextMatcher::Exact("com.example.app".to_string())),
    ..AppSelector::default()
  });

  assert!(resolve_from_windows(&windows, &selector).is_err());
}

// ROOT CAUSE:
//
// If a caller targeted an application with `--target app:<id>`, the CLI built
// a bundle-id selector, and Windows windows carry no bundle id, so every such
// command failed with a bare "main visible window was not found", even for a
// running `Notepad.exe`.
//
// The fix matches the id against the executable name as well (ignoring case
// and `.exe`), and a miss lists the visible applications.
#[test]
fn application_id_matches_the_executable_and_a_miss_lists_visible_applications() {
  let windows = vec![
    window("1", Some("notes.txt - Notepad"), Some("Notepad.exe"), 10, Rect::new(0.0, 0.0, 800.0, 600.0)),
    window("2", Some("Edge"), Some("msedge.exe"), 11, Rect::new(0.0, 0.0, 800.0, 600.0)),
  ];
  // The CLI's local path selects by name, a Runner client by application id.
  for id in ["notepad", "notepad.exe", "Notepad.exe"] {
    for app in [App::id(id), App::name(id)] {
      let selector = WindowQuery::main_visible().owned_by(app);
      assert_eq!(resolve_from_windows(&windows, &selector).unwrap().reference.id, "1", "{id}");
    }
  }
  let error = resolve_from_windows(&windows, &WindowQuery::main_visible().owned_by(App::id("com.google.Chrome"))).unwrap_err();
  assert_eq!(
    error.to_string(),
    auv_driver_common::error::DriverError::NotFound {
      target: "application \"com.google.Chrome\" (visible applications: msedge.exe, Notepad.exe)".into()
    }
    .to_string()
  );
}

// Prior art: trycua/cua `compensates_for_invisible_resize_borders`. The outer rectangle reaches
// 7 px past the visible frame on the left, right and bottom, so a visible request grows by it.
#[test]
fn a_visible_frame_request_converts_to_the_outer_rectangle_by_the_invisible_border() {
  let border = invisible_border(Rect::new(40.0, 40.0, 886.0, 633.0), Rect::new(47.0, 40.0, 872.0, 626.0));

  assert_eq!(outer_frame_for_visible(Rect::new(65.0, 52.0, 872.0, 626.0), border), Rect::new(58.0, 52.0, 886.0, 633.0));
  // Measured on luoling-windows-11 at 150%: a minimized DPI-unaware window's iconic stub has the
  // same border, which turns its restore rectangle into the visible frame it had before it was
  // minimized. A per-monitor aware stub has no bottom border (see `restore_border`).
  let stub = invisible_border(Rect::new(-48000.0, -48000.0, 356.0, 59.0), Rect::new(-47989.0, -48000.0, 334.0, 48.0));
  assert_eq!(Rect::new(450.0, 300.0, 960.0, 720.0).inset(stub), Rect::new(461.0, 300.0, 938.0, 709.0));
}

// ROOT CAUSE:
//
// If a per-monitor aware window was minimized, `window.list` reported its restore frame 9 px too
// tall because the border was measured on the iconic stub, whose DWM bounds have no bottom inset.
//
// Before the fix, Paint at 150% (luoling-windows-11, 2026-11-04) listed 1600x1009 minimized and
// 1600x1000 normal.
// The fix keeps the listed restore frame equal to the visible frame the window has once normal.
#[test]
fn a_minimized_per_monitor_aware_window_lists_its_restore_frame_without_the_stub_bottom() {
  // Paint's stub and restore rectangle, measured: the stub insets 9, 9 and 0 px.
  let stub_outer = Rect::new(-32000.0, -32000.0, 237.0, 39.0);
  let stub_visible = Rect::new(-31991.0, -32000.0, 219.0, 39.0);
  let restore_outer = Rect::new(491.0, 400.0, 1618.0, 1009.0);

  let frame = restore_outer.inset(restore_border(stub_outer, stub_visible, true));

  assert_eq!(frame, Rect::new(500.0, 400.0, 1600.0, 1000.0));
}

#[test]
fn a_restore_border_keeps_a_complete_or_empty_stub_border_and_drops_a_maximized_top() {
  // A DPI-unaware stub keeps its 11 px bottom; a borderless window's stub has no border.
  let unaware = restore_border(Rect::new(-48000.0, -48000.0, 356.0, 59.0), Rect::new(-47989.0, -48000.0, 334.0, 48.0), true);
  assert_eq!(Rect::new(600.0, 450.0, 2400.0, 1500.0).inset(unaware), Rect::new(611.0, 450.0, 2378.0, 1489.0));
  let borderless = restore_border(Rect::new(-32000.0, -32000.0, 237.0, 39.0), Rect::new(-32000.0, -32000.0, 237.0, 39.0), true);
  assert_eq!(Rect::new(500.0, 400.0, 1200.0, 800.0).inset(borderless), Rect::new(500.0, 400.0, 1200.0, 800.0));
  // A maximized window hangs 11 px past every side; the normal frame has no top border.
  let maximized = restore_border(Rect::new(-11.0, -11.0, 3862.0, 2110.0), Rect::new(0.0, 0.0, 3840.0, 2088.0), false);
  assert_eq!(maximized.top, 0.0);
  assert_eq!((maximized.left, maximized.right, maximized.bottom), (11.0, 11.0, 11.0));
}

// `main_visible` runs the shared main-window cascade: a shown window beats a minimized one, an
// application whose only window is minimized still resolves to it (so `application.activate` restores it),
// and a window on another desktop is chosen only when the app has no other. A title selector never
// picks a window on another desktop.
#[test]
fn main_visible_prefers_shown_windows_and_falls_back_to_minimized_then_other_desktops() {
  let mut minimized = window("1", Some("Doc"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 1600.0, 1200.0));
  minimized.is_visible = false;
  minimized.state.is_minimized = Some(true);
  let shown = window("2", Some("Doc 2"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 400.0, 300.0));
  let mut elsewhere = window("3", Some("Doc 3"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 2000.0, 1500.0));
  elsewhere.is_visible = false;
  elsewhere.state.desktop = Some(auv_driver_common::window::WindowDesktop {
    id: "other".into(),
    is_current: false,
  });
  let main = WindowQuery::main_visible().owned_by(App::name("app.exe"));

  let windows = [minimized.clone(), shown, elsewhere.clone()];
  assert_eq!(resolve_from_windows(&windows, &main).unwrap().reference.id, "2");
  assert_eq!(resolve_from_windows(&[minimized, elsewhere.clone()], &main).unwrap().reference.id, "1");
  assert_eq!(resolve_from_windows(std::slice::from_ref(&elsewhere), &main).unwrap().reference.id, "3");
  assert!(resolve_from_windows(&[elsewhere], &WindowQuery::title_exact("Doc 3")).is_err());
}

// `rcNormalPosition` is in workspace coordinates. A secondary monitor at -1920,0 with a 72 px
// taskbar on its left has its workspace origin at screen -1848,0.
#[test]
fn workspace_coordinates_are_offset_by_the_work_area() {
  let monitor = Rect::new(-1920.0, 0.0, 1920.0, 1080.0);
  let work = Rect::new(-1848.0, 0.0, 1848.0, 1080.0);
  let offset = workspace_offset(monitor, work);

  let screen = Rect::new(-1500.0, 100.0, 800.0, 600.0);
  let workspace = screen.offset(-offset.x, -offset.y);
  assert_eq!(workspace, Rect::new(-1572.0, 100.0, 800.0, 600.0));
  assert_eq!(workspace.offset(offset.x, offset.y), screen);
  // A bottom taskbar leaves the origin alone.
  assert_eq!(workspace_offset(Rect::new(0.0, 0.0, 3840.0, 2160.0), Rect::new(0.0, 0.0, 3840.0, 2088.0)).x, 0.0);
}
