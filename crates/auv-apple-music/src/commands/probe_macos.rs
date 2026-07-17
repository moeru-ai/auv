//! macOS Music.app AX surface probe.
//!
//! Bounded search-field discovery plus toolbar reachability diagnostics. See
//! `docs/ai/references/apps/apple-music/2026-07-15-apple-music-macos-ax-probe.md`.

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

/// App-local diagnostic output for one captured toolbar.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolbarInspection {
  pub path: String,
  pub role: String,
  pub available_actions: Vec<String>,
  pub available_attributes: Vec<String>,
  pub child_counts: ToolbarChildCounts,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolbarChildCounts {
  pub children_count: usize,
  pub visible_children_count: usize,
  pub contents_count: usize,
  pub navigation_children_count: usize,
}

/// Output produced by the probe.
///
/// TODO(apple-music-result-row): result rows require an owner-approved query
/// submission slice; see the Apple Music AX reference note.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProbeResult {
  pub command: String,
  pub bundle_id: String,
  pub node_count: usize,
  pub search_field_candidates: Vec<DiscoveredNode>,
  pub toolbar_inspections: Vec<ToolbarInspection>,
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
    node_count: 0,
    search_field_candidates: Vec::new(),
    toolbar_inspections: Vec::new(),
    artifact: None,
    diagnostics: Vec::new(),
  };

  // Step 1: activate Music.app
  session
    .activate_bundle_id(&inputs.bundle_id, Duration::from_millis(inputs.activate_settle_ms))
    .map_err(|error| format!("Music.app activation failed: {error}"))?;

  // Step 2: capture AX tree
  let snapshot = session
    .accessibility()
    .capture_app_tree(&inputs.bundle_id, DEFAULT_AX_MAX_DEPTH, DEFAULT_AX_MAX_CHILDREN)
    .map_err(|error| format!("AX tree capture failed: {error}"))?;
  result.node_count = snapshot.nodes.len();

  // Step 3: locate search field candidates
  result.search_field_candidates = find_search_field_candidates(&snapshot);
  if result.search_field_candidates.is_empty() {
    result.diagnostics.push("no search field candidates found".to_string());
  }

  // Step 3b: inspect every toolbar node for children reachable through an
  // AX attribute other than AXChildren. See ProbeResult::toolbar_inspections.
  let (toolbar_inspections, toolbar_diagnostics) = inspect_toolbar_nodes(&snapshot);
  result.toolbar_inspections = toolbar_inspections;
  result.diagnostics.extend(toolbar_diagnostics);
  for inspection in &result.toolbar_inspections {
    let counts = &inspection.child_counts;
    if counts.children_count == 0 && (counts.visible_children_count > 0 || counts.contents_count > 0 || counts.navigation_children_count > 0)
    {
      result.diagnostics.push(format!(
        "toolbar {} has 0 AXChildren but non-zero children via another attribute (visible={}, contents={}, navigation={})",
        inspection.path, counts.visible_children_count, counts.contents_count, counts.navigation_children_count
      ));
    }
  }

  // Step 4: persist artifact if requested
  if let Some(dir) = &inputs.artifact_dir {
    result.artifact = Some(save_probe_artifact(dir, &snapshot)?);
  }

  Ok(result)
}

#[cfg(target_os = "macos")]
fn inspect_toolbar_nodes(snapshot: &ObservedAxTreeSnapshot) -> (Vec<ToolbarInspection>, Vec<String>) {
  let mut inspections = Vec::new();
  let mut diagnostics = Vec::new();

  for node in snapshot.nodes.iter().filter(|node| node.role.eq_ignore_ascii_case("AXToolbar")) {
    let inspection = auv_driver_macos::native::ax_tree::inspect_ax_node_path(snapshot.pid, &node.path, &node.role)
      .map(|inspection| ToolbarInspection {
        path: inspection.path,
        role: inspection.role,
        available_actions: inspection.available_actions,
        available_attributes: inspection.available_attributes,
        child_counts: ToolbarChildCounts {
          children_count: inspection.children_count,
          visible_children_count: inspection.visible_children_count,
          contents_count: inspection.contents_count,
          navigation_children_count: inspection.navigation_children_count,
        },
      })
      .map_err(|error| error.to_string());
    record_toolbar_inspection(&mut inspections, &mut diagnostics, &node.path, &node.role, inspection);
  }

  (inspections, diagnostics)
}

fn record_toolbar_inspection(
  inspections: &mut Vec<ToolbarInspection>,
  diagnostics: &mut Vec<String>,
  path: &str,
  role: &str,
  inspection: Result<ToolbarInspection, String>,
) {
  match inspection {
    Ok(inspection) => inspections.push(inspection),
    Err(error) => diagnostics.push(format!("toolbar inspection failed for path={path} role={role}: {error}")),
  }
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

  #[test]
  fn probe_result_serializes_without_invariant_success_fields() {
    let result = ProbeResult {
      command: "probe-macos".to_string(),
      bundle_id: DEFAULT_MUSIC_APP_BUNDLE_ID.to_string(),
      node_count: 0,
      search_field_candidates: Vec::new(),
      toolbar_inspections: Vec::new(),
      artifact: None,
      diagnostics: Vec::new(),
    };

    let value = serde_json::to_value(result).expect("probe result should serialize");
    assert!(value.get("activated").is_none());
    assert!(value.get("ax_snapshot_captured").is_none());
    assert_eq!(value["toolbar_inspections"], serde_json::json!([]));
  }

  #[test]
  fn toolbar_inspection_error_is_recorded_with_target_context() {
    let mut inspections = Vec::new();
    let mut diagnostics = Vec::new();

    record_toolbar_inspection(&mut inspections, &mut diagnostics, "0.1", "AXToolbar", Err("AX path shifted".to_string()));

    assert!(inspections.is_empty());
    assert_eq!(diagnostics, ["toolbar inspection failed for path=0.1 role=AXToolbar: AX path shifted"]);
  }

  #[cfg(target_os = "macos")]
  #[test]
  fn is_search_field_candidate_matches_role() {
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
