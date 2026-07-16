//! macOS Music.app AX surface probe.
//!
//! Bounded discovery for the search field only — activate, capture, locate,
//! persist. Does not submit a search query, click results, play tracks, or
//! implement candidate selection algorithms. Result-row classification is
//! deferred; see [`ProbeResult`] doc for why.

use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use auv_driver::LocalDriverSession;
#[cfg(target_os = "macos")]
use auv_driver_macos::{ApplicationControl, DEFAULT_AX_MAX_CHILDREN, DEFAULT_AX_MAX_DEPTH, ObservedAxNode, ObservedAxTreeSnapshot};

pub const DEFAULT_MUSIC_APP_BUNDLE_ID: &str = "com.apple.Music";
pub const DEFAULT_ACTIVATE_SETTLE_MS: u64 = 800;

/// Inputs for the probe command.
#[derive(Clone, Debug)]
pub struct ProbeInputs {
  pub bundle_id: String,
  pub activate_settle_ms: u64,
  pub artifact_dir: Option<PathBuf>,
}

impl Default for ProbeInputs {
  fn default() -> Self {
    Self {
      bundle_id: DEFAULT_MUSIC_APP_BUNDLE_ID.to_string(),
      activate_settle_ms: DEFAULT_ACTIVATE_SETTLE_MS,
      artifact_dir: None,
    }
  }
}

/// A discovered search-field AX node candidate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredNode {
  pub path: String,
  pub role: String,
  pub subrole: String,
  pub title: String,
  pub value: String,
  pub bounds_x: i64,
  pub bounds_y: i64,
  pub bounds_width: i64,
  pub bounds_height: i64,
}

/// Output produced by the probe.
///
/// NOTICE(apple-music-result-row-deferred): a `result_row_candidates` field
/// was deliberately removed here. This probe never submits a search query, so
/// it only ever observes Music.app's default/landing surface. Any static-text
/// heuristic run against that surface would misclassify sidebar labels,
/// buttons, and recommendation copy as "search results" — a taxonomy that
/// cannot be validated without first observing a real post-search AX tree.
/// Unlock once a query-submission slice captures a live post-search snapshot
/// an owner can review.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
  pub command: String,
  pub bundle_id: String,
  pub activated: bool,
  pub ax_snapshot_captured: bool,
  pub node_count: usize,
  pub search_field_candidates: Vec<DiscoveredNode>,
  pub artifact: Option<String>,
  pub diagnostics: Vec<String>,
}

pub fn run_probe(inputs: &ProbeInputs) -> Result<ProbeResult, String> {
  #[cfg(target_os = "macos")]
  {
    run_probe_macos(inputs)
  }
  #[cfg(not(target_os = "macos"))]
  {
    let _ = inputs;
    Err("Music.app AX probe is only supported on macOS".to_string())
  }
}

#[cfg(target_os = "macos")]
fn run_probe_macos(inputs: &ProbeInputs) -> Result<ProbeResult, String> {
  let session = auv_driver::open_local().map_err(|error| error.to_string())?;
  let LocalDriverSession::Macos(session) = session;

  let mut result = ProbeResult {
    command: "probe-macos".to_string(),
    bundle_id: inputs.bundle_id.clone(),
    activated: false,
    ax_snapshot_captured: false,
    node_count: 0,
    search_field_candidates: Vec::new(),
    artifact: None,
    diagnostics: Vec::new(),
  };

  // Step 1: activate Music.app
  session
    .activate_bundle_id(&inputs.bundle_id, Duration::from_millis(inputs.activate_settle_ms))
    .map_err(|error| format!("Music.app activation failed: {error}"))?;
  result.activated = true;

  // Step 2: capture AX tree
  let snapshot = session
    .accessibility()
    .capture_app_tree(&inputs.bundle_id, DEFAULT_AX_MAX_DEPTH, DEFAULT_AX_MAX_CHILDREN)
    .map_err(|error| format!("AX tree capture failed: {error}"))?;
  result.ax_snapshot_captured = true;
  result.node_count = snapshot.nodes.len();

  // Step 3: locate search field candidates
  result.search_field_candidates = find_search_field_candidates(&snapshot);
  if result.search_field_candidates.is_empty() {
    result.diagnostics.push("no search field candidates found".to_string());
  }

  // Step 4: persist artifact if requested
  if let Some(dir) = &inputs.artifact_dir {
    result.artifact = Some(save_probe_artifact(dir, &snapshot)?);
  }

  Ok(result)
}

#[cfg(target_os = "macos")]
fn find_search_field_candidates(snapshot: &ObservedAxTreeSnapshot) -> Vec<DiscoveredNode> {
  snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter(|node| is_search_field_candidate(node))
    .map(node_to_discovered)
    .collect()
}

#[cfg(target_os = "macos")]
fn is_search_field_candidate(node: &ObservedAxNode) -> bool {
  let role_match = node.role.eq_ignore_ascii_case("AXTextField") || node.role.eq_ignore_ascii_case("AXSearchField");
  let subrole_match = node.subrole.eq_ignore_ascii_case("AXSearchField");
  let placeholder_match = node.placeholder.to_lowercase().contains("search");
  let title_match = node.title.to_lowercase().contains("search");

  role_match || subrole_match || placeholder_match || title_match
}

#[cfg(target_os = "macos")]
fn node_to_discovered(node: &ObservedAxNode) -> DiscoveredNode {
  DiscoveredNode {
    path: node.path.clone(),
    role: node.role.clone(),
    subrole: node.subrole.clone(),
    title: node.title.clone(),
    value: node.value.clone(),
    bounds_x: node.bounds.x,
    bounds_y: node.bounds.y,
    bounds_width: node.bounds.width,
    bounds_height: node.bounds.height,
  }
}

#[cfg(target_os = "macos")]
fn save_probe_artifact(dir: &std::path::Path, snapshot: &ObservedAxTreeSnapshot) -> Result<String, String> {
  std::fs::create_dir_all(dir).map_err(|error| format!("create probe artifact directory failed: {error}"))?;
  let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|duration| duration.as_millis()).unwrap_or(0);
  let path = dir.join(format!("music-ax-probe-{timestamp}.json"));
  let json = serde_json::to_string_pretty(snapshot).map_err(|error| format!("serialize AX snapshot failed: {error}"))?;
  std::fs::write(&path, json).map_err(|error| format!("write probe artifact failed: {error}"))?;
  Ok(path.to_string_lossy().into_owned())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[cfg(target_os = "macos")]
  #[test]
  fn is_search_field_candidate_matches_role() {
    use auv_driver_macos::ObservedAxNode;
    let node = ObservedAxNode {
      depth: 2,
      path: "0.1.2".to_string(),
      role: "AXTextField".to_string(),
      subrole: "AXSearchField".to_string(),
      title: String::new(),
      description: String::new(),
      help: String::new(),
      identifier: String::new(),
      placeholder: String::new(),
      value: String::new(),
      focused: false,
      bounds: auv_driver_macos::types::ObservedRect {
        x: 10,
        y: 20,
        width: 200,
        height: 30,
      },
    };

    assert!(is_search_field_candidate(&node));
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn is_search_field_candidate_matches_placeholder() {
    use auv_driver_macos::ObservedAxNode;
    let node = ObservedAxNode {
      depth: 2,
      path: "0.1.2".to_string(),
      role: "AXTextField".to_string(),
      subrole: String::new(),
      title: String::new(),
      description: String::new(),
      help: String::new(),
      identifier: String::new(),
      placeholder: "Search Music".to_string(),
      value: String::new(),
      focused: false,
      bounds: auv_driver_macos::types::ObservedRect {
        x: 10,
        y: 20,
        width: 200,
        height: 30,
      },
    };

    assert!(is_search_field_candidate(&node));
  }
}
