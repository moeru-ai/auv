//! Shared conversions between Runner wire messages and driver domain values.
//!
//! Client and server adapters map conversion errors to their own error types.
//! Keeping these conversions here leaves generated protobuf types independent
//! of the driver domain and shares validation without new crate dependencies.

pub mod position;
