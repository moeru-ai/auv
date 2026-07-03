//! Proto <-> host mapping for the session API, isolated from handler and
//! transport code (API-P4 handoff checklist: "isolate proto/host mapping from
//! transport handler code").

use std::collections::{BTreeMap, HashMap};

use auv_api_proto::v1::session as proto;
use auv_cli_invoke::{
  ExecutionTarget, InvokeRequest as HostInvokeRequest, InvokeResult, RunStatus,
};
use auv_tracing_driver::trace::ArtifactRecordV1Alpha1;

use crate::api::session_service::SessionApiError;
use crate::api::session_service::summary::{
  ARTIFACT_ROLE_UNAVAILABLE_KNOWN_LIMIT, JoinedOperationSummary,
};
use crate::contract::{ArtifactRef as ContractArtifactRef, OperationStatus};

/// Status string shared by `InvokeResponse` and `GetOperationResponse`
/// (`"completed"` | `"failed"`).
pub fn run_status_str(status: &RunStatus) -> String {
  status.as_str().to_string()
}

/// Same status vocabulary, projected from the persisted `OperationStatus`.
pub fn operation_status_str(status: OperationStatus) -> String {
  match status {
    OperationStatus::Completed => "completed".to_string(),
    OperationStatus::Failed => "failed".to_string(),
  }
}

// NOTICE(api-p3-od5): the json_payload envelope schema is UNRESOLVED. This
// provisional decoder follows the API-P3 sketch
// ({target:{application_id,target_label}, inputs:{...}, dry_run}); an empty
// payload maps to host defaults. A real slice must version this envelope and
// define malformed-payload behavior. Trigger: an owner-named envelope decision.
#[derive(serde::Deserialize, Default)]
struct InvokePayloadEnvelope {
  #[serde(default)]
  target: InvokeTargetEnvelope,
  #[serde(default)]
  inputs: BTreeMap<String, String>,
  #[serde(default)]
  dry_run: bool,
}

#[derive(serde::Deserialize, Default)]
struct InvokeTargetEnvelope {
  #[serde(default)]
  application_id: Option<String>,
  #[serde(default)]
  target_label: Option<String>,
}

/// Decode a proto `json_payload` into a host `InvokeRequest` (provisional, see
/// the od5 NOTICE above). An empty payload yields host defaults.
pub fn decode_invoke_payload(
  command_id: String,
  json_payload: &[u8],
) -> Result<HostInvokeRequest, SessionApiError> {
  let envelope = if json_payload.is_empty() {
    InvokePayloadEnvelope::default()
  } else {
    serde_json::from_slice(json_payload)
      .map_err(|error| SessionApiError::PayloadDecode(error.to_string()))?
  };
  Ok(HostInvokeRequest {
    command_id,
    target: ExecutionTarget {
      application_id: envelope.target.application_id,
      target_label: envelope.target.target_label,
    },
    inputs: envelope.inputs,
    dry_run: envelope.dry_run,
  })
}

// NOTICE(api-p3-od4): resolved in API-P12 — invoke path uses ArtifactRecordV1Alpha1;
// GetOperation joins contract refs against the run artifact catalog.
fn artifact_ref_from_record(run_id: &str, record: &ArtifactRecordV1Alpha1) -> proto::ArtifactRef {
  proto::ArtifactRef {
    run_id: run_id.to_string(),
    artifact_id: record.artifact_id.as_str().to_string(),
    role: record.role.clone(),
  }
}

// API-P12: role on GetOperation comes from the run artifact catalog keyed by
// artifact_id; contract::ArtifactRef stays slim (reference-only).
fn artifact_ref_from_contract(
  artifact: &ContractArtifactRef,
  artifact_roles: &BTreeMap<String, String>,
  known_limits: &mut Vec<String>,
) -> proto::ArtifactRef {
  let artifact_id = artifact.artifact_id.as_str().to_string();
  let role = artifact_roles
    .get(&artifact_id)
    .cloned()
    .unwrap_or_default();
  if role.is_empty() {
    known_limits.push(ARTIFACT_ROLE_UNAVAILABLE_KNOWN_LIMIT.to_string());
  }
  proto::ArtifactRef {
    run_id: artifact.run_id.as_str().to_string(),
    artifact_id,
    role,
  }
}

/// Map an `InvokeResult` (plus the request `command_id`) to a proto
/// `InvokeResponse`. `extra_known_limits` surfaces invoke-path durability gaps
/// without changing execution status (see API-P11 partial-success policy).
pub fn invoke_result_to_response(
  command_id: &str,
  result: &InvokeResult,
  extra_known_limits: &[&str],
) -> proto::InvokeResponse {
  // API-P12: wire operation_id is invoke command_id on both Invoke and GetOperation.
  let artifacts = result
    .artifacts
    .iter()
    .map(|record| artifact_ref_from_record(&result.run_id, record))
    .collect();
  proto::InvokeResponse {
    operation: Some(proto::OperationRef {
      run_id: result.run_id.clone(),
      operation_id: command_id.to_string(),
    }),
    status: run_status_str(&result.status),
    artifacts,
    // NOTICE(api-p3-od1): known_limits is OperationResult-sourced and is not on
    // the InvokeResult return value; empty on the invoke path until the summary
    // source is joined with the persisted record. API-P11 may append durability
    // limits when summary persistence fails after a successful command.
    known_limits: extra_known_limits
      .iter()
      .map(|limit| (*limit).to_string())
      .collect(),
    failure_message: result.failure_message.clone().unwrap_or_default(),
  }
}

/// Map a joined two-source summary (API-P7) to a proto `GetOperationResponse`.
pub fn joined_to_get_operation_response(
  joined: &JoinedOperationSummary,
) -> proto::GetOperationResponse {
  let mut known_limits = joined.known_limits.clone();
  let (output_summary, signals, failure_message): (String, HashMap<String, String>, String) =
    match &joined.runtime {
      Some(runtime) => (
        runtime.output_summary.clone(),
        runtime.signals.clone().into_iter().collect(),
        runtime.failure_message.clone().unwrap_or_default(),
      ),
      None => {
        // NOTICE(api-p4-two-source): the runtime summary is absent. API-P4 says we
        // must not fabricate empty strings as authoritative data, so we surface
        // the gap as an explicit known_limit rather than silently returning empty
        // output_summary/signals.
        known_limits.push("auv.api.session.runtime_summary_unavailable".to_string());
        (String::new(), HashMap::new(), String::new())
      }
    };
  proto::GetOperationResponse {
    operation: Some(proto::OperationRef {
      run_id: joined.run_id.clone(),
      operation_id: joined.command_id.clone().unwrap_or_default(),
    }),
    status: operation_status_str(joined.status),
    output_summary,
    signals,
    artifacts: joined
      .artifacts
      .iter()
      .map(|artifact| {
        artifact_ref_from_contract(artifact, &joined.artifact_roles, &mut known_limits)
      })
      .collect(),
    failure_message,
    known_limits,
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use auv_cli_invoke::{InvokeResult, RunStatus};

  use super::{decode_invoke_payload, invoke_result_to_response, joined_to_get_operation_response};
  use crate::api::session_service::SessionApiError;
  use crate::api::session_service::summary::{
    ARTIFACT_ROLE_UNAVAILABLE_KNOWN_LIMIT, JoinedOperationSummary, RuntimeOperationSummary,
  };
  use crate::contract::{ArtifactRef as ContractArtifactRef, OperationStatus};
  use auv_tracing_driver::trace::{ArtifactId, RunId, SpanId};

  #[test]
  fn decode_empty_payload_uses_host_defaults() {
    let request = decode_invoke_payload("fixture.observe".to_string(), &[]).expect("decode");
    assert_eq!(request.command_id, "fixture.observe");
    assert!(request.target.application_id.is_none());
    assert!(request.inputs.is_empty());
    assert!(!request.dry_run);
  }

  #[test]
  fn decode_payload_maps_target_inputs_and_dry_run() {
    let payload = br#"{"target":{"application_id":"com.example.app"},"inputs":{"key":"Return"},"dry_run":true}"#;
    let request = decode_invoke_payload("input.key".to_string(), payload).expect("decode");
    assert_eq!(
      request.target.application_id.as_deref(),
      Some("com.example.app")
    );
    assert_eq!(
      request.inputs.get("key").map(String::as_str),
      Some("Return")
    );
    assert!(request.dry_run);
  }

  #[test]
  fn decode_invalid_payload_errors() {
    let error = decode_invoke_payload("x".to_string(), b"not json").expect_err("should fail");
    assert!(matches!(error, SessionApiError::PayloadDecode(_)));
  }

  #[test]
  fn invoke_result_maps_status_operation_and_failure() {
    let result = InvokeResult {
      run_id: "run-1".to_string(),
      producer_span_id: SpanId::new("0000000000000001"),
      command_id: "input.key".to_string(),
      command_summary: "Press a key.".to_string(),
      status: RunStatus::Failed,
      output_summary: "ignored on invoke response".to_string(),
      backend: None,
      signals: BTreeMap::new(),
      notes: Vec::new(),
      known_limits: Vec::new(),
      verification: None,
      report: None,
      artifacts: Vec::new(),
      artifact_paths: Vec::new(),
      failure_message: Some("boom".to_string()),
    };
    let response = invoke_result_to_response("input.key", &result, &[]);
    assert_eq!(response.status, "failed");
    assert_eq!(response.failure_message, "boom");
    assert!(response.known_limits.is_empty());
    let operation = response.operation.expect("operation ref");
    assert_eq!(operation.run_id, "run-1");
    assert_eq!(operation.operation_id, "input.key");
  }

  #[test]
  fn invoke_result_propagates_extra_known_limits() {
    let result = InvokeResult {
      run_id: "run-limits".to_string(),
      producer_span_id: SpanId::new("0000000000000001"),
      command_id: "fixture.observe".to_string(),
      command_summary: "Observe fixture.".to_string(),
      status: RunStatus::Completed,
      output_summary: "ok".to_string(),
      backend: None,
      signals: BTreeMap::new(),
      notes: Vec::new(),
      known_limits: Vec::new(),
      verification: None,
      report: None,
      artifacts: Vec::new(),
      artifact_paths: Vec::new(),
      failure_message: None,
    };
    let response = invoke_result_to_response(
      "fixture.observe",
      &result,
      &["auv.api.session.operation_summary_persist_failed"],
    );
    assert_eq!(response.status, "completed");
    assert_eq!(
      response.known_limits,
      vec!["auv.api.session.operation_summary_persist_failed".to_string()]
    );
  }

  #[test]
  fn joined_maps_to_get_operation_response_with_runtime() {
    let joined = JoinedOperationSummary {
      run_id: "run-2".to_string(),
      domain_operation_id: "music.search.results".to_string(),
      command_id: Some("music.search".to_string()),
      status: OperationStatus::Completed,
      known_limits: vec!["lim".to_string()],
      artifacts: Vec::new(),
      artifact_roles: BTreeMap::new(),
      runtime: Some(RuntimeOperationSummary {
        output_summary: "did the thing".to_string(),
        signals: BTreeMap::from([("k".to_string(), "v".to_string())]),
        failure_message: None,
      }),
    };
    let response = joined_to_get_operation_response(&joined);
    assert_eq!(response.status, "completed");
    assert_eq!(response.output_summary, "did the thing");
    assert_eq!(response.signals.get("k").map(String::as_str), Some("v"));
    assert_eq!(response.known_limits, vec!["lim"]);
    assert_eq!(response.operation.expect("op").operation_id, "music.search");
  }

  #[test]
  fn joined_without_runtime_flags_missing_summary() {
    let joined = JoinedOperationSummary {
      run_id: "run-3".to_string(),
      domain_operation_id: "op".to_string(),
      command_id: Some("fixture.observe".to_string()),
      status: OperationStatus::Completed,
      known_limits: Vec::new(),
      artifacts: Vec::new(),
      artifact_roles: BTreeMap::new(),
      runtime: None,
    };
    let response = joined_to_get_operation_response(&joined);
    assert!(response.output_summary.is_empty());
    assert!(
      response
        .known_limits
        .iter()
        .any(|limit| limit.contains("runtime_summary_unavailable"))
    );
  }

  #[test]
  fn joined_propagates_runtime_status_mismatch_known_limit() {
    let joined = JoinedOperationSummary {
      run_id: "run-mismatch".to_string(),
      domain_operation_id: "music.search.results".to_string(),
      command_id: Some("fixture.observe".to_string()),
      status: OperationStatus::Completed,
      known_limits: vec![
        "semantic_shaping_synthetic".to_string(),
        "auv.api.session.runtime_status_mismatch".to_string(),
      ],
      artifacts: Vec::new(),
      artifact_roles: BTreeMap::new(),
      runtime: Some(RuntimeOperationSummary {
        output_summary: "runtime failed".to_string(),
        signals: BTreeMap::new(),
        failure_message: Some("boom".to_string()),
      }),
    };
    let response = joined_to_get_operation_response(&joined);
    assert_eq!(response.status, "completed");
    assert!(
      response
        .known_limits
        .iter()
        .any(|limit| limit == "auv.api.session.runtime_status_mismatch")
    );
    assert_eq!(response.failure_message, "boom");
  }

  #[test]
  fn joined_maps_evidence_artifact_role_from_catalog() {
    let artifact_id = ArtifactId::new("artifact_evidence");
    let joined = JoinedOperationSummary {
      run_id: "run-role".to_string(),
      domain_operation_id: "music.search.results".to_string(),
      command_id: Some("music.search".to_string()),
      status: OperationStatus::Completed,
      known_limits: Vec::new(),
      artifacts: vec![ContractArtifactRef {
        run_id: RunId::new("run-role"),
        artifact_id: artifact_id.clone(),
        span_id: SpanId::new("0000000000000001"),
        captured_event_id: None,
      }],
      artifact_roles: BTreeMap::from([(
        artifact_id.as_str().to_string(),
        "evidence-pack".to_string(),
      )]),
      runtime: None,
    };
    let response = joined_to_get_operation_response(&joined);
    assert_eq!(response.artifacts.len(), 1);
    assert_eq!(response.artifacts[0].role, "evidence-pack");
    assert!(
      !response
        .known_limits
        .iter()
        .any(|limit| limit == ARTIFACT_ROLE_UNAVAILABLE_KNOWN_LIMIT)
    );
  }

  #[test]
  fn joined_flags_missing_artifact_role() {
    let joined = JoinedOperationSummary {
      run_id: "run-missing-role".to_string(),
      domain_operation_id: "op".to_string(),
      command_id: Some("fixture.observe".to_string()),
      status: OperationStatus::Completed,
      known_limits: Vec::new(),
      artifacts: vec![ContractArtifactRef {
        run_id: RunId::new("run-missing-role"),
        artifact_id: ArtifactId::new("missing"),
        span_id: SpanId::new("0000000000000001"),
        captured_event_id: None,
      }],
      artifact_roles: BTreeMap::new(),
      runtime: None,
    };
    let response = joined_to_get_operation_response(&joined);
    assert!(response.artifacts[0].role.is_empty());
    assert!(
      response
        .known_limits
        .iter()
        .any(|limit| limit == ARTIFACT_ROLE_UNAVAILABLE_KNOWN_LIMIT)
    );
  }
}
