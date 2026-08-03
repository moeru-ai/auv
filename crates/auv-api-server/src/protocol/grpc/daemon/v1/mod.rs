//! gRPC adapters for daemon-owned `auv.api.daemon.v1` services.

mod device;
mod discovery;
mod pairing;
mod run;
mod runner;
mod runner_class;

pub(crate) use device::DeviceServiceGrpc;
pub(crate) use discovery::DiscoveryServiceGrpc;
pub(crate) use pairing::PairingServiceGrpc;
pub(crate) use run::RunServiceGrpc;
pub(crate) use runner::RunnerServiceGrpc;
pub(crate) use runner_class::RunnerClassServiceGrpc;

fn caller<T>(request: &tonic::Request<T>) -> Result<crate::auth::CallerId, tonic::Status> {
  request
    .extensions()
    .get::<crate::auth::CallerId>()
    .cloned()
    .ok_or_else(|| tonic::Status::internal("gRPC authentication interceptor omitted CallerId"))
}
