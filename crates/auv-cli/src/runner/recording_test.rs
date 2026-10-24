use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use auv_api_proto::auv::api::annotations::v1::GetMethodDocsRequest;
use auv_api_proto::auv::api::annotations::v1::method_docs_service_client::MethodDocsServiceClient;
use auv_tracing::{RunId, TraceRecord};

use super::*;

/// The process-wide Runner recording, backed by one leaked temporary store
/// shared by every test in this binary; tests tell their records apart by Run.
fn test_store() -> &'static PathBuf {
  static ROOT: OnceLock<PathBuf> = OnceLock::new();
  ROOT.get_or_init(|| {
    let root = tempfile::tempdir().expect("temporary store").keep();
    let store = auv_tracing::FileTracingStore::open(&root).expect("store");
    let dispatch = auv_tracing::configure().tracing_store(Arc::new(store)).build().expect("dispatch");
    let _ = RECORDING.set(Recording {
      dispatch,
      encodes: tokio_util::task::TaskTracker::new(),
    });
    root
  })
}

/// Serves the method-docs service (it needs no driver) behind `RpcLayer` and
/// returns a client for it.
async fn serve() -> MethodDocsServiceClient<tonic::transport::Channel> {
  test_store();
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
  let address = listener.local_addr().expect("address");
  let services = ["auv.api.annotations.v1.MethodDocsService"];
  let docs = auv_api_server::method_docs::service(auv_api_proto::FILE_DESCRIPTOR_SET, &services, &[]).expect("method docs");
  tokio::spawn(
    tonic::transport::Server::builder()
      .layer(RpcLayer::new().expect("layer"))
      .add_service(docs)
      .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener)),
  );
  MethodDocsServiceClient::connect(format!("http://{address}")).await.expect("connect")
}

fn traced(run_id: RunId, span_id: Option<auv_tracing::SpanId>) -> tonic::Request<GetMethodDocsRequest> {
  let mut request = tonic::Request::new(GetMethodDocsRequest {
    method: "/auv.test.Missing/Method".to_string(),
  });
  let metadata = request.metadata_mut();
  metadata.insert("auv-trace-version", "1".parse().unwrap());
  metadata.insert("auv-trace-run-id", run_id.to_string().parse().unwrap());
  if let Some(span_id) = span_id {
    metadata.insert("auv-trace-span-id", span_id.to_string().parse().unwrap());
  }
  request
}

async fn records_for(run_id: RunId) -> Vec<TraceRecord> {
  // `finish` flushes from a spawned task; a second flush orders after it.
  tokio::task::yield_now().await;
  RECORDING.get().expect("recording").dispatch.flush().await.expect("flush");
  auv_tracing::FileTracingStore::read_records(test_store())
    .expect("records")
    .map(|record| record.expect("record").record)
    .filter(|record| record.run_id() == run_id)
    .collect()
}

fn payload(record: &TraceRecord) -> (String, serde_json::Value) {
  match record {
    TraceRecord::Event {
      schema, payload, ..
    } => (schema.name().as_str().to_string(), serde_json::from_str(payload.get()).unwrap()),
    other => panic!("expected an event, got {other:?}"),
  }
}

#[tokio::test]
async fn a_traced_call_records_its_request_and_status_under_the_callers_span() {
  let mut client = serve().await;
  let run_id = RunId::new();
  let caller_span = auv_tracing::SpanId::new();
  let status = client.get_method_docs(traced(run_id, Some(caller_span))).await.unwrap_err();
  assert_eq!(status.code(), tonic::Code::NotFound);

  let records = records_for(run_id).await;
  let [
    TraceRecord::SpanStarted {
      span_id,
      remote_span_id,
      name,
      attributes,
      ..
    },
    request,
    finished,
    TraceRecord::SpanEnded { span_id: ended, .. },
  ] = records.as_slice()
  else {
    panic!("unexpected records {records:#?}");
  };
  assert_eq!(name.as_str(), "auv.runner.rpc");
  assert_eq!(*remote_span_id, Some(caller_span), "the Runner span nests under the caller's span");
  assert_eq!(attributes.get("rpc.method"), Some(&AttributeValue::string("GetMethodDocs")));
  assert_eq!(ended, span_id);

  let (name, request) = payload(request);
  assert_eq!(name, "auv.runner.rpc_request");
  assert_eq!(request["message"]["method"], "/auv.test.Missing/Method");
  let (name, finished) = payload(finished);
  assert_eq!(name, "auv.runner.rpc_finished");
  assert_eq!(finished["grpc_status"], tonic::Code::NotFound as i32);
  assert_eq!(finished["grpc_message"], "no docs for /auv.test.Missing/Method");
  assert_eq!(finished["completed"], true);
}

#[tokio::test]
async fn an_untraced_call_is_served_and_records_nothing() {
  let mut client = serve().await;
  let run_id = RunId::new();
  let mut request = traced(run_id, None);
  for name in auv_tracing::propagation::FIELD_NAMES {
    request.metadata_mut().remove(name);
  }
  let status = client.get_method_docs(request).await.unwrap_err();
  assert_eq!(status.code(), tonic::Code::NotFound, "the layer must not change the call");
  assert!(records_for(run_id).await.is_empty());
}

#[test]
fn a_large_message_is_counted_without_being_buffered() {
  // ROOT CAUSE:
  //
  // If a response carried full capture pixels (tens of MB), a tap that kept
  // every body byte would double the Runner's memory for that call.
  //
  // The fix decodes only messages within the bound and counts larger ones.
  let call = Arc::new(Call {
    service: "auv.test.Service".to_string(),
    method: "Large".to_string(),
    span: None,
    input: None,
    output: None,
    outcome: Mutex::default(),
  });
  let mut tap = MessageTap::new(tonic::body::Body::empty(), call.clone(), Direction::Response);
  let length = 10 * 1024 * 1024u32;
  let mut prefix = vec![0];
  prefix.extend_from_slice(&length.to_be_bytes());
  tap.observe(&prefix[..3]);
  tap.observe(&prefix[3..]);
  let chunk = vec![7; 1024 * 1024];
  for _ in 0..10 {
    tap.observe(&chunk);
    assert!(tap.framing.body.capacity() == 0, "an oversized body is never buffered");
  }
  // A second, small message in the same body still frames correctly.
  tap.observe(&[0, 0, 0, 0, 2, 1, 2]);
  assert_eq!(call.outcome.lock().unwrap().response_messages, 2);
}

#[test]
fn long_strings_such_as_base64_bytes_are_replaced_by_their_length() {
  let mut value = serde_json::json!({ "image": "A".repeat(5000), "keys": ["cmd", "c"] });
  bound_text(&mut value);
  assert_eq!(value, serde_json::json!({ "image": { "omitted_chars": 5000 }, "keys": ["cmd", "c"] }));
}
