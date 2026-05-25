//! Run update delivery transport.
//!
//! This module owns the data + machinery for streaming run/span/event/artifact
//! updates from runtime to recording sinks:
//!
//! - [`update`]: `RunUpdate` event enum + camelCase HTTP wire shapes.
//! - [`recorder`]: `RunRecorder` trait and concrete backends (Noop, Memory,
//!   Broadcast, Composite, InspectServer HTTP).
//! - [`backend`]: `RunRecordingBackend` façade combining one store with one
//!   recorder.
//!
//! Boundary: recorders deliver/replicate trace data; they do not execute
//! commands or interpret automation semantics. The in-memory snapshot builder
//! that consumes this transport lives in `run_builder`.

pub mod backend;
pub mod recorder;
pub mod update;

pub use backend::RunRecordingBackend;
pub use recorder::{
  BroadcastRunRecorder, CompositeRunRecorder, InspectServerRunRecorder, MemoryRunRecorder,
  NoopRunRecorder, RunRecorder,
};
pub use update::{
  ApiArtifactRecord, ApiEventRecord, ApiRunRecord, ApiRunUpdate, ApiSpanRecord, RunUpdate,
};
