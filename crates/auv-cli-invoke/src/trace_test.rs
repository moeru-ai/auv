use std::collections::BTreeMap;
use std::sync::Arc;

use auv_tracing::{AttributeValue, Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};

use crate::{InvokeCancellation, InvokeCommandInput};

fn scan_frame_input(inputs: BTreeMap<String, String>) -> InvokeCommandInput {
  InvokeCommandInput {
    command_id: "scan.frame".to_string(),
    target: None,
    inputs,
    typed_args: None,
    dry_run: true,
    cancellation: InvokeCancellation::new(),
  }
}

/// Runs `input` through the registry under a recording Run root and returns
/// the records it produced.
fn record(input: InvokeCommandInput) -> (RunId, Vec<TraceRecord>) {
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("dispatch");
  let run_id = RunId::new();
  let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
  let command = crate::default_registry().resolve(&input.command_id).cloned().expect("registered command");
  let future = root.in_scope(|| command.invoke(input));
  let _ = futures_executor::block_on(root.instrument(future));
  futures_executor::block_on(dispatch.flush()).expect("flush");
  (run_id, store.records())
}

fn event_payload(records: &[TraceRecord], name: &str) -> serde_json::Value {
  records
    .iter()
    .find_map(|record| match record {
      TraceRecord::Event {
        schema, payload, ..
      } if schema.name().as_str() == name => Some(serde_json::from_str(payload.get()).unwrap()),
      _ => None,
    })
    .unwrap_or_else(|| panic!("missing {name} event"))
}

#[test]
fn a_command_records_one_span_with_its_inputs_and_outcome_inside() {
  let inputs = BTreeMap::from([("fixture-dir".to_string(), "fixtures/frame".to_string())]);
  let (run_id, records) = record(scan_frame_input(inputs));

  let [
    TraceRecord::SpanStarted {
      span_id,
      parent_span_id,
      name,
      attributes,
      run_id: span_run,
      ..
    },
    TraceRecord::Event {
      span_id: started_span,
      ..
    },
    TraceRecord::Event {
      span_id: finished_span,
      ..
    },
    TraceRecord::SpanEnded { span_id: ended, .. },
  ] = records.as_slice()
  else {
    panic!("unexpected records {records:#?}");
  };
  assert_eq!(*span_run, run_id);
  assert_eq!(name.as_str(), "auv.invoke.command");
  assert_eq!(*parent_span_id, None, "the command is the top span of its Run");
  assert_eq!(attributes.get("command_id"), Some(&AttributeValue::string("scan.frame")));
  assert_eq!(attributes.get("execution"), Some(&AttributeValue::string("local")));
  assert_eq!(attributes.get("dry_run"), Some(&AttributeValue::boolean(true)));
  assert_eq!([*started_span, *finished_span, Some(*ended)], [Some(*span_id); 3]);

  let started = event_payload(&records, "auv.invoke.command_started");
  assert_eq!(started["inputs"]["fixture-dir"], "fixtures/frame");
  let finished = event_payload(&records, "auv.invoke.command_finished");
  assert_eq!(finished["status"], "succeeded");
}

#[test]
fn a_rejected_command_still_records_why_it_failed() {
  let inputs = BTreeMap::from([
    ("fixture-dir".to_string(), "x".to_string()),
    ("bogus".to_string(), "1".to_string()),
  ]);
  let (_, records) = record(scan_frame_input(inputs));

  let finished = event_payload(&records, "auv.invoke.command_finished");
  assert_eq!(finished["status"], "failed");
  assert_eq!(finished["failure"]["code"], "invalid_input");
  assert!(matches!(records.last(), Some(TraceRecord::SpanEnded { .. })));
}

#[test]
fn a_failed_input_command_records_unverified_submission_progress() {
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let mut input = scan_frame_input(BTreeMap::new());
  input.command_id = "input.focusText".into();
  input.dry_run = false;
  let action = auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::AxFocus);
  let failure = crate::InvokeFailure::from(auv_driver::DriverError::InputAction {
    cause: Box::new(auv_driver::DriverError::PermissionDenied {
      permission: "UI Automation",
      message: Some("readback denied".into()),
      recovery: None,
    }),
    progress: auv_driver::InputActionProgress::Submitted { action },
  });
  let future = root.in_scope(|| super::record_command(input, super::Execution::Local, |_| async { Err(failure) }));
  assert!(futures_executor::block_on(root.instrument(future)).is_err());
  futures_executor::block_on(dispatch.flush()).expect("flush");
  let finished = event_payload(&store.records(), "auv.invoke.command_finished");
  assert_eq!(finished["failure"]["code"], "permission_denied");
  assert_eq!(finished["failure"]["input_progress"]["state"], "submitted");
  assert_eq!(finished["failure"]["input_progress"]["action"]["verified"], false);
  assert_eq!(finished["failure"]["input_progress"]["action"]["attempts"][0]["succeeded"], true);
}

#[test]
fn a_long_input_is_recorded_as_a_bounded_prefix_with_its_length() {
  // ROOT CAUSE:
  //
  // If one input (such as pasted text) pushed the started event past the
  // 64 KiB event limit, `auv-tracing` dropped the whole event silently.
  //
  // The fix records long values as a prefix plus their full byte length.
  let label = "é".repeat(40_000);
  let inputs = BTreeMap::from([
    ("fixture-dir".to_string(), "x".to_string()),
    ("label".to_string(), label.clone()),
  ]);
  let (_, records) = record(scan_frame_input(inputs));

  let started = event_payload(&records, "auv.invoke.command_started");
  let recorded = &started["inputs"]["label"];
  assert_eq!(recorded["byte_length"], label.len());
  let prefix = recorded["prefix"].as_str().unwrap();
  assert!(prefix.len() <= 4096 && label.starts_with(prefix));
}

#[test]
fn an_unrecorded_command_runs_without_a_dispatch() {
  let inputs = BTreeMap::from([("fixture-dir".to_string(), "x".to_string())]);
  let command = crate::default_registry().resolve("scan.frame").cloned().unwrap();
  let result = futures_executor::block_on(command.invoke(scan_frame_input(inputs)));
  assert!(result.is_ok());
}

#[test]
fn a_dropped_command_ends_its_span_without_an_outcome() {
  // MCP request cancellation drops the command future between polls; a reader
  // must be able to tell that apart from a command that finished.
  use futures_util::FutureExt as _;
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let inputs = BTreeMap::from([("fixture-dir".to_string(), "x".to_string())]);
  let future = root.in_scope(|| {
    super::record_command(scan_frame_input(inputs), super::Execution::Local, |_| std::future::pending::<crate::InvokeExecutionResult>())
  });
  assert!(root.instrument(future).now_or_never().is_none(), "the command is still pending when dropped");
  futures_executor::block_on(dispatch.flush()).expect("flush");

  let records = store.records();
  let names = records
    .iter()
    .map(|record| match record {
      TraceRecord::SpanStarted { .. } => "span_started".to_string(),
      TraceRecord::Event { schema, .. } => schema.name().as_str().to_string(),
      TraceRecord::SpanEnded { .. } => "span_ended".to_string(),
      TraceRecord::Artifact { .. } => "artifact".to_string(),
    })
    .collect::<Vec<_>>();
  assert_eq!(names, ["span_started", "auv.invoke.command_started", "span_ended"]);
}
