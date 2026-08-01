use std::collections::BTreeMap;

use super::*;

#[test]
fn every_overlay_primitive_and_component_is_registered_and_dry_run_visualizable() {
  let registry = crate::default_registry();
  let cases = [
    ("overlay.outline", rect_inputs()),
    ("overlay.cursor", point_inputs()),
    ("overlay.status", status_inputs()),
    ("overlay.captureFrame", rect_inputs()),
    ("overlay.clickTarget", click_target_inputs()),
  ];

  for (command_id, inputs) in cases {
    let command = registry.resolve(command_id).unwrap_or_else(|| panic!("{command_id} should be registered"));
    let output = futures_executor::block_on(command.invoke(InvokeCommandInput {
      command_id: command_id.to_string(),
      target_application_id: None,
      inputs,
      typed_args: None,
      dry_run: true,
      cancellation: crate::InvokeCancellation::new(),
    }))
    .unwrap_or_else(|error| panic!("{command_id} dry run should build its real overlay: {error}"));

    let report = output.report.expect("debug overlay command should report its composition");
    assert_eq!(report.fields.last().expect("overlay field").value, "disabled");
  }
}

#[test]
fn style_arguments_refine_presets_deterministically() {
  let style = outline_style("overlay.outline", OutlineStyle::selected(), Some(4.0), Some("#7fd030cc"), Some(5.0), Some(12.0))
    .expect("style should parse");

  assert_eq!(style.padding, Insets::all(4.0));
  assert_eq!(style.stroke.width, 5.0);
  assert_eq!(style.stroke.color, Color::rgba(127.0 / 255.0, 208.0 / 255.0, 48.0 / 255.0, 204.0 / 255.0));
  assert_eq!(style.corner_radius, 12.0);
}

#[test]
fn overlay_commands_reject_target_before_native_rendering() {
  let registry = crate::default_registry();
  for (command_id, inputs) in [
    ("overlay.outline", rect_inputs()),
    ("overlay.cursor", point_inputs()),
    ("overlay.status", status_inputs()),
    ("overlay.captureFrame", rect_inputs()),
    ("overlay.clickTarget", click_target_inputs()),
  ] {
    let command = registry.resolve(command_id).expect("registered overlay command");
    let error = futures_executor::block_on(command.invoke(InvokeCommandInput {
      command_id: command_id.to_string(),
      target_application_id: Some("com.example.App".to_string()),
      inputs,
      typed_args: None,
      dry_run: false,
      cancellation: crate::InvokeCancellation::new(),
    }))
    .expect_err("target must fail before native rendering");
    assert_eq!(error, format!("{command_id} cannot use --target; overlays use global screen coordinates"));
  }
}

fn rect_inputs() -> BTreeMap<String, String> {
  pairs([
    ("x", "100"),
    ("y", "120"),
    ("width", "240"),
    ("height", "80"),
    ("label", "Target"),
  ])
}

fn point_inputs() -> BTreeMap<String, String> {
  pairs([("x", "100"), ("y", "120"), ("label", "auv · cursor")])
}

fn status_inputs() -> BTreeMap<String, String> {
  pairs([("x", "100"), ("y", "120"), ("text", "processing")])
}

fn click_target_inputs() -> BTreeMap<String, String> {
  pairs([
    ("x", "100"),
    ("y", "120"),
    ("width", "240"),
    ("height", "80"),
    ("outline-label", "Quest Start"),
    ("outline-label-visible", "true"),
    ("cursor-label", "auv · click"),
    ("cursor-label-visible", "false"),
    ("status", "click target"),
  ])
}

fn pairs<const N: usize>(values: [(&str, &str); N]) -> BTreeMap<String, String> {
  values.into_iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}
