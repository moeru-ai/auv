//! Capability-oriented accessibility (AX) session helpers for macOS.
//!
//! Native AX capture/focus stay behind this module. Product and app crates
//! should call [`crate::session::AccessibilityApi`] instead of `native`.

use crate::support::{find_best_ax_node, score_ax_node_match};
use crate::types::{ObservedAxNode, ObservedAxTreeSnapshot};
use auv_driver_common::accessibility::{AxFocusResult, AxTextRead};
use auv_driver_common::error::{DriverError, DriverResult};
use auv_driver_common::input::{DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath};

/// Default AX tree capture bounds for TextEdit-sized document trees.
pub const DEFAULT_AX_MAX_DEPTH: i64 = 16;
pub const DEFAULT_AX_MAX_CHILDREN: i64 = 64;

pub fn capture_app_tree(app: &str, max_depth: i64, max_children: i64) -> DriverResult<ObservedAxTreeSnapshot> {
  let capture = crate::native::tree::capture_ax_tree_snapshot(app, max_depth, max_children).map_err(backend)?;
  Ok(capture.snapshot)
}

pub fn focus_node_path(pid: i32, path: &str, expected_role: &str) -> DriverResult<InputActionResult> {
  // `set_ax_focused_path` already returns a classified `DriverError` (stale
  // path / role mismatch / permission / backend), so propagate it directly
  // instead of flattening everything through `backend` into `Backend`.
  let _ = crate::native::tree::set_ax_focused_path(pid, path, expected_role)?;
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::AxFocus,
    attempts: vec![InputAttempt {
      path: InputDeliveryPath::AxFocus,
      succeeded: true,
      message: Some(format!("focused AX path {path} role {expected_role}")),
    }],
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Temporary,
    clipboard_disturbance: DisturbanceLevel::None,
  })
}

pub fn focus_text_by_query(app: &str, query: &str, expected_role: Option<&str>, candidate: &str) -> DriverResult<AxFocusResult> {
  let snapshot = capture_app_tree(app, DEFAULT_AX_MAX_DEPTH, DEFAULT_AX_MAX_CHILDREN)?;
  let node = select_focus_node(&snapshot, query, expected_role, candidate)?;
  let role = if node.role.trim().is_empty() {
    expected_role.unwrap_or("").to_string()
  } else {
    node.role.clone()
  };
  let input_action_result = focus_node_path(snapshot.pid, &node.path, &role)?;
  Ok(AxFocusResult {
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

pub fn verify_text(app: &str, _expected_text: &str, expected_role: &str) -> DriverResult<AxTextRead> {
  // TODO(ax-read-text-api): remove the expected-text argument when the
  // AccessibilityApi signature is in scope; the driver must not interpret it.
  let snapshot = capture_app_tree(app, DEFAULT_AX_MAX_DEPTH, DEFAULT_AX_MAX_CHILDREN)?;
  let node = select_text_node_by_role(&snapshot, expected_role)?;
  Ok(AxTextRead {
    app: app.to_string(),
    pid: snapshot.pid,
    path: node.path.clone(),
    role: node.role.clone(),
    matched_text: node.value.clone(),
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
#[path = "accessibility_test.rs"]
mod tests;
