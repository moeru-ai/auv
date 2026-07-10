use serde::{Deserialize, Serialize};

use crate::error::DriverResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
#[serde(rename_all = "snake_case")]
pub enum PlatformKind {
  Macos,
  Windows,
  Linux,
  Android,
  Ios,
  Browser,
  Fixture,
  Remote,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DriverDescriptor {
  pub id: &'static str,
  pub platform: PlatformKind,
  pub summary: &'static str,
}

pub trait Driver {
  type Session: DriverSession;

  fn descriptor(&self) -> DriverDescriptor;

  fn open_local(&self) -> DriverResult<Self::Session>;
}

pub trait DriverSession {
  fn descriptor(&self) -> DriverDescriptor;
}
