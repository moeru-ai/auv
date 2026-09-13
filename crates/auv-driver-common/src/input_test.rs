use std::time::Duration;

use super::{
  Click, ClickOptions, DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath, InputPolicy, InputPreparationLease, Scroll,
  ScrollDeliveryCandidate, ScrollDeliveryStrategy, ScrollOptions, WindowClickStrategy,
};

#[test]
fn click_options_preserve_requested_modifiers_in_recorded_input() {
  let mut request = serde_json::to_value(ClickOptions::default()).unwrap();
  request["modifiers"] = serde_json::json!({"meta": true, "shift": true, "alt": false, "control": false});
  let decoded: ClickOptions = serde_json::from_value(request.clone()).unwrap();
  assert_eq!(serde_json::to_value(decoded).unwrap()["modifiers"], request["modifiers"]);
}

#[test]
fn existing_click_record_without_modifiers_remains_an_ordinary_click() {
  let mut request = serde_json::to_value(ClickOptions::default()).unwrap();
  request.as_object_mut().unwrap().remove("modifiers");
  let decoded: ClickOptions = serde_json::from_value(request).unwrap();
  assert!(decoded.modifiers.is_empty());
}

#[test]
fn click_modifiers_reject_physical_key_fields() {
  let error = serde_json::from_value::<super::ClickModifiers>(serde_json::json!({"keycode": 61})).unwrap_err();
  assert!(error.to_string().contains("unknown field"));
}

#[test]
fn dispatched_input_is_explicitly_unverified_on_the_wire() {
  let result = InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse);
  let encoded = serde_json::to_value(&result).expect("serialize input action result");

  assert!(!result.verified);
  assert_eq!(encoded["verified"], false);
}

#[test]
fn fallback_reason_is_derived_from_attempts_and_not_duplicated_on_the_wire() {
  let result = InputActionResult {
    selected_path: InputDeliveryPath::ForegroundSystemEvents,
    attempts: vec![
      InputAttempt::failure(InputDeliveryPath::WindowTargetedMouse, "background delivery failed"),
      InputAttempt::success(InputDeliveryPath::ForegroundSystemEvents),
    ],
    verified: false,
    mouse_disturbance: DisturbanceLevel::Temporary,
    focus_disturbance: DisturbanceLevel::Foreground,
    clipboard_disturbance: DisturbanceLevel::None,
  };

  assert_eq!(result.fallback_reason(), Some("background delivery failed"));
  assert!(serde_json::to_value(result).expect("serialize input action result").get("fallback_reason").is_none());
}

#[test]
fn input_action_result_rejects_success_on_a_path_other_than_the_selected_path() {
  let result = InputActionResult {
    selected_path: InputDeliveryPath::WindowTargetedMouse,
    attempts: vec![InputAttempt::success(InputDeliveryPath::AxPress)],
    verified: false,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::None,
    clipboard_disturbance: DisturbanceLevel::None,
  };

  assert_eq!(result.validate(), Err("successful input attempt must match selected_path".to_string()));
}

#[test]
fn click_and_click_options_serde_roundtrip() {
  let clicks = [
    Click::Single,
    Click::Double {
      interval: Duration::from_millis(42),
    },
    Click::Repeated {
      count: 3,
      interval: Duration::from_millis(75),
    },
  ];

  for click in clicks {
    let encoded = serde_json::to_string(&click).expect("serialize click");
    let decoded: Click = serde_json::from_str(&encoded).expect("deserialize click");
    assert_eq!(decoded, click);
  }

  let options = ClickOptions {
    policy: InputPolicy::ForegroundPreferred,
    click: Click::Double {
      interval: Duration::from_millis(100),
    },
    window_strategy: WindowClickStrategy::PidTargeted,
    modifiers: Default::default(),
  };

  let encoded = serde_json::to_string(&options).expect("serialize click options");
  let decoded: ClickOptions = serde_json::from_str(&encoded).expect("deserialize click options");
  assert_eq!(decoded, options);
}

#[test]
fn scroll_serde_roundtrip() {
  let scroll = Scroll::new(12.5, -42.0);

  let encoded = serde_json::to_string(&scroll).expect("serialize scroll");
  let decoded: Scroll = serde_json::from_str(&encoded).expect("deserialize scroll");

  assert_eq!(decoded, scroll);
}

#[test]
fn scroll_options_serde_uses_public_snake_case_contract() {
  let options = ScrollOptions {
    policy: InputPolicy::BackgroundPreferred,
    delivery_strategy: ScrollDeliveryStrategy {
      candidates: vec![
        ScrollDeliveryCandidate::AxScroll,
        ScrollDeliveryCandidate::WindowTargetedWheel,
        ScrollDeliveryCandidate::ForegroundHid,
      ],
    },
    settle: Duration::from_millis(25),
  };

  let encoded = serde_json::to_value(&options).expect("serialize scroll options");

  assert_eq!(
    encoded,
    serde_json::json!({
      "policy": "background_preferred",
      "delivery_strategy": {
        "candidates": [
          "ax_scroll",
          "window_targeted_wheel",
          "foreground_hid",
        ],
      },
      "settle": {
        "secs": 0,
        "nanos": 25_000_000,
      },
    })
  );
  let decoded: ScrollOptions = serde_json::from_value(encoded).expect("deserialize scroll options");
  assert_eq!(decoded, options);
}

#[test]
fn input_delivery_path_serde_matches_every_explicit_wire_value() {
  let cases = [
    (InputDeliveryPath::Noop, "noop"),
    (InputDeliveryPath::AxPress, "ax_press"),
    (InputDeliveryPath::AxFocus, "ax_focus"),
    (InputDeliveryPath::AxSetValue, "ax_set_value"),
    (InputDeliveryPath::AxScroll, "ax_scroll"),
    (InputDeliveryPath::AxSelectedText, "ax_selected_text"),
    (InputDeliveryPath::WindowTargetedMouse, "window_targeted_mouse"),
    (InputDeliveryPath::WindowTargetedWheel, "window_targeted_wheel"),
    (InputDeliveryPath::WindowTargetedKeyboard, "window_targeted_keyboard"),
    (InputDeliveryPath::WindowTargetedKeyboardScroll, "window_targeted_keyboard_scroll"),
    (InputDeliveryPath::ClipboardPaste, "clipboard_paste"),
    (InputDeliveryPath::ForegroundSystemEvents, "foreground_system_events"),
    (InputDeliveryPath::Unsupported, "unsupported"),
  ];

  for (path, expected) in cases {
    assert_eq!(path.as_str(), expected);
    assert_eq!(serde_json::to_value(path).expect("serialize input delivery path"), serde_json::Value::String(expected.to_string()));
  }
}

#[test]
fn disturbance_level_serde_matches_every_explicit_wire_value() {
  let cases = [
    (DisturbanceLevel::None, "none"),
    (DisturbanceLevel::Temporary, "temporary"),
    (DisturbanceLevel::Foreground, "foreground"),
    (DisturbanceLevel::Unknown, "unknown"),
  ];

  for (level, expected) in cases {
    assert_eq!(level.as_str(), expected);
    assert_eq!(serde_json::to_value(level).expect("serialize disturbance level"), serde_json::Value::String(expected.to_string()));
  }
}

#[test]
fn scroll_options_default_to_background_preferred() {
  let options = ScrollOptions::default();

  assert_eq!(options.policy, InputPolicy::BackgroundPreferred);
  assert_eq!(options.settle, Duration::ZERO);
}

#[test]
fn scroll_delivery_strategy_defaults_to_background_first_without_keyboard() {
  let strategy = ScrollDeliveryStrategy::default();

  assert_eq!(
    strategy.candidates,
    vec![
      ScrollDeliveryCandidate::AxScroll,
      ScrollDeliveryCandidate::WindowTargetedWheel,
      ScrollDeliveryCandidate::ForegroundHid,
    ]
  );
}

#[test]
fn scroll_options_default_include_delivery_strategy() {
  let options = ScrollOptions::default();

  assert_eq!(options.policy, InputPolicy::BackgroundPreferred);
  assert_eq!(options.delivery_strategy, ScrollDeliveryStrategy::default());
  assert_eq!(options.settle, Duration::ZERO);
}

#[test]
fn scroll_specific_delivery_paths_are_distinct_from_mouse_and_keyboard() {
  assert_ne!(InputDeliveryPath::AxScroll, InputDeliveryPath::AxSetValue);
  assert_ne!(InputDeliveryPath::WindowTargetedWheel, InputDeliveryPath::WindowTargetedMouse);
  assert_ne!(InputDeliveryPath::WindowTargetedKeyboardScroll, InputDeliveryPath::WindowTargetedKeyboard);
}

#[test]
fn input_preparation_lease_tracks_restoration() {
  let mut lease = InputPreparationLease::noop();
  assert!(!lease.is_restored());

  lease.mark_restored();

  assert!(lease.is_restored());
}

#[test]
fn legacy_key_options_convert_to_explicit_combination_without_losing_plus_key() {
  let combination: super::PressKeysOptions = super::KeyPressOptions {
    key: "cmd+shift+p".into(),
    settle: Duration::from_millis(4),
  }
  .into();
  assert_eq!(combination.keys, ["cmd", "shift", "p"]);
  assert_eq!(combination.count, 1);
  assert_eq!(combination.settle, Duration::from_millis(4));
  let plus: super::PressKeysOptions = super::KeyPressOptions {
    key: "+".into(),
    ..Default::default()
  }
  .into();
  assert_eq!(plus.keys, ["+"]);
}
