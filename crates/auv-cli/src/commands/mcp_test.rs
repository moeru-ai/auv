use std::collections::BTreeMap;
use std::sync::Arc;

use auv_auto_loop::runtime::FakeOperationExecutor;
use rmcp::{
  ClientHandler, ServiceExt,
  model::{CallToolRequestParam, ClientInfo, ErrorCode},
};

use super::{McpInvokeInput, McpServer, OpRunToolRequest, core_invoke_adapters, mcp_command_inputs};
use crate::commands::op::ExecutionModeArg;

#[test]
fn default_mcp_server_accepts_its_invoke_registry_and_adapter_catalog() {
  McpServer::new(std::path::PathBuf::from(".")).expect("default MCP invoke catalogs should agree");
}

#[test]
fn device_entry_mcp_tools_expose_only_nonsecret_selection_and_target_fields() {
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let tools = server.tool_router.list_all();
  let sessions = tools.iter().find(|tool| tool.name == "device_list_user_sessions").expect("Device sessions tool");
  let get_session = tools.iter().find(|tool| tool.name == "device_get_user_session").expect("Device get session tool");
  let unlock = tools.iter().find(|tool| tool.name == "device_ensure_user_session_unlocked").expect("Device unlock tool");
  let lock = tools.iter().find(|tool| tool.name == "device_ensure_user_session_locked").expect("Device lock tool");
  let session_schema = serde_json::to_value(&sessions.input_schema).unwrap();
  let get_schema = serde_json::to_value(&get_session.input_schema).unwrap();
  let unlock_schema = serde_json::to_value(&unlock.input_schema).unwrap();
  let lock_schema = serde_json::to_value(&lock.input_schema).unwrap();
  let session_properties = session_schema["properties"].as_object().unwrap();
  let get_properties = get_schema["properties"].as_object().unwrap();
  let unlock_properties = unlock_schema["properties"].as_object().unwrap();

  assert_eq!(session_properties.keys().map(String::as_str).collect::<Vec<_>>(), ["device_id", "device_name"]);
  assert_eq!(get_properties.keys().map(String::as_str).collect::<Vec<_>>(), ["device_id", "device_name", "session_selector"]);
  assert_eq!(get_schema["additionalProperties"], false);
  assert_eq!(unlock_properties.keys().map(String::as_str).collect::<Vec<_>>(), ["device_id", "device_name", "session_selector", "user"]);
  assert_eq!(unlock_schema["additionalProperties"], false);
  assert_eq!(lock_schema, unlock_schema);
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

// ROOT CAUSE:
//
// If an MCP host sent the input keys advertised in x-auv-commands, input.drag
// failed with `missing field 'start-x'` because serde renamed the positional
// fields while the metadata advertised their Clap ids.
//
// Before the fix, only undocumented hyphenated keys worked.
// The fix keeps the advertised keys (`start_x`, ...) as the decoded names.
#[tokio::test]
async fn drag_mcp_adapter_accepts_advertised_input_keys_with_typed_defaults() {
  // Drive the adapter with exactly the keys the MCP schema advertises.
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let tools = server.tool_router.list_all();
  let invoke = tools.iter().find(|tool| tool.name == "invoke").expect("invoke tool");
  let schema = serde_json::to_value(&invoke.input_schema).unwrap();
  let metadata = schema["x-auv-commands"].as_array().unwrap().iter().find(|command| command["id"] == "input.drag").expect("drag metadata");
  let advertised: Vec<_> = metadata["arguments"].as_array().unwrap().iter().map(|argument| argument["input_key"].as_str().unwrap()).collect();
  assert_eq!(advertised[..4], ["start_x", "start_y", "end_x", "end_y"]);
  let inputs: Vec<_> = advertised[..4].iter().copied().zip(["120", "80", "320", "80"]).collect();
  let adapters = core_invoke_adapters();
  let adapter = adapters.iter().find(|adapter| adapter.command_id == "input.drag").expect("drag adapter");
  let success = adapter
    .invoke(McpInvokeInput {
      target: None,
      inputs: pairs(&inputs),
      dry_run: true,
      cancellation: Default::default(),
    })
    .await
    .expect("screen-relative drag dry run should use typed defaults");

  assert_eq!(success.result["relative_to"], "screen");
  assert_eq!(success.result["button"], "left");
  assert_eq!(success.result["duration_ms"], 300);
  assert_eq!(success.result["screen_end"]["x"], 320.0);
  assert!(success.result["action"].is_null());
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

#[test]
fn mcp_support_metadata_comes_from_the_executable_definition() {
  let registry = auv_cli_invoke::default_registry();
  for command in registry.all() {
    let metadata = super::invoke_command_metadata(command);
    assert_eq!(metadata["support"], serde_json::to_value(command.support).unwrap(), "support of {}", command.id);
    assert_eq!(metadata["role"], serde_json::to_value(command.role).unwrap(), "role of {}", command.id);
  }

  let click_text = super::invoke_command_metadata(registry.resolve("screen.clickText").unwrap());
  assert_eq!(click_text["support"]["linux"], serde_json::json!({ "state": "unsupported" }));
  assert_eq!(click_text["support"]["macos"], serde_json::json!({ "state": "supported" }));

  let type_text = super::invoke_command_metadata(registry.resolve("input.typeText").unwrap());
  assert_eq!(
    type_text["support"]["linux"],
    serde_json::json!({ "state": "partial", "note": "focused target only; cannot focus or raise a window" })
  );

  let press_keys = super::invoke_command_metadata(registry.resolve("input.pressKeys").unwrap());
  assert_eq!(press_keys["role"], serde_json::json!({ "kind": "alias", "canonical": "input.keys" }));
}

#[test]
fn keyboard_sequence_metadata_exposes_repeat_arguments_and_target_contract() {
  let registry = auv_cli_invoke::default_registry();
  let keys = super::invoke_command_metadata(registry.resolve("input.keys").unwrap());
  assert_eq!(keys["target"]["accepted_types"], serde_json::json!(["application", "window"]));
  let arguments = keys["arguments"].as_array().unwrap();
  assert_eq!(arguments.iter().find(|arg| arg["input_key"] == "keys").unwrap()["repeated"], true);
  assert!(arguments.iter().any(|arg| arg["input_key"] == "count"));
  assert!(arguments.iter().any(|arg| arg["input_key"] == "interval-ms"));
  let sequence = super::invoke_command_metadata(registry.resolve("input.keyboard").unwrap());
  assert_eq!(sequence["target"], keys["target"]);
}

/// Server whose `op_run` executor is a counting fake, so no test drives real automation
/// (on Windows the default executor would).
fn op_run_server() -> (McpServer, Arc<FakeOperationExecutor>) {
  let fake = Arc::new(FakeOperationExecutor::new());
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap().with_op_executor(fake.clone());
  (server, fake)
}

fn op_run_request(operation: &str, mode: ExecutionModeArg) -> OpRunToolRequest {
  OpRunToolRequest {
    operation: operation.to_string(),
    mode,
    file: None,
  }
}

#[derive(Debug, Clone, Default)]
struct TestClient;

impl ClientHandler for TestClient {
  fn get_info(&self) -> ClientInfo {
    ClientInfo::default()
  }
}

#[test]
fn op_run_tool_schema_requires_an_explicit_mode_without_default() {
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let tools = server.tool_router.list_all();
  let op_run = tools.iter().find(|tool| tool.name == "op_run").expect("op_run tool must be registered");
  let schema = serde_json::to_value(&op_run.input_schema).unwrap();

  let required = schema["required"].as_array().expect("required list").iter().filter_map(|name| name.as_str()).collect::<Vec<_>>();
  assert!(required.contains(&"operation"));
  assert!(required.contains(&"mode"), "mode must be required in the schema: {schema}");
  assert!(!required.contains(&"file"));
  assert_eq!(schema["additionalProperties"], false);

  // rmcp emits draft-07, so the enum lives behind a `$ref` into `definitions`.
  let mode_reference = schema["properties"]["mode"]["$ref"].as_str().expect("mode must reference the ExecutionModeArg enum");
  let mode = schema["definitions"][mode_reference.rsplit('/').next().unwrap()].clone();
  assert_eq!(mode["enum"], serde_json::json!(["fast", "verified"]));
  assert!(mode.get("default").is_none(), "mode must have no default: {mode}");
  assert!(schema["properties"]["mode"].get("default").is_none());
}

#[tokio::test]
async fn execute_op_run_fast_succeeds_without_confirmation() {
  let (server, fake) = op_run_server();

  let result = server.execute_op_run(op_run_request("qqmusic.prepare_playback_fast", ExecutionModeArg::Fast)).await.unwrap();

  assert_eq!(result.is_error, Some(false));
  let output = result.structured_content.expect("structured op_run output");
  assert_eq!(output["status"], "success");
  assert_eq!(output["execution_mode"], "fast");
  assert_eq!(output["confirmed"], false, "Fast must never be confirmed:true");
  assert_eq!(output["reason_code"], "EXACT_KEY_MATCH");
  assert_eq!(fake.calls_count(), 1);
}

#[tokio::test]
async fn execute_op_run_verified_is_confirmed_when_the_gate_passes() {
  let (server, _fake) = op_run_server();

  let result = server.execute_op_run(op_run_request("qqmusic.prepare_playback", ExecutionModeArg::Verified)).await.unwrap();

  assert_eq!(result.is_error, Some(false));
  let output = result.structured_content.expect("structured op_run output");
  assert_eq!(output["status"], "success");
  assert_eq!(output["execution_mode"], "verified");
  assert_eq!(output["confirmed"], true);
  assert_eq!(output["reason_code"], "GATE_PASSED");
}

#[tokio::test]
async fn execute_op_run_verified_gate_failure_is_an_unconfirmed_tool_error() {
  let (server, fake) = op_run_server();
  fake.set_verified_should_pass(false);

  let result = server.execute_op_run(op_run_request("qqmusic.prepare_playback", ExecutionModeArg::Verified)).await.unwrap();

  assert_eq!(result.is_error, Some(true));
  let output = result.structured_content.expect("structured op_run output");
  assert_eq!(output["status"], "failed");
  assert_eq!(output["confirmed"], false, "a failed gate must never report confirmed:true");
  assert_eq!(output["reason_code"], "GATE_FAILED");
}

#[tokio::test]
async fn execute_op_run_mode_mismatch_is_rejected_with_zero_driver_calls() {
  let (server, fake) = op_run_server();

  // qqmusic.prepare_playback is Verified-only, so requesting Fast must be refused before the executor runs.
  let result = server.execute_op_run(op_run_request("qqmusic.prepare_playback", ExecutionModeArg::Fast)).await.unwrap();

  assert_eq!(result.is_error, Some(true));
  let output = result.structured_content.expect("structured op_run output");
  assert_eq!(output["status"], "failed");
  assert_eq!(output["confirmed"], false);
  assert_eq!(output["reason_code"], "MODE_MISMATCH_ESCALATE_VLM");
  assert_eq!(fake.calls_count(), 0, "a mode mismatch must cause zero driver side effects");
}

#[tokio::test]
async fn execute_op_run_unreadable_custom_file_is_invalid_params() {
  let (server, fake) = op_run_server();
  let request = OpRunToolRequest {
    file: Some(std::path::PathBuf::from("does-not-exist.operation.json")),
    ..op_run_request("qqmusic.prepare_playback_fast", ExecutionModeArg::Fast)
  };

  let error = server.execute_op_run(request).await.expect_err("an unreadable file must fail the call");

  assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
  assert_eq!(fake.calls_count(), 0);
}

// Drives the request through the real MCP transport and tool router, so argument deserialization
// is the code under test rather than a hand-built `OpRunToolRequest`.
#[tokio::test]
async fn op_run_over_mcp_rejects_a_missing_or_invalid_mode_before_scheduling() {
  let (server, fake) = op_run_server();
  let (server_transport, client_transport) = tokio::io::duplex(16_384);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = TestClient.serve(client_transport).await.unwrap();

  let listed = client.list_all_tools().await.unwrap();
  assert!(listed.iter().any(|tool| tool.name == "op_run"), "tools/list must include op_run");

  let bad_arguments = [
    serde_json::json!({ "operation": "qqmusic.prepare_playback_fast" }),
    serde_json::json!({ "operation": "qqmusic.prepare_playback_fast", "mode": null }),
    serde_json::json!({ "operation": "qqmusic.prepare_playback_fast", "mode": "turbo" }),
    serde_json::json!({ "operation": "qqmusic.prepare_playback_fast", "mode": "fast", "unexpected": true }),
  ];
  for arguments in bad_arguments {
    let outcome = client
      .call_tool(CallToolRequestParam {
        name: "op_run".into(),
        arguments: arguments.as_object().cloned(),
      })
      .await;
    match outcome {
      Err(rmcp::service::ServiceError::McpError(error)) => {
        assert_eq!(error.code, ErrorCode::INVALID_PARAMS, "{arguments} must be rejected as invalid params")
      }
      other => panic!("{arguments} must fail closed with invalid_params, got {other:?}"),
    }
  }
  assert_eq!(fake.calls_count(), 0, "rejected requests must never reach the executor");

  let accepted = client
    .call_tool(CallToolRequestParam {
      name: "op_run".into(),
      arguments: serde_json::json!({ "operation": "qqmusic.prepare_playback_fast", "mode": "fast" }).as_object().cloned(),
    })
    .await
    .unwrap();
  assert_eq!(accepted.structured_content.expect("structured output")["confirmed"], false);
  assert_eq!(fake.calls_count(), 1);

  client.cancel().await.unwrap();
  server_handle.await.unwrap().unwrap();
}

/// Core adapters with `display.list` replaced by one that counts local executions.
fn counting_display_list_server(selection: auv::selection::RootSelection) -> (McpServer, Arc<std::sync::atomic::AtomicUsize>) {
  let local_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
  let counter = Arc::clone(&local_calls);
  let mut adapters = core_invoke_adapters().into_iter().filter(|adapter| adapter.command_id != "display.list").collect::<Vec<_>>();
  adapters.push(super::McpInvokeAdapter::new("display.list", move |_input| {
    let counter = Arc::clone(&counter);
    async move {
      counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
      Ok(super::McpInvokeSuccess::empty())
    }
  }));
  let store_root = tempfile::tempdir().unwrap().keep();
  let server = McpServer::with_registry(store_root, Arc::new(auv_cli_invoke::default_registry()), adapters)
    .unwrap()
    .with_default_selection(selection);
  (server, local_calls)
}

async fn call_invoke(server: McpServer, arguments: serde_json::Value) -> rmcp::model::CallToolResult {
  let (server_transport, client_transport) = tokio::io::duplex(65_536);
  tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = TestClient.serve(client_transport).await.unwrap();
  client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: arguments.as_object().cloned(),
    })
    .await
    .expect("invoke reports Device failures as tool errors, not protocol errors")
}

// ROOT CAUSE:
//
// If `auv --device-id <guest> mcp serve` received an invoke call, the root
// selection was never passed to the MCP frontend, so the command ran on the
// local host instead (an OSWorld eval captured the operator's Mac screen).
//
// Before the fix, MCP invoke always executed the local adapter.
// The fix routes a selected invoke through the same remote context as the CLI.
#[tokio::test]
async fn mcp_invoke_with_a_root_device_selection_never_runs_locally() {
  let selection = auv::selection::RootSelection {
    device_name: None,
    device_id: Some("0000000000000000000000000000000000000000000000000000000000000000".into()),
    run_id: None,
  };
  let (server, local_calls) = counting_display_list_server(selection);

  let result = call_invoke(server, serde_json::json!({ "command_id": "display.list" })).await;

  assert_eq!(local_calls.load(std::sync::atomic::Ordering::SeqCst), 0, "a selected Device must not fall back to local execution");
  assert_eq!(result.is_error, Some(true), "an unreachable selected Device is a tool error: {result:?}");
}

#[tokio::test]
async fn mcp_invoke_device_fields_select_a_device_per_call() {
  let (server, local_calls) = counting_display_list_server(auv::selection::RootSelection::default());

  let result = call_invoke(
    server,
    serde_json::json!({ "command_id": "display.list", "device_id": "0000000000000000000000000000000000000000000000000000000000000000" }),
  )
  .await;

  assert_eq!(local_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
  assert_eq!(result.is_error, Some(true), "{result:?}");
}

#[tokio::test]
async fn mcp_invoke_without_a_selection_still_runs_the_local_adapter() {
  let (server, local_calls) = counting_display_list_server(auv::selection::RootSelection::default());

  let result = call_invoke(server, serde_json::json!({ "command_id": "display.list" })).await;

  assert_eq!(local_calls.load(std::sync::atomic::Ordering::SeqCst), 1);
  assert_ne!(result.is_error, Some(true), "{result:?}");
}

// ROOT CAUSE:
//
// If a model client listed MCP tools, it rendered only standard schema keys,
// so the `x-auv-commands` argument metadata never reached the model. OSWorld
// agents guessed `inputs` keys (`text` instead of `query`) and list encodings
// (`"CTRL+L"` instead of a JSON array string) in about 100 failed calls.
//
// The fix renders the command catalog with inputs and examples into the
// `invoke` description.
#[tokio::test]
async fn invoke_description_lists_inputs_and_example_calls() {
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let (server_transport, client_transport) = tokio::io::duplex(1 << 20);
  tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = TestClient.serve(client_transport).await.unwrap();
  let tools = client.list_all_tools().await.unwrap();
  let invoke = tools.iter().find(|tool| tool.name == "invoke").expect("invoke tool");
  let description = invoke.description.as_deref().unwrap_or_default();

  assert!(description.contains("  inputs.query (required)"), "{description}");
  // OSWorld agents on Linux copied the macOS example id `com.google.Chrome`
  // into most Chrome episodes; the preamble says ids differ by platform.
  assert!(description.contains("run `window.list` instead of guessing"), "{description}");
  // The same agents clicked display.capture points with an app target, which
  // the window basis shifted by the window origin.
  assert!(description.contains("Send a screen point without `target`, or with `target` and"), "{description}");
  assert!(description.contains(r#"  example: {"command_id":"input.pressKeys","inputs":{"keys":"[\"cmd\",\"shift\",\"p\"]"}}"#), "{description}");
  let schema = serde_json::to_value(&invoke.input_schema).unwrap();
  let wait = schema["x-auv-commands"].as_array().unwrap().iter().find(|command| command["id"] == "screen.waitForText").unwrap();
  assert_eq!(wait["examples"][0]["call"], serde_json::json!({ "command_id": "screen.waitForText", "inputs": { "query": "Ready" } }));
}
