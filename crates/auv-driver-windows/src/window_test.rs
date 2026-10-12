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
