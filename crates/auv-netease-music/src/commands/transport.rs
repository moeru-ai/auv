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

#[derive(Clone, Copy, Debug)]
struct NodeEvidence<'a> {
  path: &'a str,
  name: &'a str,
  automation_id: &'a str,
  control_type: &'a str,
  bounds: Rect,
}

fn choose_control<'a>(
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

fn is_in_transport_band(bounds: Rect, window_bounds: Rect) -> bool {
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

  let (names, ids): (&[&str], &[&str]) = match action {
    TransportAction::PlayPause => {
      (&["播放", "暂停", "播放暂停", "play", "pause", "playpause"], &["playpause", "playbutton", "pausebutton", "btnpcminibarplay"])
    }
    TransportAction::Next => (&["下一首", "下一曲", "next", "nexttrack"], &["next", "nexttrack"]),
    TransportAction::Previous => (
      &[
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

fn normalize(value: &str) -> String {
  value.chars().filter(|character| character.is_alphanumeric()).flat_map(char::to_lowercase).collect()
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
mod tests {
  use super::*;

  fn node<'a>(path: &'a str, name: &'a str, automation_id: &'a str, control_type: &'a str, x: f64) -> NodeEvidence<'a> {
    NodeEvidence {
      path,
      name,
      automation_id,
      control_type,
      bounds: Rect::new(x, 650.0, 40.0, 40.0),
    }
  }

  fn window_bounds() -> Rect {
    Rect::new(0.0, 0.0, 1_000.0, 800.0)
  }

  #[test]
  fn chooses_localized_transport_button_names() {
    let nodes = [
      node("0/1", "上一首", "", "button", 420.0),
      node("0/2", "暂停", "", "button", 480.0),
      node("0/3", "下一首", "", "button", 540.0),
    ];

    assert_eq!(choose_control(nodes, TransportAction::PlayPause, window_bounds()).unwrap().path, "0/2");
    assert_eq!(choose_control(nodes, TransportAction::Next, window_bounds()).unwrap().path, "0/3");
    assert_eq!(choose_control(nodes, TransportAction::Previous, window_bounds()).unwrap().path, "0/1");
  }

  #[test]
  fn automation_id_matches_when_accessible_name_is_missing() {
    let matched = choose_control([node("0/4", "", "player-next-track", "custom", 520.0)], TransportAction::Next, window_bounds()).unwrap();

    assert_eq!(matched.path, "0/4");
  }

  #[test]
  fn chooses_live_netease_minibar_menu_item_for_play_pause() {
    let matched = choose_control(
      [node(
        "0/0/0/0/36/9",
        "play",
        "btn_pc_minibar_play",
        "menu item",
        480.0,
      )],
      TransportAction::PlayPause,
      window_bounds(),
    )
    .unwrap();

    assert_eq!(matched.path, "0/0/0/0/36/9");
  }

  #[test]
  fn chooses_live_netease_pre_name_for_previous() {
    let matched = choose_control([node("0/0/0/0/36/8", "pre", "", "button", 440.0)], TransportAction::Previous, window_bounds()).unwrap();

    assert_eq!(matched.path, "0/0/0/0/36/8");
  }

  #[test]
  fn exact_button_name_beats_weaker_automation_id_match() {
    let matched = choose_control(
      [
        node("0/1", "", "next-track", "button", 500.0),
        node("0/2", "Next", "", "button", 540.0),
      ],
      TransportAction::Next,
      window_bounds(),
    )
    .unwrap();

    assert_eq!(matched.path, "0/2");
  }

  #[test]
  fn rejects_equal_best_matches_instead_of_invoking_arbitrarily() {
    let error = choose_control(
      [
        node("0/1", "Next", "", "button", 500.0),
        node("0/2", "Next", "", "button", 540.0),
      ],
      TransportAction::Next,
      window_bounds(),
    )
    .unwrap_err();

    assert!(error.contains("multiple UIA controls"));
  }

  #[test]
  fn does_not_match_non_button_text_by_name_alone() {
    let error = choose_control([node("0/1", "Next", "", "text", 500.0)], TransportAction::Next, window_bounds()).unwrap_err();

    assert!(error.contains("no UIA control matched"));
  }

  #[test]
  fn ignores_content_play_buttons_above_bottom_transport_band() {
    let error = choose_control(
      [NodeEvidence {
        path: "0/30/29",
        name: "play",
        automation_id: "",
        control_type: "button",
        bounds: Rect::new(300.0, 400.0, 43.0, 43.0),
      }],
      TransportAction::PlayPause,
      window_bounds(),
    )
    .unwrap_err();

    assert!(error.contains("no UIA control matched"));
  }
}
