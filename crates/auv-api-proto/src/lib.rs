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
  use crate::v1::session::session_service_client::SessionServiceClient;
  use crate::v1::session::{FILE_DESCRIPTOR_SET, InvokeRequest, SessionRef};
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
  fn file_descriptor_set_exposes_direct_invoke_without_operation_readback() {
    let descriptor_set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("decode FILE_DESCRIPTOR_SET");

    let session_file = descriptor_set
      .file
      .iter()
      .find(|file| file.package.as_deref() == Some("auv.api.session.v1"))
      .expect("auv.api.session.v1 file descriptor");

    let session_service =
      session_file.service.iter().find(|service| service.name.as_deref() == Some("SessionService")).expect("SessionService descriptor");

    let rpc_names: Vec<&str> = session_service.method.iter().filter_map(|method| method.name.as_deref()).collect();

    assert_eq!(rpc_names, vec!["CreateSession", "Invoke", "StreamSessionEvents",]);

    assert!(
      session_file.message_type.iter().all(|message| message.name.as_deref() != Some("OperationRef")),
      "synchronous Invoke must expose run identity directly without an operation readback type"
    );

    let invoke_response =
      session_file.message_type.iter().find(|message| message.name.as_deref() == Some("InvokeResponse")).expect("InvokeResponse descriptor");
    assert_eq!(invoke_response.field.first().and_then(|field| field.name.as_deref()), Some("run_id"));
    assert!(
      invoke_response.field.iter().any(|field| field.name.as_deref() == Some("recording_failure")),
      "InvokeResponse must expose post-execution recording failure separately from direct command status"
    );
  }

  #[test]
  fn session_service_client_is_generated() {
    fn assert_session_service_client<T>() {}
    assert_session_service_client::<SessionServiceClient<tonic::transport::Channel>>();
  }
}
