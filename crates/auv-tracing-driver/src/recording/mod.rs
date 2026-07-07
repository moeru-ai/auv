//! Run update delivery transport.
//!
//! This module owns the data + machinery for streaming run/span/event/artifact
//! updates from runtime to recording sinks:
//!
//! - [`update`]: `RunUpdate` event enum (canonical snake_case serialization).
//! - [`wire`]: `WireUpdate` newtype that re-serializes `RunUpdate` as camelCase
//!   for the inspect server HTTP write API.
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
pub mod wire;

pub use backend::{ArtifactRecordingFailure, RecordedArtifacts, RecordingHandle, RunRecordingBackend};
pub use recorder::{BroadcastRunRecorder, CompositeRunRecorder, InspectServerRunRecorder, MemoryRunRecorder, NoopRunRecorder, RunRecorder};
pub use update::RunUpdate;
pub use wire::WireUpdate;
