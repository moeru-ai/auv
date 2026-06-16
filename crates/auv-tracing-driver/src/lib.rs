//! Durable AUV driver-level run recording.
//!
//! This crate owns AUV's persisted run/span/event/artifact model and recorder
//! fan-out. It emits ordinary Rust `tracing` events for observability, but does
//! not install subscribers or OpenTelemetry exporters.

pub mod artifact;
pub mod error;
pub mod time;

pub use artifact::{ArtifactFileSource, ArtifactRef, ProducedArtifact};
pub use error::AuvResult;
pub use time::now_millis;
