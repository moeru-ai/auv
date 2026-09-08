use std::collections::BTreeMap;

use super::{McpInvokeInput, McpServer, core_invoke_adapters, mcp_command_inputs};

#[test]
fn default_mcp_server_accepts_its_invoke_registry_and_adapter_catalog() {
  McpServer::new(std::path::PathBuf::from(".")).expect("default MCP invoke catalogs should agree");
}

#[test]
fn mcp_disables_incidental_overlays_but_preserves_explicit_overlay_operations() {
  let incidental = mcp_command_inputs(auv_cli_invoke::InvokeNamespace::Window, pairs(&[("overlay", "true")]));
  assert_eq!(incidental.get("overlay").map(String::as_str), Some("false"));

  let explicit = mcp_command_inputs(auv_cli_invoke::InvokeNamespace::Overlay, BTreeMap::new());
  assert!(!explicit.contains_key("overlay"));
}

#[tokio::test]
async fn overlay_mcp_adapters_execute_the_shared_dry_run_commands() {
  let cases = [
    ("overlay.outline", pairs(&[("x", "10"), ("y", "20"), ("width", "120"), ("height", "40")])),
    ("overlay.cursor", pairs(&[("x", "10"), ("y", "20")])),
    ("overlay.status", pairs(&[("x", "10"), ("y", "20"), ("text", "processing")])),
    ("overlay.captureFrame", pairs(&[("x", "10"), ("y", "20"), ("width", "120"), ("height", "40")])),
    ("overlay.clickTarget", pairs(&[("x", "10"), ("y", "20"), ("width", "120"), ("height", "40")])),
  ];
  let adapters = core_invoke_adapters();

  for (command_id, inputs) in cases {
    let adapter = adapters.iter().find(|adapter| adapter.command_id == command_id).unwrap_or_else(|| panic!("missing {command_id} adapter"));
    adapter
      .invoke(McpInvokeInput {
        target: None,
        inputs,
        dry_run: true,
        cancellation: Default::default(),
      })
      .await
      .unwrap_or_else(|error| panic!("{command_id} MCP dry run failed: {error}"));
  }
}

// https://github.com/moeru-ai/auv/actions/runs/30577666189/job/90989876962
#[tokio::test]
async fn mcp_uses_the_same_typed_range_validation_as_cli() {
  // ROOT CAUSE:
  //
  // If invalid window-point coordinates were invoked outside macOS, the
  // platform rejection won because typed coordinate validation lived inside
  // the macOS-only command body.
  //
  // Before the fix, Linux CI observed a platform error instead of the shared
  // validation error. The fix validates command inputs before platform dispatch.
  let adapters = core_invoke_adapters();
  let adapter = adapters.iter().find(|adapter| adapter.command_id == "input.clickPoint").expect("click-point adapter");
  let error = adapter
    .invoke(McpInvokeInput {
      target: Some(auv_cli_invoke::ExecutionTarget::Window {
        id: "window-1".to_string(),
      }),
      inputs: pairs(&[("x", "2"), ("y", "0.5"), ("relative-to", "window"), ("normalized", "true")]),
      dry_run: true,
      cancellation: Default::default(),
    })
    .await
    .expect_err("out-of-range MCP input must fail typed decoding");

  assert!(error.message.contains("within 0..=1"), "unexpected typed validation error: {error}");
}

#[tokio::test]
async fn click_point_mcp_defaults_to_screen_coordinates_without_optional_inputs() {
  let adapters = core_invoke_adapters();
  let adapter = adapters.iter().find(|adapter| adapter.command_id == "input.clickPoint").expect("click-point adapter");
  let success = adapter
    .invoke(McpInvokeInput {
      target: None,
      inputs: pairs(&[("x", "120"), ("y", "80")]),
      dry_run: true,
      cancellation: Default::default(),
    })
    .await
    .expect("screen-relative dry run should use typed defaults");

  assert_eq!(success.result["relative_to"], "screen");
  assert_eq!(success.result["screen_point"]["x"], 120.0);
  assert!(success.result["action"].is_null());
}

fn pairs(values: &[(&str, &str)]) -> BTreeMap<String, String> {
  values.iter().map(|(key, value)| ((*key).to_string(), (*value).to_string())).collect()
}

#[tokio::test]
async fn keyboard_mcp_rejects_display_with_typed_failure() {
  let adapters = core_invoke_adapters();
  let adapter = adapters.iter().find(|adapter| adapter.command_id == "input.key").unwrap();
  let error = adapter.invoke(McpInvokeInput {
    target: Some(auv_cli_invoke::ExecutionTarget::Display { id: "primary".into() }),
    inputs: pairs(&[("key", "cmd+a")]), dry_run: false, cancellation: Default::default(),
  }).await.unwrap_err();
  assert_eq!(error.code, auv_cli_invoke::FailureCode::InvalidTarget);
}

#[test]
fn mcp_target_metadata_comes_from_the_executable_definition() {
  let registry = auv_cli_invoke::default_registry();
  let keyboard = super::invoke_command_metadata(registry.resolve("input.key").unwrap());
  assert_eq!(keyboard["target"]["accepted_types"], serde_json::json!(["application", "window"]));
  assert_eq!(keyboard["target"]["required"], false);
  let media = super::invoke_command_metadata(registry.resolve("mediaControl.play").unwrap());
  assert_eq!(media["target"]["accepted_types"], serde_json::json!([]));
  let focus = super::invoke_command_metadata(registry.resolve("input.focusText").unwrap());
  assert_eq!(focus["target"]["required"], true);
}
