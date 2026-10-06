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
    }),
    after_state: Some(WindowState {
      is_minimized: Some(false),
      is_visible: Some(true),
    }),
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
    app_bundle_id: None,
    process_id: None,
    frame: Rect::new(0.0, 0.0, 10.0, 10.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: false,
    is_visible: true,
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
