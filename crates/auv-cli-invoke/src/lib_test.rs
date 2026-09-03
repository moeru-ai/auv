use super::*;

#[test]
fn no_overlay_is_a_global_flag_without_a_value() {
  let parsed = parse_invoke_args(&["display.capture".to_string(), "--no-overlay".to_string()]).unwrap();
  let InvokeCliParse::Invoke { inputs, .. } = parsed else {
    panic!("expected invoke request");
  };

  assert_eq!(inputs.get("overlay").map(String::as_str), Some("false"));
}

#[test]
fn click_point_parses_screen_relative_coordinates() {
  let parsed = parse_invoke_args(&[
    "input.clickPoint".to_string(),
    "120".to_string(),
    "80".to_string(),
    "--relative-to".to_string(),
    "screen".to_string(),
  ])
  .expect("screen-relative click point should parse");
  let InvokeCliParse::Invoke { inputs, .. } = parsed else {
    panic!("expected invoke request");
  };

  assert_eq!(inputs.get("x").map(String::as_str), Some("120.0"));
  assert_eq!(inputs.get("y").map(String::as_str), Some("80.0"));
  assert_eq!(inputs.get("relative-to").map(String::as_str), Some("screen"));
}

#[test]
fn click_point_parses_typed_resource_targets() {
  for (value, expected) in [
    (
      "app:com.example.Player",
      ExecutionTarget::Application {
        id: "com.example.Player".to_string(),
      },
    ),
    (
      "window:5063",
      ExecutionTarget::Window {
        id: "5063".to_string(),
      },
    ),
    (
      "display:primary",
      ExecutionTarget::Display {
        id: "primary".to_string(),
      },
    ),
  ] {
    let parsed = parse_invoke_args(&[
      "input.clickPoint".to_string(),
      "10".to_string(),
      "20".to_string(),
      "--target".to_string(),
      value.to_string(),
    ])
    .expect("typed target should parse");
    let InvokeCliParse::Invoke { target, .. } = parsed else {
      panic!("expected invoke request");
    };
    assert_eq!(target, Some(expected));
  }
}

#[test]
fn retired_point_command_ids_are_not_registered() {
  for command_id in ["input.clickScreenPoint", "input.clickWindowPoint"] {
    let error = parse_invoke_args(&[command_id.to_string()]).expect_err("retired command id must not resolve");
    assert!(error.contains("unknown invoke command"), "{error}");
  }
}
