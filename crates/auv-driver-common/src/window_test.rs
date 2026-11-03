use super::*;
use std::time::Duration;

#[test]
fn window_mutation_options_default_to_native_preferred_ax_candidates() {
  let options = WindowMutationOptions::default();

  assert_eq!(options.policy, WindowMutationPolicy::NativePreferred);
  assert_eq!(
    options.strategy,
    WindowMutationStrategy {
      candidates: vec![
        WindowMutationCandidate::AxWindowAttribute,
        WindowMutationCandidate::AxWindowAction,
      ],
    }
  );
  assert_eq!(options.settle, Duration::from_millis(100));
  assert_eq!(options.verification, WindowMutationVerification::FrameTolerance { points: 2.0 });
}

#[test]
fn window_mutation_types_serde_as_snake_case() {
  let result = WindowMutationResult {
    selected_path: WindowMutationPath::AxWindowAttribute,
    attempts: vec![
      WindowMutationAttempt::failure(WindowMutationPath::PlatformNative, "native mutation unavailable"),
      WindowMutationAttempt::success(WindowMutationPath::AxWindowAttribute, "set AXPosition"),
    ],
    before_frame: Some(Rect::new(0.0, 0.0, 400.0, 300.0)),
    after_frame: Some(Rect::new(10.0, 20.0, 400.0, 300.0)),
    before_state: Some(WindowState {
      is_minimized: Some(false),
      is_visible: Some(true),
      ..Default::default()
    }),
    after_state: Some(WindowState {
      is_minimized: Some(false),
      is_visible: Some(true),
      ..Default::default()
    }),
    adjusted_by_app: false,
    focus_disturbance: DisturbanceLevel::None,
    mouse_disturbance: DisturbanceLevel::None,
  };

  let encoded = serde_json::to_value(&result).expect("serialize");
  assert_eq!(encoded["selected_path"], "ax_window_attribute");
  assert_eq!(encoded["attempts"][1]["path"], "ax_window_attribute");
  assert!(encoded.get("fallback_reason").is_none());
  assert_eq!(result.fallback_reason(), Some("native mutation unavailable"));

  let decoded: WindowMutationResult = serde_json::from_value(encoded).expect("deserialize");
  assert_eq!(decoded, result);
}

fn window(id: &str) -> Window {
  Window {
    reference: WindowRef { id: id.to_string() },
    title: None,
    app_name: None,
    application_id: None,
    process_id: None,
    frame: Rect::new(0.0, 0.0, 10.0, 10.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: true,
    state: Default::default(),
  }
}

#[test]
fn find_window_returns_the_window_with_the_exact_id() {
  let found = find_window([window("1"), window("12")], "12").expect("window 12 is listed");

  assert_eq!(found.reference.id, "12");
}

#[test]
fn find_window_reports_a_missing_window_as_not_found() {
  let error = find_window([window("1")], "2").expect_err("window 2 is not listed");

  assert!(matches!(error, DriverError::NotFound { ref target } if target == "window:2"), "{error:?}");
}

#[test]
fn visible_applications_hint_sorts_dedups_and_caps_the_list() {
  assert_eq!(visible_applications_hint([]), "no visible application windows");
  assert_eq!(visible_applications_hint(["b.exe", "A.exe", "a.EXE", " "]), "visible applications: A.exe, b.exe");
  let many = (0..25).map(|index| format!("app{index:02}")).collect::<Vec<_>>();
  let hint = visible_applications_hint(many.iter().map(String::as_str));
  assert!(hint.starts_with("visible applications: app00, app01,"), "{hint}");
  assert!(hint.ends_with("app19, … (5 more)"), "{hint}");
}

fn minimized(mut window: Window) -> Window {
  window.is_visible = false;
  window.state.is_minimized = Some(true);
  window
}

fn stacked(mut window: Window, stack_index: u32) -> Window {
  window.state.stack_index = Some(stack_index);
  window
}

fn selected(candidates: &[Window]) -> Option<&str> {
  select_main_window(candidates).map(|window| window.reference.id.as_str())
}

#[test]
fn select_main_window_takes_a_single_minimized_window() {
  assert_eq!(selected(&[minimized(window("1"))]), Some("1"));
  assert_eq!(selected(&[]), None);
}

#[test]
fn select_main_window_prefers_a_shown_window_over_a_minimized_one() {
  // The minimized window is frontmost and main; being shown still wins.
  let mut hidden = stacked(minimized(window("hidden")), 0);
  hidden.is_main = true;

  assert_eq!(selected(&[hidden, stacked(window("shown"), 1)]), Some("shown"));
}

#[test]
fn select_main_window_takes_the_frontmost_of_several_minimized_windows() {
  let windows = [
    stacked(minimized(window("back")), 3),
    stacked(minimized(window("front")), 1),
  ];

  assert_eq!(selected(&windows), Some("front"));
}

#[test]
fn select_main_window_takes_another_desktop_window_only_when_it_is_the_only_one() {
  let mut elsewhere = stacked(window("elsewhere"), 0);
  elsewhere.state.desktop = Some(WindowDesktop {
    id: "other".into(),
    is_current: false,
  });

  assert_eq!(selected(&[elsewhere.clone(), minimized(window("here"))]), Some("here"));
  assert_eq!(selected(&[elsewhere]), Some("elsewhere"));
}

#[test]
fn select_main_window_prefers_the_focused_window_over_the_main_one() {
  let mut main = window("main");
  main.is_main = true;
  let mut focused = window("focused");
  focused.state.is_focused = Some(true);

  assert_eq!(selected(&[main, focused]), Some("focused"));
}

#[test]
fn select_main_window_breaks_ties_by_stack_then_area_then_listing_order() {
  let mut large = window("large");
  large.frame = Rect::new(0.0, 0.0, 800.0, 600.0);

  // A stacking position beats a larger frame without one.
  assert_eq!(selected(&[large.clone(), stacked(window("stacked"), 5)]), Some("stacked"));
  assert_eq!(selected(&[window("small"), large]), Some("large"));
  assert_eq!(selected(&[window("first"), window("second")]), Some("first"));
}

#[test]
fn window_list_filter_keeps_every_window_by_default() {
  let other_desktop = Window {
    state: WindowState {
      desktop: Some(WindowDesktop {
        id: "b".to_string(),
        is_current: false,
      }),
      ..Default::default()
    },
    ..window("2")
  };
  let windows = [minimized(window("1")), other_desktop, window("3")];

  let kept = |filter: WindowListFilter| {
    windows.iter().filter(|window| filter.keeps(window)).map(|window| window.reference.id.as_str()).collect::<Vec<_>>()
  };

  assert_eq!(kept(WindowListFilter::default()), ["1", "2", "3"]);
  assert_eq!(
    kept(WindowListFilter {
      exclude_minimized: true,
      ..Default::default()
    }),
    ["2", "3"]
  );
  // Window 3 has no known desktop, so it stays.
  assert_eq!(
    kept(WindowListFilter {
      exclude_other_desktops: true,
      ..Default::default()
    }),
    ["1", "3"]
  );
}
