//! gRPC protocol implementation for AUV daemon control and capability APIs.
//!
//! [`Client`] owns one daemon connection and its route-bound transports. The
//! modules under [`clients`] track package-shaped protobuf control services.
//! The business-facing local/remote hierarchy is exposed by [`crate::Client`].

mod client;
pub mod clients;

pub use client::{
  Client, ConnectEndpoint, EndpointParseError, PairedConnectConfig, PairedConnectError, ROUTE_DEVICE_METADATA, ROUTE_RUN_METADATA,
  ROUTE_RUNNER_CLASS_METADATA, RoutedTransport, RunnerRoute, RunnerRouteInterceptor,
};
