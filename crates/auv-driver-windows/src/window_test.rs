use auv_driver_common::geometry::{CoordinateSpace, Rect};
use auv_driver_common::selector::{App, TextMatcher, Window as WindowQuery, WindowSelector};
use auv_driver_common::window::{Window, WindowRef};

use super::*;

fn window(id: &str, title: Option<&str>, app: Option<&str>, pid: u32, frame: Rect) -> Window {
  Window {
    reference: WindowRef { id: id.to_string() },
    title: title.map(str::to_string),
    app_name: app.map(str::to_string),
    app_bundle_id: None,
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

#[test]
fn invisible_windows_are_never_matched() {
  let mut hidden = window("1", Some("Doc"), Some("app.exe"), 10, Rect::new(0.0, 0.0, 100.0, 100.0));
  hidden.is_visible = false;

  let selector = WindowQuery::title_exact("Doc");

  assert!(resolve_from_windows(&[hidden], &selector).is_err());
}

#[test]
fn bundle_selector_never_matches_on_windows() {
  let windows = vec![window(
    "1",
    Some("Doc"),
    Some("app.exe"),
    10,
    Rect::new(0.0, 0.0, 100.0, 100.0),
  )];

  let selector = WindowSelector::default().owned_by(AppSelector {
    bundle: Some(TextMatcher::Exact("com.example.app".to_string())),
    ..AppSelector::default()
  });

  assert!(resolve_from_windows(&windows, &selector).is_err());
}
