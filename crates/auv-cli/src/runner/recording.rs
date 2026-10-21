//! Runner-side run recording and RPC diagnostics.
//!
//! The Runner records the calls that reach it, so callers without their own
//! command layer (`@auv-js/sdk`, the playground) are traced too. It records
//! only calls that carry `auv-trace-*` fields; the daemon adds them for calls
//! routed to a Run (`auv-api-server`'s `trace_context`).
//!
//! ```text
//! span   auv.runner.rpc            service, method; parent = caller's span
//! event  auv.runner.rpc_request    first request message as JSON
//! ...    capture evidence artifacts (capture.ref_id joins the response)
//! event  auv.runner.rpc_finished   gRPC status, first response message
//! ```
//!
//! Messages are decoded generically from the Runner's own descriptor set, so
//! a new RPC is recorded without per-handler code. Every call, recorded or
//! not, also logs its failure status as an operational log.

use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context as TaskContext, Poll};

use auv_tracing::{AttributeValue, Attributes, EventPayload, SpanSpec};
use bytes::Bytes;
use http_body::Frame;
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};
use serde::Serialize;
use tonic::codegen::http;
use tonic::{Code, Status};

pub(super) const RPC_SPAN: &str = "auv.runner.rpc";
pub(super) const RPC_REQUEST: &str = "auv.runner.rpc_request";
pub(super) const RPC_FINISHED: &str = "auv.runner.rpc_finished";

/// NOTICE(runner-trace-message-bounds): one message is buffered for decoding
/// only up to this size; larger messages (pixels for `GetCaptureImage`,
/// uploaded frames for `RecognizeText`) are recorded by byte length. Decoded
/// strings longer than `MAX_TEXT_CHARS` (base64 bytes fields) are replaced by
/// their length. The decoded JSON must also fit `auv-tracing`'s 64 KiB event.
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_TEXT_CHARS: usize = 2048;
const MAX_EVENT_MESSAGE_BYTES: usize = 48 * 1024;

static RECORDING: OnceLock<Recording> = OnceLock::new();

/// The Runner process's tracing pipeline; one per Runner process.
pub(super) struct Recording {
  dispatch: auv_tracing::Dispatch,
  /// Capture encodes run on the blocking pool after the RPC answers; shutdown
  /// waits for them before the final flush.
  encodes: tokio_util::task::TaskTracker,
}

/// Opens the Runner's trace store when the daemon configured one.
///
/// Without `AUV_RUNNER_TRACE_ROOT` and `AUV_RUNNER_ID` (a Runner started by a
/// test or by hand), nothing is recorded and calls are only logged.
pub(super) fn install() -> Result<(), String> {
  let (Some(root), Some(runner_id)) =
    (std::env::var_os(super::TRACE_ROOT_ENV), std::env::var(auv_daemon::runner_provider::RUNNER_ID_ENV).ok())
  else {
    tracing::info!("Runner trace store not configured; RPCs are logged but not recorded");
    return Ok(());
  };
  if runner_id.is_empty() || runner_id.contains(['/', '\\']) || runner_id.starts_with('.') {
    return Err(format!("invalid {} for a trace store directory: {runner_id:?}", auv_daemon::runner_provider::RUNNER_ID_ENV));
  }
  let root = std::path::PathBuf::from(root).join(&runner_id);
  let store =
    auv_tracing::FileTracingStore::open(&root).map_err(|error| format!("failed to open Runner trace store {}: {error}", root.display()))?;
  let dispatch = auv_tracing::configure()
    .tracing_store(Arc::new(store))
    .build()
    .map_err(|error| format!("failed to configure Runner tracing: {error}"))?;
  tracing::info!(store = %root.display(), "Runner records traced calls");
  let _ = RECORDING.set(Recording {
    dispatch,
    encodes: tokio_util::task::TaskTracker::new(),
  });
  Ok(())
}

/// Waits for in-flight capture encodes and flushes the store before exit.
pub(super) async fn finish() {
  let Some(recording) = RECORDING.get() else {
    return;
  };
  recording.encodes.close();
  recording.encodes.wait().await;
  if let Err(error) = recording.dispatch.flush().await {
    tracing::warn!(%error, "Runner trace store reported failures at shutdown");
  }
}

/// Records `capture_id` from `captures` as evidence under the current RPC span.
///
/// Call it in a handler right after the capture: the artifact keeps this time
/// even though encoding finishes after the RPC has answered.
pub(super) fn record_capture(captures: &super::capture_store::CaptureStore, capture_id: &str, purpose: &'static str) {
  let context = auv_tracing::Context::current();
  let Some(recording) = RECORDING.get() else {
    return;
  };
  if !context.can_publish_artifacts() {
    return;
  }
  let (Ok(occurred_at), Some(capture)) = (auv_tracing::Timestamp::now(), captures.get(capture_id)) else {
    return;
  };
  let capture_id = capture_id.to_string();
  recording.encodes.spawn_blocking(move || {
    context.in_scope(|| match auv_cli_invoke::artifact::capture_evidence(purpose, &capture, Some(&capture_id)) {
      Ok(artifact) => drop(auv_tracing::emit_artifact(artifact.with_occurred_at(occurred_at))),
      Err(error) => tracing::warn!(%error, purpose, "Runner capture evidence was not recorded"),
    });
  });
}

/// Wraps the Runner's routes: every call is logged, traced calls are recorded.
#[derive(Clone)]
pub(super) struct RpcLayer {
  pool: Arc<DescriptorPool>,
}

impl RpcLayer {
  pub(super) fn new() -> Result<Self, String> {
    let pool = DescriptorPool::decode(auv_api_proto::FILE_DESCRIPTOR_SET)
      .map_err(|error| format!("invalid embedded descriptor set for Runner recording: {error}"))?;
    Ok(Self {
      pool: Arc::new(pool),
    })
  }
}

impl<S> tower::Layer<S> for RpcLayer {
  type Service = RpcService<S>;

  fn layer(&self, inner: S) -> Self::Service {
    RpcService {
      inner,
      pool: self.pool.clone(),
    }
  }
}

#[derive(Clone)]
pub(super) struct RpcService<S> {
  inner: S,
  pool: Arc<DescriptorPool>,
}

type BoxFuture<T> = Pin<Box<dyn std::future::Future<Output = T> + Send>>;

impl<S> tower::Service<http::Request<tonic::body::Body>> for RpcService<S>
where
  S: tower::Service<http::Request<tonic::body::Body>, Response = http::Response<tonic::body::Body>> + Send + 'static,
  S::Future: Send + 'static,
{
  type Response = http::Response<tonic::body::Body>;
  type Error = S::Error;
  type Future = BoxFuture<Result<Self::Response, Self::Error>>;

  fn poll_ready(&mut self, cx: &mut TaskContext<'_>) -> Poll<Result<(), Self::Error>> {
    self.inner.poll_ready(cx)
  }

  fn call(&mut self, request: http::Request<tonic::body::Body>) -> Self::Future {
    let Some(call) = Call::begin(&self.pool, &request) else {
      return Box::pin(self.inner.call(request));
    };
    let call = Arc::new(call);
    let request = request.map(|body| tonic::body::Body::new(MessageTap::new(body, call.clone(), Direction::Request)));
    let context = call.span.as_ref().map(auv_tracing::Span::context).unwrap_or_else(auv_tracing::Context::current);
    let future = context.instrument(self.inner.call(request));
    Box::pin(async move {
      let response = future.await?;
      call.observe_headers(response.headers());
      Ok(response.map(|body| tonic::body::Body::new(MessageTap::new(body, call, Direction::Response))))
    })
  }
}

/// One RPC: its identity, its span when recorded, and its outcome so far.
struct Call {
  service: String,
  method: String,
  span: Option<auv_tracing::Span>,
  input: Option<MessageDescriptor>,
  output: Option<MessageDescriptor>,
  outcome: Mutex<Outcome>,
}

#[derive(Default)]
struct Outcome {
  status: Option<(i32, String)>,
  response: Option<RecordedMessage>,
  request_messages: u64,
  response_messages: u64,
  finished: bool,
}

impl Call {
  fn begin(pool: &DescriptorPool, request: &http::Request<tonic::body::Body>) -> Option<Self> {
    let path = request.uri().path().strip_prefix('/')?;
    let (service, method) = path.split_once('/')?;
    // Health and reflection probes are the daemon's, not a caller's.
    if service.starts_with("grpc.") {
      return None;
    }
    let descriptor = pool.get_service_by_name(service).and_then(|service| service.methods().find(|candidate| candidate.name() == method));
    let span = RECORDING.get().and_then(|recording| traced_span(recording, request.headers(), service, method));
    Some(Self {
      service: service.to_string(),
      method: method.to_string(),
      input: descriptor.as_ref().map(prost_reflect::MethodDescriptor::input),
      output: descriptor.as_ref().map(prost_reflect::MethodDescriptor::output),
      span,
      outcome: Mutex::default(),
    })
  }

  fn observe_headers(&self, headers: &http::HeaderMap) {
    // A trailers-only response (an error before any message) puts the status
    // in the headers.
    if let Some(status) = grpc_status(headers) {
      self.outcome.lock().expect("RPC outcome lock poisoned").status = Some(status);
    }
  }

  fn observe_message(&self, direction: Direction, message: Option<&[u8]>, byte_length: usize) {
    let mut outcome = self.outcome.lock().expect("RPC outcome lock poisoned");
    let first = match direction {
      Direction::Request => {
        outcome.request_messages += 1;
        outcome.request_messages == 1
      }
      Direction::Response => {
        outcome.response_messages += 1;
        outcome.response_messages == 1
      }
    };
    if !first || self.span.is_none() {
      return;
    }
    let descriptor = match direction {
      Direction::Request => self.input.as_ref(),
      Direction::Response => self.output.as_ref(),
    };
    let recorded = RecordedMessage::decode(descriptor, message, byte_length);
    match direction {
      Direction::Request => {
        drop(outcome);
        self.in_span(|| {
          auv_tracing::emit_event!(RpcRequest {
            service: &self.service,
            method: &self.method,
            message: &recorded,
          })
        });
      }
      Direction::Response => outcome.response = Some(recorded),
    }
  }

  fn observe_trailers(&self, trailers: &http::HeaderMap) {
    if let Some(status) = grpc_status(trailers) {
      self.outcome.lock().expect("RPC outcome lock poisoned").status = Some(status);
    }
  }

  /// Records and logs the outcome once, when the response ends or is dropped.
  fn finish(&self, completed: bool) {
    let mut outcome = self.outcome.lock().expect("RPC outcome lock poisoned");
    if outcome.finished {
      return;
    }
    outcome.finished = true;
    let status = outcome.status.clone();
    // A trailers-only response (an early error) carries its status in the
    // headers and has an empty body that may be dropped without a final poll.
    let completed = completed || status.is_some();
    match &status {
      Some((code, message)) if *code != Code::Ok as i32 => {
        tracing::warn!(service = %self.service, method = %self.method, code = ?Code::from_i32(*code), message = %message, "Runner RPC failed")
      }
      Some(_) => tracing::debug!(service = %self.service, method = %self.method, "Runner RPC succeeded"),
      None if completed => tracing::warn!(service = %self.service, method = %self.method, "Runner RPC ended without a gRPC status"),
      None => tracing::debug!(service = %self.service, method = %self.method, "Runner RPC response dropped before it ended"),
    }
    if self.span.is_none() {
      return;
    }
    let finished = RpcFinished {
      service: &self.service,
      method: &self.method,
      grpc_status: status.as_ref().map(|(code, _)| *code),
      grpc_message: status.as_ref().map(|(_, message)| message.as_str()).filter(|message| !message.is_empty()),
      completed,
      response: outcome.response.as_ref(),
      request_messages: outcome.request_messages,
      response_messages: outcome.response_messages,
    };
    self.in_span(|| auv_tracing::emit_event!(finished));
    drop(outcome);
    if let Some(recording) = RECORDING.get() {
      // NOTICE(runner-trace-flush): the store buffers writes, and a Runner can
      // live for hours or be killed; flushing per call keeps a reader of a live
      // Runner, and a crash, at most one call behind.
      let dispatch = recording.dispatch.clone();
      tokio::spawn(async move {
        if let Err(error) = dispatch.flush().await {
          tracing::warn!(%error, "Runner trace store reported failures");
        }
      });
    }
  }

  fn in_span(&self, operation: impl FnOnce()) {
    if let Some(span) = &self.span {
      span.in_scope(operation);
    }
  }
}

/// Opens the call's span under the caller's propagated context, if any.
fn traced_span(recording: &Recording, headers: &http::HeaderMap, service: &str, method: &str) -> Option<auv_tracing::Span> {
  let remote = match auv_tracing::extract(&HeaderCarrier(headers)) {
    Ok(Some(remote)) => remote,
    Ok(None) => return None,
    Err(error) => {
      tracing::warn!(code = %error.code(), service, method, "ignoring invalid trace context on a Runner call");
      return None;
    }
  };
  let context = auv_tracing::dispatcher::with_default(&recording.dispatch, || auv_tracing::Context::from_remote(remote)).ok()?;
  Some(context.in_scope(|| auv_tracing::start_span(RpcSpan { service, method })))
}

struct RpcSpan<'a> {
  service: &'a str,
  method: &'a str,
}

impl SpanSpec for RpcSpan<'_> {
  const NAME: &'static str = RPC_SPAN;

  fn attributes(&self) -> Attributes {
    Attributes::from_iter([
      ("rpc.service".to_string(), AttributeValue::string(self.service)),
      ("rpc.method".to_string(), AttributeValue::string(self.method)),
    ])
  }
}

#[derive(Serialize)]
struct RpcRequest<'a> {
  service: &'a str,
  method: &'a str,
  #[serde(flatten)]
  message: &'a RecordedMessage,
}

impl EventPayload for RpcRequest<'_> {
  const NAME: &'static str = RPC_REQUEST;
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
struct RpcFinished<'a> {
  service: &'a str,
  method: &'a str,
  /// Absent when the response was dropped (caller cancelled) or ended
  /// without trailers.
  #[serde(skip_serializing_if = "Option::is_none")]
  grpc_status: Option<i32>,
  #[serde(skip_serializing_if = "Option::is_none")]
  grpc_message: Option<&'a str>,
  /// Whether the response body reached its end.
  completed: bool,
  #[serde(skip_serializing_if = "Option::is_none")]
  response: Option<&'a RecordedMessage>,
  request_messages: u64,
  response_messages: u64,
}

impl EventPayload for RpcFinished<'_> {
  const NAME: &'static str = RPC_FINISHED;
  const VERSION: u32 = 1;
}

/// One decoded message, or the reason it is not included.
#[derive(Clone, Serialize)]
struct RecordedMessage {
  #[serde(skip_serializing_if = "Option::is_none")]
  message: Option<serde_json::Value>,
  /// Set when `message` is omitted: the encoded protobuf size.
  #[serde(skip_serializing_if = "Option::is_none")]
  message_byte_length: Option<usize>,
}

impl RecordedMessage {
  fn decode(descriptor: Option<&MessageDescriptor>, message: Option<&[u8]>, byte_length: usize) -> Self {
    let omitted = Self {
      message: None,
      message_byte_length: Some(byte_length),
    };
    let (Some(descriptor), Some(message)) = (descriptor, message) else {
      return omitted;
    };
    let Ok(decoded) = DynamicMessage::decode(descriptor.clone(), message) else {
      return omitted;
    };
    let Ok(mut value) = serde_json::to_value(&decoded) else {
      return omitted;
    };
    bound_text(&mut value);
    if serde_json::to_vec(&value).map_or(true, |encoded| encoded.len() > MAX_EVENT_MESSAGE_BYTES) {
      return omitted;
    }
    Self {
      message: Some(value),
      message_byte_length: None,
    }
  }
}

/// Replaces long strings, such as base64 image bytes, with their length.
fn bound_text(value: &mut serde_json::Value) {
  match value {
    serde_json::Value::String(text) if text.chars().count() > MAX_TEXT_CHARS => {
      *value = serde_json::json!({ "omitted_chars": text.chars().count() });
    }
    serde_json::Value::Array(items) => items.iter_mut().for_each(bound_text),
    serde_json::Value::Object(fields) => fields.values_mut().for_each(bound_text),
    _ => {}
  }
}

#[derive(Clone, Copy)]
enum Direction {
  Request,
  Response,
}

/// Passes a gRPC body through unchanged while reporting its messages, and,
/// for a response, its end.
struct MessageTap {
  inner: tonic::body::Body,
  call: Arc<Call>,
  direction: Direction,
  framing: Framing,
}

/// Progress through the current gRPC length-prefixed message (1-byte
/// compression flag, 4-byte big-endian length, body).
#[derive(Default)]
struct Framing {
  prefix: [u8; 5],
  prefix_filled: usize,
  /// The body so far, kept only for messages within the decode bound.
  body: Vec<u8>,
  body_seen: usize,
}

impl MessageTap {
  fn new(inner: tonic::body::Body, call: Arc<Call>, direction: Direction) -> Self {
    Self {
      inner,
      call,
      direction,
      framing: Framing::default(),
    }
  }

  /// Reports each complete message. Longer messages than the decode bound are
  /// counted with their size and never buffered.
  fn observe(&mut self, mut data: &[u8]) {
    while !data.is_empty() {
      let framing = &mut self.framing;
      if framing.prefix_filled < 5 {
        let take = (5 - framing.prefix_filled).min(data.len());
        framing.prefix[framing.prefix_filled..framing.prefix_filled + take].copy_from_slice(&data[..take]);
        framing.prefix_filled += take;
        data = &data[take..];
        if framing.prefix_filled < 5 {
          return;
        }
      }
      let length = u32::from_be_bytes(framing.prefix[1..5].try_into().expect("five-byte prefix")) as usize;
      let decodable = length <= MAX_MESSAGE_BYTES && framing.prefix[0] == 0;
      let take = (length - framing.body_seen).min(data.len());
      if decodable {
        framing.body.extend_from_slice(&data[..take]);
      }
      framing.body_seen += take;
      data = &data[take..];
      if framing.body_seen == length {
        let body = std::mem::take(&mut framing.body);
        *framing = Framing::default();
        self.call.observe_message(self.direction, decodable.then_some(body.as_slice()), length);
      }
    }
  }
}

impl http_body::Body for MessageTap {
  type Data = Bytes;
  type Error = Status;

  fn poll_frame(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
    let poll = Pin::new(&mut self.inner).poll_frame(cx);
    match &poll {
      Poll::Ready(Some(Ok(frame))) => {
        if let Some(data) = frame.data_ref() {
          let data = data.clone();
          self.observe(&data);
        }
        if let Some(trailers) = frame.trailers_ref() {
          self.call.observe_trailers(trailers);
        }
      }
      Poll::Ready(None) | Poll::Ready(Some(Err(_))) => {
        if matches!(self.direction, Direction::Response) {
          self.call.finish(matches!(poll, Poll::Ready(None)));
        }
      }
      Poll::Pending => {}
    }
    poll
  }

  fn is_end_stream(&self) -> bool {
    self.inner.is_end_stream()
  }

  fn size_hint(&self) -> http_body::SizeHint {
    self.inner.size_hint()
  }
}

impl Drop for MessageTap {
  fn drop(&mut self) {
    if matches!(self.direction, Direction::Response) {
      self.call.finish(false);
    }
  }
}

fn grpc_status(headers: &http::HeaderMap) -> Option<(i32, String)> {
  let code = headers.get("grpc-status")?.to_str().ok()?.parse().ok()?;
  let message = headers
    .get("grpc-message")
    .map(|value| percent_encoding::percent_decode(value.as_bytes()).decode_utf8_lossy().into_owned())
    .unwrap_or_default();
  Some((code, message))
}

struct HeaderCarrier<'a>(&'a http::HeaderMap);

impl auv_tracing::TextMapReader for HeaderCarrier<'_> {
  fn values<'a>(&'a self, name: &str) -> Box<dyn Iterator<Item = &'a str> + 'a> {
    Box::new(self.0.get_all(name).iter().map(|value| value.to_str().unwrap_or_default()))
  }
}

#[cfg(test)]
#[path = "recording_test.rs"]
mod tests;
