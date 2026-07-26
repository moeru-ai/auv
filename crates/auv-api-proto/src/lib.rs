pub mod v1 {
  pub mod session {
    tonic::include_proto!("auv.api.session.v1");

    /// Encoded protobuf schema metadata for gRPC reflection and tools such as
    /// `grpcurl`; normal clients use the generated request/response types.
    pub const FILE_DESCRIPTOR_SET: &[u8] = tonic::include_file_descriptor_set!("auv.api.session.v1");
  }
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
