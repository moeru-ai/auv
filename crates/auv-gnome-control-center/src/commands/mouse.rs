use std::time::Duration;

use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::interaction::{InteractionStep, StepOutcome};
use crate::views::MatchedNode;
use crate::windows::OpenWindowReport;

#[derive(Clone, Debug, PartialEq)]
pub struct PointerSpeedSetInputs {
  pub position: f64,
  pub settle_ms: u64,
}

impl Default for PointerSpeedSetInputs {
  fn default() -> Self {
    Self {
      position: 0.5,
      settle_ms: 8_000,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointerSpeedRoundtripInputs {
  pub first_position: f64,
  pub restore_position: f64,
  pub settle_ms: u64,
}

impl Default for PointerSpeedRoundtripInputs {
  fn default() -> Self {
    Self {
      first_position: 0.75,
      restore_position: 0.5,
      settle_ms: 8_000,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct NaturalScrollingToggleInputs {
  pub settle_ms: u64,
}

impl Default for NaturalScrollingToggleInputs {
  fn default() -> Self {
    Self { settle_ms: 8_000 }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointerSpeedSetResult {
  pub command: &'static str,
  pub window: OpenWindowReport,
  pub steps: Vec<InteractionStep>,
  pub mouse_node: MatchedNode,
  pub slider_node: MatchedNode,
  pub requested_position: f64,
  pub clicked_point: WindowPoint,
  pub delivery: InputActionResult,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointerSpeedRoundtripResult {
  pub command: &'static str,
  pub first: PointerSpeedSetResult,
  pub restore: PointerSpeedSetResult,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NaturalScrollingToggleResult {
  pub command: &'static str,
  pub window: OpenWindowReport,
  pub steps: Vec<InteractionStep>,
  pub mouse_node: MatchedNode,
  pub switch_node: MatchedNode,
  pub observed_value_before: Option<String>,
  pub observed_value_after: Option<String>,
  pub delivery: InputActionResult,
}

pub fn run_pointer_speed_set(
  inputs: &PointerSpeedSetInputs,
) -> Result<PointerSpeedSetResult, String> {
  platform::run_set(inputs)
}

pub fn run_pointer_speed_roundtrip(
  inputs: &PointerSpeedRoundtripInputs,
) -> Result<PointerSpeedRoundtripResult, String> {
  platform::run_roundtrip(inputs)
}

pub fn run_natural_scrolling_toggle(
  inputs: &NaturalScrollingToggleInputs,
) -> Result<NaturalScrollingToggleResult, String> {
  platform::run_toggle_natural_scrolling(inputs)
}

#[cfg(target_os = "linux")]
mod platform {
  use std::process::Command;

  use auv_driver::{Click, ClickOptions, Driver};
  use auv_driver_linux::{LinuxDriver, LinuxDriverSession};

  use super::*;
  use crate::app::{MOUSE_PAGE, NATURAL_SCROLLING, POINTER_SPEED, TRADITIONAL_SCROLLING};
  use crate::commands::{click_visible_labeled_node_with_delivery, select_visible_labeled_node};
  use crate::views::{SettingsNode, find_slider_near_label, visible_labels};
  use crate::windows::{ResolveOptions, open_or_resolve};

  pub fn run_set(inputs: &PointerSpeedSetInputs) -> Result<PointerSpeedSetResult, String> {
    validate_position(inputs.position)?;
    let (window, open_report) = open_or_resolve(&ResolveOptions {
      settle: Duration::from_millis(inputs.settle_ms),
    })?;
    let session = LinuxDriver::new()
      .open_local()
      .map_err(|error| format!("failed to open Linux driver: {error}"))?;
    set_pointer_speed(&session, &window, open_report, inputs.position)
  }

  pub fn run_roundtrip(
    inputs: &PointerSpeedRoundtripInputs,
  ) -> Result<PointerSpeedRoundtripResult, String> {
    validate_position(inputs.first_position)?;
    validate_position(inputs.restore_position)?;
    let first = run_set(&PointerSpeedSetInputs {
      position: inputs.first_position,
      settle_ms: inputs.settle_ms,
    })?;
    std::thread::sleep(Duration::from_millis(300));
    let restore = run_set(&PointerSpeedSetInputs {
      position: inputs.restore_position,
      settle_ms: inputs.settle_ms,
    })?;
    Ok(PointerSpeedRoundtripResult {
      command: "mouse.roundtrip-pointer-speed",
      first,
      restore,
    })
  }

  pub fn run_toggle_natural_scrolling(
    inputs: &NaturalScrollingToggleInputs,
  ) -> Result<NaturalScrollingToggleResult, String> {
    let (window, open_report) = open_or_resolve(&ResolveOptions {
      settle: Duration::from_millis(inputs.settle_ms),
    })?;
    let session = LinuxDriver::new()
      .open_local()
      .map_err(|error| format!("failed to open Linux driver: {error}"))?;
    toggle_natural_scrolling(&session, &window, open_report)
  }

  fn set_pointer_speed(
    session: &LinuxDriverSession,
    window: &auv_driver::Window,
    open_report: OpenWindowReport,
    position: f64,
  ) -> Result<PointerSpeedSetResult, String> {
    let mut steps = open_report.steps.clone();
    let mouse_node = select_mouse_page(session, window, &mut steps)?;
    std::thread::sleep(Duration::from_millis(350));
    let nodes = snapshot_nodes(session, window)?;
    let slider_node = find_slider_near_label(&nodes, POINTER_SPEED).ok_or_else(|| {
      format!(
        "could not find pointer speed slider near [{}]; visible labels: {}",
        POINTER_SPEED.display(),
        visible_labels(&nodes).join(" | ")
      )
    })?;
    let clicked_point = slider_click_point(window, &slider_node, position);
    let delivery = session
      .window()
      .click(
        window,
        clicked_point,
        ClickOptions {
          click: Click::Single,
          ..ClickOptions::default()
        },
      )
      .map_err(|error| {
        format!(
          "failed to click pointer speed slider at position {:.2}: {error}",
          position
        )
      })?;
    steps.push(
      InteractionStep::new("set-pointer-speed", StepOutcome::Clicked)
        .target(format!("{position:.2}"))
        .note(format!("{delivery:?}")),
    );
    Ok(PointerSpeedSetResult {
      command: "mouse.set-pointer-speed",
      window: open_report,
      steps,
      mouse_node,
      slider_node,
      requested_position: position,
      clicked_point,
      delivery,
    })
  }

  fn toggle_natural_scrolling(
    session: &LinuxDriverSession,
    window: &auv_driver::Window,
    open_report: OpenWindowReport,
  ) -> Result<NaturalScrollingToggleResult, String> {
    let mut steps = open_report.steps.clone();
    let mouse_node = select_mouse_page(session, window, &mut steps)?;
    std::thread::sleep(Duration::from_millis(350));

    let natural_scroll_before = read_natural_scroll_setting()?;
    let target = if natural_scroll_before {
      TRADITIONAL_SCROLLING
    } else {
      NATURAL_SCROLLING
    };
    let (switch_node, delivery) = click_visible_labeled_node_with_delivery(
      session,
      window,
      target,
      "toggle-natural-scrolling",
      &mut steps,
    )?;
    std::thread::sleep(Duration::from_millis(250));
    let natural_scroll_after = read_natural_scroll_setting()?;
    if natural_scroll_after == natural_scroll_before {
      return Err(format!(
        "clicked {}, but natural-scroll remained {}",
        switch_node.label, natural_scroll_after
      ));
    }
    Ok(NaturalScrollingToggleResult {
      command: "mouse.toggle-natural-scrolling",
      window: open_report,
      steps,
      mouse_node,
      switch_node,
      observed_value_before: Some(natural_scroll_before.to_string()),
      observed_value_after: Some(natural_scroll_after.to_string()),
      delivery,
    })
  }

  fn select_mouse_page(
    session: &LinuxDriverSession,
    window: &auv_driver::Window,
    steps: &mut Vec<InteractionStep>,
  ) -> Result<MatchedNode, String> {
    select_visible_labeled_node(session, window, MOUSE_PAGE, "select-mouse", steps)
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

  fn read_natural_scroll_setting() -> Result<bool, String> {
    let output = Command::new("gsettings")
      .args([
        "get",
        "org.gnome.desktop.peripherals.mouse",
        "natural-scroll",
      ])
      .output()
      .map_err(|error| format!("failed to run gsettings: {error}"))?;
    if !output.status.success() {
      return Err(format!(
        "gsettings natural-scroll read failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
      ));
    }
    match String::from_utf8_lossy(&output.stdout).trim() {
      "true" => Ok(true),
      "false" => Ok(false),
      value => Err(format!(
        "unexpected gsettings natural-scroll value: {value}"
      )),
    }
  }

  fn slider_click_point(
    window: &auv_driver::Window,
    slider: &MatchedNode,
    position: f64,
  ) -> WindowPoint {
    let position = position.clamp(0.0, 1.0);
    let screen = slider.bounds.center();
    WindowPoint::new(
      slider.bounds.origin.x + slider.bounds.size.width * position - window.frame.origin.x,
      screen.y - window.frame.origin.y,
    )
  }

  fn validate_position(position: f64) -> Result<(), String> {
    if !position.is_finite() || !(0.0..=1.0).contains(&position) {
      return Err("pointer speed position must be a finite number between 0 and 1".to_string());
    }
    Ok(())
  }
}

#[cfg(not(target_os = "linux"))]
mod platform {
  use super::*;

  pub fn run_set(_inputs: &PointerSpeedSetInputs) -> Result<PointerSpeedSetResult, String> {
    Err("GNOME Control Center workflows are only supported on Linux".to_string())
  }

  pub fn run_roundtrip(
    _inputs: &PointerSpeedRoundtripInputs,
  ) -> Result<PointerSpeedRoundtripResult, String> {
    Err("GNOME Control Center workflows are only supported on Linux".to_string())
  }

  pub fn run_toggle_natural_scrolling(
    _inputs: &NaturalScrollingToggleInputs,
  ) -> Result<NaturalScrollingToggleResult, String> {
    Err("GNOME Control Center workflows are only supported on Linux".to_string())
  }
}
