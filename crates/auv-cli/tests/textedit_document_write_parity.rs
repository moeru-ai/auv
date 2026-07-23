//! Direct CLI/MCP parity for TextEdit document.write (#101).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use auv_cli::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID;
use auv_cli::product_registry;
use auv_cli_invoke::{InvokeCancellation, InvokeCommandInput};
use auv_tracing::{Context, EventPayload, FileRunStore, RunId, RunStore, configure, dispatcher};
use rmcp::{
  ClientHandler, ServiceExt,
  model::{CallToolRequestParam, ClientInfo},
};

#[derive(Debug, Clone, Default)]
struct DummyClientHandler;

impl ClientHandler for DummyClientHandler {
  fn get_info(&self) -> ClientInfo {
    ClientInfo::default()
  }
}

#[derive(serde::Serialize)]
struct CliFrontendLifecycle {
  frontend: &'static str,
}

impl EventPayload for CliFrontendLifecycle {
  const NAME: &'static str = "auv.frontend.lifecycle";
  const VERSION: u32 = 1;
}

#[tokio::test]
async fn textedit_rejected_fixture_input_preserves_direct_cli_mcp_parity() {
  let store_root = tempfile_dir("textedit-direct-parity");
  let store = Arc::new(FileRunStore::open(&store_root).expect("file store"));
  let dispatch = configure().run_store(store.clone()).build().expect("CLI dispatch");
  let cli_run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(cli_run_id));
  let command = product_registry().resolve(DOCUMENT_WRITE_COMMAND_ID).expect("TextEdit command").clone();
  let input = InvokeCommandInput {
    command_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
    target_application_id: Some("com.apple.TextEdit".to_string()),
    inputs: rejected_fixture_inputs(),
    dry_run: false,
    cancellation: InvokeCancellation::new(),
  };
  let future = root.in_scope(|| {
    auv_tracing::emit_event!(CliFrontendLifecycle { frontend: "cli" });
    command.invoke(input)
  });
  let cli_value = root.instrument(future).await;
  dispatch.flush().await.expect("flush CLI run");

  let server = auv_cli::mcp::server(PathBuf::from(env!("CARGO_MANIFEST_DIR"))).expect("TextEdit MCP server");
  let (server_transport, client_transport) = tokio::io::duplex(16384);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await.expect("MCP server start");
    service.waiting().await.expect("MCP server exit");
  });
  let client = DummyClientHandler.serve(client_transport).await.expect("MCP client");
  let response = client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: Some(
        serde_json::json!({
          "command_id": DOCUMENT_WRITE_COMMAND_ID,
          "target": { "application_id": "com.apple.TextEdit" },
          "inputs": rejected_fixture_inputs(),
          "inspect": { "store_root": store_root.display().to_string() }
        })
        .as_object()
        .expect("MCP arguments")
        .clone(),
      ),
    })
    .await
    .expect("MCP invoke");
  let mcp_value: serde_json::Value =
    serde_json::from_str(&response.content.first().and_then(|content| content.raw.as_text()).expect("MCP text response").text)
      .expect("MCP JSON response");

  let cli_error = cli_value.expect_err("CLI rejects fixture-only input");
  assert_eq!(mcp_value["status"], "failed");
  assert_eq!(mcp_value["failure_message"], cli_error);
  let mcp_run_id = mcp_value["run_id"].as_str().expect("MCP run id").parse::<RunId>().expect("valid MCP run id");
  assert_ne!(cli_run_id, mcp_run_id);

  for run_id in [cli_run_id, mcp_run_id] {
    let snapshot = store.load_snapshot(run_id).await.expect("snapshot read").expect("frontend run snapshot");
    assert_eq!(snapshot.run_id(), run_id);
    assert!(snapshot.artifacts().is_empty(), "rejected input must not fabricate result artifacts");
    assert!(snapshot.events().iter().any(|event| event.schema().name().as_str() == "auv.frontend.lifecycle"));
  }

  client.cancel().await.expect("cancel MCP client");
  server_handle.await.expect("join MCP server");
  let _ = std::fs::remove_dir_all(store_root);
}

// ROOT CAUSE:
//
// A typed driver control failure (activate/focus/paste returning DriverError)
// used to be flattened to a String at the CLI invoke adapter and never
// persisted as a classification, so inspect surfaces could only show a free-text
// failure message. PR8-B maps DriverError -> FailureLayer::ControlFailed and
// persists it on OperationResult.control_failure.
//
// This regression locks that the typed classification is produced once and read
// identically across the inspect-family surfaces (CLI inspect, MCP run_inspect
// text, HTTP enrichment JSON).
#[test]
fn textedit_control_failure_persists_typed_classification_across_inspect_surfaces() {
  let root = tempfile_dir("textedit-control-failure-parity");
  let store = LocalStore::new(root.clone()).expect("store");
  let recording = RunRecordingBackend::new(store.clone(), Arc::new(MemoryRunRecorder::new()));
  let registry = product_registry();

  let mut inputs = BTreeMap::new();
  inputs.insert("content".to_string(), "AUV_TEXTEDIT_FIXTURE_MARKER".to_string());
  inputs.insert("driver".to_string(), "fixture".to_string());
  // Force a typed control-layer DriverError (PermissionDenied) at focus, before
  // any verification could run — hermetic, no live macOS.
  inputs.insert("fixture_control_error".to_string(), "permission_denied".to_string());
  inputs.insert("verify".to_string(), "true".to_string());

  let result = invoke_recorded(
    &recording,
    &registry,
    InvokeRequest {
      command_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
      target: ExecutionTarget {
        application_id: Some("com.apple.TextEdit".to_string()),
        target_label: None,
      },
      inputs,
      dry_run: false,
    },
  )
  .expect("fixture control-failure invoke completes (failure rides Ok output, not handler Err)");

  let run_id = result.run_id.clone();
  assert_ne!(run_id, "unassigned");
  // Invoke-time surface stays untyped (owner decision): a human failure string,
  // no typed classification field on the transient result.
  assert!(
    result.failure_message.as_deref().is_some_and(|message| message.contains("permission was denied")),
    "{:?}",
    result.failure_message
  );

  // Persisted OperationResult carries the typed control-layer classification.
  let operation = run_read::read_operation_result(&store, &run_id).expect("read operation-result").expect("operation-result should exist");
  assert_eq!(operation.status, OperationStatus::Failed);
  assert!(operation.verifications.is_empty(), "a control failure runs before verification, so no VerificationResult is attached");
  let control_failure = operation.control_failure.as_ref().expect("control_failure must be persisted for a driver control failure");
  assert_eq!(control_failure.layer, FailureLayer::ControlFailed);
  assert!(control_failure.message.contains("permission was denied"), "{}", control_failure.message);
  assert_eq!(control_failure.recovery.as_deref(), Some("grant Accessibility to the terminal in System Settings"));
  assert!(
    !control_failure.message.contains("grant Accessibility to the terminal in System Settings"),
    "recovery must remain a separate typed field, not be duplicated in message: {}",
    control_failure.message
  );

  // Reload through a fresh SessionService handler: the GetOperation projection
  // must read the canonical persisted OperationResult, not a process-local cache
  // or the transient InvokeResult signal carrier.
  let reloaded_handler = auv_runtime::api::session_service::handler::SessionApiHandler::new(root.clone());
  let get_operation = reloaded_handler
    .get_operation(auv_api_proto::v1::session::GetOperationRequest {
      operation: Some(auv_api_proto::v1::session::OperationRef {
        run_id: run_id.clone(),
        operation_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
      }),
    })
    .expect("GetOperation should reload the persisted TextEdit control failure");
  assert_eq!(get_operation.status, "failed");
  let get_control_failure = get_operation.control_failure.expect("GetOperation must expose persisted control_failure");
  assert_eq!(get_control_failure.layer, "control_failed");
  assert!(
    !get_control_failure.message.contains("grant Accessibility to the terminal in System Settings"),
    "GetOperation message must exclude the separately stored recovery hint: {}",
    get_control_failure.message
  );
  assert_eq!(get_control_failure.message, control_failure.message);
  assert_eq!(get_control_failure.recovery, "grant Accessibility to the terminal in System Settings");

  let run = store.read_run(&run_id).expect("run");
  assert_eq!(run.run.status_code, TraceStatusCode::Error);

  // Inspect-family parity: the typed classification is readable identically
  // across CLI inspect text, MCP run_inspect text, and HTTP enrichment JSON.
  let composer = inspect::build_product_inspect_composer().expect("composer");
  let cli_text = inspect::inspect_run_with(&composer, &store, &run_id).expect("cli inspect");
  assert!(cli_text.contains("Control Failure:"), "cli inspect must render a Control Failure section:\n{cli_text}");
  assert!(cli_text.contains("failure_layer=control_failed"), "cli inspect must render the typed layer:\n{cli_text}");

  let mcp_text = composer.collect_document(&store, &run).expect("mcp-style document").render_text();
  assert!(mcp_text.contains("failure_layer=control_failed"), "mcp run_inspect text must render the typed layer:\n{mcp_text}");
  assert_eq!(extract_section_ids(&cli_text), extract_section_ids(&mcp_text));

  let projection = ProductInspectReadProjection::default();
  let enrichment = projection.run_enrichment(&store, &run).expect("enrichment");
  let enriched = enrichment.control_failure.as_ref().expect("HTTP enrichment must expose control_failure");
  assert_eq!(enriched["layer"], "control_failed");
  assert!(enriched["message"].as_str().is_some_and(|message| message.contains("permission was denied")));
  assert_eq!(enriched["recovery"], "grant Accessibility to the terminal in System Settings");
  assert!(enrichment.verifications.is_empty(), "no verification claim on a control-failure run");

  let _ = std::fs::remove_dir_all(root);
}

#[test]
fn product_help_lists_textedit_command_once() {
  let help = auv_cli_invoke::render_help_index(&product_registry());
  assert_eq!(help.matches(DOCUMENT_WRITE_COMMAND_ID).count(), 1);
  let command = product_registry().resolve(DOCUMENT_WRITE_COMMAND_ID).expect("TextEdit command").clone();
  assert!(!auv_cli_invoke::render_command_help(&command).contains("--driver"));
  assert!(!auv_cli_invoke::render_help_index(&auv_cli_invoke::default_registry()).contains(DOCUMENT_WRITE_COMMAND_ID));
}

fn rejected_fixture_inputs() -> BTreeMap<String, String> {
  BTreeMap::from([
    ("content".to_string(), "AUV_TEXTEDIT_FIXTURE_MARKER".to_string()),
    ("driver".to_string(), "fixture".to_string()),
    ("verify".to_string(), "true".to_string()),
  ])
}

fn tempfile_dir(label: &str) -> PathBuf {
  let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("system clock").as_nanos();
  let path = std::env::temp_dir().join(format!("auv-{label}-{}-{nonce}", std::process::id()));
  std::fs::create_dir_all(&path).expect("temp dir");
  path
}
