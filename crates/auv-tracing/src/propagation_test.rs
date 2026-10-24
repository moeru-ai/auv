use std::collections::HashMap;

use super::*;

#[derive(Default)]
struct Carrier(HashMap<&'static str, Vec<String>>);

impl TextMapWriter for Carrier {
  fn set(&mut self, name: &'static str, value: &str) {
    self.0.insert(name, vec![value.to_string()]);
  }
  fn remove(&mut self, name: &'static str) {
    self.0.remove(name);
  }
}

impl TextMapReader for Carrier {
  fn values<'a>(&'a self, name: &str) -> Box<dyn Iterator<Item = &'a str> + 'a> {
    Box::new(self.0.get(name).into_iter().flatten().map(String::as_str))
  }
}

#[test]
fn trace_fields_never_reuse_the_daemon_routing_header() {
  // ROOT CAUSE:
  //
  // If a client sent both a routed Run and a trace context, the trace run id
  // used the same `auv-run-id` key as the daemon's routing header, which the
  // daemon strips before forwarding to a Runner.
  //
  // Before the fix, trace propagation could never reach a Runner.
  // The fix keeps trace fields under their own `auv-trace-*` names.
  assert!(!FIELD_NAMES.contains(&"auv-run-id"));
  assert!(FIELD_NAMES.iter().all(|name| name.starts_with("auv-trace-")));
}

#[test]
fn remote_context_round_trips_through_a_carrier() {
  let run_id = RunId::new();
  let span_id = SpanId::new();
  let mut carrier = Carrier::default();
  carrier.set("auv-trace-span-id", "stale");
  RemoteContext::new(run_id, Some(span_id)).inject(&mut carrier);
  assert_eq!(extract(&carrier).unwrap(), Some(RemoteContext::new(run_id, Some(span_id))));

  RemoteContext::new(run_id, None).inject(&mut carrier);
  let extracted = extract(&carrier).unwrap().unwrap();
  assert_eq!(extracted.remote_span_id(), None, "inject replaces a stale span field");
}

#[test]
fn extract_rejects_a_span_without_a_run() {
  let mut carrier = Carrier::default();
  carrier.set("auv-trace-span-id", &SpanId::new().to_string());
  assert_eq!(extract(&carrier).unwrap_err().code().as_str(), "auv.propagation.partial_context");
}

#[test]
fn a_daemon_run_id_and_its_trace_id_name_one_run() {
  let daemon = "56490ab8191b99cef8e18e293fd0ae51";
  let trace = RunId::from_daemon_run_id(daemon).unwrap();
  assert_eq!(trace.to_string(), "56490ab8-191b-99ce-f8e1-8e293fd0ae51");
  assert_eq!(RunId::from_daemon_run_id(&format!("run_{daemon}")).unwrap(), trace);
  assert_eq!(RunId::from_daemon_run_id(&trace.to_string()).unwrap(), trace);
  assert!(RunId::from_daemon_run_id("00000000000000000000000000000000").is_err(), "nil is not a Run");
  assert!(RunId::from_daemon_run_id("not-a-run").is_err());
}
