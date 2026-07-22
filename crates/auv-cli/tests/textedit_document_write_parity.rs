//! Same-run frontend parity for TextEdit document.write (#101).

use std::collections::BTreeMap;
use std::sync::Arc;

use auv_cli_invoke::InvokeOutputOptions;
use auv_inspect_server::legacy::InspectReadProjection;
use auv_runtime::model::{ExecutionTarget, InvokeRequest};
use auv_runtime::run_read;
use auv_tracing_driver::{MemoryRunRecorder, RunRecordingBackend, store::LocalStore};

use auv_cli::integrations::textedit::{
  DOCUMENT_WRITE_COMMAND_ID, TEXTEDIT_DOCUMENT_WRITE_KNOWN_LIMIT, TEXTEDIT_DOCUMENT_WRITE_STATE_CHANGED_KNOWN_LIMIT,
};
use auv_cli::{inspect, invoke_recorded, product_registry, projection::ProductInspectReadProjection};

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
  assert!(!auv_cli_invoke::render_help_index(&auv_cli_invoke::default_registry()).contains(DOCUMENT_WRITE_COMMAND_ID));
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

fn tempfile_dir(label: &str) -> std::path::PathBuf {
  let path = std::env::temp_dir().join(format!("auv-{label}-{}-{}", std::process::id(), auv_tracing_driver::now_millis()));
  std::fs::create_dir_all(&path).expect("temp dir");
  path
}
