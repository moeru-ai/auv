use crate::store::CanonicalRun;

pub fn render_text(run: &CanonicalRun) -> String {
  let mut output = format!(
    "Run {}\nType: {}\nStatus: {}\nState: {}\n",
    run.run.run_id,
    run.run.run_type.as_str(),
    run.run.status_code.as_str(),
    run.run.state.as_str()
  );
  if let Some(summary) = &run.run.summary {
    output.push_str(&format!("Summary: {summary}\n"));
  }
  if let Some(failure) = &run.run.failure {
    output.push_str(&format!("Failure: {}\n", failure.message));
  }

  output.push_str("\nSpans:\n");
  for span in &run.spans {
    output.push_str(&format!(
      "- {} name={} parent={} status={}\n",
      span.span_id,
      span.name,
      span
        .parent_span_id
        .as_ref()
        .map(|span_id| span_id.as_str())
        .unwrap_or("n/a"),
      span.status_code.as_str()
    ));
  }

  output.push_str("\nEvents:\n");
  for event in &run.events {
    let message = event.message.as_deref().unwrap_or("");
    output.push_str(&format!(
      "- {} span={} name={} {}\n",
      event.event_id, event.span_id, event.name, message
    ));
    if !event.artifact_ids.is_empty() {
      let artifact_ids = event
        .artifact_ids
        .iter()
        .map(|artifact_id| artifact_id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
      output.push_str(&format!("  artifacts={artifact_ids}\n"));
    }
  }

  output.push_str("\nArtifacts:\n");
  for artifact in &run.artifacts {
    output.push_str(&format!(
      "- {} span={} role={} path={}\n",
      artifact.artifact_id, artifact.span_id, artifact.role, artifact.path
    ));
  }

  output
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use super::render_text;
  use crate::store::CanonicalRun;
  use crate::trace::{
    ARTIFACT_API_VERSION, ArtifactId, ArtifactRecordV1Alpha1, EVENT_API_VERSION, EventId,
    EventRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION,
    SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

  #[test]
  fn render_text_includes_run_span_event_and_artifact_records() {
    let run_id = RunId::new("run_inspect_test");
    let root_span_id = SpanId::new("span_root");
    let event_id = EventId::new("event_test");
    let artifact_id = ArtifactId::new("artifact_test");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_test"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("inspection summary".to_string()),
        failure: None,
      },
      spans: vec![SpanRecordV1Alpha1 {
        api_version: SPAN_API_VERSION.to_string(),
        span_id: root_span_id.clone(),
        parent_span_id: None,
        name: "auv.inspect.span".to_string(),
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        attributes: BTreeMap::new(),
        summary: None,
        failure: None,
      }],
      events: vec![EventRecordV1Alpha1 {
        api_version: EVENT_API_VERSION.to_string(),
        event_id,
        span_id: root_span_id.clone(),
        name: "inspect.event".to_string(),
        timestamp_millis: 1,
        attributes: BTreeMap::new(),
        message: Some("event message".to_string()),
        artifact_ids: vec![artifact_id.clone()],
      }],
      artifacts: vec![ArtifactRecordV1Alpha1 {
        api_version: ARTIFACT_API_VERSION.to_string(),
        artifact_id: artifact_id.clone(),
        span_id: root_span_id,
        event_id: None,
        role: "driver.output".to_string(),
        mime_type: "text/plain".to_string(),
        path: "artifacts/output.txt".to_string(),
        sha256: None,
        attributes: BTreeMap::new(),
        summary: Some("output".to_string()),
      }],
    };

    let output = render_text(&run);

    assert!(output.contains("Run run_inspect_test"));
    assert!(output.contains("Type: command"));
    assert!(output.contains("Status: ok"));
    assert!(output.contains("auv.inspect.span"));
    assert!(output.contains("inspect.event"));
    assert!(output.contains("artifact_test"));
  }
}
