use super::*;

fn window() -> Window {
  Window {
    reference: WindowRef {
      id: "42".to_string(),
    },
    title: Some("Untitled".to_string()),
    app_name: Some("TextEdit".to_string()),
    application_id: Some("com.apple.TextEdit".to_string()),
    process_id: Some(7),
    frame: Rect::new(10.0, 20.0, 400.0, 300.0),
    coordinate_space: CoordinateSpace::Screen,
    is_main: true,
    is_visible: false,
    state: WindowState {
      is_minimized: Some(true),
      is_maximized: Some(false),
      is_focused: None,
      stack_index: Some(3),
      desktop: Some(WindowDesktop {
        id: "desk-1".to_string(),
        is_current: false,
      }),
      ..Default::default()
    },
  }
}

#[test]
fn window_round_trips_with_unknown_facts_kept_apart_from_false() {
  let encoded = encode(window());

  assert_eq!(encoded.state.as_ref().and_then(|state| state.is_maximized), Some(false));
  assert_eq!(encoded.state.as_ref().and_then(|state| state.is_focused), None);
  assert_eq!(decode(encoded), Ok(window()));
}

#[test]
fn mutation_result_round_trips_and_rejects_an_unspecified_path() {
  let result = WindowMutationResult {
    selected_path: WindowMutationPath::AxWindowAttribute,
    attempts: vec![WindowMutationAttempt::success(
      WindowMutationPath::AxWindowAttribute,
      "pass 1: wrote origin",
    )],
    before_frame: Some(Rect::new(0.0, 0.0, 400.0, 300.0)),
    after_frame: Some(Rect::new(10.0, 20.0, 400.0, 300.0)),
    before_state: Some(window().state),
    after_state: Some(WindowState::default()),
    adjusted_by_app: true,
    focus_disturbance: DisturbanceLevel::Foreground,
    mouse_disturbance: DisturbanceLevel::None,
  };

  assert_eq!(decode_mutation_result(encode_mutation_result(result.clone())), Ok(result.clone()));

  let mut wire = encode_mutation_result(result);
  wire.selected_path = proto::WindowMutationPath::Unspecified as i32;
  assert_eq!(decode_mutation_result(wire), Err(DecodeError::UnknownEnum("WindowMutationPath")));
}
