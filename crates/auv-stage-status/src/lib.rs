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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn as_str_covers_all_labels() {
    assert_eq!(StageStatus::Ready.as_str(), "ready");
    assert_eq!(StageStatus::Blocked.as_str(), "blocked");
    assert_eq!(StageStatus::Failed.as_str(), "failed");
  }

  #[test]
  fn display_matches_as_str() {
    assert_eq!(StageStatus::Ready.to_string(), "ready");
    assert_eq!(StageStatus::Blocked.to_string(), "blocked");
    assert_eq!(StageStatus::Failed.to_string(), "failed");
  }

  #[test]
  fn serde_roundtrip_preserves_wire_labels() {
    for (status, wire) in [
      (StageStatus::Ready, "\"ready\""),
      (StageStatus::Blocked, "\"blocked\""),
      (StageStatus::Failed, "\"failed\""),
    ] {
      assert_eq!(serde_json::to_string(&status).expect("serialize"), wire);
      let decoded: StageStatus = serde_json::from_str(wire).expect("deserialize");
      assert_eq!(decoded, status);
    }
  }
}
