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
#[cfg(test)]
mod tests {
  use std::fs;
  use std::path::Path;

  #[test]
  fn crate_does_not_initialize_global_trace_subscriber() {
    let manifest = include_str!("../Cargo.toml");
    for forbidden_dependency in [
      concat!("tracing", "-subscriber"),
      concat!("open", "telemetry"),
      concat!("open", "telemetry", "-otlp"),
      concat!("open", "telemetry", "_sdk"),
    ] {
      assert!(
        !manifest.contains(forbidden_dependency),
        "auv-tracing-driver must not depend on subscriber/exporter setup crate `{forbidden_dependency}`"
      );
    }

    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut source_files = Vec::new();
    collect_rust_files(&source_root, &mut source_files);

    for source_file in source_files {
      let source = fs::read_to_string(&source_file)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", source_file.display()));
      for forbidden_api in [
        concat!("tracing", "_subscriber"),
        concat!("set", "_global", "_default"),
        concat!("set", "_default"),
        concat!("open", "telemetry", "::global"),
      ] {
        assert!(
          !source.contains(forbidden_api),
          "auv-tracing-driver must emit tracing data without installing subscribers or exporters; found `{forbidden_api}` in {}",
          source_file.display()
        );
      }
    }
  }

  fn collect_rust_files(directory: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(directory)
      .unwrap_or_else(|error| panic!("failed to read {}: {error}", directory.display()))
    {
      let path = entry
        .expect("source directory entry should be readable")
        .path();
      if path.is_dir() {
        collect_rust_files(&path, files);
      } else if path.extension().and_then(|extension| extension.to_str()) == Some("rs") {
        files.push(path);
      }
    }
  }
}
