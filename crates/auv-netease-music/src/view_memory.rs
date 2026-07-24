//! NetEase playlist view-memory integration.
//!
//! Reacquisition results remain domain data, but the retired raw trace
//! evidence adapter is not part of this module's public API:
//!
//! ```compile_fail
//! use auv_netease_music::view_memory::ReacquireTraceEvidence;
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use auv_tracing::ArtifactUri;
use auv_view::ParserDiagnostic;
use auv_view::VIEW_IR_SCHEMA_VERSION;
use auv_view::ViewBounds;
use auv_view::memory::{
  MemoryReadConfig, ReacquireConfig, ReacquireDriverAdapter, ReacquireOutcome, ReacquireStrategy, ReacquireTarget, StaleReason, ViewMemory,
  reacquire,
};
use auv_view::memory::{MemoryWriteInput, ViewMemoryScopeSnapshot, try_build_memory};
use serde::{Deserialize, Serialize};

use crate::PlaylistSelectTarget;
use crate::PlaylistSidebarScan;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PlaylistReacquireResult {
  Reacquired {
    bounds: ViewBounds,
    strategy: ReacquireStrategy,
    observation_count: usize,
  },
  Stale {
    reason: StaleReason,
    observation_count: usize,
  },
  NotFound {
    observation_count: usize,
  },
}

pub const PLAYLIST_SIDEBAR_SCOPE_ID: &str = "playlist_sidebar";

pub fn enabled() -> bool {
  enabled_with_env(std::env::var("AUV_NETEASE_VIEW_MEMORY").ok().as_deref())
}

pub(crate) fn enabled_with_env(value: Option<&str>) -> bool {
  matches!(value, Some("1"))
}

pub fn system_time_millis() -> u64 {
  SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis() as u64).unwrap_or(0)
}

// NOTICE: NetEase ViewMemory write treats selected diagnostics as non-blocking
// when reconstruction is still trustworthy: `deduplicated_item` (A6c-3) and the
// paired sidebar fallback path (`sidebar_region_not_found` only when
// `sidebar_region_fallback_used` is present). Any other diagnostic still blocks.
fn diagnostics_allow_memory_write(diagnostics: &[ParserDiagnostic]) -> bool {
  if diagnostics.is_empty() {
    return true;
  }
  let used_fallback = diagnostics.iter().any(|diagnostic| diagnostic.code == "sidebar_region_fallback_used");
  diagnostics.iter().all(|diagnostic| match diagnostic.code.as_str() {
    "deduplicated_item" | "sidebar_region_fallback_used" => true,
    "sidebar_region_not_found" if used_fallback => true,
    _ => false,
  })
}

pub(crate) fn try_build_writable_memory(
  inputs: &crate::Inputs,
  scan: &PlaylistSidebarScan,
  source_scan_uri: &ArtifactUri,
) -> Option<ViewMemory> {
  let reconstruction = scan.reconstruction();
  let sidebar_bounds = scan.sidebar_region().bounds.unwrap_or_else(|| ViewBounds::new(0.0, 0.0, 240.0, 400.0));
  let baseline_width = sidebar_bounds.width.round().max(1.0) as u32;
  try_build_memory(
    MemoryWriteInput {
      source_scan_uri: source_scan_uri.clone(),
      app_bundle_id: &inputs.app_id,
      scope_id: PLAYLIST_SIDEBAR_SCOPE_ID,
      root: &reconstruction.root,
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: PLAYLIST_SIDEBAR_SCOPE_ID.to_string(),
        region_bounds_window_local: sidebar_bounds,
        baseline_width,
        schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
      },
      last_reconstructed_at_millis: system_time_millis(),
      clean: diagnostics_allow_memory_write(scan.diagnostics()),
    },
    reconstruction,
  )
}

pub fn try_reacquire_playlist_target(
  memory: &ViewMemory,
  target: &PlaylistSelectTarget,
  adapter: &mut dyn ReacquireDriverAdapter,
  read_config: &MemoryReadConfig,
  current_baseline_width: Option<u32>,
) -> PlaylistReacquireResult {
  let reacquire_target = ReacquireTarget::LabelWithSection {
    label: target.label.clone(),
    section_hint: Some(target.section_kind.domain_kind().to_string()),
  };
  // TODO(view-memory-reacquire-tracing): typed span/event emission is deferred
  // because this migration only removes the unvalidated string-pair adapter;
  // reopen after an owner-approved event schema has a concrete Inspect reader.
  let outcome = reacquire(
    memory,
    reacquire_target,
    adapter,
    &ReacquireConfig {
      max_scroll_attempts: 5,
      memory_read: Some(read_config.clone()),
      current_baseline_width,
    },
  );
  result_from_outcome(outcome)
}

fn result_from_outcome(outcome: ReacquireOutcome) -> PlaylistReacquireResult {
  match outcome {
    ReacquireOutcome::Reacquired {
      node,
      strategy_used,
      observation_count,
      ..
    } => PlaylistReacquireResult::Reacquired {
      bounds: node.bounds,
      strategy: strategy_used,
      observation_count,
    },
    ReacquireOutcome::Stale {
      reason,
      observation_count,
      ..
    } => PlaylistReacquireResult::Stale {
      reason,
      observation_count,
    },
    ReacquireOutcome::NotFound {
      observation_count, ..
    } => PlaylistReacquireResult::NotFound { observation_count },
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::SidebarSectionKind;
  use auv_tracing::{ArtifactId, ArtifactUri, RunId};
  use auv_view::memory::{ReacquireCandidate, ReacquireObservation, VIEW_MEMORY_SCHEMA_VERSION, ViewMemoryScopeSnapshot};
  use auv_view::{ParserDiagnostic, ViewBounds};

  struct FakeAdapter {
    observations: Vec<ReacquireObservation>,
    cursor: usize,
  }

  impl ReacquireDriverAdapter for FakeAdapter {
    fn observe_viewport(&mut self) -> Result<ReacquireObservation, ParserDiagnostic> {
      self
        .observations
        .get(self.cursor)
        .cloned()
        .map(|observation| {
          self.cursor += 1;
          observation
        })
        .ok_or_else(|| ParserDiagnostic {
          code: "no_observation".into(),
          message: "fake adapter exhausted".into(),
          node_id: None,
        })
    }

    fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
      Ok(())
    }

    fn scroll_up(&mut self) -> Result<(), ParserDiagnostic> {
      Ok(())
    }
  }

  fn sample_memory() -> ViewMemory {
    ViewMemory {
      schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
      source_scan_uri: ArtifactUri::from_ids(RunId::new(), ArtifactId::new()),
      memory_id: "com.netease.163music:playlist_sidebar".into(),
      app_bundle_id: "com.netease.163music".into(),
      scope_id: PLAYLIST_SIDEBAR_SCOPE_ID.into(),
      last_reconstructed_at_millis: 1_719_744_000_000,
      anchors: Vec::new(),
      landmarks: Vec::new(),
      node_snapshots: Default::default(),
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: PLAYLIST_SIDEBAR_SCOPE_ID.into(),
        region_bounds_window_local: ViewBounds::new(0.0, 220.0, 346.0, 720.0),
        baseline_width: 346,
        schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
      },
      diagnostics: Vec::new(),
    }
  }

  fn road_trip_target() -> PlaylistSelectTarget {
    PlaylistSelectTarget {
      label: "Road Trip".into(),
      section_id: "section.favorite_playlists".into(),
      section_kind: SidebarSectionKind::FavoritePlaylists,
      item_id: "item.road-trip".into(),
      anchor_id: None,
      candidate_id: Some("item.road-trip".into()),
      observation_index: Some(0),
      bounds: Some(ViewBounds::new(32.0, 106.0, 120.0, 20.0)),
    }
  }

  #[test]
  fn diagnostics_allow_memory_write_cases() {
    let cases = [
      (vec![], true),
      (
        vec![ParserDiagnostic {
          code: "deduplicated_item".into(),
          message: "dedup".into(),
          node_id: Some("item.test".into()),
        }],
        true,
      ),
      (
        vec![
          ParserDiagnostic {
            code: "deduplicated_item".into(),
            message: "dedup a".into(),
            node_id: None,
          },
          ParserDiagnostic {
            code: "deduplicated_item".into(),
            message: "dedup b".into(),
            node_id: None,
          },
        ],
        true,
      ),
      (
        vec![
          ParserDiagnostic {
            code: "deduplicated_item".into(),
            message: "dedup".into(),
            node_id: None,
          },
          ParserDiagnostic {
            code: "parser_no_reliable_candidates".into(),
            message: "mixed".into(),
            node_id: None,
          },
        ],
        false,
      ),
      (
        vec![ParserDiagnostic {
          code: "sidebar_region_not_found".into(),
          message: "blocking".into(),
          node_id: None,
        }],
        false,
      ),
    ];

    for (diagnostics, expected) in cases {
      assert_eq!(diagnostics_allow_memory_write(&diagnostics), expected, "diagnostics={diagnostics:?}");
    }
  }

  #[test]
  fn diagnostics_allow_memory_write_allows_fallback_pair() {
    let diagnostics = vec![
      ParserDiagnostic {
        code: "deduplicated_item".into(),
        message: "dedup".into(),
        node_id: Some("item.3".into()),
      },
      ParserDiagnostic {
        code: "sidebar_region_not_found".into(),
        message: "markers missing after restore".into(),
        node_id: None,
      },
      ParserDiagnostic {
        code: "sidebar_region_fallback_used".into(),
        message: "using conservative playlist sidebar bounds".into(),
        node_id: None,
      },
    ];
    assert!(diagnostics_allow_memory_write(&diagnostics));
  }

  #[test]
  fn diagnostics_allow_memory_write_rejects_unpaired_sidebar_region_not_found() {
    let diagnostics = vec![ParserDiagnostic {
      code: "sidebar_region_not_found".into(),
      message: "blocking".into(),
      node_id: None,
    }];
    assert!(!diagnostics_allow_memory_write(&diagnostics));
  }

  #[test]
  fn enabled_with_env_requires_exact_value() {
    assert!(!enabled_with_env(None));
    assert!(!enabled_with_env(Some("0")));
    assert!(!enabled_with_env(Some("true")));
    assert!(enabled_with_env(Some("1")));
  }

  #[test]
  fn playlist_select_uses_reacquire_when_memory_hit() {
    let memory = sample_memory();
    let target = road_trip_target();
    let mut adapter = FakeAdapter {
      observations: vec![ReacquireObservation {
        fingerprint: "favorite".into(),
        candidates: vec![ReacquireCandidate {
          node_id: Some("item.road-trip".into()),
          label: "Road Trip".into(),
          section_hint: Some("netease.favorite_playlists".into()),
          bounds: ViewBounds::new(32.0, 106.0, 120.0, 20.0),
        }],
      }],
      cursor: 0,
    };

    let attempt = try_reacquire_playlist_target(
      &memory,
      &target,
      &mut adapter,
      &MemoryReadConfig {
        now_millis: memory.last_reconstructed_at_millis,
        ..Default::default()
      },
      Some(memory.scope_snapshot.baseline_width),
    );

    match attempt {
      PlaylistReacquireResult::Reacquired { strategy, .. } => {
        assert_eq!(strategy, ReacquireStrategy::LabelCurrentViewport);
      }
      other => panic!("expected reacquire hit, got {other:?}"),
    }
  }

  #[test]
  fn playlist_select_reacquire_miss_when_viewport_empty_candidates() {
    let memory = sample_memory();
    let target = road_trip_target();
    let mut adapter = FakeAdapter {
      observations: vec![ReacquireObservation {
        fingerprint: "empty".into(),
        candidates: vec![],
      }],
      cursor: 0,
    };

    let attempt = try_reacquire_playlist_target(
      &memory,
      &target,
      &mut adapter,
      &MemoryReadConfig {
        now_millis: memory.last_reconstructed_at_millis,
        ..Default::default()
      },
      Some(memory.scope_snapshot.baseline_width),
    );

    match attempt {
      PlaylistReacquireResult::NotFound { observation_count } => {
        assert_eq!(observation_count, 1);
      }
      other => panic!("expected reacquire miss, got {other:?}"),
    }
  }

  #[test]
  fn playlist_select_falls_back_on_stale_memory() {
    let mut memory = sample_memory();
    memory.last_reconstructed_at_millis = 1_000;
    let target = road_trip_target();
    let mut adapter = FakeAdapter {
      observations: vec![],
      cursor: 0,
    };

    let attempt = try_reacquire_playlist_target(
      &memory,
      &target,
      &mut adapter,
      &MemoryReadConfig {
        now_millis: 1_000 + auv_view::memory::DEFAULT_MEMORY_TTL_MILLIS + 1,
        ..Default::default()
      },
      Some(memory.scope_snapshot.baseline_width),
    );

    match attempt {
      PlaylistReacquireResult::Stale {
        reason,
        observation_count,
      } => {
        assert_eq!(reason, StaleReason::MemoryRejectedAtFreshness);
        assert_eq!(observation_count, 0);
      }
      other => panic!("expected stale memory, got {other:?}"),
    }
  }
}
