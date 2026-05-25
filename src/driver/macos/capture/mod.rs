// TODO(driver-crates): temporary root compatibility while command handlers
// still produce root `DriverResponse` and runtime artifacts.
pub(crate) use auv_driver_macos::capture::{artifact, types};

pub(crate) mod commands;
pub(crate) mod xcap_backend;
