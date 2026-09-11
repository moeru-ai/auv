//! Transport-aware Rust client for AUV control and capability APIs.
//!
//! This crate owns explicit API transport and wire-service clients. The
//! domain-facing local/remote facade lives in the `auv` crate.

pub mod protocol;

pub use protocol::grpc::{
  ConnectEndpoint, EndpointParseError, PairedConnectConfig, PairedConnectError, RoutedTransport, RunnerRoute, RunnerRouteInterceptor,
};
