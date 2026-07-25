use std::sync::Arc;

use auv_tracing::{ArtifactPurpose, Attributes, ContentType, Context, EventPayload, FileTracingStore, RunId, configure, dispatcher};

#[derive(serde::Serialize)]
struct Event {
  ok: bool,
}

#[test]
fn file_store_writes_artifact_bytes_before_the_metadata_record() {
  let directory = tempfile::tempdir().unwrap();
  let store = Arc::new(FileTracingStore::open(directory.path()).unwrap());
  let dispatch = configure().tracing_store(store).build().unwrap();
  let run_id = RunId::new();
  let body = b"artifact".to_vec();
  let emission = dispatcher::with_default(&dispatch, || {
    Context::root(run_id).in_scope(|| {
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

  let path = directory.path().join("artifacts").join(run_id.to_string()).join(metadata.uri().artifact_id().to_string());
  assert_eq!(std::fs::read(path).unwrap(), body);
  let line = std::fs::read_to_string(directory.path().join("records.jsonl")).unwrap();
  let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
  assert_eq!(value["record"]["type"], "artifact");
}
impl EventPayload for Event {
  const NAME: &'static str = "auv.test.file";
  const VERSION: u32 = 1;
}

#[test]
fn file_store_appends_stable_json_lines_without_exposing_a_reader() {
  let directory = tempfile::tempdir().unwrap();
  let store = Arc::new(FileTracingStore::open(directory.path()).unwrap());
  let dispatch = configure().tracing_store(store).build().unwrap();
  dispatcher::with_default(&dispatch, || Context::root(RunId::new()).in_scope(|| auv_tracing::emit_event!(Event { ok: true })));
  futures_executor::block_on(dispatch.flush()).unwrap();

  let line = std::fs::read_to_string(directory.path().join("records.jsonl")).unwrap();
  let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
  assert_eq!(value["version"], 1);
  assert_eq!(value["record"]["type"], "event");
  assert_eq!(value["record"]["payload"]["ok"], true);
}
