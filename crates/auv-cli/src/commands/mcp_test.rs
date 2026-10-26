use std::collections::BTreeMap;
use std::sync::Arc;

use auv_auto_loop::runtime::FakeOperationExecutor;
use rmcp::{
  ClientHandler, ServerHandler, ServiceExt,
  model::{CallToolRequestParam, ClientInfo, ErrorCode},
};

use super::{McpInvokeInput, McpServer, OpRunToolRequest, core_invoke_adapters, mcp_command_inputs};
use crate::commands::op::ExecutionModeArg;

#[test]
fn default_mcp_server_accepts_its_invoke_registry_and_adapter_catalog() {
  McpServer::new(std::path::PathBuf::from(".")).expect("default MCP invoke catalogs should agree");
}

#[test]
fn mcp_handshake_names_the_auv_build_rather_than_the_transport_crate() {
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let info = server.get_info();

  assert_eq!(info.server_info.name, "auv");
  assert_eq!(info.server_info.version, env!("CARGO_PKG_VERSION"));
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
fn mcp_runner_support_metadata_states_the_route_and_cells() {
  let registry = auv_cli_invoke::default_registry();
  for command in registry.all() {
    let metadata = super::invoke_command_metadata(command);
    let expected_route = match command.runner {
      auv_cli_invoke::RunnerSupport::NoRoute => "no_route",
      auv_cli_invoke::RunnerSupport::Direct => "direct",
      auv_cli_invoke::RunnerSupport::Cells(_) => "cells",
    };
    assert_eq!(metadata["runner_support"]["route"], serde_json::json!(expected_route), "runner route of {}", command.id);
    match command.runner_cells() {
      None => assert!(
        metadata["runner_support"].get("cells").is_none(),
        "{} has no Runner route, so it must not carry runner cells",
        command.id
      ),
      Some(cells) => assert_eq!(metadata["runner_support"]["cells"], serde_json::to_value(cells).unwrap(), "runner cells of {}", command.id),
    }
  }

  // Pins from the 2026-10-25 source audit: the cells where the Runner route
  // differs from the direct path must stay stated as different cells.
  let click_text = super::invoke_command_metadata(registry.resolve("screen.clickText").unwrap());
  assert_eq!(click_text["support"]["linux"], serde_json::json!({ "state": "unsupported" }));
  assert_eq!(click_text["runner_support"]["cells"]["linux"], serde_json::json!({ "state": "supported" }));

  let window_click_text = super::invoke_command_metadata(registry.resolve("window.clickText").unwrap());
  assert_eq!(
    window_click_text["runner_support"]["cells"]["linux"],
    serde_json::json!({ "state": "partial", "note": "foreground portal delivery only; background_only input policy is rejected" })
  );

  let drag = super::invoke_command_metadata(registry.resolve("input.drag").unwrap());
  assert_eq!(
    drag["runner_support"]["cells"]["macos"],
    serde_json::json!({ "state": "partial", "note": "foreground-preferred input policy is refused; DragMouse carries no window policy" })
  );

  // The direct macOS refusal hints a selected Runner may expose it remotely; the
  // Runner-side ScrollPoint handler refuses it for the same driver fact, so the
  // audited cells stay equal and the declaration is the direct shorthand.
  let scroll_point = super::invoke_command_metadata(registry.resolve("input.scrollPoint").unwrap());
  assert_eq!(scroll_point["runner_support"]["route"], serde_json::json!("direct"));
  assert_eq!(
    scroll_point["runner_support"]["cells"]["macos"],
    serde_json::json!({ "state": "unsupported", "note": "the driver exposes only window-owned scrolling" })
  );

  let type_text = super::invoke_command_metadata(registry.resolve("input.typeText").unwrap());
  assert_eq!(type_text["runner_support"]["route"], serde_json::json!("direct"));
  assert_eq!(type_text["runner_support"]["cells"], type_text["support"]);

  // Fixtures state no route at all, not "unsupported platforms": dispatch
  // refuses them before any platform judgment.
  let frame = super::invoke_command_metadata(registry.resolve("scan.frame").unwrap());
  assert_eq!(frame["runner_support"], serde_json::json!({ "route": "no_route" }));
}

// ROOT CAUSE:
//
// If the CLI help and the MCP `x-auv-commands` metadata rendered support from
// different sources, one surface could claim a path the other denies; the
// screen.clickText Linux gap (direct path refuses, Runner route works) is
// exactly a cell the two surfaces must state identically.
//
// The fix renders both surfaces from one registry declaration. This test
// proves the words a user reads in CLI help equal the JSON a model reads.
#[test]
fn cli_help_and_mcp_metadata_render_runner_support_from_the_same_registry_cells() {
  let registry = auv_cli_invoke::default_registry();
  for command in registry.all() {
    let metadata = super::invoke_command_metadata(command);
    let help = auv_cli_invoke::render_command_help(command);

    let direct = help_block_cells(&help, "Platform support:").expect("direct cells are listed");
    assert_eq!(
      direct,
      metadata_cells(&metadata["support"]),
      "direct cells of {} must agree between CLI help and MCP metadata",
      command.id
    );

    match metadata["runner_support"]["route"].as_str().unwrap() {
      "no_route" => {
        assert!(help.contains("no Runner route; run it without a selected Run or Device"), "fixture note of {}:\n{help}", command.id);
        assert!(metadata["runner_support"].get("cells").is_none(), "{} must not carry runner cells", command.id);
      }
      route => {
        let cells = metadata["runner_support"]["cells"].clone();
        if route == "direct" {
          assert!(help.contains("same as the direct local path above"), "direct-route note of {}:\n{help}", command.id);
          assert_eq!(cells, metadata["support"], "{}: a direct route repeats the direct cells", command.id);
        } else {
          let runner = help_block_cells(&help, "Selected Runner support:").expect("runner cells are listed");
          assert_eq!(runner, metadata_cells(&cells), "runner cells of {} must agree between CLI help and MCP metadata", command.id);
        }
      }
    }
  }
}

#[test]
fn invoke_description_states_runner_support_where_it_differs_from_direct() {
  let registry = auv_cli_invoke::default_registry();
  let description = super::invoke_tool_description(&registry);

  let click_text = description_command_block(&description, "screen.clickText");
  assert!(
    click_text.contains("\n  runner support: macOS supported; Linux supported; Windows partial"),
    "screen.clickText block:\n{click_text}"
  );

  let type_text = description_command_block(&description, "input.typeText");
  assert!(!type_text.contains("runner support:"), "equal cells stay covered by the support line:\n{type_text}");

  let frame = description_command_block(&description, "scan.frame");
  assert!(frame.contains("no Runner route"), "fixture block:\n{frame}");
}

/// The per-platform cells one help block lists, as MCP `(state, note)` pairs.
/// Help renders each cell as `<label> <word>` plus `: <note>` when the cell
/// has one, so the note is everything after the word's separating space.
fn help_block_cells<'a>(help: &'a str, header: &'a str) -> Option<Vec<(String, Option<&'a str>)>> {
  let mut lines = help.lines().skip_while(|line| *line != header);
  lines.next()?;
  let labels = ["macOS", "Linux", "Windows"];
  let mut cells = Vec::new();
  for line in lines {
    let mut tokens = line.trim_start().splitn(2, char::is_whitespace);
    let Some(label) = tokens.next() else { break };
    if !labels.contains(&label) {
      break;
    }
    let rest = tokens.next().unwrap_or_default().trim_start();
    let mut rest_tokens = rest.splitn(2, char::is_whitespace);
    let word = rest_tokens.next().unwrap_or_else(|| panic!("support word missing in {line:?}"));
    let state = match word.trim_end_matches(':') {
      "yes" => "supported",
      "partial" => "partial",
      "no" => "unsupported",
      "?" => "not_evidenced",
      other => panic!("unknown support word {other:?} in {line:?}"),
    };
    cells.push((state.to_string(), rest_tokens.next()));
  }
  (cells.len() == labels.len()).then_some(cells)
}

fn metadata_cells(cells: &serde_json::Value) -> Vec<(String, Option<&str>)> {
  ["macos", "linux", "windows"]
    .iter()
    .map(|key| {
      let cell = &cells[key];
      let state = cell["state"].as_str().unwrap_or_else(|| panic!("missing {key} cell")).to_string();
      let note = cell.get("note").and_then(|note| note.as_str());
      (state, note)
    })
    .collect()
}

fn description_command_block(description: &str, id: &str) -> String {
  description
    .split("\n\n")
    .find(|block| block.starts_with(&format!("{id}: ")))
    .unwrap_or_else(|| panic!("{id} block missing from invoke description"))
    .to_string()
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
    credential: None,
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

#[test]
fn invoke_tool_request_deserialization_handles_dry_run_and_rejects_unknown_fields() {
  use super::InvokeToolRequest;

  // 1. dry_run: true correctly decodes
  let req: InvokeToolRequest = serde_json::from_value(serde_json::json!({
    "command_id": "test.cmd",
    "dry_run": true,
  })).expect("dry_run: true should decode");
  assert_eq!(req.command_id, "test.cmd");
  assert!(req.dry_run);

  // 2. dry_run: false correctly decodes
  let req: InvokeToolRequest = serde_json::from_value(serde_json::json!({
    "command_id": "test.cmd",
    "dry_run": false,
  })).expect("dry_run: false should decode");
  assert!(!req.dry_run);

  // 3. Omitting dry_run preserves existing default semantics (false)
  let req: InvokeToolRequest = serde_json::from_value(serde_json::json!({
    "command_id": "test.cmd",
  })).expect("omitting dry_run should decode");
  assert!(!req.dry_run);

  // 4. inputs with arbitrary command-level keys is accepted
  let req: InvokeToolRequest = serde_json::from_value(serde_json::json!({
    "command_id": "input.typeText",
    "inputs": { "text": "hello", "custom_key": "custom_val" },
    "dry_run": true,
  })).expect("inputs map should allow arbitrary command keys");
  assert_eq!(req.inputs.get("text").map(String::as_str), Some("hello"));
  assert_eq!(req.inputs.get("custom_key").map(String::as_str), Some("custom_val"));

  // 5. camelCase dryRun is rejected as an unknown field
  let err = serde_json::from_value::<InvokeToolRequest>(serde_json::json!({
    "command_id": "test.cmd",
    "dryRun": true,
  })).expect_err("dryRun should be rejected");
  assert!(err.to_string().contains("unknown field `dryRun`"), "error was: {err}");

  // 6. camelCase storeRoot is rejected
  let err = serde_json::from_value::<InvokeToolRequest>(serde_json::json!({
    "command_id": "test.cmd",
    "storeRoot": "/tmp",
  })).expect_err("storeRoot should be rejected");
  assert!(err.to_string().contains("unknown field `storeRoot`"), "error was: {err}");

  // 7. Other unknown top-level fields are rejected
  let err = serde_json::from_value::<InvokeToolRequest>(serde_json::json!({
    "command_id": "test.cmd",
    "unexpected_field": true,
  })).expect_err("unexpected_field should be rejected");
  assert!(err.to_string().contains("unknown field `unexpected_field`"), "error was: {err}");
}

#[test]
fn invoke_tool_schema_rejects_additional_properties_matching_serde_contract() {
  let server = McpServer::new(std::path::PathBuf::from(".")).unwrap();
  let tools = server.tool_router.list_all();
  let invoke = tools.iter().find(|tool| tool.name == "invoke").expect("invoke tool must be registered");
  let schema = serde_json::to_value(&invoke.input_schema).unwrap();

  assert_eq!(schema["additionalProperties"], false, "invoke input schema must deny additionalProperties to match Serde contract: {schema}");
}

#[tokio::test]
async fn invoke_over_mcp_rejects_unknown_top_level_arguments_before_dispatch() {
  let (server, local_calls) = counting_display_list_server(auv::selection::RootSelection::default());
  let (server_transport, client_transport) = tokio::io::duplex(65_536);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await?;
    service.waiting().await?;
    Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
  });
  let client = TestClient.serve(client_transport).await.unwrap();

  let bad_arguments = [
    serde_json::json!({ "command_id": "display.list", "dryRun": true }),
    serde_json::json!({ "command_id": "display.list", "storeRoot": "/custom/path" }),
    serde_json::json!({ "command_id": "display.list", "unexpected": true }),
    serde_json::json!({ "command_id": "display.list", "dry_run": true, "extra": "forbidden" }),
  ];

  for arguments in bad_arguments {
    let outcome = client
      .call_tool(CallToolRequestParam {
        name: "invoke".into(),
        arguments: arguments.as_object().cloned(),
      })
      .await;

    match outcome {
      Err(rmcp::service::ServiceError::McpError(error)) => {
        assert_eq!(
          error.code,
          ErrorCode::INVALID_PARAMS,
          "{arguments} must fail closed with INVALID_PARAMS, got: {error:?}"
        );
      }
      other => panic!("{arguments} must fail closed with McpError(INVALID_PARAMS), got: {other:?}"),
    }
  }

  assert_eq!(
    local_calls.load(std::sync::atomic::Ordering::SeqCst),
    0,
    "rejected invoke requests with unknown fields must never reach the executor/adapter"
  );

  // Legitimate invoke with dry_run: true must succeed and dispatch
  let accepted = client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: serde_json::json!({ "command_id": "display.list", "dry_run": true }).as_object().cloned(),
    })
    .await
    .expect("valid invoke with dry_run: true should be accepted");

  assert_ne!(accepted.is_error, Some(true), "valid invoke must not report an error: {accepted:?}");
  assert_eq!(
    local_calls.load(std::sync::atomic::Ordering::SeqCst),
    1,
    "valid invoke must dispatch to the adapter exactly once"
  );

  client.cancel().await.unwrap();
  server_handle.await.unwrap().unwrap();
}
