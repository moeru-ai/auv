#![cfg(feature = "memory-store")]

mod support;

use std::collections::BTreeMap;

use auv_tracing::{Attributes, Context, RunId, RunStore, SpanId, SpanSpec, TextMapReader, TextMapWriter, dispatcher, extract};
use support::{TestDispatch, block_on_timeout};

const VERSION: &str = "auv-context-version";
const RUN_ID: &str = "auv-run-id";
const AUTHORITY_ID: &str = "auv-authority-id";
const SPAN_ID: &str = "auv-span-id";
const VALID_ID: &str = "018f47a0-4b5c-7d6e-8f90-123456789abc";

#[derive(Default)]
struct MapCarrier {
  fields: BTreeMap<String, Vec<String>>,
}

impl MapCarrier {
  fn insert(&mut self, name: &str, value: impl Into<String>) {
    self.fields.entry(name.to_owned()).or_default().push(value.into());
  }

  fn replace(&mut self, name: &str, value: impl Into<String>) {
    self.fields.insert(name.to_owned(), vec![value.into()]);
  }

  fn value(&self, name: &str) -> Option<&str> {
    self.fields.get(name).and_then(|values| values.first()).map(String::as_str)
  }
}

impl TextMapWriter for MapCarrier {
  fn set(&mut self, name: &'static str, value: &str) {
    self.replace(name, value);
  }

  fn remove(&mut self, name: &'static str) {
    self.fields.remove(name);
  }
}

impl TextMapReader for MapCarrier {
  fn values<'a>(&'a self, name: &str) -> Box<dyn Iterator<Item = &'a str> + 'a> {
    Box::new(self.fields.get(name).into_iter().flat_map(|values| values.iter().map(String::as_str)))
  }
}

struct TestSpan;

impl SpanSpec for TestSpan {
  const NAME: &'static str = "auv.test.propagation";

  fn attributes(&self) -> Attributes {
    Attributes::empty()
  }
}

fn minimal_carrier(run_id: impl ToString) -> MapCarrier {
  let mut carrier = MapCarrier::default();
  carrier.insert(VERSION, "1");
  carrier.insert(RUN_ID, run_id.to_string());
  carrier
}

fn complete_carrier(run_id: RunId, authority_id: impl ToString, span_id: SpanId) -> MapCarrier {
  let mut carrier = minimal_carrier(run_id);
  carrier.insert(AUTHORITY_ID, authority_id.to_string());
  carrier.insert(SPAN_ID, span_id.to_string());
  carrier
}

#[test]
fn inject_removes_all_auv_fields_when_context_has_no_run() {
  let mut carrier = MapCarrier::default();
  for name in [VERSION, RUN_ID, AUTHORITY_ID, SPAN_ID] {
    carrier.insert(name, "stale");
    carrier.insert(name, "duplicate");
  }
  carrier.insert("unrelated", "preserved");

  Context::current().inject(&mut carrier);

  assert_eq!(carrier.fields.len(), 1);
  assert_eq!(carrier.value("unrelated"), Some("preserved"));
}

#[test]
fn inject_writes_exactly_the_four_contract_fields_for_an_active_span() {
  let fixture = TestDispatch::memory();
  let root = fixture.root();
  let run_id = *root.run_id().unwrap();
  let authority_id = fixture.store.authority_id();
  let span = root.in_scope(|| auv_tracing::start_span!(TestSpan));
  let span_id = *span.id().unwrap();
  let context = span.context();
  let mut carrier = MapCarrier::default();
  carrier.insert("unrelated", "preserved");

  context.inject(&mut carrier);

  assert_eq!(carrier.fields.len(), 5);
  assert_eq!(carrier.value(VERSION), Some("1"));
  assert_eq!(carrier.value(RUN_ID), Some(run_id.to_string().as_str()));
  assert_eq!(carrier.value(AUTHORITY_ID), Some(authority_id.to_string().as_str()));
  assert_eq!(carrier.value(SPAN_ID), Some(span_id.to_string().as_str()));
  assert_eq!(carrier.value("unrelated"), Some("preserved"));

  drop(context);
  drop(span);
  block_on_timeout(fixture.dispatch.flush()).unwrap();
}

#[test]
fn extract_all_absent_returns_none() {
  let mut carrier = MapCarrier::default();
  carrier.insert("unrelated", "value");

  assert!(extract(&carrier).unwrap().is_none());
}

#[test]
fn extract_accepts_required_fields_without_optional_ids() {
  let run_id = RunId::new();
  let remote = extract(&minimal_carrier(run_id)).unwrap().unwrap();
  let context = Context::from_remote(remote).unwrap();
  let mut reinjected = MapCarrier::default();

  context.inject(&mut reinjected);

  assert_eq!(reinjected.fields.len(), 2);
  assert_eq!(reinjected.value(VERSION), Some("1"));
  assert_eq!(reinjected.value(RUN_ID), Some(run_id.to_string().as_str()));
}

#[test]
fn extract_rejects_a_duplicate_of_any_contract_field() {
  for name in [VERSION, RUN_ID, AUTHORITY_ID, SPAN_ID] {
    let mut carrier = minimal_carrier(VALID_ID);
    let value = match name {
      VERSION => "1",
      RUN_ID | AUTHORITY_ID | SPAN_ID => VALID_ID,
      _ => unreachable!(),
    };
    carrier.insert(name, value);
    if matches!(name, AUTHORITY_ID | SPAN_ID) {
      carrier.insert(name, value);
    }

    assert!(extract(&carrier).is_err(), "duplicate {name} must fail");
  }
}

#[test]
fn extract_rejects_partial_contexts() {
  for fields in [
    vec![(VERSION, "1")],
    vec![(RUN_ID, VALID_ID)],
    vec![(AUTHORITY_ID, VALID_ID)],
    vec![(SPAN_ID, VALID_ID)],
    vec![(VERSION, "1"), (AUTHORITY_ID, VALID_ID)],
  ] {
    let mut carrier = MapCarrier::default();
    for (name, value) in fields {
      carrier.insert(name, value);
    }
    assert!(extract(&carrier).is_err());
  }
}

#[test]
fn extract_rejects_invalid_or_noncanonical_ids() {
  for (name, value) in [
    (RUN_ID, "not-a-uuid"),
    (RUN_ID, "018F47A0-4B5C-7D6E-8F90-123456789ABC"),
    (RUN_ID, "00000000-0000-0000-0000-000000000000"),
    (AUTHORITY_ID, "018F47A0-4B5C-7D6E-8F90-123456789ABC"),
    (SPAN_ID, "018F47A0-4B5C-7D6E-8F90-123456789ABC"),
  ] {
    let mut carrier = minimal_carrier(VALID_ID);
    carrier.replace(name, value);
    assert!(extract(&carrier).is_err(), "invalid {name} must fail");
  }
}

#[test]
fn extract_rejects_unknown_versions() {
  let mut carrier = minimal_carrier(VALID_ID);
  carrier.replace(VERSION, "2");

  assert!(extract(&carrier).is_err());
}

#[test]
fn from_remote_rejects_conflicting_local_authority() {
  let fixture = TestDispatch::memory();
  let remote_authority = loop {
    let candidate = auv_tracing::AuthorityId::new();
    if candidate != fixture.store.authority_id() {
      break candidate;
    }
  };
  let remote = extract(&complete_carrier(RunId::new(), remote_authority, SpanId::new())).unwrap().unwrap();

  let result = dispatcher::with_default(&fixture.dispatch, || Context::from_remote(remote));

  assert!(result.is_err());
}

#[test]
fn disabled_remote_context_preserves_authority_and_span_for_reinjection() {
  let run_id = RunId::new();
  let authority_id = auv_tracing::AuthorityId::new();
  let span_id = SpanId::new();
  let remote = extract(&complete_carrier(run_id, authority_id, span_id)).unwrap().unwrap();
  let context = Context::from_remote(remote).unwrap();
  let mut reinjected = MapCarrier::default();

  assert!(!context.is_enabled());
  assert_eq!(context.authority_id(), Some(&authority_id));
  assert!(context.span_id().is_none(), "a remote span is not a local current span");
  context.inject(&mut reinjected);

  assert_eq!(reinjected.value(VERSION), Some("1"));
  assert_eq!(reinjected.value(RUN_ID), Some(run_id.to_string().as_str()));
  assert_eq!(reinjected.value(AUTHORITY_ID), Some(authority_id.to_string().as_str()));
  assert_eq!(reinjected.value(SPAN_ID), Some(span_id.to_string().as_str()));
}

#[test]
fn remote_span_links_direct_local_spans_then_children_use_local_parentage() {
  let fixture = TestDispatch::memory();
  let run_id = RunId::new();
  let remote_span_id = SpanId::new();
  let remote = extract(&complete_carrier(run_id, fixture.store.authority_id(), remote_span_id)).unwrap().unwrap();
  let context = dispatcher::with_default(&fixture.dispatch, || Context::from_remote(remote)).unwrap();

  assert!(context.is_enabled());
  let first = context.in_scope(|| auv_tracing::start_span!(TestSpan));
  let first_id = *first.id().unwrap();
  let child = first.in_scope(|| auv_tracing::start_span!(TestSpan));
  let child_id = *child.id().unwrap();
  drop(child);
  let second = context.in_scope(|| auv_tracing::start_span!(TestSpan));
  let second_id = *second.id().unwrap();
  drop(second);
  drop(first);
  block_on_timeout(fixture.dispatch.flush()).unwrap();

  let snapshot = fixture.snapshot(run_id).unwrap();
  let first_start = snapshot.spans().get(&first_id).unwrap().started();
  let second_start = snapshot.spans().get(&second_id).unwrap().started();
  let child_start = snapshot.spans().get(&child_id).unwrap().started();
  assert!(first_start.parent_span_id().is_none());
  assert_eq!(first_start.remote_link().unwrap().span_id(), remote_span_id);
  assert!(second_start.parent_span_id().is_none());
  assert_eq!(second_start.remote_link().unwrap().span_id(), remote_span_id);
  assert_eq!(child_start.parent_span_id(), Some(first_id));
  assert!(child_start.remote_link().is_none());
}
