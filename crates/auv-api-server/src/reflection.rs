//! gRPC Server Reflection with lossless protobuf descriptor responses.
//!
//! `tonic-reflection` indexes descriptors through `prost_types` and then
//! re-encodes them for each response. `prost_types` does not retain protobuf
//! extensions, so that path silently removes AUV's method annotations. This
//! adapter uses `prost-reflect` for indexing and response encoding because its
//! descriptor model retains registered custom options.

use std::collections::HashMap;
use std::sync::Arc;

use prost_reflect::{DescriptorPool, FileDescriptor};
use prost_types::{DescriptorProto, EnumDescriptorProto, FieldDescriptorProto, FileDescriptorProto};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::{StreamExt as _, wrappers};
use tonic::{Request, Response, Status, Streaming};
use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;
use tonic_reflection::pb::v1::server_reflection_response::MessageResponse;
use tonic_reflection::pb::v1::server_reflection_server::{ServerReflection, ServerReflectionServer};
use tonic_reflection::pb::v1::{
  ErrorResponse, ExtensionNumberResponse, FileDescriptorResponse, ListServiceResponse, ServerReflectionRequest, ServerReflectionResponse,
  ServiceResponse,
};

/// Builds the standard v1 reflection service for one Runner descriptor set.
///
/// The descriptor set must include the complete dependency closure for every
/// service the Runner serves. All services in that descriptor closure are
/// advertised. Runner descriptor builders must therefore include only service
/// definitions that the server actually serves.
pub fn service(encoded_file_descriptor_set: &[u8]) -> Result<ServerReflectionServer<Service>, Error> {
  let state = State::new(&[
    encoded_file_descriptor_set,
    tonic_reflection::pb::v1::FILE_DESCRIPTOR_SET,
  ])?;
  Ok(ServerReflectionServer::new(Service {
    state: Arc::new(state),
  }))
}

/// A reflection service that returns descriptors without dropping extensions.
#[derive(Debug)]
pub struct Service {
  state: Arc<State>,
}

#[tonic::async_trait]
impl ServerReflection for Service {
  type ServerReflectionInfoStream = ReceiverStream<Result<ServerReflectionResponse, Status>>;

  async fn server_reflection_info(
    &self,
    request: Request<Streaming<ServerReflectionRequest>>,
  ) -> Result<Response<Self::ServerReflectionInfoStream>, Status> {
    let mut requests = request.into_inner();
    let (responses, receiver) = mpsc::channel(1);
    let state = self.state.clone();

    tokio::spawn(async move {
      while let Some(request) = requests.next().await {
        let request = match request {
          Ok(request) => request,
          Err(status) => {
            let _ = responses.send(Err(status)).await;
            return;
          }
        };
        let message_response = message_response_for(&state, &request);
        let response = ServerReflectionResponse {
          valid_host: request.host.clone(),
          original_request: Some(request),
          message_response: Some(message_response),
        };
        if responses.send(Ok(response)).await.is_err() {
          return;
        }
      }
    });

    Ok(Response::new(wrappers::ReceiverStream::new(receiver)))
  }
}

fn message_response_for(state: &State, request: &ServerReflectionRequest) -> MessageResponse {
  match response_for(state, request) {
    Ok(response) => response,
    Err(status) => MessageResponse::ErrorResponse(ErrorResponse {
      error_code: status.code() as i32,
      error_message: status.message().to_string(),
    }),
  }
}

fn response_for(state: &State, request: &ServerReflectionRequest) -> Result<MessageResponse, Status> {
  match request.message_request.as_ref() {
    None => Err(Status::invalid_argument("invalid MessageRequest")),
    Some(MessageRequest::FileByFilename(filename)) => file_response(state, state.file_by_filename(filename)?),
    Some(MessageRequest::FileContainingSymbol(symbol)) => file_response(state, state.file_containing_symbol(symbol)?),
    Some(MessageRequest::FileContainingExtension(_)) => {
      // TODO(reflection-extension-query): Dynamic tool discovery resolves
      // option definitions through file imports. Add extension-number lookup
      // when a concrete Reflection consumer needs this request form.
      Err(Status::not_found("extensions are not supported"))
    }
    Some(MessageRequest::AllExtensionNumbersOfType(_)) => {
      // NOTICE: grpcurl expects an empty response instead of UNIMPLEMENTED.
      // Match `tonic-reflection` until extension-number lookup has a concrete AUV consumer.
      Ok(MessageResponse::AllExtensionNumbersResponse(ExtensionNumberResponse::default()))
    }
    Some(MessageRequest::ListServices(_)) => Ok(MessageResponse::ListServicesResponse(ListServiceResponse {
      service: state.services.iter().cloned().map(|name| ServiceResponse { name }).collect(),
    })),
  }
}

fn file_response(state: &State, root: Arc<ReflectedFile>) -> Result<MessageResponse, Status> {
  let mut files = Vec::new();
  let mut visited = std::collections::HashSet::new();
  let mut pending = vec![root];
  while let Some(file) = pending.pop() {
    if !visited.insert(file.name.clone()) {
      continue;
    }
    files.push(file.encoded.clone());
    for dependency in file.dependencies.iter().rev() {
      pending.push(state.file_by_filename(dependency)?);
    }
  }
  Ok(MessageResponse::FileDescriptorResponse(FileDescriptorResponse {
    file_descriptor_proto: files,
  }))
}

#[derive(Debug)]
struct ReflectedFile {
  dependencies: Vec<String>,
  encoded: Vec<u8>,
  name: String,
}

#[derive(Debug)]
struct State {
  services: Vec<String>,
  files: HashMap<String, Arc<ReflectedFile>>,
  symbols: HashMap<String, Arc<ReflectedFile>>,
}

impl State {
  fn new(encoded_file_descriptor_sets: &[&[u8]]) -> Result<Self, Error> {
    let mut state = Self {
      services: Vec::new(),
      files: HashMap::new(),
      symbols: HashMap::new(),
    };

    for encoded in encoded_file_descriptor_sets {
      let pool = DescriptorPool::decode(*encoded)?;
      for descriptor in pool.files() {
        state.insert_file(descriptor)?;
      }
    }
    state.services.sort();
    state.services.dedup();
    Ok(state)
  }

  fn insert_file(&mut self, descriptor: FileDescriptor) -> Result<(), Error> {
    let name = descriptor.name().to_string();
    if self.files.contains_key(&name) {
      return Ok(());
    }

    let proto = descriptor.file_descriptor_proto();
    let file = Arc::new(ReflectedFile {
      dependencies: proto.dependency.clone(),
      encoded: descriptor.encode_to_vec(),
      name: name.clone(),
    });
    self.files.insert(name, file.clone());
    self.index_file(file, proto)
  }

  fn index_file(&mut self, file: Arc<ReflectedFile>, descriptor: &FileDescriptorProto) -> Result<(), Error> {
    let prefix = descriptor.package.as_deref().unwrap_or_default();
    for message in &descriptor.message_type {
      self.index_message(file.clone(), prefix, message)?;
    }
    for enumeration in &descriptor.enum_type {
      self.index_enum(file.clone(), prefix, enumeration)?;
    }
    for extension in &descriptor.extension {
      self.index_field(file.clone(), prefix, extension)?;
    }
    for service in &descriptor.service {
      let service_name = qualified_name(prefix, "service", service.name.as_deref())?;
      self.services.push(service_name.clone());
      self.symbols.insert(service_name.clone(), file.clone());
      for method in &service.method {
        let method_name = qualified_name(&service_name, "method", method.name.as_deref())?;
        self.symbols.insert(method_name, file.clone());
      }
    }
    Ok(())
  }

  fn index_message(&mut self, file: Arc<ReflectedFile>, prefix: &str, message: &DescriptorProto) -> Result<(), Error> {
    let message_name = qualified_name(prefix, "message", message.name.as_deref())?;
    self.symbols.insert(message_name.clone(), file.clone());
    for nested in &message.nested_type {
      self.index_message(file.clone(), &message_name, nested)?;
    }
    for enumeration in &message.enum_type {
      self.index_enum(file.clone(), &message_name, enumeration)?;
    }
    for field in message.field.iter().chain(&message.extension) {
      self.index_field(file.clone(), &message_name, field)?;
    }
    for oneof in &message.oneof_decl {
      let oneof_name = qualified_name(&message_name, "oneof", oneof.name.as_deref())?;
      self.symbols.insert(oneof_name, file.clone());
    }
    Ok(())
  }

  fn index_enum(&mut self, file: Arc<ReflectedFile>, prefix: &str, enumeration: &EnumDescriptorProto) -> Result<(), Error> {
    let enum_name = qualified_name(prefix, "enum", enumeration.name.as_deref())?;
    self.symbols.insert(enum_name.clone(), file.clone());
    for value in &enumeration.value {
      let value_name = qualified_name(&enum_name, "enum value", value.name.as_deref())?;
      self.symbols.insert(value_name, file.clone());
    }
    Ok(())
  }

  fn index_field(&mut self, file: Arc<ReflectedFile>, prefix: &str, field: &FieldDescriptorProto) -> Result<(), Error> {
    let field_name = qualified_name(prefix, "field", field.name.as_deref())?;
    self.symbols.insert(field_name, file);
    Ok(())
  }

  fn file_by_filename(&self, filename: &str) -> Result<Arc<ReflectedFile>, Status> {
    self.files.get(filename).cloned().ok_or_else(|| Status::not_found(format!("file '{filename}' not found")))
  }

  fn file_containing_symbol(&self, symbol: &str) -> Result<Arc<ReflectedFile>, Status> {
    self.symbols.get(symbol).cloned().ok_or_else(|| Status::not_found(format!("symbol '{symbol}' not found")))
  }
}

fn qualified_name(prefix: &str, kind: &str, name: Option<&str>) -> Result<String, Error> {
  let name = name.ok_or_else(|| Error::InvalidDescriptor(format!("missing {kind} name")))?;
  if prefix.is_empty() {
    Ok(name.to_string())
  } else {
    Ok(format!("{prefix}.{name}"))
  }
}

/// Error returned when a Runner descriptor set cannot be reflected safely.
#[derive(Debug, thiserror::Error)]
pub enum Error {
  #[error("failed to decode Runner descriptor set: {0}")]
  Decode(#[from] prost_reflect::DescriptorError),
  #[error("invalid Runner descriptor: {0}")]
  InvalidDescriptor(String),
}

#[cfg(test)]
mod tests {
  use super::*;
  use prost::Message as _;

  #[test]
  fn response_preserves_custom_method_options() {
    let pool = DescriptorPool::decode(auv_api_proto::FILE_DESCRIPTOR_SET).expect("AUV descriptors");
    let expected = pool.get_service_by_name("auv.api.driver.v1.DisplayService").expect("display service").parent_file().encode_to_vec();
    let state = State::new(&[auv_api_proto::FILE_DESCRIPTOR_SET]).expect("reflection state");

    let reflected =
      state.file_containing_symbol("auv.api.driver.v1.DisplayService.ListDisplays").expect("method descriptor").encoded.clone();

    assert_eq!(reflected, expected);
    let lossy = <FileDescriptorProto as prost::Message>::decode(expected.as_slice()).expect("prost descriptor").encode_to_vec();
    assert_ne!(lossy, reflected, "the fixture must contain custom method options");
  }

  #[test]
  fn dependency_files_are_queryable_without_non_service_entries() {
    let state = State::new(&[auv_api_proto::FILE_DESCRIPTOR_SET]).expect("reflection state");

    assert!(state.file_by_filename("google/protobuf/descriptor.proto").is_ok());
    assert!(state.services.iter().any(|name| name == "auv.api.driver.v1.DisplayService"));
    assert!(!state.services.iter().any(|name| name.starts_with("google.protobuf.")));

    let response = file_response(&state, state.file_containing_symbol("auv.api.driver.v1.DisplayService").expect("service descriptor"))
      .expect("descriptor response");
    let MessageResponse::FileDescriptorResponse(response) = response else {
      panic!("expected descriptor response");
    };
    assert!(response.file_descriptor_proto.len() > 1, "response must include imported descriptors");
  }

  #[test]
  fn missing_symbols_are_encoded_as_reflection_error_responses() {
    let state = State::new(&[auv_api_proto::FILE_DESCRIPTOR_SET]).expect("reflection state");
    let request = ServerReflectionRequest {
      message_request: Some(MessageRequest::FileContainingSymbol("auv.missing.Service".to_string())),
      ..Default::default()
    };
    let response = message_response_for(&state, &request);

    let MessageResponse::ErrorResponse(error) = response else {
      panic!("expected error response");
    };
    assert_eq!(error.error_code, tonic::Code::NotFound as i32);
    assert!(error.error_message.contains("auv.missing.Service"));
  }
}
