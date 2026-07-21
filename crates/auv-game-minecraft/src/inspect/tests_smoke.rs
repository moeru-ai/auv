use auv_inspect_model::legacy::InspectComposer;
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::{
  RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION, SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
};

fn write_minimal_run(store: &LocalStore, run_id: &RunId) {
  let span_id = SpanId::new("span_root");
  store
    .write_run_snapshot(&CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: span_id.clone(),
        attributes: Default::default(),
        summary: None,
        failure: None,
      },
      spans: vec![SpanRecordV1Alpha1 {
        api_version: SPAN_API_VERSION.to_string(),
        span_id,
        parent_span_id: None,
        name: "root".to_string(),
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        attributes: Default::default(),
        summary: None,
        failure: None,
      }],
      events: Vec::new(),
      artifacts: Vec::new(),
    })
    .expect("snapshot");
}

#[test]
fn minecraft_sections_emit_legacy_empty_headers_without_query_wired() {
  let root = std::env::temp_dir()
    .join(format!("auv-mc-inspect-{}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
  let store = LocalStore::new(root.clone()).expect("store");
  let run_id = RunId::new("run_mc_empty");
  write_minimal_run(&store, &run_id);
  let mut sections = crate::inspect_sections_primary();
  sections.extend(crate::inspect_sections_quality_spatial());
  let composer = InspectComposer::try_new(sections).expect("composer");
  let text = composer.inspect_text(&store, run_id.as_str()).expect("text");
  assert!(text.contains("\nMC-1 Telemetry Samples:\n- none\n"));
  assert!(text.contains("\nMC-17 Quality Baseline Report:\n"));
  assert!(!text.contains("MC-19 Query Wired Live Action"));
  let _ = std::fs::remove_dir_all(root);
}
