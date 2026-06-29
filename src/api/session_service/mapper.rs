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
use crate::api::session_service::summary::JoinedOperationSummary;
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

// NOTICE(api-p3-od4): proto ArtifactRef requires `role`. The invoke-path record
// (ArtifactRecordV1Alpha1) carries `role`; the persisted `contract::ArtifactRef`
// does NOT. This mapper fills `role` from the record on the invoke path and
// leaves it empty for persisted refs until the role-source rule is resolved.
fn artifact_ref_from_record(run_id: &str, record: &ArtifactRecordV1Alpha1) -> proto::ArtifactRef {
  proto::ArtifactRef {
    run_id: run_id.to_string(),
    artifact_id: record.artifact_id.as_str().to_string(),
    role: record.role.clone(),
  }
}

fn artifact_ref_from_contract(artifact: &ContractArtifactRef) -> proto::ArtifactRef {
  proto::ArtifactRef {
    run_id: artifact.run_id.as_str().to_string(),
    artifact_id: artifact.artifact_id.as_str().to_string(),
    // NOTICE(api-p3-od4): role unavailable on contract::ArtifactRef; empty until resolved.
    role: String::new(),
  }
}

/// Map an `InvokeResult` (plus the request `command_id`) to a proto
/// `InvokeResponse`.
pub fn invoke_result_to_response(command_id: &str, result: &InvokeResult) -> proto::InvokeResponse {
  // NOTICE(api-p3-od2): operation_id on the invoke path is the request
  // command_id (per the API-P2 proto comment). GetOperation instead returns the
  // persisted OperationResult.operation_id (a domain label). These can diverge
  // for the same run; unifying them is API-P3 open decision 2 / API-P4 gate 3
  // and needs an owner decision (e.g. persist command_id or widen OperationRef).
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
    // source is joined with the persisted record.
    known_limits: Vec::new(),
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
      operation_id: joined.operation_id.clone(),
    }),
    status: operation_status_str(joined.status),
    output_summary,
    signals,
    artifacts: joined
      .artifacts
      .iter()
      .map(artifact_ref_from_contract)
      .collect(),
    failure_message,
    known_limits,
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use auv_cli_invoke::{InvokeResult, RunStatus};
  use auv_tracing_driver::trace::SpanId;

  use super::{decode_invoke_payload, invoke_result_to_response, joined_to_get_operation_response};
  use crate::api::session_service::SessionApiError;
  use crate::api::session_service::summary::{JoinedOperationSummary, RuntimeOperationSummary};
  use crate::contract::OperationStatus;

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
      status: RunStatus::Failed,
      output_summary: "ignored on invoke response".to_string(),
      signals: BTreeMap::new(),
      artifacts: Vec::new(),
      artifact_paths: Vec::new(),
      failure_message: Some("boom".to_string()),
    };
    let response = invoke_result_to_response("input.key", &result);
    assert_eq!(response.status, "failed");
    assert_eq!(response.failure_message, "boom");
    assert!(response.known_limits.is_empty());
    let operation = response.operation.expect("operation ref");
    assert_eq!(operation.run_id, "run-1");
    assert_eq!(operation.operation_id, "input.key");
  }

  #[test]
  fn joined_maps_to_get_operation_response_with_runtime() {
    let joined = JoinedOperationSummary {
      run_id: "run-2".to_string(),
      operation_id: "music.search.results".to_string(),
      status: OperationStatus::Completed,
      known_limits: vec!["lim".to_string()],
      artifacts: Vec::new(),
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
    assert_eq!(
      response.operation.expect("op").operation_id,
      "music.search.results"
    );
  }

  #[test]
  fn joined_without_runtime_flags_missing_summary() {
    let joined = JoinedOperationSummary {
      run_id: "run-3".to_string(),
      operation_id: "op".to_string(),
      status: OperationStatus::Completed,
      known_limits: Vec::new(),
      artifacts: Vec::new(),
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
}
