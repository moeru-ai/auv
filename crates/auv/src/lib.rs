//! Domain-facing local and remote operation interface for AUV.

pub mod client;
mod context;
pub mod devices;
pub mod discovery;
pub mod error;
pub mod pairing;
pub mod profile;
pub mod resource;
pub mod runners;
pub mod runs;
pub mod selection;
pub mod time;

pub use client::Client;
pub use context::{AuvContext, ContextError};

#[cfg(test)]
#[path = "resource_test.rs"]
mod resource_tests;
