#![forbid(unsafe_code)]

//! Disposable viewer DTOs derived only from canonical [`auv_tracing::RunSnapshot`] values.

pub mod legacy;

use auv_tracing::{
  ArtifactPurpose, ArtifactUri, Attributes, AuthorityId, ByteLength, ContentType, EventId, EventSchema, JsonPayload, RunId, RunRevision,
  RunSnapshot, Sha256Digest, SpanId, SpanLink, SpanName, Timestamp,
};
use serde::Serialize;

// TODO(run-contract-tasks-17-23): Remove these hidden aliases when the
// out-of-scope CLI reader, Balatro, and root contract imports move to `legacy`.
#[doc(hidden)]
pub use legacy::{
  ArtifactRefView, InspectComposer, InspectError, InspectSection, InspectSectionOutput, TELEMETRY_SAMPLE_ARTIFACT_ROLE,
  artifact_record_view, is_json_mime, open_artifact_file, read_artifact_json, read_telemetry_artifact_summary,
};

/// Complete Inspect read projection through one canonical run revision.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InspectDocument {
  pub authority_id: AuthorityId,
  pub run_id: RunId,
  pub through_revision: RunRevision,
  pub spans: Vec<InspectSpan>,
  pub events: Vec<InspectEvent>,
  pub artifacts: Vec<InspectArtifact>,
}

/// Viewer-oriented span fields without an inferred execution or semantic status.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InspectSpan {
  pub span_id: SpanId,
  pub parent_span_id: Option<SpanId>,
  pub remote_link: Option<SpanLink>,
  pub name: SpanName,
  pub started_at: Timestamp,
  pub ended_at: Option<Timestamp>,
  pub attributes: Attributes,
}

/// Typed event data preserved for inspection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct InspectEvent {
  pub event_id: EventId,
  pub span_id: Option<SpanId>,
  pub occurred_at: Timestamp,
  pub schema: EventSchema,
  pub payload: JsonPayload,
}

/// Canonical artifact identity and committed metadata only.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct InspectArtifact {
  pub span_id: Option<SpanId>,
  pub uri: ArtifactUri,
  pub purpose: ArtifactPurpose,
  pub content_type: ContentType,
  pub byte_length: ByteLength,
  pub sha256: Sha256Digest,
  pub attributes: Attributes,
}

impl From<&RunSnapshot> for InspectDocument {
  fn from(snapshot: &RunSnapshot) -> Self {
    let spans = snapshot
      .spans()
      .values()
      .map(|span| {
        let started = span.started();
        InspectSpan {
          span_id: started.span_id(),
          parent_span_id: started.parent_span_id(),
          remote_link: started.remote_link().cloned(),
          name: started.name().clone(),
          started_at: started.started_at(),
          ended_at: span.ended().map(|ended| ended.ended_at()),
          attributes: started.attributes().clone(),
        }
      })
      .collect();
    let events = snapshot
      .events()
      .iter()
      .map(|event| InspectEvent {
        event_id: event.event_id(),
        span_id: event.span_id(),
        occurred_at: event.occurred_at(),
        schema: event.schema().clone(),
        payload: event.payload().clone(),
      })
      .collect();
    let artifacts = snapshot
      .artifacts()
      .values()
      .map(|artifact| {
        let metadata = artifact.metadata();
        InspectArtifact {
          span_id: artifact.span_id(),
          uri: metadata.uri().clone(),
          purpose: metadata.purpose().clone(),
          content_type: metadata.content_type().clone(),
          byte_length: metadata.byte_length(),
          sha256: metadata.sha256(),
          attributes: metadata.attributes().clone(),
        }
      })
      .collect();
    Self {
      authority_id: snapshot.authority_id(),
      run_id: snapshot.run_id(),
      through_revision: snapshot.through_revision(),
      spans,
      events,
      artifacts,
    }
  }
}
