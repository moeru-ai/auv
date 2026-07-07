pub mod mouse;
pub mod system_details;

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::interaction::InteractionStep;
#[cfg(target_os = "linux")]
use crate::interaction::StepOutcome;
#[cfg(target_os = "linux")]
use crate::views::{MatchedNode, SettingsNode, find_labeled_node, visible_labels};
use crate::windows::{OpenWindowReport, ResolveOptions, open_or_resolve};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenInputs {
  pub settle_ms: u64,
}

impl Default for OpenInputs {
  fn default() -> Self {
    Self { settle_ms: 8_000 }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenResult {
  pub command: &'static str,
  pub window: OpenWindowReport,
  pub steps: Vec<InteractionStep>,
}

pub fn run_open(inputs: &OpenInputs) -> Result<OpenResult, String> {
  let (_, window) = open_or_resolve(&ResolveOptions {
    settle: Duration::from_millis(inputs.settle_ms),
  })?;
  Ok(OpenResult {
    command: "open",
    steps: window.steps.clone(),
    window,
  })
}

#[cfg(target_os = "linux")]
pub(crate) fn select_visible_labeled_node(
  session: &auv_driver_linux::LinuxDriverSession,
  window: &auv_driver::Window,
  labels: crate::app::LabelSet,
  step_name: &str,
  steps: &mut Vec<InteractionStep>,
) -> Result<MatchedNode, String> {
  let mut last_visible_labels = Vec::new();
  for _ in 0..8 {
    let nodes = snapshot_nodes(session, window)?;
    last_visible_labels = visible_labels(&nodes);
    let matched = find_labeled_node(&nodes, labels)
      .ok_or_else(|| format!("could not find one of [{}]; visible labels: {}", labels.display(), last_visible_labels.join(" | ")))?;
    if node_is_visible(window, &matched) {
      let delivery = select_node_or_click(session, window, &matched)?;
      steps.push(InteractionStep::new(step_name, StepOutcome::Selected).target(matched.label.clone()).note(format!("{delivery:?}")));
      return Ok(matched);
    }
    scroll_toward_node(session, window, &matched)?;
    std::thread::sleep(Duration::from_millis(250));
  }
  Err(format!("could not bring one of [{}] into view; visible labels: {}", labels.display(), last_visible_labels.join(" | ")))
}

#[cfg(target_os = "linux")]
pub(crate) fn click_visible_labeled_node(
  session: &auv_driver_linux::LinuxDriverSession,
  window: &auv_driver::Window,
  labels: crate::app::LabelSet,
  step_name: &str,
  steps: &mut Vec<InteractionStep>,
) -> Result<MatchedNode, String> {
  let (matched, _) = click_visible_labeled_node_with_delivery(session, window, labels, step_name, steps)?;
  Ok(matched)
}

#[cfg(target_os = "linux")]
pub(crate) fn click_visible_labeled_node_with_delivery(
  session: &auv_driver_linux::LinuxDriverSession,
  window: &auv_driver::Window,
  labels: crate::app::LabelSet,
  step_name: &str,
  steps: &mut Vec<InteractionStep>,
) -> Result<(MatchedNode, auv_driver::InputActionResult), String> {
  let mut last_visible_labels = Vec::new();
  for _ in 0..8 {
    let nodes = snapshot_nodes(session, window)?;
    last_visible_labels = visible_labels(&nodes);
    let matched = find_labeled_node(&nodes, labels)
      .ok_or_else(|| format!("could not find one of [{}]; visible labels: {}", labels.display(), last_visible_labels.join(" | ")))?;
    if node_is_visible(window, &matched) {
      let delivery = session
        .window()
        .click(
          window,
          window_point_for_node(window, &matched),
          auv_driver::ClickOptions {
            click: auv_driver::Click::Single,
            ..auv_driver::ClickOptions::default()
          },
        )
        .map_err(|error| format!("failed to click {} at {}: {error}", matched.label, matched.path))?;
      steps.push(InteractionStep::new(step_name, StepOutcome::Clicked).target(matched.label.clone()).note(format!("{delivery:?}")));
      return Ok((matched, delivery));
    }
    scroll_toward_node(session, window, &matched)?;
    std::thread::sleep(Duration::from_millis(250));
  }
  Err(format!("could not bring one of [{}] into view; visible labels: {}", labels.display(), last_visible_labels.join(" | ")))
}

#[cfg(target_os = "linux")]
pub(crate) fn snapshot_nodes(
  session: &auv_driver_linux::LinuxDriverSession,
  window: &auv_driver::Window,
) -> Result<Vec<SettingsNode>, String> {
  let snapshot = session.accessibility().snapshot_window(window).map_err(|error| format!("failed to capture AT-SPI tree: {error}"))?;
  Ok(snapshot.nodes.iter().map(SettingsNode::from).collect())
}

#[cfg(target_os = "linux")]
fn select_node_or_click(
  session: &auv_driver_linux::LinuxDriverSession,
  window: &auv_driver::Window,
  matched: &MatchedNode,
) -> Result<auv_driver::InputActionResult, String> {
  match session.accessibility().select_node(window, &matched.path) {
    Ok(result) => Ok(result),
    Err(select_error) => session
      .window()
      .click(
        window,
        window_point_for_node(window, matched),
        auv_driver::ClickOptions {
          click: auv_driver::Click::Single,
          ..auv_driver::ClickOptions::default()
        },
      )
      .map_err(|click_error| {
        format!(
          "failed to select {} at {}; AT-SPI select failed: {select_error}; click fallback failed: {click_error}",
          matched.label, matched.path
        )
      }),
  }
}

#[cfg(target_os = "linux")]
fn node_is_visible(window: &auv_driver::Window, node: &MatchedNode) -> bool {
  let center = node.bounds.center();
  center.y >= 0.0 && center.y <= window.frame.size.height && center.x >= 0.0 && center.x <= window.frame.size.width
}

#[cfg(target_os = "linux")]
fn scroll_toward_node(
  session: &auv_driver_linux::LinuxDriverSession,
  window: &auv_driver::Window,
  node: &MatchedNode,
) -> Result<(), String> {
  let center = node.bounds.center();
  let window_center_y = window.frame.size.height / 2.0;
  let delta_y = if center.y > window_center_y {
    300.0
  } else {
    -300.0
  };
  session
    .window()
    .scroll(
      window,
      auv_driver::WindowPoint::new(122.0, (window.frame.size.height - 55.0).max(46.0)),
      auv_driver::Scroll::new(0.0, delta_y),
      auv_driver::ScrollOptions::default(),
    )
    .map(|_| ())
    .map_err(|error| format!("failed to scroll Settings sidebar toward {}: {error}", node.label))
}

#[cfg(target_os = "linux")]
fn window_point_for_node(window: &auv_driver::Window, node: &MatchedNode) -> auv_driver::WindowPoint {
  let center = node.bounds.center();
  let x = center.x.clamp(0.0, window.frame.size.width);
  let y = center.y.clamp(0.0, window.frame.size.height);
  auv_driver::WindowPoint::new(x, y)
}
