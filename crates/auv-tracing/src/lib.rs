#![forbid(unsafe_code)]

//! Typed, opt-in AUV instrumentation with write-only storage boundaries.

pub mod artifact;
pub mod context;
pub mod dispatch;
pub mod event;
mod macros;
pub mod propagation;
pub mod record;
pub mod store;
pub mod telemetry;
pub mod value;

pub use artifact::{
  ArtifactEmission, ArtifactMetadata, ArtifactUri, JsonArtifactError, NewArtifact, emit_artifact, emit_bytes_artifact, emit_json_artifact,
};
pub use context::{Context, ContextGuard, Instrumented, Span, SpanSpec, WithContext, emit_event, start_span};
pub use dispatch::{BuildError, Dispatch, DispatchBuilder, DispatchFailure, DispatchStage, FlushError, configure, dispatcher};
pub use event::{EventPayload, EventSchema, JsonPayload, JsonPayloadError};
pub use propagation::{PropagationError, RemoteContext, TextMapReader, TextMapWriter, extract};
pub use record::TraceRecord;
#[cfg(feature = "file-store")]
pub use store::FileTracingStore;
#[cfg(feature = "memory-store")]
pub use store::MemoryTracingStore;
pub use store::{ArtifactBody, ArtifactRequest, BoxFuture, StoreError, TracingStore};
pub use telemetry::{DispatchErrorReporter, ExportError, TraceExporter};
pub use value::{
  ArtifactId, ArtifactPurpose, AttributeKey, AttributeValue, Attributes, BoundedString, ByteLength, ContentType, ErrorCode, EventId,
  EventName, FiniteF64, NamespacedName, RunId, Sha256Digest, SpanId, SpanName, Timestamp, ValidationError,
};
