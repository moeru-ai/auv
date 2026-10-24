use std::sync::Arc;

use auv_tracing::{
  Context, EmitBytesOptions, EventPayload, FileTracingStore, NewArtifact, RunId, Timestamp, TraceRecord, configure, dispatcher,
};

#[derive(serde::Serialize)]
struct Event {
  ok: bool,
}

#[test]
fn file_store_writes_artifact_bytes_before_the_metadata_record() {
  let directory = tempfile::tempdir().unwrap();
  let store = Arc::new(FileTracingStore::open(directory.path()).unwrap());
  let dispatch = configure().tracing_store(store.clone()).build().unwrap();
  let run_id = RunId::new();
  let body = b"artifact".to_vec();
  let emission = dispatcher::with_default(&dispatch, || {
    Context::root(run_id).in_scope(|| {
      let options = EmitBytesOptions::new().with_purpose("auv.test.output").with_content_type("text/plain").with_file_extension("txt");
      auv_tracing::emit_bytes_artifact(options, body.clone()).unwrap()
    })
  });
  let metadata = futures_executor::block_on(emission).unwrap().unwrap();
  futures_executor::block_on(dispatch.flush()).unwrap();

  let path = directory.path().join("artifacts").join(run_id.to_string()).join(format!("{}.txt", metadata.uri().artifact_id()));
  assert_eq!(store.artifact_path(&metadata), path);
  assert_eq!(std::fs::read(path).unwrap(), body);
  let line = std::fs::read_to_string(directory.path().join("records.jsonl")).unwrap();
  let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
  assert_eq!(value["record"]["type"], "artifact");
  assert_eq!(value["record"]["metadata"]["file_extension"], "txt");
}

#[test]
fn file_store_uses_explicit_extensions_without_inferring_them_from_content_type() {
  let cases = [
    ("image/png", Some("png")),
    ("application/json", Some("json")),
    ("text/plain", Some("txt")),
    ("caller declared", Some("tar.gz")),
    ("image/png", None),
  ];

  for (content_type, expected_extension) in cases {
    let directory = tempfile::tempdir().unwrap();
    let store = Arc::new(FileTracingStore::open(directory.path()).unwrap());
    let dispatch = configure().tracing_store(store).build().unwrap();
    let run_id = RunId::new();
    let emission = dispatcher::with_default(&dispatch, || {
      Context::root(run_id).in_scope(|| {
        let options = EmitBytesOptions::new().with_purpose("auv.test.output").with_content_type(content_type);
        let options = match expected_extension {
          Some(extension) => options.with_file_extension(extension),
          None => options,
        };
        auv_tracing::emit_bytes_artifact(options, b"artifact".to_vec()).unwrap()
      })
    });
    let metadata = futures_executor::block_on(emission).unwrap().unwrap();
    futures_executor::block_on(dispatch.flush()).unwrap();

    let mut file_name = metadata.uri().artifact_id().to_string();
    if let Some(extension) = expected_extension {
      file_name.push('.');
      file_name.push_str(extension);
    }
    let path = directory.path().join("artifacts").join(run_id.to_string()).join(file_name);
    assert_eq!(std::fs::read(path).unwrap(), b"artifact", "content type {content_type}");
    assert_eq!(metadata.file_extension(), expected_extension);
  }
}
impl EventPayload for Event {
  const NAME: &'static str = "auv.test.file";
  const VERSION: u32 = 1;
}

#[test]
fn file_store_appends_stable_versioned_json_lines() {
  let directory = tempfile::tempdir().unwrap();
  let store = Arc::new(FileTracingStore::open(directory.path()).unwrap());
  let dispatch = configure().tracing_store(store).build().unwrap();
  dispatcher::with_default(&dispatch, || Context::root(RunId::new()).in_scope(|| auv_tracing::emit_event!(Event { ok: true })));
  futures_executor::block_on(dispatch.flush()).unwrap();

  let line = std::fs::read_to_string(directory.path().join("records.jsonl")).unwrap();
  let value: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
  assert_eq!(value["version"], 2);
  assert_eq!(value["record"]["type"], "event");
  assert_eq!(value["record"]["payload"]["ok"], true);
}

#[test]
fn file_store_records_when_an_artifact_occurred() {
  // ROOT CAUSE:
  //
  // If a reader needed the order of captures, it had only the line order,
  // because artifact records carried no time.
  //
  // Before the fix, `artifact` records had run, span, and metadata only.
  // The fix keeps the producer's time, or the submit time when none is given.
  let directory = tempfile::tempdir().unwrap();
  let store = Arc::new(FileTracingStore::open(directory.path()).unwrap());
  let dispatch = configure().tracing_store(store).build().unwrap();
  let run_id = RunId::new();
  let captured_at = Timestamp::new(1_700_000_000, 5).unwrap();
  let before = Timestamp::now().unwrap();
  dispatcher::with_default(&dispatch, || {
    Context::root(run_id).in_scope(|| {
      let options = EmitBytesOptions::new().with_purpose("auv.test.capture");
      drop(auv_tracing::emit_artifact(NewArtifact::from_bytes(options.clone(), b"a".to_vec()).unwrap().with_occurred_at(captured_at)));
      drop(auv_tracing::emit_artifact(NewArtifact::from_bytes(options, b"b".to_vec()).unwrap()));
    })
  });
  futures_executor::block_on(dispatch.flush()).unwrap();

  let records = FileTracingStore::read_records(directory.path()).unwrap().map(|record| record.unwrap().record).collect::<Vec<_>>();
  let times = records
    .iter()
    .map(|record| match record {
      TraceRecord::Artifact { occurred_at, .. } => *occurred_at,
      other => panic!("unexpected record {other:?}"),
    })
    .collect::<Vec<_>>();
  assert_eq!(times[0], captured_at);
  assert!(times[1] >= before, "a defaulted artifact time is its submit time");
}

#[test]
fn file_store_reader_returns_records_in_write_order_with_line_numbers() {
  let directory = tempfile::tempdir().unwrap();
  let store = Arc::new(FileTracingStore::open(directory.path()).unwrap());
  let dispatch = configure().tracing_store(store).build().unwrap();
  let run_id = RunId::new();
  dispatcher::with_default(&dispatch, || {
    Context::root(run_id).in_scope(|| {
      auv_tracing::emit_event!(Event { ok: true });
      auv_tracing::emit_event!(Event { ok: false });
    })
  });
  futures_executor::block_on(dispatch.flush()).unwrap();

  let records = FileTracingStore::read_records(directory.path()).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
  assert_eq!(records.iter().map(|record| record.line).collect::<Vec<_>>(), [1, 2]);
  let payloads = records
    .iter()
    .map(|record| match &record.record {
      TraceRecord::Event {
        payload,
        run_id: id,
        ..
      } => {
        assert_eq!(*id, run_id);
        payload.get().to_string()
      }
      other => panic!("unexpected record {other:?}"),
    })
    .collect::<Vec<_>>();
  assert_eq!(payloads, [r#"{"ok":true}"#, r#"{"ok":false}"#]);
}

#[test]
fn file_store_reader_reports_old_envelopes_and_torn_lines_by_line() {
  let directory = tempfile::tempdir().unwrap();
  let run_id = RunId::new();
  let old = format!(
    r#"{{"version":1,"record":{{"type":"span_ended","run_id":"{run_id}","span_id":"{}","ended_at":{{"unix_seconds":1,"nanoseconds":0}}}}}}"#,
    auv_tracing::SpanId::new()
  );
  let current = old.replacen(r#""version":1"#, r#""version":2"#, 1);
  std::fs::write(directory.path().join("records.jsonl"), format!("{old}\n{current}\n{{\"version\":2,\"rec")).unwrap();

  let results = FileTracingStore::read_records(directory.path()).unwrap().collect::<Vec<_>>();
  assert_eq!(results.len(), 3);
  let first = results[0].as_ref().unwrap_err();
  assert_eq!((first.line, first.code.as_str()), (1, "auv.tracing.store.unsupported_envelope_version"));
  assert!(matches!(&results[1], Ok(record) if record.line == 2));
  let torn = results[2].as_ref().unwrap_err();
  assert_eq!((torn.line, torn.code.as_str()), (3, "auv.tracing.store.envelope_invalid"));
}

#[test]
fn file_store_reads_back_every_record_kind_it_writes() {
  // ROOT CAUSE:
  //
  // If an event record was read back as a `TraceRecord`, deserialization
  // failed, because the internally tagged enum buffers fields and
  // `JsonPayload` required a raw JSON value.
  //
  // Before the fix, no reader could load event records written by this store.
  // The fix reads payloads through a buffered value and re-canonicalizes them.
  let directory = tempfile::tempdir().unwrap();
  let store = Arc::new(FileTracingStore::open(directory.path()).unwrap());
  let dispatch = configure().tracing_store(store).build().unwrap();
  let run_id = RunId::new();
  dispatcher::with_default(&dispatch, || {
    Context::root(run_id).in_scope(|| {
      auv_tracing::in_span!("auv.test.span", || {
        auv_tracing::emit_event!(Event { ok: true });
        let attributes = auv_tracing::Attributes::from_iter([
          ("scale".to_string(), auv_tracing::AttributeValue::float(2.5).unwrap()),
          ("count".to_string(), auv_tracing::AttributeValue::integer(3)),
        ]);
        let options = EmitBytesOptions::new().with_purpose("auv.test.output").with_attributes(attributes);
        drop(auv_tracing::emit_bytes_artifact(options, b"x".to_vec()).unwrap());
      });
    })
  });
  futures_executor::block_on(dispatch.flush()).unwrap();

  let kinds = FileTracingStore::read_records(directory.path())
    .unwrap()
    .map(|record| match record.unwrap().record {
      TraceRecord::SpanStarted { .. } => "span_started",
      TraceRecord::Event { payload, .. } => {
        assert_eq!(payload.get(), r#"{"ok":true}"#);
        "event"
      }
      TraceRecord::Artifact { metadata, .. } => {
        assert_eq!(metadata.attributes().get("scale"), Some(&auv_tracing::AttributeValue::float(2.5).unwrap()));
        "artifact"
      }
      TraceRecord::SpanEnded { .. } => "span_ended",
    })
    .collect::<Vec<_>>();
  assert_eq!(kinds, ["span_started", "event", "artifact", "span_ended"]);
}

#[test]
fn two_writers_on_one_store_never_interleave_inside_a_line() {
  // ROOT CAUSE:
  //
  // If two processes appended to one store (a long-lived `auv mcp` and an
  // `auv invoke` in the same project), a record larger than the write buffer
  // went out as its JSON and its newline in separate writes, so the other
  // process's line could land between them and corrupt both.
  //
  // The fix writes each record and its newline as one buffer.
  let directory = tempfile::tempdir().unwrap();
  let writers = (0..2)
    .map(|writer| {
      let path = directory.path().to_path_buf();
      std::thread::spawn(move || {
        let store = Arc::new(FileTracingStore::open(&path).unwrap());
        let dispatch = configure().tracing_store(store).build().unwrap();
        #[derive(serde::Serialize)]
        struct Large {
          writer: u32,
          text: String,
        }
        impl EventPayload for Large {
          const NAME: &'static str = "auv.test.large";
          const VERSION: u32 = 1;
        }
        let run_id = RunId::new();
        for _ in 0..200 {
          dispatcher::with_default(&dispatch, || {
            Context::root(run_id).in_scope(|| {
              auv_tracing::emit_event!(Large {
                writer,
                text: "x".repeat(12_000),
              })
            })
          });
        }
        futures_executor::block_on(dispatch.flush()).unwrap();
      })
    })
    .collect::<Vec<_>>();
  for writer in writers {
    writer.join().unwrap();
  }

  let results = FileTracingStore::read_records(directory.path()).unwrap().collect::<Vec<_>>();
  let broken = results.iter().filter(|result| result.is_err()).count();
  assert_eq!((results.len(), broken), (400, 0));
}
