use super::{DEFAULT_MEMORY_TTL_MILLIS, VIEW_MEMORY_SCHEMA_VERSION, ViewMemory};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StaleReason {
  MemoryRejectedAtFreshness,
  SchemaMismatch,
  BaselineMismatchHard,
  // NOTICE(a4-min): produced only by reacquire(), not read_memory().
  RegionGoneAtReacquisition,
  ObservationFailedAtReacquisition,
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
mod tests {
  use super::ViewMemory;
  use super::*;
  use crate::ViewBounds;
  use crate::memory::{VIEW_MEMORY_SCHEMA_VERSION, ViewMemoryScopeSnapshot};

  fn sample_memory(last_millis: u64) -> ViewMemory {
    ViewMemory {
      schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
      memory_id: "app:scope".into(),
      app_bundle_id: "app".into(),
      scope_id: "scope".into(),
      last_reconstructed_at_millis: last_millis,
      source_run_id: "artifact-dir-bridge-a3".into(),
      source_reconstruction_ref: String::new(),
      anchors: Vec::new(),
      landmarks: Vec::new(),
      node_snapshots: Default::default(),
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: "playlist_sidebar".into(),
        region_bounds_window_local: ViewBounds::default(),
        baseline_width: 240,
        schema_version_view_ir: "view-ir-v0".into(),
      },
      diagnostics: Vec::new(),
    }
  }

  #[test]
  fn read_rejects_expired_memory() {
    let memory = sample_memory(1_000);
    let config = MemoryReadConfig {
      now_millis: 1_000 + DEFAULT_MEMORY_TTL_MILLIS + 1,
      ..Default::default()
    };
    match read_memory(memory, &config, None) {
      MemoryReadOutcome::Rejected {
        reason: StaleReason::MemoryRejectedAtFreshness,
      } => {}
      other => panic!("expected freshness rejection, got {other:?}"),
    }
  }

  #[test]
  fn read_rejects_schema_mismatch() {
    let mut memory = sample_memory(1_000);
    memory.schema_version = "view-memory-v99".into();
    let config = MemoryReadConfig {
      now_millis: 1_000,
      ..Default::default()
    };
    match read_memory(memory, &config, None) {
      MemoryReadOutcome::Rejected {
        reason: StaleReason::SchemaMismatch,
      } => {}
      other => panic!("expected schema rejection, got {other:?}"),
    }
  }

  #[test]
  fn read_rejects_baseline_mismatch() {
    let memory = sample_memory(1_000);
    let config = MemoryReadConfig {
      now_millis: 1_000,
      ..Default::default()
    };
    match read_memory(memory, &config, Some(400)) {
      MemoryReadOutcome::Rejected {
        reason: StaleReason::BaselineMismatchHard,
      } => {}
      other => panic!("expected baseline rejection, got {other:?}"),
    }
  }
}
