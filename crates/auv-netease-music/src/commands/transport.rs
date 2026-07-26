//! NetEase playback transport through Windows UI Automation.
//!
//! This path intentionally requires a visible NetEase window and an actionable
//! UIA control. It does not fall back to global media keys or screen
//! coordinates, because either fallback could control the wrong application or
//! hide the actual delivery path.

use std::fmt;

use auv_driver::geometry::Rect;
use auv_driver::input::InputActionResult;
use serde::{Deserialize, Serialize};

use crate::views::player::PlaybackControlState;
use crate::windows::ResolveOptions;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportAction {
  PlayPause,
  Next,
  Previous,
}

impl fmt::Display for TransportAction {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::PlayPause => write!(f, "play_pause"),
      Self::Next => write!(f, "next"),
      Self::Previous => write!(f, "previous"),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportInputs {
  pub action: TransportAction,
  pub settle_ms: u64,
  pub resolve: ResolveOptions,
}

impl TransportInputs {
  pub fn new(action: TransportAction) -> Self {
    Self {
      action,
      settle_ms: 150,
      resolve: ResolveOptions::default(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransportResult {
  pub command: String,
  pub action: TransportAction,
  pub window_title: Option<String>,
  pub node_path: String,
  pub control_name: String,
  pub automation_id: String,
  pub delivery: InputActionResult,
}

pub fn run_transport_action(inputs: &TransportInputs) -> Result<TransportResult, String> {
  platform::run(inputs)
}

pub(crate) const PLAY_STATE_NAMES: &[&str] = &["播放", "play"];
pub(crate) const PAUSE_STATE_NAMES: &[&str] = &["暂停", "pause"];
pub(crate) const PLAYPAUSE_GENERIC_NAMES: &[&str] = &["播放暂停", "playpause"];
const PLAYPAUSE_AUTOMATION_IDS: &[&str] = &["playpause", "playbutton", "pausebutton", "btnpcminibarplay"];

#[derive(Clone, Copy, Debug)]
pub(crate) struct NodeEvidence<'a> {
  pub(crate) path: &'a str,
  pub(crate) name: &'a str,
  pub(crate) value: Option<&'a str>,
  pub(crate) automation_id: &'a str,
  pub(crate) control_type: &'a str,
  pub(crate) bounds: Rect,
}

pub(crate) fn choose_control<'a>(
  nodes: impl IntoIterator<Item = NodeEvidence<'a>>,
  action: TransportAction,
  window_bounds: Rect,
) -> Result<NodeEvidence<'a>, String> {
  let mut matches = nodes
    .into_iter()
    .filter(|node| is_in_transport_band(node.bounds, window_bounds))
    .filter_map(|node| control_score(node, action).map(|score| (score, node)))
    .collect::<Vec<_>>();
  matches.sort_by(|left, right| right.0.cmp(&left.0));

  let Some((best_score, best)) = matches.first().copied() else {
    return Err(format!("no UIA control matched NetEase transport action {action}"));
  };
  if matches.get(1).is_some_and(|(score, _)| *score == best_score) {
    let paths = matches.iter().take_while(|(score, _)| *score == best_score).map(|(_, node)| node.path).collect::<Vec<_>>().join(", ");
    return Err(format!("multiple UIA controls matched NetEase transport action {action} equally: {paths}"));
  }
  Ok(best)
}

pub(crate) fn is_in_transport_band(bounds: Rect, window_bounds: Rect) -> bool {
  if bounds.size.width <= 0.0 || bounds.size.height <= 0.0 {
    return false;
  }
  let center_x = bounds.origin.x + bounds.size.width * 0.5;
  let center_y = bounds.origin.y + bounds.size.height * 0.5;
  center_y >= window_bounds.origin.y + window_bounds.size.height * 0.82
    && center_x >= window_bounds.origin.x + window_bounds.size.width * 0.28
    && center_x <= window_bounds.origin.x + window_bounds.size.width * 0.72
}

fn control_score(node: NodeEvidence<'_>, action: TransportAction) -> Option<u8> {
  let name = normalize(&node.name);
  let automation_id = normalize(&node.automation_id);
  let control_type = normalize(&node.control_type);
  let actionable = control_type.contains("button") || control_type.contains("menuitem");

  let (names, ids): (Vec<&str>, &[&str]) = match action {
    TransportAction::PlayPause => {
      (PLAY_STATE_NAMES.iter().chain(PAUSE_STATE_NAMES).chain(PLAYPAUSE_GENERIC_NAMES).copied().collect(), PLAYPAUSE_AUTOMATION_IDS)
    }
    TransportAction::Next => (vec!["下一首", "下一曲", "next", "nexttrack"], &["next", "nexttrack"]),
    TransportAction::Previous => (
      vec![
        "上一首",
        "上一曲",
        "previous",
        "previoustrack",
        "prev",
        "pre",
      ],
      &["previous", "previoustrack", "prev"],
    ),
  };

  if actionable && names.iter().any(|candidate| name == *candidate) {
    return Some(4);
  }
  if ids.iter().any(|candidate| automation_id.contains(candidate)) {
    return Some(if actionable { 3 } else { 2 });
  }
  if actionable && names.iter().any(|candidate| name.contains(candidate) && !candidate.is_empty()) {
    return Some(1);
  }
  None
}

pub(crate) fn normalize(value: &str) -> String {
  value.chars().filter(|character| character.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Finds NetEase's Windows play/pause UIA control without invoking it.
///
/// Read-only wrapper over `choose_control` so callers that only need to
/// inspect current state (see `classify_playpause_state`) don't need to model
/// their call site around `TransportAction`, which is otherwise an
/// invoke-flavored concept.
pub(crate) fn find_playpause_control<'a>(
  nodes: impl IntoIterator<Item = NodeEvidence<'a>>,
  window_bounds: Rect,
) -> Result<NodeEvidence<'a>, String> {
  choose_control(nodes, TransportAction::PlayPause, window_bounds)
}

// TODO(netease-windows-playpause-toggle-state): this infers play/pause state
// from the control's accessible Name/ValuePattern text, not a UIA
// TogglePattern read (auv-driver-windows does not currently expose
// TogglePattern.CurrentToggleState). If NetEase's control exposes only a
// static/generic label regardless of playback state, this will always
// return `Unknown`; validate live before relying on this signal. Adding a
// TogglePattern read to auv-driver-windows is a separate, owner-approved
// driver-side slice, not part of this one.
pub(crate) fn classify_playpause_state(name: &str, value: Option<&str>) -> PlaybackControlState {
  for candidate in [Some(name), value].into_iter().flatten() {
    let normalized = normalize(candidate);
    if PLAYPAUSE_GENERIC_NAMES.iter().any(|generic| normalized == *generic) {
      continue;
    }
    if PAUSE_STATE_NAMES.iter().any(|pause| normalized == *pause) {
      return PlaybackControlState::PauseVisible;
    }
    if PLAY_STATE_NAMES.iter().any(|play| normalized == *play) {
      return PlaybackControlState::PlayVisible;
    }
  }
  PlaybackControlState::Unknown
}

#[cfg(target_os = "windows")]
mod platform {
  use std::time::Duration;

  use super::*;
  use crate::windows::resolve_window;

  pub fn run(inputs: &TransportInputs) -> Result<TransportResult, String> {
    let window =
      resolve_window(&inputs.resolve)?.ok_or_else(|| "NetEase Cloud Music has no visible window; run `open-window` first".to_string())?;
    let session = auv_driver::open_local().map_err(|error| format!("failed to open Windows driver: {error}"))?;
    let snapshot =
      session.accessibility().snapshot_window(&window).map_err(|error| format!("failed to capture NetEase UIA tree: {error}"))?;
    let matched = choose_control(
      snapshot.nodes.iter().map(|node| NodeEvidence {
        path: &node.path,
        name: &node.name,
        value: node.value.as_deref(),
        automation_id: &node.automation_id,
        control_type: &node.control_type,
        bounds: node.bounds,
      }),
      inputs.action,
      window.frame,
    )
    .map_err(|error| {
      // TODO(netease-windows-uia-hit-test): point-based UIA element lookup was
      // considered but is deferred until a live playing-state capture proves
      // the transport bar is absent from the expanded tree and identifies
      // stable hit-test geometry. Do not replace this with a coordinate click.
      format!(
        "{error}; captured {} UIA nodes. If the tree contains only CEF containers, fully exit NetEase and relaunch it with `auv-netease-music open-window` so renderer accessibility is enabled",
        snapshot.nodes.len()
      )
    })?;
    let node_path = matched.path.to_string();
    let control_name = matched.name.to_string();
    let automation_id = matched.automation_id.to_string();
    let delivery = session
      .accessibility()
      .select_node(&window, &node_path)
      .map_err(|error| format!("failed to invoke NetEase UIA control {control_name:?} at {node_path}: {error}"))?;

    if inputs.settle_ms > 0 {
      std::thread::sleep(Duration::from_millis(inputs.settle_ms));
    }

    Ok(TransportResult {
      command: format!("transport.{}", inputs.action),
      action: inputs.action,
      window_title: window.title,
      node_path,
      control_name,
      automation_id,
      delivery,
    })
  }
}

#[cfg(not(target_os = "windows"))]
mod platform {
  use super::*;

  pub fn run(_inputs: &TransportInputs) -> Result<TransportResult, String> {
    Err("NetEase UIA transport controls are only supported on Windows".to_string())
  }
}

#[cfg(test)]
#[path = "transport_test.rs"]
mod tests;
