//! Shared status for persisted semantic, witness, and quality stages.
//!
//! Query outcomes and action readiness use their own domain contracts. Stage
//! reasons, lineage, and vertical-specific policy remain in the producing crate.

use std::fmt;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StageStatus {
  Ready,
  Blocked,
  Failed,
}

impl StageStatus {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Ready => "ready",
      Self::Blocked => "blocked",
      Self::Failed => "failed",
    }
  }
}

impl fmt::Display for StageStatus {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.write_str(self.as_str())
  }
}
