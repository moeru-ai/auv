#[cfg(target_os = "linux")]
use std::time::Duration;

use auv_driver::InputActionResult;
use serde::{Deserialize, Serialize};

use crate::interaction::InteractionStep;
#[cfg(target_os = "linux")]
use crate::interaction::StepOutcome;
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
  pub about_node: MatchedNode,
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
  use std::time::Instant;

  use auv_driver::Driver;
  use auv_driver_linux::{LinuxDriver, LinuxDriverSession};

  use super::*;
  use crate::app::{ABOUT_PAGE, COPY_BUTTON, SYSTEM_DETAILS_PAGE, SYSTEM_PAGE};
  use crate::commands::{click_visible_labeled_node, select_visible_labeled_node};
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
      select_visible_labeled_node(&session, &window, SYSTEM_PAGE, "select-system", &mut steps)?;
    std::thread::sleep(Duration::from_millis(350));

    let about_node =
      select_visible_labeled_node(&session, &window, ABOUT_PAGE, "select-about", &mut steps)?;
    std::thread::sleep(Duration::from_millis(350));

    let details_node = select_visible_labeled_node(
      &session,
      &window,
      SYSTEM_DETAILS_PAGE,
      "select-system-details",
      &mut steps,
    )?;
    std::thread::sleep(Duration::from_millis(350));

    let clipboard_before = session.clipboard().snapshot().unwrap_or_default();
    let copy_node = click_visible_labeled_node(
      &session,
      &window,
      COPY_BUTTON,
      "copy-system-details",
      &mut steps,
    )?;

    let clipboard_text =
      wait_for_clipboard_text(&session, &clipboard_before, Duration::from_secs(2))?;
    steps.push(
      InteractionStep::new("read-clipboard", StepOutcome::Copied)
        .note(format!("{} bytes", clipboard_text.len())),
    );

    Ok(CopySystemDetailsResult {
      command: "copy-system-details",
      window: open_report,
      steps,
      system_node,
      about_node,
      details_node,
      copy_node,
      clipboard_text,
      delivery: None,
    })
  }

  fn wait_for_clipboard_text(
    session: &LinuxDriverSession,
    previous: &str,
    timeout: Duration,
  ) -> Result<String, String> {
    let deadline = Instant::now() + timeout;
    let mut last_text = String::new();
    while Instant::now() < deadline {
      last_text = session
        .clipboard()
        .snapshot()
        .map_err(|error| format!("failed to read clipboard after copy: {error}"))?;
      if !last_text.trim().is_empty() && last_text != previous {
        return Ok(last_text);
      }
      std::thread::sleep(Duration::from_millis(100));
    }
    if !last_text.trim().is_empty() {
      return Ok(last_text);
    }
    Err("copy-system-details clicked Copy, but clipboard remained empty".to_string())
  }
}

#[cfg(not(target_os = "linux"))]
mod platform {
  use super::*;

  pub fn run(_inputs: &CopySystemDetailsInputs) -> Result<CopySystemDetailsResult, String> {
    Err("GNOME Control Center workflows are only supported on Linux".to_string())
  }
}
