//! gRPC adapters for daemon-owned `auv.api.daemon.v1` services.

mod device;
mod discovery;
mod health;
mod pairing;
mod user_session;

pub(crate) use device::DeviceServiceGrpc;
pub(crate) use discovery::DiscoveryServiceGrpc;
pub(crate) use health::HealthServiceGrpc;
pub(crate) use pairing::PairingServiceGrpc;
pub(crate) use user_session::UserSessionServiceGrpc;
