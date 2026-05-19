use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use crate::model::now_millis;
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
  pub fn new(span_id: SpanId) -> Self {
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

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunStreamEvent {
  SpanStarted {
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  SpanFinished {
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  EventAppended {
    run_id: RunId,
    event: EventRecordV1Alpha1,
  },
  ArtifactCreated {
    run_id: RunId,
    artifact: ArtifactRecordV1Alpha1,
  },
  RunFinished {
    run_id: RunId,
    run: RunRecordV1Alpha1,
  },
}

impl RunStreamEvent {
  pub fn run_id(&self) -> &RunId {
    match self {
      Self::SpanStarted { run_id, .. }
      | Self::SpanFinished { run_id, .. }
      | Self::EventAppended { run_id, .. }
      | Self::ArtifactCreated { run_id, .. }
      | Self::RunFinished { run_id, .. } => run_id,
    }
  }
}

pub trait RunEventSink: Send + Sync {
  fn on_event(&self, event: RunStreamEvent);
}

#[derive(Clone)]
pub struct MemoryRunEventSink {
  events: Arc<Mutex<Vec<RunStreamEvent>>>,
}

impl MemoryRunEventSink {
  pub fn new() -> Self {
    Self {
      events: Arc::new(Mutex::new(Vec::new())),
    }
  }

  pub fn drain_for_test(&self) -> Vec<RunStreamEvent> {
    self
      .events
      .lock()
      .map(|events| events.clone())
      .unwrap_or_default()
  }
}

impl Default for MemoryRunEventSink {
  fn default() -> Self {
    Self::new()
  }
}

impl RunEventSink for MemoryRunEventSink {
  fn on_event(&self, event: RunStreamEvent) {
    if let Ok(mut events) = self.events.lock() {
      events.push(event);
    }
  }
}

pub struct RecordingRun {
  run: RunRecordV1Alpha1,
  spans: Vec<SpanRecordV1Alpha1>,
  events: Vec<EventRecordV1Alpha1>,
  artifacts: Vec<ArtifactRecordV1Alpha1>,
  event_sink: Arc<dyn RunEventSink>,
}

pub struct RecordedRun {
  pub snapshot: CanonicalRun,
}

impl RecordingRun {
  pub fn new(
    run: RunRecordV1Alpha1,
    root_span: SpanRecordV1Alpha1,
    event_sink: Arc<dyn RunEventSink>,
  ) -> Self {
    event_sink.on_event(RunStreamEvent::SpanStarted {
      run_id: run.run_id.clone(),
      span: root_span.clone(),
    });
    Self {
      run,
      spans: vec![root_span],
      events: Vec::new(),
      artifacts: Vec::new(),
      event_sink,
    }
  }

  pub fn id(&self) -> &RunId {
    &self.run.run_id
  }

  pub fn root_span(&self) -> SpanRef {
    SpanRef::new(self.run.root_span_id.clone())
  }

  pub fn start_span(&mut self, parent: &SpanRef, mut span: SpanRecordV1Alpha1) -> SpanRef {
    span.parent_span_id = Some(parent.id().clone());
    let span_ref = SpanRef::new(span.span_id.clone());
    self.event_sink.on_event(RunStreamEvent::SpanStarted {
      run_id: self.run.run_id.clone(),
      span: span.clone(),
    });
    self.spans.push(span);
    span_ref
  }

  pub fn finish_span(&mut self, span: &SpanRef, finish: SpanFinish) {
    if let Some(record) = self
      .spans
      .iter_mut()
      .find(|record| record.span_id == *span.id())
    {
      if record.state == TraceState::Ended {
        return;
      }
      record.state = TraceState::Ended;
      record.status_code = finish.status_code;
      record.finished_at_millis = Some(now_millis());
      record.summary = finish.summary;
      record.failure = finish.failure.map(|message| TraceFailure { message });
      self.event_sink.on_event(RunStreamEvent::SpanFinished {
        run_id: self.run.run_id.clone(),
        span: record.clone(),
      });
    }
  }

  pub fn record_event(&mut self, event: EventRecordV1Alpha1) -> EventId {
    let event_id = event.event_id.clone();
    self.event_sink.on_event(RunStreamEvent::EventAppended {
      run_id: self.run.run_id.clone(),
      event: event.clone(),
    });
    self.events.push(event);
    event_id
  }

  pub fn record_artifact(&mut self, artifact: ArtifactRecordV1Alpha1) -> ArtifactId {
    let artifact_id = artifact.artifact_id.clone();
    self.event_sink.on_event(RunStreamEvent::ArtifactCreated {
      run_id: self.run.run_id.clone(),
      artifact: artifact.clone(),
    });
    self.artifacts.push(artifact);
    artifact_id
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
    for span in &mut self.spans {
      if span.state == TraceState::Running {
        span.state = TraceState::Ended;
        span.status_code = status_code;
        span.finished_at_millis = Some(finished_at_millis);
        self.event_sink.on_event(RunStreamEvent::SpanFinished {
          run_id: self.run.run_id.clone(),
          span: span.clone(),
        });
      }
    }
    RecordedRun {
      snapshot: CanonicalRun {
        run: self.run,
        spans: self.spans,
        events: self.events,
        artifacts: self.artifacts,
      },
    }
  }
}
