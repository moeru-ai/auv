//! Root compatibility re-exports for durable recording primitives.
//!
//! NOTICE(recording-handle-root-methods): the pre-extraction root-specific
//! `RecordingHandle` helpers such as inspect and candidate-action facades are
//! intentionally not preserved on this re-exported handle. This project is still
//! pre-public, and keeping those inherent methods would require a root wrapper
//! that re-couples the extracted recording crate to root-only behavior. Use
//! `Runtime` for root read/command facades, or `RecordingHandle` directly for
//! durable run recording. Restore a wrapper only if the owner approves a public
//! compatibility slice.

pub use auv_tracing_driver::recording::*;
