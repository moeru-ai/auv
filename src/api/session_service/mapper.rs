//! Proto mapping for the session API frontend.

use std::collections::BTreeMap;

use auv_api_proto::v1::session as proto;
use auv_cli_invoke::{ExecutionTarget, InvokeRequest as HostInvokeRequest, InvokeResult};

use crate::api::session_service::SessionApiError;

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

pub fn decode_invoke_payload(command_id: String, json_payload: &[u8]) -> Result<HostInvokeRequest, SessionApiError> {
  let envelope = if json_payload.is_empty() {
    InvokePayloadEnvelope::default()
  } else {
    serde_json::from_slice(json_payload).map_err(|error| SessionApiError::PayloadDecode(error.to_string()))?
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

pub fn invoke_result_to_response(command_id: &str, result: &InvokeResult, recording_failure: Option<&str>) -> proto::InvokeResponse {
  let mut known_limits = Vec::new();
  if recording_failure.is_some() {
    known_limits.push("auv.api.session.recording_flush_failed".to_string());
  }
  let artifacts = result
    .canonical_artifacts
    .iter()
    .map(|artifact| proto::ArtifactRef {
      run_id: artifact.uri().run_id().to_string(),
      artifact_id: artifact.uri().artifact_id().to_string(),
      // TODO(session-artifact-ref-proto-v2): Remove the historical `role` field from
      // the protobuf surface; canonical identity and purpose are not roles.
      role: String::new(),
    })
    .collect();
  proto::InvokeResponse {
    operation: Some(proto::OperationRef {
      run_id: result.run_id.clone(),
      operation_id: command_id.to_string(),
    }),
    status: result.status.as_str().to_string(),
    artifacts,
    known_limits,
    failure_message: result.failure_message.clone().unwrap_or_default(),
  }
}

#[cfg(test)]
mod tests {
  use auv_cli_invoke::{InvokeCommandOutput, InvokeResult, RunStatus, default_registry};

  use super::{decode_invoke_payload, invoke_result_to_response};

  #[test]
  fn decode_payload_maps_target_inputs_and_dry_run() {
    let payload = br#"{"target":{"application_id":"com.example.app"},"inputs":{"key":"Return"},"dry_run":true}"#;
    let request = decode_invoke_payload("input.key".to_string(), payload).expect("decode");
    assert_eq!(request.target.application_id.as_deref(), Some("com.example.app"));
    assert_eq!(request.inputs.get("key").map(String::as_str), Some("Return"));
    assert!(request.dry_run);
  }

  #[test]
  fn invoke_response_is_derived_from_the_direct_value() {
    let registry = default_registry();
    let command = registry.resolve("scan.coverage").expect("command");
    let result = InvokeResult::from_command_result("run-direct", command, Ok(InvokeCommandOutput::new("coverage")));
    let response = invoke_result_to_response(command.id, &result, None);
    assert_eq!(response.status, RunStatus::Completed.as_str());
    assert_eq!(response.operation.expect("operation").run_id, "run-direct");
    assert!(response.known_limits.is_empty());
  }
}
