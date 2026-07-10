use std::time::Duration;

use auv_driver::{Rect, Window};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "linux")]
use crate::app::{APP_ID, DISPLAY_NAME, PROCESS_NAME, SETTINGS_WINDOW};
use crate::interaction::InteractionStep;
#[cfg(target_os = "linux")]
use crate::interaction::StepOutcome;

#[cfg(target_os = "linux")]
const POLL_INTERVAL_MS: u64 = 250;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveOptions {
  pub settle: Duration,
}

impl Default for ResolveOptions {
  fn default() -> Self {
    Self {
      settle: Duration::from_secs(8),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenWindowReport {
  pub window_found: bool,
  pub window_title: Option<String>,
  pub window_ref: Option<String>,
  pub app_name: Option<String>,
  pub frame: Option<Rect>,
  pub app_id: &'static str,
  pub process_name: &'static str,
  pub steps: Vec<InteractionStep>,
}

pub fn open_or_resolve(options: &ResolveOptions) -> Result<(Window, OpenWindowReport), String> {
  platform::open_or_resolve(options)
}

#[cfg(target_os = "linux")]
mod platform {
  use std::process::Command;
  use std::process::Stdio;
  use std::time::Instant;

  use auv_driver::DriverError;
  use auv_driver::selector::{AppSelector, TextMatcher, Window as SelectWindow, WindowSelector};
  use auv_driver_linux::LinuxDriverSession;

  use super::*;
  pub fn open_or_resolve(options: &ResolveOptions) -> Result<(Window, OpenWindowReport), String> {
    let session = auv_driver::open_local().map_err(|error| format!("failed to open Linux driver: {error}"))?;
    let mut report = report();

    match resolve_window(&session) {
      Ok(window) => {
        report.steps.push(InteractionStep::new("resolve", StepOutcome::Found));
        record_window(&mut report, &window);
        return Ok((window, report));
      }
      Err(DriverError::NotFound { .. }) => {
        report.steps.push(InteractionStep::new("resolve", StepOutcome::NotFound));
      }
      Err(error) => return Err(format!("failed to resolve {DISPLAY_NAME}: {error}")),
    }

    Command::new(PROCESS_NAME)
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::null())
      .spawn()
      .map_err(|error| format!("failed to launch {PROCESS_NAME}: {error}"))?;
    report.steps.push(InteractionStep::new("launch", StepOutcome::Started));

    let deadline = Instant::now() + options.settle;
    loop {
      match resolve_window(&session) {
        Ok(window) => {
          report.steps.push(InteractionStep::new("wait", StepOutcome::Found));
          record_window(&mut report, &window);
          return Ok((window, report));
        }
        Err(DriverError::NotFound { .. }) if Instant::now() < deadline => {
          std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
        }
        Err(DriverError::NotFound { .. }) => {
          report
            .steps
            .push(InteractionStep::new("wait", StepOutcome::NotFound).note(format!("no visible {DISPLAY_NAME} window before timeout")));
          return Err(format!("no visible {DISPLAY_NAME} window appeared"));
        }
        Err(error) => return Err(format!("failed while waiting for {DISPLAY_NAME}: {error}")),
      }
    }
  }

  pub fn resolve_window(session: &LinuxDriverSession) -> Result<Window, DriverError> {
    let selectors = [
      WindowSelector::default().owned_by(AppSelector {
        bundle: Some(TextMatcher::Contains(APP_ID.to_string())),
        ..AppSelector::default()
      }),
      WindowSelector::default().owned_by(AppSelector {
        name: Some(TextMatcher::Contains(PROCESS_NAME.to_string())),
        ..AppSelector::default()
      }),
      SelectWindow::title_contains("Settings"),
      SelectWindow::title_contains("设置"),
      SelectWindow::main_visible(),
    ];
    for selector in selectors {
      match session.window().resolve(selector) {
        Ok(window) if matches_settings_window(&window) => return Ok(window),
        Ok(_) => {}
        Err(DriverError::NotFound { .. }) => {}
        Err(error) => return Err(error),
      }
    }
    Err(DriverError::NotFound {
      target: DISPLAY_NAME.to_string(),
    })
  }

  fn matches_settings_window(window: &Window) -> bool {
    window.app_bundle_id.as_deref().is_some_and(|id| id.contains(APP_ID))
      || window.app_name.as_deref().is_some_and(|name| name.contains(PROCESS_NAME) || name.contains("Settings"))
      || window.title.as_deref().is_some_and(|title| SETTINGS_WINDOW.best_match(title).is_some())
  }
}

#[cfg(not(target_os = "linux"))]
mod platform {
  use super::*;

  pub fn open_or_resolve(_options: &ResolveOptions) -> Result<(Window, OpenWindowReport), String> {
    Err("GNOME Control Center workflows are only supported on Linux".to_string())
  }
}

#[cfg(target_os = "linux")]
fn record_window(report: &mut OpenWindowReport, window: &Window) {
  report.window_found = true;
  report.window_title = window.title.clone();
  report.window_ref = Some(window.reference.id.clone());
  report.app_name = window.app_name.clone();
  report.frame = Some(window.frame);
}

#[cfg(target_os = "linux")]
fn report() -> OpenWindowReport {
  OpenWindowReport {
    window_found: false,
    window_title: None,
    window_ref: None,
    app_name: None,
    frame: None,
    app_id: APP_ID,
    process_name: PROCESS_NAME,
    steps: Vec::new(),
  }
}
