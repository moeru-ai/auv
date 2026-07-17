//! Capability-oriented accessibility (AX) session helpers for macOS.
//!
//! Native AX capture/focus stay behind this module. Product and app crates
//! should call [`crate::session::AccessibilityApi`] instead of `native`.

use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::input::{DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath};
use serde::{Deserialize, Serialize};

use crate::support::{find_best_ax_node, score_ax_node_match};
use crate::types::{ObservedAxNode, ObservedAxTreeSnapshot};

/// Default AX tree capture bounds for TextEdit-sized document trees.
pub const DEFAULT_AX_MAX_DEPTH: i64 = 16;
pub const DEFAULT_AX_MAX_CHILDREN: i64 = 64;

/// Evidence returned after focusing a selected AX node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxFocusObservation {
  pub app: String,
  pub pid: i32,
  pub path: String,
  pub role: String,
  pub title: String,
  pub value: String,
  pub query: String,
  pub input_action_result: InputActionResult,
}

/// Evidence returned after reading/verifying AX text on a node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxTextObservation {
  pub app: String,
  pub pid: i32,
  pub path: String,
  pub role: String,
  pub matched_text: String,
  pub expected_text: String,
  pub semantic_matched: bool,
}

pub fn capture_app_tree(app: &str, max_depth: i64, max_children: i64) -> DriverResult<ObservedAxTreeSnapshot> {
  let capture = crate::native::ax_tree::capture_ax_tree_snapshot(app, max_depth, max_children).map_err(backend)?;
  Ok(capture.snapshot)
}

pub fn focus_node_path(pid: i32, path: &str, expected_role: &str) -> DriverResult<InputActionResult> {
  let _ = crate::native::ax_tree::set_ax_focused_path(pid, path, expected_role).map_err(backend)?;
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::AxFocus,
    attempts: vec![InputAttempt {
      path: InputDeliveryPath::AxFocus,
      succeeded: true,
      message: Some(format!("focused AX path {path} role {expected_role}")),
    }],
    fallback_reason: None,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Temporary,
    clipboard_disturbance: DisturbanceLevel::None,
  })
}

pub fn focus_text_by_query(app: &str, query: &str, expected_role: Option<&str>, candidate: &str) -> DriverResult<AxFocusObservation> {
  let snapshot = capture_app_tree(app, DEFAULT_AX_MAX_DEPTH, DEFAULT_AX_MAX_CHILDREN)?;
  let node = select_focus_node(&snapshot, query, expected_role, candidate)?;
  let role = if node.role.trim().is_empty() {
    expected_role.unwrap_or("").to_string()
  } else {
    node.role.clone()
  };
  let input_action_result = focus_node_path(snapshot.pid, &node.path, &role)?;
  Ok(AxFocusObservation {
    app: app.to_string(),
    pid: snapshot.pid,
    path: node.path.clone(),
    role,
    title: node.title.clone(),
    value: node.value.clone(),
    query: query.to_string(),
    input_action_result,
  })
}

pub fn verify_text(app: &str, expected_text: &str, expected_role: &str) -> DriverResult<AxTextObservation> {
  let snapshot = capture_app_tree(app, DEFAULT_AX_MAX_DEPTH, DEFAULT_AX_MAX_CHILDREN)?;
  let node = select_text_node_by_role(&snapshot, expected_role)?;
  let matched_text = node.value.clone();
  let semantic_matched = matched_text.contains(expected_text);
  Ok(AxTextObservation {
    app: app.to_string(),
    pid: snapshot.pid,
    path: node.path.clone(),
    role: node.role.clone(),
    matched_text,
    expected_text: expected_text.to_string(),
    semantic_matched,
  })
}

/// Locates the primary text node by role/focus/area — not by expected content.
fn select_text_node_by_role<'a>(snapshot: &'a ObservedAxTreeSnapshot, expected_role: &str) -> DriverResult<&'a ObservedAxNode> {
  let role = expected_role.trim();
  if role.is_empty() {
    return Err(DriverError::InvalidInput {
      message: "accessibility.verify_text requires a non-empty expected_role".to_string(),
    });
  }

  let candidates = snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter(|node| node.role.eq_ignore_ascii_case(role))
    .collect::<Vec<_>>();

  if let Some(focused) = candidates.iter().find(|node| node.focused) {
    return Ok(*focused);
  }

  candidates.into_iter().max_by_key(|node| (node.bounds.width.saturating_mul(node.bounds.height), node.depth)).ok_or_else(|| {
    DriverError::NotFound {
      target: format!("AX text node with role {role}"),
    }
  })
}

fn select_focus_node<'a>(
  snapshot: &'a ObservedAxTreeSnapshot,
  query: &str,
  expected_role: Option<&str>,
  candidate: &str,
) -> DriverResult<&'a ObservedAxNode> {
  let candidate = candidate.trim();
  if !candidate.is_empty() {
    if let Some(node) = snapshot.nodes.iter().find(|node| node.path == candidate) {
      return Ok(node);
    }
    // NOTICE(textedit-ax-candidate-json): full promoted CandidateRef JSON
    // decode is deferred. Non-path candidate strings currently fail closed
    // so invoke cannot silently focus the wrong node. Unlock when product
    // invoke needs CandidateRef promotion for TextEdit focus.
    return Err(DriverError::NotFound {
      target: format!("AX candidate path {candidate}"),
    });
  }

  let query = query.trim();
  if query.is_empty() {
    return Err(DriverError::InvalidInput {
      message: "accessibility.focus_text_by_query requires --query or a path candidate".to_string(),
    });
  }

  let mut ranked = snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter(|node| {
      expected_role.map(|role| role.trim()).filter(|role| !role.is_empty()).is_none_or(|role| node.role.eq_ignore_ascii_case(role))
    })
    .filter_map(|node| score_ax_node_match(node, &query.to_lowercase()).map(|score| (score, node)))
    .collect::<Vec<_>>();
  ranked.sort_by(|left, right| right.0.cmp(&left.0));

  if let Some((_, node)) = ranked.first() {
    return Ok(node);
  }

  find_best_ax_node(snapshot, query).ok_or_else(|| DriverError::NotFound {
    target: format!("AX text node matching query {query}"),
  })
}

fn backend(message: impl std::fmt::Display) -> DriverError {
  DriverError::Backend {
    message: message.to_string(),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::types::ObservedRect;

  fn sample_snapshot() -> ObservedAxTreeSnapshot {
    ObservedAxTreeSnapshot {
      observed_at: "now".to_string(),
      app_name: "TextEdit".to_string(),
      bundle_id: "com.apple.TextEdit".to_string(),
      pid: 4242,
      window_title: "Untitled".to_string(),
      nodes: vec![
        ObservedAxNode {
          depth: 1,
          path: "0.1".to_string(),
          role: "AXWindow".to_string(),
          subrole: String::new(),
          title: "Untitled".to_string(),
          description: String::new(),
          help: String::new(),
          identifier: String::new(),
          placeholder: String::new(),
          value: String::new(),
          focused: false,
          bounds: ObservedRect {
            x: 0,
            y: 0,
            width: 800,
            height: 600,
          },
        },
        ObservedAxNode {
          depth: 2,
          path: "0.1.2".to_string(),
          role: "AXTextArea".to_string(),
          subrole: String::new(),
          title: "First Text View".to_string(),
          description: String::new(),
          help: String::new(),
          identifier: String::new(),
          placeholder: String::new(),
          value: "hello body".to_string(),
          focused: false,
          bounds: ObservedRect {
            x: 10,
            y: 40,
            width: 780,
            height: 540,
          },
        },
      ],
    }
  }

  #[test]
  fn select_focus_node_prefers_query_match_with_role() {
    let snapshot = sample_snapshot();
    let node = select_focus_node(&snapshot, "First Text View", Some("AXTextArea"), "").expect("node");
    assert_eq!(node.path, "0.1.2");
    assert_eq!(node.role, "AXTextArea");
  }

  #[test]
  fn select_focus_node_accepts_exact_path_candidate() {
    let snapshot = sample_snapshot();
    let node = select_focus_node(&snapshot, "", None, "0.1.2").expect("path candidate");
    assert_eq!(node.title, "First Text View");
  }

  #[test]
  fn select_focus_node_rejects_unknown_candidate_without_fallback() {
    let snapshot = sample_snapshot();
    let error = select_focus_node(&snapshot, "First Text View", Some("AXTextArea"), "missing.path").expect_err("unknown candidate");
    assert!(error.to_string().contains("missing.path"));
  }

  #[test]
  fn select_text_node_by_role_ignores_expected_content() {
    let snapshot = sample_snapshot();
    let node = select_text_node_by_role(&snapshot, "AXTextArea").expect("role node");
    assert_eq!(node.path, "0.1.2");
    assert_eq!(node.value, "hello body");
  }

  #[test]
  fn verify_text_observation_can_report_semantic_mismatch_without_erroring() {
    let snapshot = sample_snapshot();
    let node = select_text_node_by_role(&snapshot, "AXTextArea").expect("role node");
    let expected = "not-present-in-body";
    let semantic_matched = node.value.contains(expected);
    assert!(!semantic_matched);
    assert_eq!(node.value, "hello body");
  }
}
