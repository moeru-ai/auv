use std::time::{SystemTime, UNIX_EPOCH};

use auv_tracing::ArtifactUri;
use auv_view::memory::{
  ARTIFACT_DIR_BRIDGE_RUN_ID, ATTR_REACQUIRE_FATAL_DIAGNOSTIC_KIND, ATTR_REACQUIRE_OBSERVATION_COUNT, ATTR_REACQUIRE_OUTCOME,
  ATTR_REACQUIRE_SCOPE_ID, ATTR_REACQUIRE_SKIPPED_RESCAN_REPLAY, ATTR_REACQUIRE_STAGE_USED, ATTR_REACQUIRE_TARGET_KIND, MemoryReadConfig,
  MemoryWriteInput, ReacquireConfig, ReacquireDriverAdapter, ReacquireOutcome, ReacquireTarget, StaleReason, ViewMemory,
  ViewMemoryScopeSnapshot, memory_file_path, outcome_label, reacquire, reacquire_stage_span_name, strategy_name, try_build_memory,
  write_memory_file,
};
use auv_view::{ParserDiagnostic, VIEW_IR_SCHEMA_VERSION, ViewBounds};
use serde::{Deserialize, Serialize};

use crate::{PlaylistSelectTarget, PlaylistSidebarScan};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaylistReacquireSummary {
  pub outcome: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub strategy_used: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub stale_reason: Option<String>,
  pub observation_count: usize,
  pub skipped_rescan_replay: bool,
}

/// NetEase-side trace evidence for controlled `view.reacquire.*` span emission (A8a).
#[derive(Clone, Debug, PartialEq)]
pub struct ReacquireTraceEvidence {
  pub scope_id: String,
  pub target_kind: String,
  pub outcome: String,
  pub stage_used: String,
  pub observation_count: usize,
  pub skipped_rescan_replay: bool,
  pub stale_reason: Option<String>,
  pub strategy_used: Option<String>,
}

impl ReacquireTraceEvidence {
  pub fn from_select_parts(scope_id: &str, target: &PlaylistSelectTarget, reacquire: Option<&PlaylistReacquireSummary>) -> Option<Self> {
    let summary = reacquire?;
    Some(Self {
      scope_id: scope_id.to_string(),
      target_kind: reacquire_target_kind(target).to_string(),
      outcome: summary.outcome.clone(),
      stage_used: summary.strategy_used.clone().unwrap_or_else(|| "none".to_string()),
      observation_count: summary.observation_count,
      skipped_rescan_replay: summary.skipped_rescan_replay,
      stale_reason: summary.stale_reason.clone(),
      strategy_used: summary.strategy_used.clone(),
    })
  }

  pub fn to_reacquire_root_attributes(&self) -> Vec<(String, String)> {
    let mut attrs = vec![
      (ATTR_REACQUIRE_SCOPE_ID.to_string(), self.scope_id.clone()),
      (ATTR_REACQUIRE_TARGET_KIND.to_string(), self.target_kind.clone()),
      (ATTR_REACQUIRE_OUTCOME.to_string(), self.outcome.clone()),
      (ATTR_REACQUIRE_STAGE_USED.to_string(), self.stage_used.clone()),
      (ATTR_REACQUIRE_OBSERVATION_COUNT.to_string(), self.observation_count.to_string()),
      (ATTR_REACQUIRE_SKIPPED_RESCAN_REPLAY.to_string(), self.skipped_rescan_replay.to_string()),
    ];
    if self.outcome == "not_found" {
      if let Some(reason) = &self.stale_reason {
        attrs.push((ATTR_REACQUIRE_FATAL_DIAGNOSTIC_KIND.to_string(), reason.clone()));
      }
    }
    attrs
  }

  /// NOTICE(a8-controlled-subset): only the winning stage span is emitted in A8 v1.
  pub fn winning_stage_span_name(&self) -> Option<String> {
    let strategy = self.strategy_used.as_deref()?;
    let stage = reacquire_strategy_stage_index(strategy)?;
    Some(reacquire_stage_span_name(stage, strategy))
  }
}

fn reacquire_target_kind(target: &PlaylistSelectTarget) -> &'static str {
  if target.anchor_id.is_some() {
    "anchor"
  } else {
    "label"
  }
}

// NOTICE(a8-controlled-subset): stage index mapping aligns with anchor-reacquisition-v0
// cascade ordering; only the winning stage span is recorded in A8 v1.
fn reacquire_strategy_stage_index(strategy: &str) -> Option<u8> {
  match strategy {
    "direct_id" => Some(1),
    "label_current_viewport" => Some(3),
    "viewport_fingerprint" => Some(4),
    "label_plus_section" => Some(5),
    _ => None,
  }
}

#[derive(Clone, Debug, PartialEq)]
pub enum PlaylistReacquireAttempt {
  Hit {
    bounds: ViewBounds,
    summary: PlaylistReacquireSummary,
  },
  Stale {
    summary: PlaylistReacquireSummary,
  },
  Miss {
    summary: PlaylistReacquireSummary,
  },
}

pub const PLAYLIST_SIDEBAR_SCOPE_ID: &str = "playlist_sidebar";
pub const PLAYLIST_SCAN_CACHE_FILE_NAME: &str = "playlist-scan-cache.json";

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

pub(crate) fn write_from_scan_when_enabled(enabled: bool, inputs: &crate::Inputs, scan: &PlaylistSidebarScan) -> Result<(), String> {
  if !enabled {
    return Ok(());
  }

  let reconstruction = scan.reconstruction();
  let sidebar_bounds = scan.sidebar_region().bounds.unwrap_or_else(|| ViewBounds::new(0.0, 0.0, 240.0, 400.0));
  let baseline_width = sidebar_bounds.width.round().max(1.0) as u32;
  let memory = try_build_memory(
    MemoryWriteInput {
      app_bundle_id: &inputs.app_id,
      scope_id: PLAYLIST_SIDEBAR_SCOPE_ID,
      root: &reconstruction.root,
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: PLAYLIST_SIDEBAR_SCOPE_ID.to_string(),
        region_bounds_window_local: sidebar_bounds,
        baseline_width,
        schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
      },
      source_reconstruction_ref: PLAYLIST_SCAN_CACHE_FILE_NAME.to_string(),
      source_run_id: ARTIFACT_DIR_BRIDGE_RUN_ID.to_string(),
      last_reconstructed_at_millis: system_time_millis(),
      clean: diagnostics_allow_memory_write(scan.diagnostics()),
    },
    reconstruction,
  )
  .ok_or_else(|| "scan did not produce writable ViewMemory".to_string())?;

  let path = memory_file_path(&inputs.artifact_dir, PLAYLIST_SIDEBAR_SCOPE_ID);
  write_memory_file(&path, &memory)
}

pub fn write_from_scan(inputs: &crate::Inputs, scan: &PlaylistSidebarScan) -> Result<(), String> {
  write_from_scan_when_enabled(enabled(), inputs, scan)
}

pub fn write_from_scan_with_lineage(inputs: &crate::Inputs, scan: &PlaylistSidebarScan, scan_uri: &ArtifactUri) -> Result<(), String> {
  let memory = try_build_writable_memory(inputs, scan, scan_uri).ok_or_else(|| "scan did not produce writable ViewMemory".to_string())?;
  write_memory_mirror(inputs, &memory)
}

pub(crate) fn try_build_writable_memory(inputs: &crate::Inputs, scan: &PlaylistSidebarScan, scan_uri: &ArtifactUri) -> Option<ViewMemory> {
  let reconstruction = scan.reconstruction();
  let sidebar_bounds = scan.sidebar_region().bounds.unwrap_or_else(|| ViewBounds::new(0.0, 0.0, 240.0, 400.0));
  let baseline_width = sidebar_bounds.width.round().max(1.0) as u32;
  try_build_memory(
    MemoryWriteInput {
      app_bundle_id: &inputs.app_id,
      scope_id: PLAYLIST_SIDEBAR_SCOPE_ID,
      root: &reconstruction.root,
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: PLAYLIST_SIDEBAR_SCOPE_ID.to_string(),
        region_bounds_window_local: sidebar_bounds,
        baseline_width,
        schema_version_view_ir: VIEW_IR_SCHEMA_VERSION.to_string(),
      },
      source_reconstruction_ref: scan_uri.to_string(),
      source_run_id: scan_uri.run_id().to_string(),
      last_reconstructed_at_millis: system_time_millis(),
      clean: diagnostics_allow_memory_write(scan.diagnostics()),
    },
    reconstruction,
  )
}

pub(crate) fn write_memory_mirror(inputs: &crate::Inputs, memory: &ViewMemory) -> Result<(), String> {
  let path = memory_file_path(&inputs.artifact_dir, PLAYLIST_SIDEBAR_SCOPE_ID);
  write_memory_file(&path, memory)
}

pub fn load_memory_raw(inputs: &crate::Inputs) -> Option<ViewMemory> {
  if !enabled() {
    return None;
  }
  crate::recording::try_load_view_memory(inputs)
}

pub fn try_reacquire_playlist_target(
  memory: &ViewMemory,
  target: &PlaylistSelectTarget,
  adapter: &mut dyn ReacquireDriverAdapter,
  read_config: &MemoryReadConfig,
  current_baseline_width: Option<u32>,
) -> PlaylistReacquireAttempt {
  let reacquire_target = ReacquireTarget::LabelWithSection {
    label: target.label.clone(),
    section_hint: Some(target.section_kind.domain_kind().to_string()),
  };
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
  summary_from_outcome(outcome)
}

fn summary_from_outcome(outcome: ReacquireOutcome) -> PlaylistReacquireAttempt {
  let outcome_label_str = outcome_label(&outcome).to_string();
  match outcome {
    ReacquireOutcome::Reacquired {
      node,
      strategy_used,
      observation_count,
      ..
    } => PlaylistReacquireAttempt::Hit {
      bounds: node.bounds,
      summary: PlaylistReacquireSummary {
        outcome: outcome_label_str,
        strategy_used: Some(strategy_name(strategy_used).to_string()),
        stale_reason: None,
        observation_count,
        skipped_rescan_replay: true,
      },
    },
    ReacquireOutcome::Stale {
      reason,
      observation_count,
      ..
    } => PlaylistReacquireAttempt::Stale {
      summary: PlaylistReacquireSummary {
        outcome: outcome_label_str,
        strategy_used: None,
        stale_reason: Some(stale_reason_wire(reason).to_string()),
        observation_count,
        skipped_rescan_replay: false,
      },
    },
    ReacquireOutcome::NotFound {
      observation_count, ..
    } => PlaylistReacquireAttempt::Miss {
      summary: PlaylistReacquireSummary {
        outcome: outcome_label_str,
        strategy_used: None,
        stale_reason: None,
        observation_count,
        skipped_rescan_replay: false,
      },
    },
  }
}

fn stale_reason_wire(reason: StaleReason) -> &'static str {
  match reason {
    StaleReason::MemoryRejectedAtFreshness => "memory_rejected_at_freshness",
    StaleReason::SchemaMismatch => "schema_mismatch",
    StaleReason::BaselineMismatchHard => "baseline_mismatch_hard",
    StaleReason::RegionGoneAtReacquisition => "region_gone_at_reacquisition",
    StaleReason::ObservationFailedAtReacquisition => "observation_failed_at_reacquisition",
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::SidebarSectionKind;
  use crate::view_parsers::sidebar::reconstruct::reconstruct_playlist_sidebar;
  use crate::view_parsers::sidebar::test_support::fake_recognition;
  use crate::{ScanAppContext, ScanWindowContext, ViewRegionRecord, parse_sidebar_viewport};
  use auv_view::memory::parse_memory_file;
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
      memory_id: "com.netease.163music:playlist_sidebar".into(),
      app_bundle_id: "com.netease.163music".into(),
      scope_id: PLAYLIST_SIDEBAR_SCOPE_ID.into(),
      last_reconstructed_at_millis: 1_719_744_000_000,
      source_run_id: ARTIFACT_DIR_BRIDGE_RUN_ID.into(),
      source_reconstruction_ref: PLAYLIST_SCAN_CACHE_FILE_NAME.into(),
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

  fn minimal_writable_scan_json(diagnostics: serde_json::Value) -> String {
    serde_json::json!({
      "schema_version": VIEW_IR_SCHEMA_VERSION,
      "app": {},
      "window": {},
      "sidebar_region": {
        "bounds": {"x": 0.0, "y": 220.0, "width": 240.0, "height": 400.0}
      },
      "observations": [],
      "reconstruction": {
        "root": {
          "id": "root.sidebar",
          "kind": "collection",
          "bounds": {"x": 0.0, "y": 0.0, "width": 240.0, "height": 400.0},
          "anchors": [],
          "landmarks": [],
          "actions": [],
          "evidence": [],
          "children": [{
            "id": "item.test",
            "kind": "item",
            "label": "Test Playlist",
            "bounds": {"x": 32.0, "y": 74.0, "width": 120.0, "height": 20.0},
            "anchors": [{
              "id": "anchor.test",
              "label": "Test Playlist",
              "strength": "strong",
              "bounds": {"x": 32.0, "y": 74.0, "width": 120.0, "height": 20.0},
              "evidence_ids": []
            }],
            "landmarks": [],
            "actions": [],
            "evidence": [],
            "children": []
          }]
        },
        "anchor_index": [{
          "id": "anchor.test",
          "label": "Test Playlist",
          "strength": "strong",
          "bounds": {"x": 32.0, "y": 74.0, "width": 120.0, "height": 20.0},
          "evidence_ids": []
        }],
        "landmark_index": []
      },
      "projection": {"sections": []},
      "boundary": {
        "top": "unknown",
        "bottom": "unknown",
        "left": "unknown",
        "right": "unknown"
      },
      "diagnostics": diagnostics,
      "known_limits": []
    })
    .to_string()
  }

  fn decode_minimal_scan(diagnostics: serde_json::Value) -> PlaylistSidebarScan {
    crate::decode_playlist_sidebar_scan_json(&minimal_writable_scan_json(diagnostics)).expect("minimal synthetic scan should decode")
  }

  fn reconstructed_dedup_only_scan() -> PlaylistSidebarScan {
    let page0 = parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    );
    let page1 =
      parse_sidebar_viewport(1, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("Coding BGM", 32.0, 42.0, 120.0, 20.0)]));

    reconstruct_playlist_sidebar(ScanAppContext::default(), ScanWindowContext::default(), ViewRegionRecord::default(), vec![page0, page1])
  }

  #[test]
  fn write_from_scan_when_enabled_allows_reconstructed_dedup_only_scan() {
    let scan = reconstructed_dedup_only_scan();
    assert!(scan.diagnostics().iter().any(|diagnostic| diagnostic.code == "deduplicated_item"));
    assert!(diagnostics_allow_memory_write(scan.diagnostics()));

    let artifact_dir = std::env::temp_dir().join(format!("auv-netease-view-memory-reconstruct-test-{}", std::process::id()));
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let inputs = crate::Inputs {
      artifact_dir,
      ..crate::Inputs::with_defaults()
    };

    write_from_scan_when_enabled(true, &inputs, &scan).expect("reconstructed dedup-only scan should write");

    let path = memory_file_path(&inputs.artifact_dir, PLAYLIST_SIDEBAR_SCOPE_ID);
    let memory = parse_memory_file(&path).expect("memory file should parse");
    assert!(memory.diagnostics.is_empty());
    assert_eq!(memory.app_bundle_id, inputs.app_id);
    assert_eq!(memory.scope_id, PLAYLIST_SIDEBAR_SCOPE_ID);
    let _ = std::fs::remove_dir_all(&inputs.artifact_dir);
  }

  #[test]
  fn write_from_scan_when_enabled_no_op_when_disabled() {
    let scan = reconstructed_dedup_only_scan();
    let artifact_dir = std::env::temp_dir().join(format!("auv-netease-view-memory-disabled-test-{}", std::process::id()));
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let inputs = crate::Inputs {
      artifact_dir: artifact_dir.clone(),
      ..crate::Inputs::with_defaults()
    };

    write_from_scan_when_enabled(false, &inputs, &scan).expect("disabled write is no-op");

    let path = memory_file_path(&artifact_dir, PLAYLIST_SIDEBAR_SCOPE_ID);
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(&artifact_dir);
  }

  #[test]
  fn write_from_scan_when_enabled_rejects_mixed_diagnostics() {
    let diagnostics = serde_json::json!([
      {
        "code": "deduplicated_item",
        "message": "dedup",
        "node_id": "item.test"
      },
      {
        "code": "parser_no_reliable_candidates",
        "message": "blocking",
        "node_id": null
      }
    ]);
    let scan = decode_minimal_scan(diagnostics);
    assert!(!diagnostics_allow_memory_write(scan.diagnostics()));

    let artifact_dir = std::env::temp_dir().join(format!("auv-netease-view-memory-mixed-test-{}", std::process::id()));
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let inputs = crate::Inputs {
      artifact_dir,
      ..crate::Inputs::with_defaults()
    };

    let error = write_from_scan_when_enabled(true, &inputs, &scan).expect_err("mixed diagnostics should not write");
    assert!(error.contains("scan did not produce writable ViewMemory"));
    let path = memory_file_path(&inputs.artifact_dir, PLAYLIST_SIDEBAR_SCOPE_ID);
    assert!(!path.exists());
    let _ = std::fs::remove_dir_all(&inputs.artifact_dir);
  }

  #[test]
  fn write_from_scan_when_enabled_allows_fallback_diagnostics() {
    let diagnostics = serde_json::json!([
      {
        "code": "deduplicated_item",
        "message": "deduplicated repeated sidebar item \"44\"",
        "node_id": "obs1.candidate.ocr3.44"
      },
      {
        "code": "sidebar_region_not_found",
        "message": "sidebar markers were not recognized after default screen restore",
        "node_id": null
      },
      {
        "code": "sidebar_region_fallback_used",
        "message": "using conservative playlist sidebar bounds",
        "node_id": null
      }
    ]);
    let scan = decode_minimal_scan(diagnostics);
    assert!(diagnostics_allow_memory_write(scan.diagnostics()));

    let artifact_dir = std::env::temp_dir().join(format!("auv-netease-view-memory-fallback-test-{}", std::process::id()));
    std::fs::create_dir_all(&artifact_dir).expect("artifact dir");
    let inputs = crate::Inputs {
      artifact_dir,
      ..crate::Inputs::with_defaults()
    };

    write_from_scan_when_enabled(true, &inputs, &scan).expect("fallback diagnostics should write");

    let path = memory_file_path(&inputs.artifact_dir, PLAYLIST_SIDEBAR_SCOPE_ID);
    let memory = parse_memory_file(&path).expect("memory file should parse");
    assert_eq!(memory.scope_id, PLAYLIST_SIDEBAR_SCOPE_ID);
    assert!(!memory.node_snapshots.is_empty());
    let _ = std::fs::remove_dir_all(&inputs.artifact_dir);
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
      PlaylistReacquireAttempt::Hit { summary, .. } => {
        assert!(summary.skipped_rescan_replay);
        assert_eq!(summary.strategy_used.as_deref(), Some("label_current_viewport"));
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
      PlaylistReacquireAttempt::Miss { summary, .. } => {
        assert!(!summary.skipped_rescan_replay);
        assert_eq!(summary.outcome, "not_found");
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
      PlaylistReacquireAttempt::Stale { summary } => {
        assert!(!summary.skipped_rescan_replay);
        assert_eq!(summary.stale_reason.as_deref(), Some("memory_rejected_at_freshness"));
      }
      other => panic!("expected stale memory, got {other:?}"),
    }
  }
}
