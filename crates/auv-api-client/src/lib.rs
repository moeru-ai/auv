//! Transport-aware Rust client for AUV control and capability APIs.
//!
//! This crate owns explicit API transport and wire-service clients. The
//! domain-facing local/remote facade lives in the `auv-core` package.

pub mod protocol;

#[cfg(any(unix, windows))]
pub mod device_local;

pub use protocol::grpc::{
  ConnectEndpoint, EndpointParseError, PairedConnectConfig, PairedConnectError, RoutedTransport, RunnerRoute, RunnerRouteInterceptor,
};
