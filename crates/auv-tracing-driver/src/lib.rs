//! Durable AUV driver-level run recording.
//!
//! This crate owns AUV's persisted run/span/event/artifact model and recorder
//! fan-out. It emits ordinary Rust `tracing` events for observability, but does
//! not install subscribers or OpenTelemetry exporters.

pub mod artifact;
pub mod error;
pub mod recorded_operation;
pub mod recording;
pub mod run_builder;
pub mod store;
pub mod time;
pub mod trace;

pub use artifact::{ArtifactFileSource, ArtifactRef, ProducedArtifact};
pub use error::AuvResult;
pub use recorded_operation::{
  RecordedOperationContext, RecordedOperationOutput, RecordedOperationServices,
  run_recorded_operation,
};
pub use recording::{
  BroadcastRunRecorder, CompositeRunRecorder, InspectServerRunRecorder, MemoryRunRecorder,
  NoopRunRecorder, RecordingHandle, RunRecorder, RunRecordingBackend, RunUpdate, WireUpdate,
};
pub use run_builder::{
  Attributes, RecordedRun, RecordingRun, RunFinish, RunSpec, SpanFinish, SpanRef,
};
pub use store::{CanonicalRun, LocalStore};
pub use time::now_millis;
pub use trace::{
  ARTIFACT_API_VERSION, ArtifactId, ArtifactRecordV1Alpha1, DeviceId, EVENT_API_VERSION, EventId,
  EventRecordV1Alpha1, RUN_API_VERSION, RUN_ATTR_DEVICE_ID, RUN_ATTR_SESSION_ID, RunId,
  RunRecordV1Alpha1, RunType, SPAN_API_VERSION, SessionId, SpanId, SpanRecordV1Alpha1,
  TraceFailure, TraceId, TraceState, TraceStatusCode, new_event_id, new_run_id, new_span_id,
  new_trace_id, string_attr,
};
