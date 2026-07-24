//! Proto mapping for the session API frontend.

use std::collections::BTreeMap;

use auv_api_proto::v1::session as proto;
use auv_cli_invoke::{ExecutionTarget, InvokeRequest as HostInvokeRequest, InvokeResult};

use crate::api::session_service::SessionApiError;

#[derive(serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct InvokePayloadEnvelope {
  #[serde(default)]
  target: InvokeTargetEnvelope,
  #[serde(default)]
  inputs: BTreeMap<String, String>,
  #[serde(default)]
  dry_run: bool,
}

#[derive(serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct InvokeTargetEnvelope {
  #[serde(default)]
  application_id: Option<String>,
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
    },
    inputs: envelope.inputs,
    dry_run: envelope.dry_run,
  })
}

pub fn invoke_result_to_response(result: &InvokeResult, recording_failure: Option<&str>) -> proto::InvokeResponse {
  let terminal = match result.failure() {
    Some(message) => proto::invoke_response::Terminal::Failed(proto::InvokeFailed {
      message: message.to_string(),
    }),
    None => proto::invoke_response::Terminal::Completed(proto::InvokeCompleted {
      result_json: result.result().map_or_else(|| b"null".to_vec(), |value| value.to_string().into_bytes()),
    }),
  };
  proto::InvokeResponse {
    run_id: result.run_id.to_string(),
    terminal: Some(terminal),
    recording_failure: recording_failure.unwrap_or_default().to_string(),
  }
}

#[cfg(test)]
mod tests {
  use crate::api::session_service::SessionApiError;
  use auv_api_proto::v1::session as proto;
  use auv_cli_invoke::{InvokeCommandOutput, InvokeResult, default_registry};
  use auv_tracing::RunId;

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
  fn decode_payload_rejects_unused_target_fields() {
    let payload = br#"{"target":{"target_label":"unused"}}"#;

    let error = decode_invoke_payload("input.key".to_string(), payload).expect_err("unknown target fields must not be ignored");

    assert!(matches!(error, SessionApiError::PayloadDecode(_)));
  }

  #[test]
  fn invoke_response_is_derived_from_the_direct_value() {
    let registry = default_registry();
    let command = registry.resolve("scan.coverage").expect("command");
    let run_id = RunId::new();
    let typed_result = serde_json::json!({ "covered": 3, "missing": 1 });
    let result =
      InvokeResult::from_command_result(run_id, command, Ok(InvokeCommandOutput::from_result(&typed_result).expect("typed invoke result")));
    let response = invoke_result_to_response(&result, None);
    let Some(proto::invoke_response::Terminal::Completed(completed)) = response.terminal else {
      panic!("completed terminal");
    };
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&completed.result_json).expect("result JSON"), typed_result);
    assert_eq!(response.run_id, run_id.to_string());
    assert!(response.recording_failure.is_empty());
  }

  #[test]
  fn invoke_response_keeps_recording_failure_separate_from_direct_status() {
    let registry = default_registry();
    let command = registry.resolve("scan.coverage").expect("command");
    let result = InvokeResult::from_command_result(RunId::new(), command, Ok(InvokeCommandOutput::completed()));
    let response = invoke_result_to_response(&result, Some("recorded run snapshot is missing after execution"));

    assert!(matches!(response.terminal, Some(proto::invoke_response::Terminal::Completed(_))));
    assert_eq!(response.recording_failure, "recorded run snapshot is missing after execution");
  }
}
