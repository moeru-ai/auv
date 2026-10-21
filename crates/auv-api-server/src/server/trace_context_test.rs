use axum::http::{HeaderMap, HeaderValue};

use super::forward;

const RUN: &str = "0192c3a8-7b1e-7cc0-9b4e-3f2a1d0c9e8b";

fn trace_headers(run_id: &str, span_id: Option<&str>) -> HeaderMap {
  let mut headers = HeaderMap::new();
  headers.insert("auv-trace-version", HeaderValue::from_static("1"));
  headers.insert("auv-trace-run-id", HeaderValue::from_str(run_id).unwrap());
  if let Some(span_id) = span_id {
    headers.insert("auv-trace-span-id", HeaderValue::from_str(span_id).unwrap());
  }
  headers
}

#[test]
fn a_routed_call_without_trace_fields_carries_its_run() {
  let mut headers = HeaderMap::new();
  forward(&mut headers, Some(RUN)).unwrap();
  assert_eq!(headers.get("auv-trace-run-id").unwrap(), RUN);
  assert_eq!(headers.get("auv-trace-version").unwrap(), "1");
  assert!(headers.get("auv-trace-span-id").is_none());
}

#[test]
fn a_routed_call_keeps_the_callers_span_in_the_same_run() {
  let span = "0192c3a8-7b1e-7cc0-9b4e-000000000001";
  let mut headers = trace_headers(RUN, Some(span));
  forward(&mut headers, Some(&format!("run_{RUN}"))).unwrap();
  assert_eq!(headers.get("auv-trace-span-id").unwrap(), span);
}

#[test]
fn a_trace_context_for_another_run_is_rejected() {
  // A caller admitted to one Run must not make the Runner write into another.
  let mut headers = trace_headers("0192c3a8-7b1e-7cc0-9b4e-ffffffffffff", None);
  let status = forward(&mut headers, Some(RUN)).unwrap_err();
  assert_eq!(status.code(), tonic::Code::InvalidArgument);
}

#[test]
fn a_call_without_a_run_forwards_no_trace_fields() {
  let mut headers = trace_headers(RUN, None);
  forward(&mut headers, None).unwrap();
  assert!(auv_tracing::propagation::FIELD_NAMES.iter().all(|name| headers.get(*name).is_none()));
}

#[test]
fn a_malformed_trace_context_is_rejected() {
  let mut headers = trace_headers("not-a-uuid", None);
  assert_eq!(forward(&mut headers, Some(RUN)).unwrap_err().code(), tonic::Code::InvalidArgument);
}
