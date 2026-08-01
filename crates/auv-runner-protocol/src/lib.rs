//! Private process transport used by daemon-owned Runner children.
//!
//! This crate does not define business services or expose a listener. It only
//! adopts the connected descriptor inherited from the parent daemon so a
//! Runner can serve its own generated tonic services over that private stream.

use std::collections::BTreeMap;
use std::pin::Pin;
use std::task::{Context as TaskContext, Poll as TaskPoll};

use prost::Message as _;
use prost_reflect::DescriptorPool;
use tokio_stream::{Stream, StreamExt};
use tonic::{Request, Response, Status, Streaming};
use tonic_reflection::pb::v1::server_reflection_request::MessageRequest;
use tonic_reflection::pb::v1::server_reflection_response::MessageResponse;
use tonic_reflection::pb::v1::server_reflection_server::{ServerReflection, ServerReflectionServer};
use tonic_reflection::pb::v1::{
  ExtensionNumberResponse, FileDescriptorResponse, ListServiceResponse, ServerReflectionRequest, ServerReflectionResponse, ServiceResponse,
};

#[cfg(unix)]
use std::os::fd::{FromRawFd, RawFd};
#[cfg(unix)]
use std::pin::Pin as StdPin;
#[cfg(unix)]
use std::task::{Context, Poll};

pub const RUNNER_IPC_FD_ENV: &str = "AUV_RUNNER_IPC_FD";
pub const RUNNER_IPC_FD: i32 = 3;
pub const RUNTIME_SERVICE_NAME: &str = "auv.api.runner.v1.RunnerRuntimeService";

/// Adds the mandatory Runner runtime descriptor closure to an app-owned
/// business descriptor set without decoding away custom protobuf options.
pub fn merge_runtime_descriptor_set(business_descriptor_set: &[u8]) -> Result<Vec<u8>, String> {
  extract_file_descriptor_payloads(business_descriptor_set)?;
  let runtime = auv_api_proto::descriptor_set_for_service(RUNTIME_SERVICE_NAME)?;
  let mut merged = business_descriptor_set.to_vec();
  merged.extend_from_slice(&runtime);
  // Validate duplicate names and the combined descriptor graph before serving
  // it through reflection.
  RawReflection::new(&merged)?;
  Ok(merged)
}

use auv_api_proto::auv::api::runner::v1 as runtime_proto;
use runtime_proto::runner_runtime_service_server::RunnerRuntimeService;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, Ordering};
use tokio::sync::{Notify, Semaphore, watch};
use tower::Service;

/// Stable identity reported by a compatible Runner runtime.
#[derive(Clone, Debug)]
pub struct RuntimeMetadata {
  pub runner_class: String,
  pub display_name: String,
  pub labels: std::collections::HashMap<String, String>,
  pub operation_capacity: u32,
}

/// Shared runtime status and operation-admission controller.
#[derive(Clone, Debug)]
pub struct RuntimeControl {
  metadata: RuntimeMetadata,
  state: Arc<RuntimeState>,
}

#[derive(Debug)]
struct RuntimeState {
  phase: AtomicU8,
  active: AtomicU32,
  queued: AtomicU32,
  capacity: u32,
  draining: AtomicBool,
  permits: Arc<Semaphore>,
  changed: watch::Sender<runtime_proto::RunnerRuntimeStatus>,
  operations_finished: Notify,
}

impl RuntimeControl {
  /// Creates a ready runtime with bounded operation concurrency.
  pub fn ready(metadata: RuntimeMetadata) -> Result<Self, String> {
    if metadata.operation_capacity == 0 {
      return Err("Runner runtime operation capacity must be positive".to_string());
    }
    let initial = runtime_status(runtime_proto::RunnerRuntimePhase::Ready, 0, 0, metadata.operation_capacity);
    let (changed, _) = watch::channel(initial);
    Ok(Self {
      state: Arc::new(RuntimeState {
        phase: AtomicU8::new(runtime_proto::RunnerRuntimePhase::Ready as u8),
        active: AtomicU32::new(0),
        queued: AtomicU32::new(0),
        capacity: metadata.operation_capacity,
        draining: AtomicBool::new(false),
        permits: Arc::new(Semaphore::new(metadata.operation_capacity as usize)),
        changed,
        operations_finished: Notify::new(),
      }),
      metadata,
    })
  }

  /// Waits for one operation slot and returns a guard that updates status.
  pub async fn begin_operation(&self) -> Result<RuntimeOperationGuard, Status> {
    if self.state.draining.load(Ordering::Acquire) {
      return Err(Status::unavailable("Runner runtime is draining"));
    }
    self.state.queued.fetch_add(1, Ordering::AcqRel);
    self.publish();
    let permit = self.state.permits.clone().acquire_owned().await;
    self.state.queued.fetch_sub(1, Ordering::AcqRel);
    self.publish();
    let permit = permit.map_err(|_| Status::unavailable("Runner runtime is draining"))?;
    if self.state.draining.load(Ordering::Acquire) {
      drop(permit);
      return Err(Status::unavailable("Runner runtime is draining"));
    }
    self.state.active.fetch_add(1, Ordering::AcqRel);
    self.publish();
    Ok(RuntimeOperationGuard {
      state: self.state.clone(),
      _permit: permit,
    })
  }

  pub fn service(&self) -> runtime_proto::runner_runtime_service_server::RunnerRuntimeServiceServer<Self> {
    runtime_proto::runner_runtime_service_server::RunnerRuntimeServiceServer::new(self.clone())
  }

  /// Wraps one tonic business service with runtime admission and full response
  /// body/stream lifetime accounting.
  pub fn track<S>(&self, service: S) -> RuntimeTracked<S> {
    RuntimeTracked {
      inner: service,
      runtime: self.clone(),
    }
  }

  pub fn descriptor_set_for_services(service_names: &[&str]) -> Result<Vec<u8>, String> {
    let mut names = service_names.to_vec();
    names.push(RUNTIME_SERVICE_NAME);
    auv_api_proto::descriptor_set_for_services(&names)
  }

  fn snapshot(&self) -> runtime_proto::RunnerRuntimeStatus {
    runtime_status(
      phase_from_u8(self.state.phase.load(Ordering::Acquire)),
      self.state.active.load(Ordering::Acquire),
      self.state.queued.load(Ordering::Acquire),
      self.metadata.operation_capacity,
    )
  }

  fn publish(&self) {
    self.state.changed.send_replace(self.snapshot());
  }
}

/// Tonic service adapter that keeps an operation active until its response
/// body is completed or dropped, including server-streaming responses.
#[derive(Clone, Debug)]
pub struct RuntimeTracked<S> {
  inner: S,
  runtime: RuntimeControl,
}

impl<S> tonic::server::NamedService for RuntimeTracked<S>
where
  S: tonic::server::NamedService,
{
  const NAME: &'static str = S::NAME;
}

impl<S> Service<http::Request<tonic::body::Body>> for RuntimeTracked<S>
where
  S: Service<http::Request<tonic::body::Body>, Response = http::Response<tonic::body::Body>, Error = std::convert::Infallible>
    + Clone
    + Send
    + 'static,
  S::Future: Send + 'static,
{
  type Response = http::Response<tonic::body::Body>;
  type Error = std::convert::Infallible;
  type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send + 'static>>;

  fn poll_ready(&mut self, context: &mut TaskContext<'_>) -> TaskPoll<Result<(), Self::Error>> {
    self.inner.poll_ready(context)
  }

  fn call(&mut self, request: http::Request<tonic::body::Body>) -> Self::Future {
    let mut inner = self.inner.clone();
    let runtime = self.runtime.clone();
    Box::pin(async move {
      let guard = match runtime.begin_operation().await {
        Ok(guard) => guard,
        Err(status) => return Ok(status.into_http()),
      };
      let response = inner.call(request).await?;
      Ok(response.map(|body| tonic::body::Body::new(RuntimeTrackedBody::new(body, guard))))
    })
  }
}

struct RuntimeTrackedBody {
  inner: Pin<Box<tonic::body::Body>>,
  guard: Option<RuntimeOperationGuard>,
}

impl RuntimeTrackedBody {
  fn new(inner: tonic::body::Body, guard: RuntimeOperationGuard) -> Self {
    Self {
      inner: Box::pin(inner),
      guard: Some(guard),
    }
  }
}

impl http_body::Body for RuntimeTrackedBody {
  type Data = <tonic::body::Body as http_body::Body>::Data;
  type Error = <tonic::body::Body as http_body::Body>::Error;

  fn poll_frame(
    mut self: Pin<&mut Self>,
    context: &mut TaskContext<'_>,
  ) -> TaskPoll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
    let polled = self.inner.as_mut().poll_frame(context);
    if matches!(polled, TaskPoll::Ready(None) | TaskPoll::Ready(Some(Err(_)))) {
      self.guard.take();
    }
    polled
  }

  fn is_end_stream(&self) -> bool {
    self.inner.is_end_stream()
  }

  fn size_hint(&self) -> http_body::SizeHint {
    self.inner.size_hint()
  }
}

/// Releases one active operation slot and publishes the resulting status.
#[derive(Debug)]
pub struct RuntimeOperationGuard {
  state: Arc<RuntimeState>,
  _permit: tokio::sync::OwnedSemaphorePermit,
}

impl Drop for RuntimeOperationGuard {
  fn drop(&mut self) {
    self.state.active.fetch_sub(1, Ordering::AcqRel);
    let status = runtime_status(
      phase_from_u8(self.state.phase.load(Ordering::Acquire)),
      self.state.active.load(Ordering::Acquire),
      self.state.queued.load(Ordering::Acquire),
      self.state.capacity,
    );
    self.state.changed.send_replace(status);
    self.state.operations_finished.notify_waiters();
  }
}

type RuntimeStatusStream = Pin<Box<dyn Stream<Item = Result<runtime_proto::WatchStatusResponse, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl RunnerRuntimeService for RuntimeControl {
  type WatchStatusStream = RuntimeStatusStream;

  async fn get_metadata(
    &self,
    _request: Request<runtime_proto::GetMetadataRequest>,
  ) -> Result<Response<runtime_proto::GetMetadataResponse>, Status> {
    Ok(Response::new(runtime_proto::GetMetadataResponse {
      runner_class: self.metadata.runner_class.clone(),
      display_name: self.metadata.display_name.clone(),
      labels: self.metadata.labels.clone(),
      operation_capacity: self.metadata.operation_capacity,
    }))
  }

  async fn get_status(
    &self,
    _request: Request<runtime_proto::GetStatusRequest>,
  ) -> Result<Response<runtime_proto::GetStatusResponse>, Status> {
    Ok(Response::new(runtime_proto::GetStatusResponse {
      status: Some(self.snapshot()),
    }))
  }

  async fn watch_status(&self, _request: Request<runtime_proto::WatchStatusRequest>) -> Result<Response<Self::WatchStatusStream>, Status> {
    let stream = tokio_stream::wrappers::WatchStream::new(self.state.changed.subscribe()).map(|status| {
      Ok(runtime_proto::WatchStatusResponse {
        status: Some(status),
      })
    });
    Ok(Response::new(Box::pin(stream)))
  }

  async fn drain(&self, request: Request<runtime_proto::DrainRequest>) -> Result<Response<runtime_proto::DrainResponse>, Status> {
    self.state.draining.store(true, Ordering::Release);
    self.state.phase.store(runtime_proto::RunnerRuntimePhase::Draining as u8, Ordering::Release);
    self.state.permits.close();
    self.publish();

    let grace = request.into_inner().grace_period.map(duration_from_proto).transpose()?.unwrap_or_default();
    let wait = async {
      while self.state.active.load(Ordering::Acquire) != 0 {
        self.state.operations_finished.notified().await;
      }
    };
    if grace.is_zero() {
      if self.state.active.load(Ordering::Acquire) != 0 {
        return Err(Status::deadline_exceeded("Runner drain grace period elapsed"));
      }
    } else if tokio::time::timeout(grace, wait).await.is_err() {
      return Err(Status::deadline_exceeded("Runner drain grace period elapsed"));
    }
    Ok(Response::new(runtime_proto::DrainResponse {
      status: Some(self.snapshot()),
    }))
  }
}

fn runtime_status(phase: runtime_proto::RunnerRuntimePhase, active: u32, queued: u32, capacity: u32) -> runtime_proto::RunnerRuntimeStatus {
  runtime_proto::RunnerRuntimeStatus {
    phase: phase as i32,
    operations: Some(runtime_proto::RunnerRuntimeOperationsStatus {
      active,
      queued,
      capacity,
    }),
    observed_at: Some(system_timestamp()),
  }
}

fn system_timestamp() -> prost_types::Timestamp {
  let duration = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
  prost_types::Timestamp {
    seconds: duration.as_secs() as i64,
    nanos: duration.subsec_nanos() as i32,
  }
}

fn duration_from_proto(value: prost_types::Duration) -> Result<std::time::Duration, Status> {
  if value.seconds < 0 || value.nanos < 0 || value.nanos >= 1_000_000_000 {
    return Err(Status::invalid_argument("grace_period must be a non-negative protobuf duration"));
  }
  Ok(std::time::Duration::new(value.seconds as u64, value.nanos as u32))
}

fn phase_from_u8(value: u8) -> runtime_proto::RunnerRuntimePhase {
  runtime_proto::RunnerRuntimePhase::try_from(i32::from(value)).unwrap_or(runtime_proto::RunnerRuntimePhase::Failed)
}

/// Builds gRPC reflection without decoding descriptors through `prost_types`.
///
/// `tonic-reflection` currently drops protobuf custom option extensions while
/// indexing an encoded descriptor set. Runner readiness compares those options
/// against the daemon-trusted schema, so this small service retains each raw
/// `FileDescriptorProto` byte string exactly as generated by `protoc`.
///
/// This is the private daemon-readiness reflection subset: filename lookup,
/// service/message/enum/extension symbol lookup, and service listing. It is not
/// the daemon's merged public reflection surface.
// TODO(merged-public-reflection): add method/field/enum-value symbol indexing,
// extension-number lookup, and cross-Runner conflict semantics only with the
// separately approved public reflection/streaming slice.
pub fn reflection_service(descriptor_set: &[u8]) -> Result<ServerReflectionServer<RawReflection>, String> {
  Ok(ServerReflectionServer::new(RawReflection::new(descriptor_set)?))
}

impl RawReflection {
  fn new(descriptor_set: &[u8]) -> Result<Self, String> {
    let raw_files = extract_file_descriptor_payloads(descriptor_set)?;
    let pool = DescriptorPool::decode(descriptor_set).map_err(|error| format!("invalid Runner descriptor set: {error}"))?;
    let mut files = BTreeMap::new();
    let mut symbols = BTreeMap::new();
    let mut services = Vec::new();
    for encoded in raw_files {
      let descriptor =
        prost_types::FileDescriptorProto::decode(encoded.as_slice()).map_err(|error| format!("invalid Runner file descriptor: {error}"))?;
      let name = descriptor.name.filter(|name| !name.is_empty()).ok_or_else(|| "Runner file descriptor is missing its name".to_string())?;
      if files.insert(name.clone(), encoded).is_some() {
        return Err(format!("Runner descriptor set contains duplicate file {name}"));
      }
    }
    for file in pool.files() {
      let name = file.name().to_string();
      for service in file.services() {
        let full_name = service.full_name().to_string();
        symbols.insert(full_name.clone(), name.clone());
        services.push(full_name);
      }
    }
    for message in pool.all_messages() {
      symbols.insert(message.full_name().to_string(), message.parent_file().name().to_string());
    }
    for enumeration in pool.all_enums() {
      symbols.insert(enumeration.full_name().to_string(), enumeration.parent_file().name().to_string());
    }
    for extension in pool.all_extensions() {
      symbols.insert(extension.full_name().to_string(), extension.parent_file().name().to_string());
    }
    services.push("grpc.reflection.v1.ServerReflection".to_string());
    services.sort();
    services.dedup();
    Ok(Self {
      files,
      symbols,
      services,
    })
  }
}

const MAX_DESCRIPTOR_SET_BYTES: usize = 4 * 1024 * 1024;
const MAX_DESCRIPTOR_FILES: usize = 128;
const MAX_DESCRIPTOR_FILE_BYTES: usize = 1024 * 1024;

fn extract_file_descriptor_payloads(descriptor_set: &[u8]) -> Result<Vec<Vec<u8>>, String> {
  if descriptor_set.len() > MAX_DESCRIPTOR_SET_BYTES {
    return Err("Runner descriptor set exceeds the 4 MiB limit".to_string());
  }
  let mut offset = 0usize;
  let mut files = Vec::new();
  while offset < descriptor_set.len() {
    let key = decode_varint(descriptor_set, &mut offset)?;
    if key != 0x0a {
      return Err(format!("Runner descriptor set contains unsupported field key {key}"));
    }
    let length = usize::try_from(decode_varint(descriptor_set, &mut offset)?)
      .map_err(|_| "Runner file descriptor length does not fit usize".to_string())?;
    if length > MAX_DESCRIPTOR_FILE_BYTES {
      return Err("Runner file descriptor exceeds the 1 MiB limit".to_string());
    }
    let end = offset.checked_add(length).ok_or_else(|| "Runner file descriptor length overflow".to_string())?;
    let payload = descriptor_set.get(offset..end).ok_or_else(|| "Runner descriptor set has a truncated file payload".to_string())?;
    files.push(payload.to_vec());
    if files.len() > MAX_DESCRIPTOR_FILES {
      return Err("Runner descriptor set exceeds the 128 file limit".to_string());
    }
    offset = end;
  }
  if files.is_empty() {
    return Err("Runner descriptor set is empty".to_string());
  }
  Ok(files)
}

fn decode_varint(bytes: &[u8], offset: &mut usize) -> Result<u64, String> {
  let mut value = 0u64;
  for shift in (0..70).step_by(7) {
    let byte = *bytes.get(*offset).ok_or_else(|| "Runner descriptor set has a truncated varint".to_string())?;
    *offset += 1;
    if shift == 63 && byte > 1 {
      return Err("Runner descriptor set has an overflowing varint".to_string());
    }
    value |= u64::from(byte & 0x7f) << shift;
    if byte & 0x80 == 0 {
      return Ok(value);
    }
  }
  Err("Runner descriptor set has an overflowing varint".to_string())
}

#[derive(Clone, Debug)]
pub struct RawReflection {
  files: BTreeMap<String, Vec<u8>>,
  symbols: BTreeMap<String, String>,
  services: Vec<String>,
}

type ReflectionStream = Pin<Box<dyn Stream<Item = Result<ServerReflectionResponse, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl ServerReflection for RawReflection {
  type ServerReflectionInfoStream = ReflectionStream;

  async fn server_reflection_info(
    &self,
    request: Request<Streaming<ServerReflectionRequest>>,
  ) -> Result<Response<Self::ServerReflectionInfoStream>, Status> {
    let state = self.clone();
    let stream = request.into_inner().map(move |request| {
      let request = request?;
      let message_response = match request.message_request.as_ref() {
        Some(MessageRequest::FileByFilename(name)) => state.file(name),
        Some(MessageRequest::FileContainingSymbol(symbol)) => {
          let name = state.symbols.get(symbol).ok_or_else(|| Status::not_found(format!("symbol not found: {symbol}")))?;
          state.file(name)
        }
        Some(MessageRequest::ListServices(_)) => Ok(MessageResponse::ListServicesResponse(ListServiceResponse {
          service: state.services.iter().cloned().map(|name| ServiceResponse { name }).collect(),
        })),
        Some(MessageRequest::AllExtensionNumbersOfType(_)) => {
          Ok(MessageResponse::AllExtensionNumbersResponse(ExtensionNumberResponse::default()))
        }
        Some(MessageRequest::FileContainingExtension(_)) => Err(Status::not_found("extensions are not indexed as symbols")),
        None => Err(Status::invalid_argument("reflection request is missing its message")),
      }?;
      Ok(ServerReflectionResponse {
        valid_host: request.host.clone(),
        original_request: Some(request),
        message_response: Some(message_response),
      })
    });
    Ok(Response::new(Box::pin(stream)))
  }
}

impl RawReflection {
  fn file(&self, name: &str) -> Result<MessageResponse, Status> {
    let descriptor = self.files.get(name).ok_or_else(|| Status::not_found(format!("descriptor file not found: {name}")))?;
    Ok(MessageResponse::FileDescriptorResponse(FileDescriptorResponse {
      file_descriptor_proto: vec![descriptor.clone()],
    }))
  }
}

#[cfg(unix)]
pub struct InheritedStream {
  inner: tokio::net::UnixStream,
  disconnected: Option<tokio::sync::oneshot::Sender<()>>,
}

#[cfg(unix)]
impl tokio::io::AsyncRead for InheritedStream {
  fn poll_read(mut self: StdPin<&mut Self>, context: &mut Context<'_>, buffer: &mut tokio::io::ReadBuf<'_>) -> Poll<std::io::Result<()>> {
    StdPin::new(&mut self.inner).poll_read(context, buffer)
  }
}

#[cfg(unix)]
impl tokio::io::AsyncWrite for InheritedStream {
  fn poll_write(mut self: StdPin<&mut Self>, context: &mut Context<'_>, buffer: &[u8]) -> Poll<Result<usize, std::io::Error>> {
    StdPin::new(&mut self.inner).poll_write(context, buffer)
  }

  fn poll_flush(mut self: StdPin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
    StdPin::new(&mut self.inner).poll_flush(context)
  }

  fn poll_shutdown(mut self: StdPin<&mut Self>, context: &mut Context<'_>) -> Poll<Result<(), std::io::Error>> {
    StdPin::new(&mut self.inner).poll_shutdown(context)
  }
}

#[cfg(unix)]
impl Drop for InheritedStream {
  fn drop(&mut self) {
    if let Some(disconnected) = self.disconnected.take() {
      let _ = disconnected.send(());
    }
  }
}

#[cfg(unix)]
impl tonic::transport::server::Connected for InheritedStream {
  type ConnectInfo = ();

  fn connect_info(&self) -> Self::ConnectInfo {}
}

/// One adopted daemon connection and a shutdown signal that resolves when the
/// parent side disconnects.
#[cfg(unix)]
pub struct InheritedTransport {
  stream: InheritedStream,
  parent_disconnected: tokio::sync::oneshot::Receiver<()>,
}

#[cfg(unix)]
impl InheritedTransport {
  pub fn into_parts(
    self,
  ) -> (impl tokio_stream::Stream<Item = Result<InheritedStream, std::io::Error>> + Send + 'static, impl Future<Output = ()> + Send + 'static)
  {
    let incoming = tokio_stream::iter([Ok::<_, std::io::Error>(self.stream)]).chain(tokio_stream::pending());
    let shutdown = async move {
      let _ = self.parent_disconnected.await;
    };
    (incoming, shutdown)
  }
}

/// Adopts the exact connected descriptor supplied by the parent daemon.
///
/// The daemon clears the child environment and sets only this fixed descriptor
/// contract. The descriptor is duplicated before constructing the owned stream
/// so this safe API neither assumes ownership of an arbitrary raw descriptor
/// nor becomes unsound if a Runner library calls it more than once.
#[cfg(unix)]
pub fn inherited_transport() -> Result<InheritedTransport, String> {
  let fd = std::env::var(RUNNER_IPC_FD_ENV)
    .map_err(|_| format!("{RUNNER_IPC_FD_ENV} is required"))?
    .parse::<RawFd>()
    .map_err(|error| format!("invalid {RUNNER_IPC_FD_ENV}: {error}"))?;
  if fd != RUNNER_IPC_FD {
    return Err(format!("{RUNNER_IPC_FD_ENV} must name inherited descriptor {RUNNER_IPC_FD}"));
  }
  // SAFETY: dup has no pointer preconditions. On success it returns a new file
  // descriptor owned by this call; from_raw_fd then takes that sole ownership.
  let owned_fd = unsafe { libc::dup(fd) };
  if owned_fd == -1 {
    return Err(format!("failed to duplicate inherited Runner descriptor: {}", std::io::Error::last_os_error()));
  }
  // SAFETY: the successful dup above returned a fresh descriptor that has not
  // been transferred elsewhere. The resulting stream closes it on drop.
  let stream = unsafe { std::os::unix::net::UnixStream::from_raw_fd(owned_fd) };
  stream.set_nonblocking(true).map_err(|error| format!("failed to configure inherited Runner stream: {error}"))?;
  let stream = tokio::net::UnixStream::from_std(stream).map_err(|error| format!("failed to adopt inherited Runner stream: {error}"))?;
  let (disconnected, parent_disconnected) = tokio::sync::oneshot::channel();
  Ok(InheritedTransport {
    stream: InheritedStream {
      inner: stream,
      disconnected: Some(disconnected),
    },
    parent_disconnected,
  })
}

#[cfg(not(unix))]
pub fn inherited_transport() -> Result<(), String> {
  // TODO(runner-named-pipe-v1): add the Windows inherited named-pipe/handle
  // transport when daemon-owned Windows custom Runners are implemented.
  Err("the inherited Runner transport currently requires Unix".to_string())
}

#[cfg(test)]
mod tests {
  use super::*;
  use prost_reflect::Value;

  #[test]
  fn reflection_retains_exact_descriptor_payloads_and_custom_options() {
    let descriptor_set = auv_api_proto::descriptor_set_for_service("auv.api.driver.v1.DisplayService").expect("Display descriptor");
    let payloads = extract_file_descriptor_payloads(&descriptor_set).expect("extract raw descriptor payloads");
    let reflection = RawReflection::new(&descriptor_set).expect("build raw reflection index");
    for payload in &payloads {
      let descriptor = prost_types::FileDescriptorProto::decode(payload.as_slice()).expect("decode file identity");
      let name = descriptor.name.expect("file name");
      assert_eq!(reflection.files.get(&name), Some(payload), "reflection returns the protoc payload byte-for-byte");
    }

    let reflected = encode_descriptor_set(reflection.files.values());
    let pool = DescriptorPool::decode(reflected.as_slice()).expect("reflected descriptor set retains extensions");
    let discoverable = pool.get_extension_by_name("auv.api.annotations.v1.discoverable").expect("discoverable extension");
    let effect = pool.get_extension_by_name("auv.api.annotations.v1.effect").expect("effect extension");
    let method =
      pool.get_service_by_name("auv.api.driver.v1.DisplayService").expect("DisplayService").methods().next().expect("ListDisplays");
    let options = method.options();
    assert_eq!(options.get_extension(&discoverable).as_ref(), &Value::Bool(true));
    assert_eq!(options.get_extension(&effect).as_ref(), &Value::EnumNumber(1));

    assert!(reflection.symbols.contains_key("auv.api.driver.v1.DisplayService"));
    assert!(reflection.symbols.contains_key("auv.api.driver.v1.ListDisplaysRequest"));
    assert!(reflection.symbols.contains_key("auv.api.annotations.v1.MethodEffect"));
    assert!(reflection.symbols.contains_key("auv.api.annotations.v1.discoverable"));
  }

  #[test]
  fn reflection_rejects_non_file_fields_and_truncated_payloads() {
    assert!(extract_file_descriptor_payloads(&[0x10, 0x01]).unwrap_err().contains("unsupported field"));
    assert!(extract_file_descriptor_payloads(&[0x0a, 0x02, 0x01]).unwrap_err().contains("truncated file payload"));
  }

  #[tokio::test]
  async fn runtime_status_watch_and_drain_share_one_typed_state() {
    let runtime = RuntimeControl::ready(RuntimeMetadata {
      runner_class: "auv.test.runtime".to_string(),
      display_name: "Runtime test".to_string(),
      labels: Default::default(),
      operation_capacity: 1,
    })
    .expect("runtime");

    let mut watch = runtime.watch_status(Request::new(runtime_proto::WatchStatusRequest {})).await.expect("watch status").into_inner();
    let initial = watch.next().await.expect("initial snapshot").expect("status event").status.expect("status");
    assert_eq!(initial.phase, runtime_proto::RunnerRuntimePhase::Ready as i32);
    assert_eq!(initial.operations.expect("operation gauges").capacity, 1);

    let operation = runtime.begin_operation().await.expect("operation admission");
    let active =
      runtime.get_status(Request::new(runtime_proto::GetStatusRequest {})).await.expect("get status").into_inner().status.expect("status");
    assert_eq!(active.operations.expect("operation gauges").active, 1);
    let error = runtime
      .drain(Request::new(runtime_proto::DrainRequest { grace_period: None }))
      .await
      .expect_err("zero-grace drain rejects an active operation");
    assert_eq!(error.code(), tonic::Code::DeadlineExceeded);
    drop(operation);
    let drained = runtime
      .drain(Request::new(runtime_proto::DrainRequest { grace_period: None }))
      .await
      .expect("idempotent drain after operation completion")
      .into_inner()
      .status
      .expect("status");
    assert_eq!(drained.phase, runtime_proto::RunnerRuntimePhase::Draining as i32);
    assert_eq!(drained.operations.expect("operation gauges").active, 0);
  }

  fn encode_descriptor_set<'a>(files: impl Iterator<Item = &'a Vec<u8>>) -> Vec<u8> {
    let mut encoded = Vec::new();
    for file in files {
      encoded.push(0x0a);
      encode_varint(file.len() as u64, &mut encoded);
      encoded.extend_from_slice(file);
    }
    encoded
  }

  fn encode_varint(mut value: u64, output: &mut Vec<u8>) {
    while value >= 0x80 {
      output.push((value as u8 & 0x7f) | 0x80);
      value >>= 7;
    }
    output.push(value as u8);
  }
}
