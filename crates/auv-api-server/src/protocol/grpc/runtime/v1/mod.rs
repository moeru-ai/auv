//! gRPC adapters for daemon-served `auv.api.runtime.v1` services.

mod run;
mod runner;
mod runner_class;

pub(crate) use run::RunServiceGrpc;
pub(crate) use runner::RunnerServiceGrpc;
pub(crate) use runner_class::RunnerClassServiceGrpc;
