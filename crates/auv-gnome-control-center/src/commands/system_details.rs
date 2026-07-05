use std::time::Duration;

use auv_driver::InputActionResult;
use serde::{Deserialize, Serialize};

use crate::interaction::{InteractionStep, StepOutcome};
use crate::views::MatchedNode;
use crate::windows::OpenWindowReport;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopySystemDetailsInputs {
  pub settle_ms: u64,
}

impl Default for CopySystemDetailsInputs {
  fn default() -> Self {
    Self { settle_ms: 8_000 }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CopySystemDetailsResult {
  pub command: &'static str,
  pub window: OpenWindowReport,
  pub steps: Vec<InteractionStep>,
  pub system_node: MatchedNode,
  pub details_node: MatchedNode,
  pub copy_node: MatchedNode,
  pub clipboard_text: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub delivery: Option<InputActionResult>,
}

pub fn run_copy_system_details(
  inputs: &CopySystemDetailsInputs,
) -> Result<CopySystemDetailsResult, String> {
  platform::run(inputs)
}

#[cfg(target_os = "linux")]
mod platform {
  use auv_driver::Driver;
  use auv_driver_linux::{LinuxDriver, LinuxDriverSession};

  use super::*;
  use crate::app::{COPY_BUTTON, SYSTEM_DETAILS_PAGE, SYSTEM_PAGE};
  use crate::views::{SettingsNode, find_labeled_node, visible_labels};
  use crate::windows::{ResolveOptions, open_or_resolve};

  pub fn run(inputs: &CopySystemDetailsInputs) -> Result<CopySystemDetailsResult, String> {
    let (window, open_report) = open_or_resolve(&ResolveOptions {
      settle: Duration::from_millis(inputs.settle_ms),
    })?;
    let session = LinuxDriver::new()
      .open_local()
      .map_err(|error| format!("failed to open Linux driver: {error}"))?;
    let mut steps = open_report.steps.clone();

    let system_node =
      select_labeled_node(&session, &window, SYSTEM_PAGE, "select-system", &mut steps)?;
    std::thread::sleep(Duration::from_millis(350));

    let details_node = select_labeled_node(
      &session,
      &window,
      SYSTEM_DETAILS_PAGE,
      "select-system-details",
      &mut steps,
    )?;
    std::thread::sleep(Duration::from_millis(350));

    let copy_node = select_labeled_node(
      &session,
      &window,
      COPY_BUTTON,
      "copy-system-details",
      &mut steps,
    )?;
    std::thread::sleep(Duration::from_millis(250));

    let clipboard_text = session
      .clipboard()
      .snapshot()
      .map_err(|error| format!("failed to read clipboard after copy: {error}"))?;
    steps.push(
      InteractionStep::new("read-clipboard", StepOutcome::Copied)
        .note(format!("{} bytes", clipboard_text.len())),
    );

    Ok(CopySystemDetailsResult {
      command: "copy-system-details",
      window: open_report,
      steps,
      system_node,
      details_node,
      copy_node,
      clipboard_text,
      delivery: None,
    })
  }

  fn select_labeled_node(
    session: &LinuxDriverSession,
    window: &auv_driver::Window,
    labels: crate::app::LabelSet,
    step_name: &str,
    steps: &mut Vec<InteractionStep>,
  ) -> Result<MatchedNode, String> {
    let nodes = snapshot_nodes(session, window)?;
    let matched = find_labeled_node(&nodes, labels).ok_or_else(|| {
      format!(
        "could not find one of [{}]; visible labels: {}",
        labels.display(),
        visible_labels(&nodes).join(" | ")
      )
    })?;
    let delivery = session
      .accessibility()
      .select_node(window, &matched.path)
      .map_err(|error| {
        format!(
          "failed to select {} at {}: {error}",
          matched.label, matched.path
        )
      })?;
    steps.push(
      InteractionStep::new(step_name, StepOutcome::Selected)
        .target(matched.label.clone())
        .note(format!("{delivery:?}")),
    );
    Ok(matched)
  }

  fn snapshot_nodes(
    session: &LinuxDriverSession,
    window: &auv_driver::Window,
  ) -> Result<Vec<SettingsNode>, String> {
    let snapshot = session
      .accessibility()
      .snapshot_window(window)
      .map_err(|error| format!("failed to capture AT-SPI tree: {error}"))?;
    Ok(snapshot.nodes.iter().map(SettingsNode::from).collect())
  }
}

#[cfg(not(target_os = "linux"))]
mod platform {
  use super::*;

  pub fn run(_inputs: &CopySystemDetailsInputs) -> Result<CopySystemDetailsResult, String> {
    Err("GNOME Control Center workflows are only supported on Linux".to_string())
  }
}
