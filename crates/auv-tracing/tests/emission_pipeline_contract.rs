use std::sync::{Arc, Mutex};

use auv_tracing::{
  ArtifactBody, ArtifactMetadata, ArtifactPurpose, ArtifactRequest, Attributes, BoxFuture, ContentType, Context, EventPayload, ExportError,
  MemoryTracingStore, RunId, SpanSpec, StoreError, TraceExporter, TraceRecord, TracingStore, configure, dispatcher,
};

struct Operation;
impl SpanSpec for Operation {
  const NAME: &'static str = "auv.test.operation";
  fn attributes(&self) -> Attributes {
    Attributes::empty()
  }
}

#[derive(serde::Serialize)]
struct Observed {
  value: u32,
}
impl EventPayload for Observed {
  const NAME: &'static str = "auv.test.observed";
  const VERSION: u32 = 1;
}

#[derive(Default)]
struct RecordingExporter(Mutex<Vec<TraceRecord>>);
impl TraceExporter for RecordingExporter {
  fn export(&self, record: TraceRecord) -> BoxFuture<'_, Result<(), ExportError>> {
    Box::pin(async move {
      self.0.lock().unwrap().push(record);
      Ok(())
    })
  }
  fn flush(&self) -> BoxFuture<'_, Result<(), ExportError>> {
    Box::pin(async { Ok(()) })
  }
}

#[test]
fn public_context_emits_the_same_ordered_records_to_store_and_exporter() {
  let store = Arc::new(MemoryTracingStore::new());
  let exporter = Arc::new(RecordingExporter::default());
  let dispatch = configure().tracing_store(store.clone()).exporter(exporter.clone()).build().unwrap();
  let run_id = RunId::new();

  dispatcher::with_default(&dispatch, || {
    let root = Context::root(run_id);
    let span = root.in_scope(|| auv_tracing::start_span!(Operation));
    span.in_scope(|| auv_tracing::emit_event!(Observed { value: 42 }));
    drop(span);
  });
  futures_executor::block_on(dispatch.flush()).unwrap();

  let stored = store.records();
  let exported = exporter.0.lock().unwrap().clone();
  assert_eq!(stored, exported);
  assert!(matches!(
    stored.as_slice(),
    [
      TraceRecord::SpanStarted { .. },
      TraceRecord::Event { .. },
      TraceRecord::SpanEnded { .. }
    ]
  ));
  let TraceRecord::Event { payload, .. } = &stored[1] else {
    unreachable!()
  };
  assert_eq!(payload.get(), r#"{"value":42}"#);
}

#[test]
fn disabled_context_does_not_serialize_events() {
  struct Panics;
  impl serde::Serialize for Panics {
    fn serialize<S>(&self, _: S) -> Result<S::Ok, S::Error>
    where
      S: serde::Serializer,
    {
      panic!("must stay disabled")
    }
  }
  impl EventPayload for Panics {
    const NAME: &'static str = "auv.test.disabled";
    const VERSION: u32 = 1;
  }

  let root = Context::root(RunId::new());
  root.in_scope(|| auv_tracing::emit_event!(Panics));
}

#[test]
fn dropping_context_guards_out_of_order_preserves_the_newer_scope() {
  let first = Context::root(RunId::new());
  let second = Context::root(RunId::new());
  let first_guard = first.enter();
  let second_guard = second.enter();
  drop(first_guard);
  assert_eq!(Context::current().run_id(), second.run_id());
  drop(second_guard);
}

#[test]
fn event_serialization_panic_is_observational() {
  struct Panics;
  impl serde::Serialize for Panics {
    fn serialize<S>(&self, _: S) -> Result<S::Ok, S::Error>
    where
      S: serde::Serializer,
    {
      panic!("producer serializer panic")
    }
  }
  impl EventPayload for Panics {
    const NAME: &'static str = "auv.test.panics";
    const VERSION: u32 = 1;
  }

  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().unwrap();
  dispatcher::with_default(&dispatch, || Context::root(RunId::new()).in_scope(|| auv_tracing::emit_event!(Panics)));
  futures_executor::block_on(dispatch.flush()).unwrap();
  assert!(store.records().is_empty());
}

#[test]
fn artifact_bytes_are_written_once_and_observed_as_metadata() {
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).build().unwrap();
  let body = b"full fidelity artifact".to_vec();
  let emission = dispatcher::with_default(&dispatch, || {
    Context::root(RunId::new()).in_scope(|| {
      auv_tracing::emit_bytes_artifact(
        ArtifactPurpose::parse("auv.test.output").unwrap(),
        ContentType::parse("text/plain").unwrap(),
        Attributes::empty(),
        body.clone(),
      )
      .unwrap()
    })
  });
  let metadata = futures_executor::block_on(emission).unwrap().unwrap();
  futures_executor::block_on(dispatch.flush()).unwrap();

  assert_eq!(store.artifact(metadata.uri()).unwrap(), body);
  assert!(matches!(store.records().as_slice(), [TraceRecord::Artifact { metadata: stored, .. }] if stored == &metadata));
}

struct FailingExporter;
impl TraceExporter for FailingExporter {
  fn export(&self, _: TraceRecord) -> BoxFuture<'_, Result<(), ExportError>> {
    Box::pin(async { Err(ExportError::new(auv_tracing::ErrorCode::parse("auv.test.export_failed").unwrap())) })
  }
  fn flush(&self) -> BoxFuture<'_, Result<(), ExportError>> {
    Box::pin(async { Ok(()) })
  }
}

#[test]
fn exporter_failure_does_not_prevent_full_fidelity_storage() {
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store.clone()).exporter(Arc::new(FailingExporter)).build().unwrap();
  dispatcher::with_default(&dispatch, || Context::root(RunId::new()).in_scope(|| auv_tracing::emit_event!(Observed { value: 7 })));

  let error = futures_executor::block_on(dispatch.flush()).unwrap_err();
  assert_eq!(error.failure_count().get(), 1);
  assert!(matches!(store.records().as_slice(), [TraceRecord::Event { .. }]));
}

struct FailingStore;
impl TracingStore for FailingStore {
  fn write(&self, _: TraceRecord) -> BoxFuture<'_, Result<(), StoreError>> {
    Box::pin(async { Err(StoreError::new(auv_tracing::ErrorCode::parse("auv.test.store_failed").unwrap())) })
  }
  fn write_artifact(&self, _: ArtifactRequest, _: ArtifactBody) -> BoxFuture<'_, Result<ArtifactMetadata, StoreError>> {
    Box::pin(async { Err(StoreError::new(auv_tracing::ErrorCode::parse("auv.test.store_failed").unwrap())) })
  }
  fn flush(&self) -> BoxFuture<'_, Result<(), StoreError>> {
    Box::pin(async { Ok(()) })
  }
}

#[test]
fn store_failure_does_not_prevent_export() {
  let exporter = Arc::new(RecordingExporter::default());
  let dispatch = configure().tracing_store(Arc::new(FailingStore)).exporter(exporter.clone()).build().unwrap();
  dispatcher::with_default(&dispatch, || Context::root(RunId::new()).in_scope(|| auv_tracing::emit_event!(Observed { value: 9 })));

  assert!(futures_executor::block_on(dispatch.flush()).is_err());
  assert!(matches!(exporter.0.lock().unwrap().as_slice(), [TraceRecord::Event { .. }]));
}
