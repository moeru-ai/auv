//! Donor-neutral inspect read wire types for ViewParser proof surfaces (A8b extractors).

use serde::{Deserialize, Serialize};

use super::ViewMemory;

/// Artifact role for playlist-select durable proof JSON.
///
/// NOTICE: role string must match the NetEase producer constant in the same slice.
pub const PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE: &str = "netease-playlist-select-result";

/// Minimal read wire for `netease-playlist-select-result` artifact JSON (A8b).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewParserSelectResultWire {
  pub run_id: Option<String>,
  pub query: String,
  pub target: ViewParserSelectTargetWire,
  pub steps: Vec<ViewParserSelectStepWire>,
  pub verification: ViewParserSelectVerificationWire,
  pub reacquire: Option<ViewParserReacquireWire>,
  #[serde(default)]
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewParserSelectTargetWire {
  pub label: String,
  pub section_kind: String,
  #[serde(default)]
  pub anchor_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewParserSelectStepWire {
  pub name: String,
  #[serde(default)]
  pub target_bounds: Option<crate::ViewBounds>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewParserSelectVerificationWire {
  pub status: String,
  pub method: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ViewParserReacquireWire {
  pub outcome: String,
  #[serde(default)]
  pub strategy_used: Option<String>,
  #[serde(default)]
  pub stale_reason: Option<String>,
  pub observation_count: usize,
  pub skipped_rescan_replay: bool,
}

/// Reacquisition span record — extracted from `view.reacquire.<scope_id>` root spans.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReacquisitionRecord {
  pub span_name: String,
  pub scope_id: String,
  pub target_kind: String,
  pub outcome: String,
  pub stage_used: String,
  pub observation_count: usize,
  pub skipped_rescan_replay: Option<bool>,
  pub stale_reason: Option<String>,
  pub strategy_used: Option<String>,
}

/// Tier I durable identity keys for inspect proof (A8c).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct IdentityProofSummary {
  pub label: String,
  pub section_kind: String,
  pub anchor_id: Option<String>,
}

/// Tier II memory / freshness proof (A8c).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MemoryProofSummary {
  pub present: bool,
  pub memory_id: Option<String>,
  pub source_run_id: Option<String>,
  pub last_reconstructed_at_millis: Option<u64>,
  pub anchor_count: Option<usize>,
}

/// Tier III reacquire resolution proof (A8c).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ResolutionProofSummary {
  pub outcome: String,
  pub strategy_used: Option<String>,
  pub stale_reason: Option<String>,
  pub observation_count: usize,
  pub span_scope_id: Option<String>,
}

/// Tier III delivery / replay path proof (A8c).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ReplayProofSummary {
  pub step_names: Vec<String>,
  pub skipped_rescan_replay: bool,
}

/// Semantic verification proof — separate from identity tiers I–III (A5).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VerificationProofSummary {
  pub status: String,
  pub method: String,
}

/// Tier IV ephemeral geometry note (A8c).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct GeometryProofSummary {
  pub has_ephemeral_target_bounds: bool,
  pub note: String,
}

/// Machine-readable answers for the six owner inspect questions (A8c).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ViewResolutionSummary {
  pub query: String,
  pub identity: IdentityProofSummary,
  pub memory: MemoryProofSummary,
  pub resolution: ResolutionProofSummary,
  pub replay: ReplayProofSummary,
  pub verification: VerificationProofSummary,
  pub geometry_note: GeometryProofSummary,
}

/// Aggregated view-parser inspect read surface for one run (A8b/A8c).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ViewParserInspect {
  pub memory_writes: Vec<ViewMemory>,
  pub reacquisitions: Vec<ReacquisitionRecord>,
  pub select_results: Vec<ViewParserSelectResultWire>,
  pub resolution_summaries: Vec<ViewResolutionSummary>,
}

impl Default for ViewParserInspect {
  fn default() -> Self {
    Self {
      memory_writes: Vec::new(),
      reacquisitions: Vec::new(),
      select_results: Vec::new(),
      resolution_summaries: Vec::new(),
    }
  }
}

/// Lightweight list read-model for run-list proof badges (B1c).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ViewParserListSummary {
  pub has_proof: bool,
  pub resolution_count: usize,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub latest_outcome: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub latest_verification_status: Option<String>,
  pub has_known_limits: bool,
}

/// Summarize a full [`ViewParserInspect`] for run-list badges (B1c).
///
/// Aggregation rules:
/// - `has_proof` — `!resolution_summaries.is_empty()`
/// - `resolution_count` — `resolution_summaries.len()`
/// - `latest_outcome` — **last** resolution summary's `resolution.outcome`, only when
///   `reacquired` / `not_found` / `stale`; otherwise `None`
/// - `latest_verification_status` — **last** summary's `verification.status`, only when
///   `passed` / `failed`; otherwise `None`
/// - `has_known_limits` — **any** `select_results[].known_limits` is non-empty (not limited
///   to the last resolution)
pub fn summarize_view_parser_inspect(inspect: &ViewParserInspect) -> ViewParserListSummary {
  if inspect.resolution_summaries.is_empty() {
    return ViewParserListSummary::default();
  }

  let latest = inspect
    .resolution_summaries
    .last()
    .expect("non-empty resolution_summaries checked above");
  let latest_outcome = match latest.resolution.outcome.as_str() {
    "reacquired" | "not_found" | "stale" => Some(latest.resolution.outcome.clone()),
    _ => None,
  };
  let latest_verification_status = match latest.verification.status.as_str() {
    "passed" | "failed" => Some(latest.verification.status.clone()),
    _ => None,
  };
  let has_known_limits = inspect
    .select_results
    .iter()
    .any(|select| !select.known_limits.is_empty());

  ViewParserListSummary {
    has_proof: true,
    resolution_count: inspect.resolution_summaries.len(),
    latest_outcome,
    latest_verification_status,
    has_known_limits,
  }
}

/// Render human-readable inspect text from a machine summary (A8c).
pub fn format_view_resolution_summary_text(summary: &ViewResolutionSummary) -> String {
  let anchor = summary.identity.anchor_id.as_deref().unwrap_or("-");
  let memory_id = summary.memory.memory_id.as_deref().unwrap_or("-");
  let source_run = summary.memory.source_run_id.as_deref().unwrap_or("-");
  let strategy = summary.resolution.strategy_used.as_deref().unwrap_or("-");
  let stale = summary.resolution.stale_reason.as_deref().unwrap_or("-");
  let steps = if summary.replay.step_names.is_empty() {
    "-".to_string()
  } else {
    summary.replay.step_names.join(",")
  };
  format!(
    "query={}\nidentity: label={} section_kind={} anchor_id={}\nmemory: present={} memory_id={} source_run_id={}\nresolution: outcome={} strategy={} stale_reason={} observations={}\nreplay: steps=[{steps}] skipped_rescan_replay={}\nverification: status={} method={}\ngeometry: ephemeral_bounds={} note={}\n",
    summary.query,
    summary.identity.label,
    summary.identity.section_kind,
    anchor,
    summary.memory.present,
    memory_id,
    source_run,
    summary.resolution.outcome,
    strategy,
    stale,
    summary.resolution.observation_count,
    summary.replay.skipped_rescan_replay,
    summary.verification.status,
    summary.verification.method,
    summary.geometry_note.has_ephemeral_target_bounds,
    summary.geometry_note.note,
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  fn sample_resolution_summary(outcome: &str, verification_status: &str) -> ViewResolutionSummary {
    ViewResolutionSummary {
      query: "Test".into(),
      identity: IdentityProofSummary {
        label: "Test Playlist".into(),
        section_kind: "my_playlists".into(),
        anchor_id: None,
      },
      memory: MemoryProofSummary {
        present: true,
        memory_id: Some("com.example:playlist_sidebar".into()),
        source_run_id: Some("run_ls".into()),
        last_reconstructed_at_millis: Some(1),
        anchor_count: Some(2),
      },
      resolution: ResolutionProofSummary {
        outcome: outcome.into(),
        strategy_used: Some("label_current_viewport".into()),
        stale_reason: None,
        observation_count: 1,
        span_scope_id: Some("playlist_sidebar".into()),
      },
      replay: ReplayProofSummary {
        step_names: vec!["reacquire-target".into()],
        skipped_rescan_replay: true,
      },
      verification: VerificationProofSummary {
        status: verification_status.into(),
        method: "main_title_ocr_full_window_v1".into(),
      },
      geometry_note: GeometryProofSummary {
        has_ephemeral_target_bounds: true,
        note: "bounds are tier IV only".into(),
      },
    }
  }

  #[test]
  fn summarize_view_parser_inspect_empty() {
    let summary = summarize_view_parser_inspect(&ViewParserInspect::default());
    assert_eq!(summary, ViewParserListSummary::default());
    assert!(!summary.has_proof);
    assert_eq!(summary.resolution_count, 0);
    assert!(summary.latest_outcome.is_none());
    assert!(summary.latest_verification_status.is_none());
    assert!(!summary.has_known_limits);
  }

  #[test]
  fn summarize_view_parser_inspect_latest_from_last_resolution() {
    let inspect = ViewParserInspect {
      resolution_summaries: vec![
        sample_resolution_summary("reacquired", "passed"),
        sample_resolution_summary("stale", "failed"),
      ],
      ..Default::default()
    };
    let summary = summarize_view_parser_inspect(&inspect);
    assert!(summary.has_proof);
    assert_eq!(summary.resolution_count, 2);
    assert_eq!(summary.latest_outcome.as_deref(), Some("stale"));
    assert_eq!(
      summary.latest_verification_status.as_deref(),
      Some("failed")
    );
    assert!(!summary.has_known_limits);
  }

  #[test]
  fn summarize_view_parser_inspect_has_known_limits_from_any_select_result() {
    let inspect = ViewParserInspect {
      resolution_summaries: vec![sample_resolution_summary("reacquired", "passed")],
      select_results: vec![
        ViewParserSelectResultWire {
          run_id: None,
          query: "First".into(),
          target: ViewParserSelectTargetWire {
            label: "A".into(),
            section_kind: "my_playlists".into(),
            anchor_id: None,
          },
          steps: Vec::new(),
          verification: ViewParserSelectVerificationWire {
            status: "passed".into(),
            method: "main_title_ocr_full_window_v1".into(),
          },
          reacquire: None,
          known_limits: Vec::new(),
        },
        ViewParserSelectResultWire {
          run_id: None,
          query: "Second".into(),
          target: ViewParserSelectTargetWire {
            label: "B".into(),
            section_kind: "my_playlists".into(),
            anchor_id: None,
          },
          steps: Vec::new(),
          verification: ViewParserSelectVerificationWire {
            status: "passed".into(),
            method: "main_title_ocr_full_window_v1".into(),
          },
          reacquire: None,
          known_limits: vec!["ocr-only".into()],
        },
      ],
      ..Default::default()
    };
    let summary = summarize_view_parser_inspect(&inspect);
    assert!(summary.has_known_limits);
    assert_eq!(summary.latest_outcome.as_deref(), Some("reacquired"));
    assert_eq!(
      summary.latest_verification_status.as_deref(),
      Some("passed")
    );
  }

  #[test]
  fn format_view_resolution_summary_text_includes_all_tiers() {
    let summary = ViewResolutionSummary {
      query: "Test".into(),
      identity: IdentityProofSummary {
        label: "Test Playlist".into(),
        section_kind: "my_playlists".into(),
        anchor_id: None,
      },
      memory: MemoryProofSummary {
        present: true,
        memory_id: Some("com.example:playlist_sidebar".into()),
        source_run_id: Some("run_ls".into()),
        last_reconstructed_at_millis: Some(1),
        anchor_count: Some(2),
      },
      resolution: ResolutionProofSummary {
        outcome: "reacquired".into(),
        strategy_used: Some("label_current_viewport".into()),
        stale_reason: None,
        observation_count: 1,
        span_scope_id: Some("playlist_sidebar".into()),
      },
      replay: ReplayProofSummary {
        step_names: vec!["reacquire-target".into()],
        skipped_rescan_replay: true,
      },
      verification: VerificationProofSummary {
        status: "passed".into(),
        method: "main_title_ocr_full_window_v1".into(),
      },
      geometry_note: GeometryProofSummary {
        has_ephemeral_target_bounds: true,
        note: "bounds are tier IV only".into(),
      },
    };
    let text = format_view_resolution_summary_text(&summary);
    assert!(text.contains("identity: label=Test Playlist"));
    assert!(text.contains("memory: present=true"));
    assert!(text.contains("resolution: outcome=reacquired"));
    assert!(text.contains("replay: steps=[reacquire-target]"));
    assert!(text.contains("verification: status=passed"));
    assert!(text.contains("geometry: ephemeral_bounds=true"));
  }
}
