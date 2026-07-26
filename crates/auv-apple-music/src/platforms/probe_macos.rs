//! Music.app AX surface probe through the macOS driver.
//!
//! Bounded search-field discovery plus toolbar reachability diagnostics. See
//! `docs/ai/references/apps/apple-music/2026-07-15-apple-music-macos-ax-probe.md`.

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
}

impl Default for ProbeInputs {
  fn default() -> Self {
    Self {
      bundle_id: DEFAULT_MUSIC_APP_BUNDLE_ID.to_string(),
      activate_settle_ms: DEFAULT_ACTIVATE_SETTLE_MS,
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
  pub activated: bool,
  pub ax_snapshot_captured: bool,
  pub node_count: usize,
  pub search_field_candidates: Vec<DiscoveredNode>,
  pub toolbar_inspections: Vec<ToolbarInspection>,
  pub diagnostics: Vec<String>,
}

pub fn run_probe(inputs: &ProbeInputs) -> Result<ProbeResult, String> {
  crate::tracing::ax_probe(|| run_probe_inner(inputs))
}

fn run_probe_inner(inputs: &ProbeInputs) -> Result<ProbeResult, String> {
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
    toolbar_inspections: Vec::new(),
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

  crate::tracing::json_artifact("auv.apple_music.ax_snapshot", &snapshot);

  Ok(result)
}

#[cfg(target_os = "macos")]
fn inspect_toolbar_nodes(snapshot: &ObservedAxTreeSnapshot) -> (Vec<ToolbarInspection>, Vec<String>) {
  let mut inspections = Vec::new();
  let mut diagnostics = Vec::new();

  for node in snapshot.nodes.iter().filter(|node| node.role.eq_ignore_ascii_case("AXToolbar")) {
    let inspection = auv_driver_macos::native::tree::inspect_ax_node_path(snapshot.pid, &node.path, &node.role)
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
