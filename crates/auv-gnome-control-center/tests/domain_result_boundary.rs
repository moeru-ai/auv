use auv_driver::{InputActionResult, InputDeliveryPath, Rect, WindowPoint};
use auv_gnome_control_center::views::MatchedNode;
use auv_gnome_control_center::{
  CopySystemDetailsResult, NaturalScrollingToggleResult, OpenResult, PointerSpeedRoundtripResult, PointerSpeedSetResult,
  windows::OpenWindowReport,
};
use serde::Serialize;
use serde_json::Value;

#[test]
fn public_domain_results_do_not_embed_interaction_steps() {
  let window = window_report();
  let node = matched_node();
  let pointer = PointerSpeedSetResult {
    command: "mouse.set-pointer-speed",
    window: window.clone(),
    mouse_node: node.clone(),
    slider_node: node.clone(),
    requested_position: 0.75,
    clicked_point: WindowPoint::new(40.0, 20.0),
    delivery: delivery(),
  };

  assert_has_no_steps(&OpenResult {
    command: "open",
    window: window.clone(),
  });
  assert_has_no_steps(&CopySystemDetailsResult {
    command: "copy-system-details",
    window: window.clone(),
    system_node: node.clone(),
    about_node: node.clone(),
    details_node: node.clone(),
    copy_node: node.clone(),
    clipboard_text: "GNOME 48".to_string(),
    delivery: delivery(),
  });
  assert_has_no_steps(&pointer);
  assert_has_no_steps(&PointerSpeedRoundtripResult {
    command: "mouse.roundtrip-pointer-speed",
    first: pointer.clone(),
    restore: pointer,
  });
  assert_has_no_steps(&NaturalScrollingToggleResult {
    command: "mouse.toggle-natural-scrolling",
    window,
    mouse_node: node.clone(),
    switch_node: node,
    observed_value_before: Some("false".to_string()),
    observed_value_after: Some("true".to_string()),
    delivery: delivery(),
  });
}

fn window_report() -> OpenWindowReport {
  OpenWindowReport {
    window_found: true,
    window_title: Some("Settings".to_string()),
    window_ref: Some("window-1".to_string()),
    app_name: Some("Settings".to_string()),
    frame: Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
    app_id: "org.gnome.Settings",
    process_name: "gnome-control-center",
  }
}

fn matched_node() -> MatchedNode {
  MatchedNode {
    path: "0/1".to_string(),
    label: "Mouse".to_string(),
    matched_label: "Mouse".to_string(),
    role: "button".to_string(),
    bounds: Rect::new(20.0, 30.0, 100.0, 24.0),
    value: None,
  }
}

fn delivery() -> InputActionResult {
  InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse)
}

fn assert_has_no_steps(value: &impl Serialize) {
  let value = serde_json::to_value(value).expect("serialize domain result");
  assert_no_steps(&value, "$");
}

fn assert_no_steps(value: &Value, location: &str) {
  match value {
    Value::Object(object) => {
      for (key, child) in object {
        assert_ne!(key, "steps", "parallel interaction timeline at {location}");
        assert_no_steps(child, &format!("{location}.{key}"));
      }
    }
    Value::Array(items) => {
      for (index, child) in items.iter().enumerate() {
        assert_no_steps(child, &format!("{location}[{index}]"));
      }
    }
    _ => {}
  }
}
