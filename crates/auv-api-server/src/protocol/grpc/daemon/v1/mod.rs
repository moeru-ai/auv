//! gRPC adapters for daemon-owned `auv.api.daemon.v1` services.

mod device;
mod discovery;
mod health;
mod pairing;
mod run;
mod runner;
mod runner_class;

pub(crate) use device::DeviceServiceGrpc;
pub(crate) use discovery::DiscoveryServiceGrpc;
pub(crate) use health::HealthServiceGrpc;
pub(crate) use pairing::PairingServiceGrpc;
pub(crate) use run::RunServiceGrpc;
pub(crate) use runner::RunnerServiceGrpc;
pub(crate) use runner_class::RunnerClassServiceGrpc;
