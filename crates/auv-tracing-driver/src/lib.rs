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
pub use recorded_operation::{RecordedOperationContext, RecordedOperationOutput};
pub use recording::{
  BroadcastRunRecorder, CompositeRunRecorder, InspectServerRunRecorder, MemoryRunRecorder,
  NoopRunRecorder, RecordingHandle, RunRecorder, RunRecordingBackend, RunUpdate, WireUpdate,
};
pub use run_builder::{Attributes, RecordingRun, RunFinish, RunSpec, SpanFinish, SpanRef};
pub use store::{CanonicalRun, LocalStore};
pub use time::now_millis;
