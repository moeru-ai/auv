use serde::{Deserialize, Serialize};

use crate::{ArtifactMetadata, Attributes, EventId, EventSchema, JsonPayload, RunId, SpanId, SpanName, Timestamp};

/// One full-fidelity observation emitted by AUV instrumentation.
///
/// Records are append-only observations. They are not commits, revisions, or
/// materialized run state; read-side tools may derive their own models later.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum TraceRecord {
  /// A span began.
  SpanStarted {
    run_id: RunId,
    span_id: SpanId,
    parent_span_id: Option<SpanId>,
    remote_span_id: Option<SpanId>,
    name: SpanName,
    started_at: Timestamp,
    attributes: Attributes,
  },
  /// A span ended.
  SpanEnded {
    run_id: RunId,
    span_id: SpanId,
    ended_at: Timestamp,
  },
  /// A typed point event occurred, including its canonical payload.
  Event {
    run_id: RunId,
    span_id: Option<SpanId>,
    event_id: EventId,
    schema: EventSchema,
    occurred_at: Timestamp,
    payload: JsonPayload,
  },
  /// An artifact body was stored and its metadata became observable.
  Artifact {
    run_id: RunId,
    span_id: Option<SpanId>,
    metadata: ArtifactMetadata,
  },
}

impl TraceRecord {
  /// Returns the run correlation carried by every record.
  pub fn run_id(&self) -> RunId {
    match self {
      Self::SpanStarted { run_id, .. } | Self::SpanEnded { run_id, .. } | Self::Event { run_id, .. } | Self::Artifact { run_id, .. } => {
        *run_id
      }
    }
  }
}
