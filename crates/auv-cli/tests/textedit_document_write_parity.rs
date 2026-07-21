//! Same-run frontend parity for TextEdit document.write (#101).

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use auv_cli_invoke::InvokeOutputOptions;
use auv_inspect_server::legacy::InspectReadProjection;
use auv_runtime::contract::{FailureLayer, OperationStatus};
use auv_runtime::model::{ExecutionTarget, InvokeRequest};
use auv_runtime::run_read;
use auv_tracing_driver::{MemoryRunRecorder, RunRecordingBackend, TraceStatusCode, store::LocalStore};
use rmcp::{
  ClientHandler, ServiceExt,
  model::{CallToolRequestParam, ClientInfo},
};

use auv_cli::integrations::textedit::{
  DOCUMENT_WRITE_COMMAND_ID, TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT, TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT,
  finalize_recorded_invoke,
};
use auv_cli::{inspect, invoke_recorded, product_registry, projection::ProductInspectReadProjection};

#[derive(Debug, Clone, Default)]
struct DummyClientHandler;

impl ClientHandler for DummyClientHandler {
  fn get_info(&self) -> ClientInfo {
    ClientInfo::default()
  }
}

#[test]
fn textedit_document_write_same_run_cli_mcp_inspect_parity() {
  let root = tempfile_dir("textedit-same-run-parity");
  let store = LocalStore::new(root.clone()).expect("store");
  let recording = RunRecordingBackend::new(store.clone(), Arc::new(MemoryRunRecorder::new()));
  let registry = product_registry();

  let mut inputs = BTreeMap::new();
  inputs.insert("content".to_string(), "AUV_TEXTEDIT_FIXTURE_MARKER".to_string());
  inputs.insert("driver".to_string(), "fixture".to_string());
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
  .expect("fixture invoke should succeed");

  assert_eq!(result.command_id, DOCUMENT_WRITE_COMMAND_ID);
  assert!(result.failure_message.is_none(), "{:?}", result.failure_message);
  assert_ne!(result.run_id, "unassigned");
  let run_id = result.run_id.clone();

  let operation = run_read::read_operation_result(&store, &run_id).expect("read operation-result").expect("operation-result should exist");
  assert_eq!(operation.operation_id, DOCUMENT_WRITE_COMMAND_ID);
  assert_eq!(operation.run_id.as_str(), run_id.as_str(), "canonical operation must use assigned run_id, not unassigned");
  assert!(!operation.evidence_artifacts.is_empty(), "canonical operation must reference evidence artifacts");
  assert!(
    operation.evidence_artifacts.iter().all(|artifact| artifact.run_id.as_str() == run_id.as_str()),
    "every evidence ArtifactRef must share the assigned run_id"
  );
  assert!(
    operation.known_limits.iter().any(|limit| limit == TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT),
    "known_limits must include {TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT}"
  );
  assert_eq!(operation.verifications.len(), 1);
  assert!(matches!(operation.verifications[0].method, auv_runtime::contract::VerificationMethod::AxText));
  assert_eq!(operation.verifications[0].semantic_matched, Some(true));
  assert!(!operation.verifications[0].state_changed);
  assert!(operation.known_limits.iter().any(|limit| limit == TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT));

  let run = store.read_run(&run_id).expect("run");
  let artifact_roles: BTreeMap<String, String> =
    run.artifacts.iter().map(|artifact| (artifact.artifact_id.as_str().to_string(), artifact.role.clone())).collect();
  assert!(artifact_roles.values().any(|role| role == "operation-result"));
  assert!(artifact_roles.values().any(|role| role == "input-action-result"));
  assert!(artifact_roles.values().any(|role| role == "ax-text-observation"));
  let operation_artifact_ids: Vec<&str> =
    run.artifacts.iter().filter(|artifact| artifact.role == "operation-result").map(|artifact| artifact.artifact_id.as_str()).collect();
  assert_eq!(operation_artifact_ids.len(), 1);

  let composer = inspect::build_product_inspect_composer().expect("composer");
  let cli_text = inspect::inspect_run_with(&composer, &store, &run_id).expect("cli inspect");
  assert!(cli_text.contains(&run_id), "cli inspect should mention run id");
  assert!(cli_text.contains("ax_text") || cli_text.contains("AxText") || cli_text.contains("method=ax_text"));

  let mcp_document = composer.collect_document(&store, &run).expect("mcp-style document");
  let mcp_text = mcp_document.render_text();
  assert_eq!(extract_section_ids(&cli_text), extract_section_ids(&mcp_text));

  let projection = ProductInspectReadProjection::default();
  let server_document = projection.inspect_document(&store, &run).expect("inspect-server document").expect("document present");
  let server_text = server_document.render_text();
  assert_eq!(extract_section_ids(&cli_text), extract_section_ids(&server_text));

  let enrichment = projection.run_enrichment(&store, &run).expect("enrichment");
  assert_eq!(enrichment.verifications.len(), 1);
  assert_eq!(enrichment.verifications[0]["method"]["kind"], "ax_text");
  assert_eq!(enrichment.verifications[0]["semantic_matched"], true);
  assert_eq!(enrichment.verifications[0]["state_changed"], false);

  // Lock same-run artifact identity: store fingerprint + evidence refs + shared projection sections.
  let expected_identity = artifact_identity_fingerprint(&run);
  assert_eq!(expected_identity, artifact_identity_fingerprint(&store.read_run(&run_id).expect("re-read")));
  assert!(
    expected_identity.iter().any(|(_, role)| role == "operation-result")
      && expected_identity.iter().any(|(_, role)| role == "ax-text-observation")
      && expected_identity.iter().any(|(_, role)| role == "input-action-result")
  );
  for artifact_ref in &operation.evidence_artifacts {
    assert_eq!(artifact_ref.run_id.as_str(), run_id.as_str());
    assert!(
      expected_identity.iter().any(|(id, _)| id == artifact_ref.artifact_id.as_str()),
      "evidence ArtifactRef must point at an artifact on the same run"
    );
  }
  for text in [&cli_text, &mcp_text, &server_text] {
    assert!(text.contains(&run_id), "each projection must anchor the same run_id");
  }
  assert_eq!(extract_section_ids(&cli_text), extract_section_ids(&mcp_text));
  assert_eq!(extract_section_ids(&cli_text), extract_section_ids(&server_text));

  let rendered = result.render_to_string(InvokeOutputOptions::default()).expect("render");
  assert!(rendered.contains(DOCUMENT_WRITE_COMMAND_ID) || rendered.contains("TextEdit"));

  let _ = std::fs::remove_dir_all(root);
}

#[test]
fn product_help_lists_textedit_command_once() {
  let help = auv_cli_invoke::render_help_index(&product_registry());
  assert_eq!(help.matches(DOCUMENT_WRITE_COMMAND_ID).count(), 1);
  assert!(!auv_cli_invoke::render_help_index(&auv_cli_invoke::default_registry()).contains(DOCUMENT_WRITE_COMMAND_ID));
}

// ROOT CAUSE:
//
// TextEdit semantic mismatch used to be applied after recorded invoke had
// persisted a successful run, leaving the invoke, trace, and operation statuses
// contradictory and CLI/MCP artifact views different.
//
// The regression locks both frontends to one in-lifecycle finalization contract.
// https://github.com/moeru-ai/auv/pull/102#issuecomment-4958351155
#[tokio::test]
async fn textedit_recorded_mismatch_keeps_cli_mcp_run_and_operation_in_sync() {
  let root = tempfile_dir("textedit-mismatch-mcp-parity");
  let store = LocalStore::new(root.clone()).expect("store");
  let recording = RunRecordingBackend::new(store.clone(), Arc::new(MemoryRunRecorder::new()));

  let cli_result = invoke_recorded(
    &recording,
    &product_registry(),
    InvokeRequest {
      command_id: DOCUMENT_WRITE_COMMAND_ID.to_string(),
      target: ExecutionTarget {
        application_id: Some("com.apple.TextEdit".to_string()),
        target_label: None,
      },
      inputs: mismatch_inputs(),
      dry_run: false,
    },
  )
  .expect("cli invoke");

  let project_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
  let composer = inspect::build_product_inspect_composer().expect("composer");
  let finalize: Arc<auv_cli_invoke::InvokeFinalizeHook> = Arc::new(finalize_recorded_invoke);
  let server =
    auv_runtime::mcp::McpServer::with_inspect_composer_and_registry(project_root, composer, Arc::new(product_registry()), Some(finalize));
  let (server_transport, client_transport) = tokio::io::duplex(16384);
  let server_handle = tokio::spawn(async move {
    let service = server.serve(server_transport).await.expect("server should start");
    service.waiting().await.expect("server should exit cleanly");
  });
  let client = DummyClientHandler::default().serve(client_transport).await.expect("client");

  let invoke = client
    .call_tool(CallToolRequestParam {
      name: "invoke".into(),
      arguments: Some(
        serde_json::json!({
          "command_id": DOCUMENT_WRITE_COMMAND_ID,
          "dry_run": false,
          "inputs": mismatch_inputs(),
          "target": {
            "application_id": "com.apple.TextEdit"
          },
          "inspect": {
            "store_root": root.display().to_string()
          }
        })
        .as_object()
        .unwrap()
        .clone(),
      ),
    })
    .await
    .expect("invoke");
  let invoke_json: serde_json::Value =
    serde_json::from_str(&invoke.content.first().and_then(|content| content.raw.as_text()).expect("invoke text").text).expect("invoke json");

  assert_eq!(cli_result.status.as_str(), "failed");
  assert_recorded_semantic_mismatch(&store, &cli_result.run_id);
  assert_eq!(invoke_json.get("status").and_then(|value| value.as_str()), Some("failed"));
  let mcp_run_id = invoke_json.get("run_id").and_then(|value| value.as_str()).expect("MCP run_id");
  assert_recorded_semantic_mismatch(&store, mcp_run_id);
  assert!(cli_result.failure_message.as_deref().is_some_and(|message| message.contains("semantic verification failed")));
  assert!(
    invoke_json
      .get("failure_message")
      .and_then(|value| value.as_str())
      .is_some_and(|message| message.contains("semantic verification failed"))
  );
  assert_eq!(artifact_role_paths(&cli_result), artifact_role_paths_from_json(&invoke_json));
  assert_eq!(artifact_path_basenames(&cli_result), artifact_path_basenames_from_json(&invoke_json));

  client.cancel().await.expect("cancel");
  server_handle.await.expect("join");
  let _ = std::fs::remove_dir_all(root);
}

#[test]
#[ignore = "live macOS TextEdit + Accessibility permission required; run manually with AUV_TEXTEDIT_LIVE=1"]
fn textedit_document_write_live_macos_closure() {
  if std::env::var_os("AUV_TEXTEDIT_LIVE").is_none() {
    return;
  }
  let root = tempfile_dir("textedit-live-closure");
  let store = LocalStore::new(root.clone()).expect("store");
  let recording = RunRecordingBackend::new(store.clone(), Arc::new(MemoryRunRecorder::new()));
  let mut inputs = BTreeMap::new();
  inputs.insert("content".to_string(), "AUV_TEXTEDIT_LIVE_MARKER".to_string());
  inputs.insert("driver".to_string(), "live".to_string());
  let result = invoke_recorded(
    &recording,
    &product_registry(),
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
  .expect("live invoke");
  let operation = run_read::read_operation_result(&store, &result.run_id).expect("read").expect("operation-result");
  assert_eq!(operation.run_id.as_str(), result.run_id.as_str());
  assert!(!operation.evidence_artifacts.is_empty());
  assert_eq!(operation.verifications[0].semantic_matched, Some(true));
  assert!(!operation.verifications[0].state_changed);
  let _ = std::fs::remove_dir_all(root);
}

fn extract_section_ids(text: &str) -> Vec<String> {
  text
    .lines()
    .filter_map(|line| {
      let trimmed = line.trim();
      if trimmed.starts_with('[') && trimmed.ends_with(']') {
        Some(trimmed.to_string())
      } else if let Some(rest) = trimmed.strip_prefix("## ") {
        Some(rest.to_string())
      } else {
        None
      }
    })
    .collect()
}

fn artifact_identity_fingerprint(run: &auv_tracing_driver::store::CanonicalRun) -> Vec<(String, String)> {
  let mut pairs =
    run.artifacts.iter().map(|artifact| (artifact.artifact_id.as_str().to_string(), artifact.role.clone())).collect::<Vec<_>>();
  pairs.sort();
  pairs
}

fn mismatch_inputs() -> BTreeMap<String, String> {
  let mut inputs = BTreeMap::new();
  inputs.insert("content".to_string(), "AUV_TEXTEDIT_EXPECTED_MARKER".to_string());
  inputs.insert("driver".to_string(), "fixture".to_string());
  inputs.insert("fixture_observed_text".to_string(), "observed-without-expected".to_string());
  inputs
}

fn assert_recorded_semantic_mismatch(store: &LocalStore, run_id: &str) {
  let canonical = store.read_run(run_id).expect("recorded mismatch run");
  assert_eq!(canonical.run.status_code, TraceStatusCode::Error);
  let command_span = canonical.spans.iter().find(|span| span.name == "auv.command.invoke").expect("command span");
  assert_eq!(command_span.status_code, TraceStatusCode::Error);

  let operation = run_read::read_operation_result(store, run_id).expect("read mismatch operation-result").expect("operation-result");
  assert_eq!(operation.run_id.as_str(), run_id);
  assert_eq!(operation.status, OperationStatus::Failed);
  assert_eq!(operation.verifications.len(), 1);
  assert_eq!(operation.verifications[0].semantic_matched, Some(false));
  assert!(!operation.verifications[0].state_changed);
  assert_eq!(operation.verifications[0].failure_layer, Some(FailureLayer::SemanticMismatch));
  assert!(operation.known_limits.iter().any(|limit| limit == TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT));

  let operation_artifacts = canonical.artifacts.iter().filter(|artifact| artifact.role == "operation-result").collect::<Vec<_>>();
  assert_eq!(operation_artifacts.len(), 1);
  assert_eq!(operation_artifacts[0].span_id, command_span.span_id);
  assert!(!operation.evidence_artifacts.is_empty());
  for artifact_ref in &operation.evidence_artifacts {
    assert_eq!(artifact_ref.run_id.as_str(), run_id);
    assert!(canonical.artifacts.iter().any(|artifact| artifact.artifact_id == artifact_ref.artifact_id));
  }
}

fn artifact_role_paths(result: &auv_cli_invoke::InvokeResult) -> Vec<(String, String)> {
  result.artifacts.iter().map(|artifact| (artifact.role.clone(), artifact.path.clone())).collect()
}

fn artifact_role_paths_from_json(value: &serde_json::Value) -> Vec<(String, String)> {
  value
    .get("artifacts")
    .and_then(|items| items.as_array())
    .expect("artifacts array")
    .iter()
    .map(|artifact| {
      (
        artifact.get("role").and_then(|field| field.as_str()).expect("artifact role").to_string(),
        artifact.get("path").and_then(|field| field.as_str()).expect("artifact path").to_string(),
      )
    })
    .collect()
}

fn artifact_path_basenames(result: &auv_cli_invoke::InvokeResult) -> Vec<String> {
  result.artifact_paths.iter().map(|path| path.file_name().and_then(|name| name.to_str()).expect("artifact filename").to_string()).collect()
}

fn artifact_path_basenames_from_json(value: &serde_json::Value) -> Vec<String> {
  value
    .get("artifact_paths")
    .and_then(|items| items.as_array())
    .expect("artifact_paths array")
    .iter()
    .map(|path| {
      Path::new(path.as_str().expect("artifact path")).file_name().and_then(|name| name.to_str()).expect("artifact filename").to_string()
    })
    .collect()
}

fn tempfile_dir(label: &str) -> std::path::PathBuf {
  let path = std::env::temp_dir().join(format!("auv-{label}-{}-{}", std::process::id(), auv_tracing_driver::now_millis()));
  std::fs::create_dir_all(&path).expect("temp dir");
  path
}
