use super::{DEFAULT_MEMORY_TTL_MILLIS, VIEW_MEMORY_SCHEMA_VERSION, ViewMemory};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StaleReason {
  MemoryRejectedAtFreshness,
  SchemaMismatch,
  BaselineMismatchHard,
  // NOTICE(a4-min): produced only by reacquire(), not read_memory().
  RegionGoneAtReacquisition,
  ObservationFailedAtReacquisition,
}

impl StaleReason {
  pub const fn as_str(self) -> &'static str {
    match self {
      Self::MemoryRejectedAtFreshness => "memory_rejected_at_freshness",
      Self::SchemaMismatch => "schema_mismatch",
      Self::BaselineMismatchHard => "baseline_mismatch_hard",
      Self::RegionGoneAtReacquisition => "region_gone_at_reacquisition",
      Self::ObservationFailedAtReacquisition => "observation_failed_at_reacquisition",
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum MemoryReadOutcome {
  Accepted(ViewMemory),
  Rejected { reason: StaleReason },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MemoryReadConfig {
  pub now_millis: u64,
  pub hard_ttl_millis: u64,
  pub baseline_mismatch_tolerance_ratio: f64,
}

impl Default for MemoryReadConfig {
  fn default() -> Self {
    Self {
      now_millis: 0,
      hard_ttl_millis: DEFAULT_MEMORY_TTL_MILLIS,
      baseline_mismatch_tolerance_ratio: 0.25,
    }
  }
}

pub fn read_memory(memory: ViewMemory, config: &MemoryReadConfig, current_baseline_width: Option<u32>) -> MemoryReadOutcome {
  if memory.schema_version != VIEW_MEMORY_SCHEMA_VERSION {
    return MemoryReadOutcome::Rejected {
      reason: StaleReason::SchemaMismatch,
    };
  }

  if config.now_millis.saturating_sub(memory.last_reconstructed_at_millis) > config.hard_ttl_millis {
    return MemoryReadOutcome::Rejected {
      reason: StaleReason::MemoryRejectedAtFreshness,
    };
  }

  if let Some(current) = current_baseline_width {
    let saved = memory.scope_snapshot.baseline_width;
    if saved > 0 {
      let drift = (current as f64 - saved as f64).abs() / saved as f64;
      if drift > config.baseline_mismatch_tolerance_ratio {
        // NOTICE(a3-min-baseline-hard-reject): spec warns on drift; A3-min rejects load
        // so playlist select can fall back to rescan replay with a known_limits note.
        return MemoryReadOutcome::Rejected {
          reason: StaleReason::BaselineMismatchHard,
        };
      }
    }
  }

  MemoryReadOutcome::Accepted(memory)
}

#[cfg(test)]
#[path = "read_test.rs"]
mod tests;
