//! Typed clap declarations for the built-in root commands.

pub mod api_server;
pub mod device_local;
pub mod devices;
pub mod doctor;
pub mod invoke;
pub mod mcp;
pub mod plugin;
pub mod run;
pub mod runner;
pub mod serve;
pub mod setup;
#[cfg(windows)]
pub mod windows_service;
