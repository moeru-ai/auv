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
