// File: src/driver/macos/mod.rs
use super::Driver;
use crate::model::{
  AuvResult, DriverCall, DriverDescriptor, DriverResponse, ProducedArtifact, now_millis,
};

mod ax_tree;
pub(crate) mod capture;
mod constants;
mod control;
mod descriptor;
mod dispatch;
mod native;
mod observe;
mod overlay;
mod support;
#[cfg(test)]
mod tests;

mod types;

pub(crate) use self::constants::*;
pub(crate) use self::support::*;
pub(crate) use self::types::*;

pub(crate) struct MacOsDesktopDriver;
