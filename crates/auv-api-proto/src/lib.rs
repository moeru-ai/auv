pub mod v1 {
  pub mod session {
    tonic::include_proto!("auv.api.session.v1");

    /// Encoded protobuf schema metadata for gRPC reflection and tools such as
    /// `grpcurl`; normal clients use the generated request/response types.
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("auv.api.session.v1");
  }
}

#[cfg(test)]
mod tests {
  use std::collections::HashMap;

  use crate::v1::session::session_service_client::SessionServiceClient;
  use crate::v1::session::{
    ArtifactRef, ControlFailure, FILE_DESCRIPTOR_SET, GetOperationResponse, InvokeRequest, OperationRef, SessionRef,
  };
  use prost::Message;
  use prost_types::FileDescriptorSet;

  #[test]
  fn invoke_request_round_trip() {
    let request = InvokeRequest {
      session: Some(SessionRef {
        session_id: "sess-abc".to_string(),
      }),
      command_id: "minecraft.query".to_string(),
      json_payload: br#"{"target":"block"}"#.to_vec(),
    };

    let encoded = request.encode_to_vec();
    let decoded = InvokeRequest::decode(encoded.as_slice()).expect("decode InvokeRequest");

    assert_eq!(decoded.command_id, "minecraft.query");
    assert_eq!(decoded.json_payload, br#"{"target":"block"}"#);
    let session = decoded.session.expect("session ref");
    assert_eq!(session.session_id, "sess-abc");
  }

  #[test]
  fn get_operation_response_round_trip() {
    let response = GetOperationResponse {
      operation: Some(OperationRef {
        run_id: "run-1".to_string(),
        operation_id: "minecraft.query".to_string(),
      }),
      status: "failed".to_string(),
      output_summary: "command rejected".to_string(),
      signals: HashMap::from([("exit_code".to_string(), "1".to_string())]),
      artifacts: vec![ArtifactRef {
        run_id: "run-1".to_string(),
        artifact_id: "art-1".to_string(),
        role: "trace".to_string(),
      }],
      failure_message: "invalid payload".to_string(),
      known_limits: vec!["json_payload_max_bytes".to_string()],
      control_failure: Some(ControlFailure {
        layer: "control_failed".to_string(),
        message: "accessibility permission was denied".to_string(),
        recovery: "grant Accessibility in System Settings".to_string(),
      }),
    };

    let encoded = response.encode_to_vec();
    let decoded = GetOperationResponse::decode(encoded.as_slice()).expect("decode GetOperationResponse");

    assert_eq!(decoded.status, "failed");
    assert_eq!(decoded.failure_message, "invalid payload");
    assert_eq!(decoded.known_limits, vec!["json_payload_max_bytes"]);
    let operation = decoded.operation.expect("operation ref");
    assert_eq!(operation.run_id, "run-1");
    assert_eq!(operation.operation_id, "minecraft.query");
    assert_eq!(decoded.artifacts.len(), 1);
    assert_eq!(decoded.signals.get("exit_code"), Some(&"1".to_string()));
    let control_failure = decoded.control_failure.expect("control_failure should round-trip");
    assert_eq!(control_failure.layer, "control_failed");
    assert_eq!(control_failure.message, "accessibility permission was denied");
    assert_eq!(control_failure.recovery, "grant Accessibility in System Settings");
  }

  #[test]
  fn file_descriptor_set_lists_session_service_rpcs() {
    let descriptor_set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("decode FILE_DESCRIPTOR_SET");

    let session_file = descriptor_set
      .file
      .iter()
      .find(|file| file.package.as_deref() == Some("auv.api.session.v1"))
      .expect("auv.api.session.v1 file descriptor");

    let session_service =
      session_file.service.iter().find(|service| service.name.as_deref() == Some("SessionService")).expect("SessionService descriptor");

    let rpc_names: Vec<&str> = session_service.method.iter().filter_map(|method| method.name.as_deref()).collect();

    assert_eq!(
      rpc_names,
      vec![
        "CreateSession",
        "Invoke",
        "GetOperation",
        "StreamSessionEvents",
      ]
    );
  }

  #[test]
  fn session_service_client_is_generated() {
    fn assert_session_service_client<T>() {}
    assert_session_service_client::<SessionServiceClient<tonic::transport::Channel>>();
  }
}
