//! Run update events.
//!
//! `RunUpdate` is the canonical event emitted by `RecordingRun` and consumed
//! by every `RunRecorder` sink. It serializes as snake_case (matching the
//! on-disk canonical JSON and the REST inspect endpoints).
//!
//! For the camelCase HTTP write API, see [`super::wire::WireUpdate`].

use crate::trace::{ArtifactRecordV1Alpha1, EventRecordV1Alpha1, RunId, RunRecordV1Alpha1, SpanRecordV1Alpha1};

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RunUpdate {
  RunStarted {
    run_id: RunId,
    run: RunRecordV1Alpha1,
  },
  SpanStarted {
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
  SpanFinished {
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  RunFinished {
    run_id: RunId,
    run: RunRecordV1Alpha1,
  },
}

impl RunUpdate {
  pub fn run_id(&self) -> &RunId {
    match self {
      Self::RunStarted { run_id, .. }
      | Self::SpanStarted { run_id, .. }
      | Self::EventAppended { run_id, .. }
      | Self::ArtifactCreated { run_id, .. }
      | Self::SpanFinished { run_id, .. }
      | Self::RunFinished { run_id, .. } => run_id,
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::trace::{RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SpanId, TraceId, TraceState, TraceStatusCode};

  use super::RunUpdate;

  fn test_run() -> RunRecordV1Alpha1 {
    RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: RunId::new("run_update_test"),
      trace_id: TraceId::new("00000000000000000000000000000001"),
      run_type: RunType::Execute,
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: 100,
      finished_at_millis: None,
      root_span_id: SpanId::new("0000000000000001"),
      attributes: Default::default(),
      summary: None,
      failure: None,
    }
  }

  #[test]
  fn run_update_serializes_canonical_snake_case() {
    let update = RunUpdate::RunStarted {
      run_id: RunId::new("run_update_test"),
      run: test_run(),
    };

    let value = serde_json::to_value(&update).expect("update should serialize");
    assert_eq!(value["type"], "run_started");
    assert_eq!(value["run_id"], "run_update_test");
    assert_eq!(value["run"]["api_version"], "auv.run.v1alpha1");
    assert_eq!(value["run"]["root_span_id"], "0000000000000001");
    assert!(value["run"].get("rootSpanId").is_none());
  }
}
