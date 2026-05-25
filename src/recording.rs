// File: src/recording.rs
//! In-process run recording (canonical snapshot builder).
//!
//! `RecordingRun` constructs an in-memory `CanonicalRun` (run + spans + events
//! + artifacts) while also emitting `RunUpdate` notifications to a configured
//! `RunRecorder` (for live inspection or tests).
//!
//! Boundary: this module does not persist snapshots (`store` does) and does not
//! execute commands (`runtime` + drivers do).

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::model::{AuvResult, now_millis};
use crate::run_recording::{RunRecorder, RunUpdate};
use crate::store::CanonicalRun;
use crate::trace::{
  ArtifactId, ArtifactRecordV1Alpha1, EventId, EventRecordV1Alpha1, RunId, RunRecordV1Alpha1,
  RunType, SpanId, SpanRecordV1Alpha1, TraceFailure, TraceState, TraceStatusCode,
};

pub type Attributes = BTreeMap<String, serde_json::Value>;

pub struct RunSpec {
  pub run_type: RunType,
  pub root_span_name: String,
  pub attributes: Attributes,
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;

  use crate::trace::{
    EVENT_API_VERSION, EventRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType,
    SPAN_API_VERSION, SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

  use crate::run_recording::{BroadcastRunRecorder, MemoryRunRecorder, RunUpdate};

  use super::{RecordingRun, SpanFinish, SpanRef};

  #[test]
  fn start_span_rejects_parent_from_another_run() {
    let mut run = recording_run("run_invalid_parent");
    let foreign_parent = SpanRef::new(SpanId::new("0000000000009999"));

    let error = run
      .start_span(&foreign_parent, span_record("auv.invalid.child"))
      .expect_err("foreign parent span should be rejected");

    assert!(error.contains("does not belong to run"));
  }

  #[test]
  fn broadcast_recorder_replays_updates_to_subscribers() {
    let recorder = BroadcastRunRecorder::new(16);
    let mut receiver = recorder.subscribe();
    let mut run = RecordingRun::new(
      RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: RunId::new("run_broadcast_test"),
        trace_id: TraceId::new("00000000000000000000000000000001"),
        run_type: RunType::Command,
        state: TraceState::Running,
        status_code: TraceStatusCode::Unset,
        started_at_millis: 100,
        finished_at_millis: None,
        root_span_id: SpanId::new("0000000000000001"),
        attributes: Default::default(),
        summary: None,
        failure: None,
      },
      SpanRecordV1Alpha1 {
        api_version: SPAN_API_VERSION.to_string(),
        span_id: SpanId::new("0000000000000001"),
        parent_span_id: None,
        name: "auv.command".to_string(),
        state: TraceState::Running,
        status_code: TraceStatusCode::Unset,
        started_at_millis: 100,
        finished_at_millis: None,
        attributes: Default::default(),
        summary: None,
        failure: None,
      },
      Arc::new(recorder),
    );

    run.record_event(EventRecordV1Alpha1 {
      api_version: EVENT_API_VERSION.to_string(),
      event_id: crate::trace::EventId::new("event_broadcast_test"),
      span_id: SpanId::new("0000000000000001"),
      name: "broadcast.event".to_string(),
      timestamp_millis: 101,
      attributes: Default::default(),
      message: None,
      artifact_ids: Vec::new(),
    });

    let first = receiver.try_recv().expect("run start should broadcast");
    assert!(matches!(first, RunUpdate::RunStarted { .. }));
    let second = receiver.try_recv().expect("root span should broadcast");
    assert!(matches!(second, RunUpdate::SpanStarted { .. }));
    let third = receiver
      .try_recv()
      .expect("recorded event should broadcast");
    assert!(matches!(
      third,
      RunUpdate::EventAppended { event, .. } if event.name == "broadcast.event"
    ));
  }

  #[test]
  fn finish_span_rejects_span_from_another_run() {
    let mut run = recording_run("run_invalid_finish");
    let foreign_span = SpanRef::new(SpanId::new("0000000000009998"));

    let error = run
      .finish_span(
        &foreign_span,
        SpanFinish {
          status_code: TraceStatusCode::Ok,
          summary: None,
          failure: None,
        },
      )
      .expect_err("foreign span should be rejected");

    assert!(error.contains("does not belong to run"));
  }

  fn recording_run(run_id: &str) -> RecordingRun {
    let root_span_id = SpanId::new("0000000000000001");
    RecordingRun::new(
      RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: RunId::new(run_id),
        trace_id: TraceId::new("00000000000000000000000000000001"),
        run_type: RunType::Command,
        state: TraceState::Running,
        status_code: TraceStatusCode::Unset,
        started_at_millis: 100,
        finished_at_millis: None,
        root_span_id: root_span_id.clone(),
        attributes: Default::default(),
        summary: None,
        failure: None,
      },
      SpanRecordV1Alpha1 {
        api_version: SPAN_API_VERSION.to_string(),
        span_id: root_span_id,
        parent_span_id: None,
        name: "auv.command".to_string(),
        state: TraceState::Running,
        status_code: TraceStatusCode::Unset,
        started_at_millis: 100,
        finished_at_millis: None,
        attributes: Default::default(),
        summary: None,
        failure: None,
      },
      Arc::new(MemoryRunRecorder::new()),
    )
  }

  fn span_record(name: &str) -> SpanRecordV1Alpha1 {
    SpanRecordV1Alpha1 {
      api_version: SPAN_API_VERSION.to_string(),
      span_id: SpanId::new("0000000000000002"),
      parent_span_id: None,
      name: name.to_string(),
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: 101,
      finished_at_millis: None,
      attributes: Default::default(),
      summary: None,
      failure: None,
    }
  }
}

impl RunSpec {
  pub fn new(run_type: RunType, root_span_name: impl Into<String>) -> Self {
    Self {
      run_type,
      root_span_name: root_span_name.into(),
      attributes: Attributes::new(),
    }
  }

  pub fn with_attributes(mut self, attributes: Attributes) -> Self {
    self.attributes = attributes;
    self
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpanRef {
  span_id: SpanId,
}

impl SpanRef {
  pub(crate) fn new(span_id: SpanId) -> Self {
    Self { span_id }
  }

  pub fn id(&self) -> &SpanId {
    &self.span_id
  }
}

pub struct RunFinish {
  pub status_code: TraceStatusCode,
  pub summary: Option<String>,
  pub failure: Option<String>,
}

pub struct SpanFinish {
  pub status_code: TraceStatusCode,
  pub summary: Option<String>,
  pub failure: Option<String>,
}

pub struct RecordingRun {
  run: RunRecordV1Alpha1,
  spans: Vec<SpanRecordV1Alpha1>,
  events: Vec<EventRecordV1Alpha1>,
  artifacts: Vec<ArtifactRecordV1Alpha1>,
  recorder: Arc<dyn RunRecorder>,
  recording_errors: Vec<String>,
}

pub struct RecordedRun {
  pub snapshot: CanonicalRun,
  pub recording_errors: Vec<String>,
}

impl RecordingRun {
  pub fn new(
    run: RunRecordV1Alpha1,
    root_span: SpanRecordV1Alpha1,
    recorder: Arc<dyn RunRecorder>,
  ) -> Self {
    let mut recording = Self {
      run,
      spans: vec![root_span],
      events: Vec::new(),
      artifacts: Vec::new(),
      recorder,
      recording_errors: Vec::new(),
    };
    recording.record_update(RunUpdate::RunStarted {
      run_id: recording.run.run_id.clone(),
      run: recording.run.clone(),
    });
    recording.record_update(RunUpdate::SpanStarted {
      run_id: recording.run.run_id.clone(),
      span: recording.spans[0].clone(),
    });
    recording
  }

  pub fn id(&self) -> &RunId {
    &self.run.run_id
  }

  pub fn root_span(&self) -> SpanRef {
    SpanRef::new(self.run.root_span_id.clone())
  }

  pub fn recording_errors(&self) -> &[String] {
    &self.recording_errors
  }

  pub fn start_span(
    &mut self,
    parent: &SpanRef,
    mut span: SpanRecordV1Alpha1,
  ) -> AuvResult<SpanRef> {
    if !self.has_span(parent.id()) {
      return Err(format!(
        "parent span {} does not belong to run {}",
        parent.id(),
        self.run.run_id
      ));
    }
    if self.has_span(&span.span_id) {
      return Err(format!(
        "span {} already belongs to run {}",
        span.span_id, self.run.run_id
      ));
    }
    span.parent_span_id = Some(parent.id().clone());
    let span_ref = SpanRef::new(span.span_id.clone());
    self.record_update(RunUpdate::SpanStarted {
      run_id: self.run.run_id.clone(),
      span: span.clone(),
    });
    self.spans.push(span);
    Ok(span_ref)
  }

  pub fn finish_span(&mut self, span: &SpanRef, finish: SpanFinish) -> AuvResult<()> {
    let update = if let Some(record) = self
      .spans
      .iter_mut()
      .find(|record| record.span_id == *span.id())
    {
      if record.state == TraceState::Ended {
        return Ok(());
      }
      record.state = TraceState::Ended;
      record.status_code = finish.status_code;
      record.finished_at_millis = Some(now_millis());
      record.summary = finish.summary;
      record.failure = finish.failure.map(|message| TraceFailure { message });
      Some(RunUpdate::SpanFinished {
        run_id: self.run.run_id.clone(),
        span: record.clone(),
      })
    } else {
      None
    };
    if let Some(update) = update {
      self.record_update(update);
      Ok(())
    } else {
      Err(format!(
        "span {} does not belong to run {}",
        span.id(),
        self.run.run_id
      ))
    }
  }

  pub fn record_event(&mut self, event: EventRecordV1Alpha1) -> EventId {
    let event_id = event.event_id.clone();
    self.record_update(RunUpdate::EventAppended {
      run_id: self.run.run_id.clone(),
      event: event.clone(),
    });
    self.events.push(event);
    event_id
  }

  pub fn record_artifact(&mut self, artifact: ArtifactRecordV1Alpha1) -> ArtifactId {
    let artifact_id = artifact.artifact_id.clone();
    self.record_update(RunUpdate::ArtifactCreated {
      run_id: self.run.run_id.clone(),
      artifact: artifact.clone(),
    });
    self.artifacts.push(artifact);
    artifact_id
  }

  pub fn artifact_count(&self) -> usize {
    self.artifacts.len()
  }

  pub fn finish(
    mut self,
    status_code: TraceStatusCode,
    summary: Option<String>,
    failure: Option<TraceFailure>,
  ) -> RecordedRun {
    let finished_at_millis = now_millis();
    self.run.state = TraceState::Ended;
    self.run.status_code = status_code;
    self.run.finished_at_millis = Some(finished_at_millis);
    self.run.summary = summary;
    self.run.failure = failure;
    let mut finish_updates = Vec::new();
    for span in &mut self.spans {
      if span.state == TraceState::Running {
        span.state = TraceState::Ended;
        span.status_code = status_code;
        span.finished_at_millis = Some(finished_at_millis);
        finish_updates.push(RunUpdate::SpanFinished {
          run_id: self.run.run_id.clone(),
          span: span.clone(),
        });
      }
    }
    for update in finish_updates {
      self.record_update(update);
    }
    RecordedRun {
      snapshot: CanonicalRun {
        run: self.run,
        spans: self.spans,
        events: self.events,
        artifacts: self.artifacts,
      },
      recording_errors: self.recording_errors,
    }
  }

  fn record_update(&mut self, update: RunUpdate) {
    if let Err(error) = self.recorder.record(update)
      && self.recorder.requires_successful_delivery()
    {
      self.recording_errors.push(error);
    }
  }

  fn has_span(&self, span_id: &SpanId) -> bool {
    self.spans.iter().any(|span| span.span_id == *span_id)
  }
}
