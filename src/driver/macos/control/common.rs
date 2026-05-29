// File: src/driver/macos/control/common.rs
use std::thread;
use std::time::Duration;

use super::super::{DriverCall, ObservedAxNode};
use super::super::support::runtime::{activate_target_app, send_shortcut};
use super::super::{optional_positive_u64, render_rect_compact};
use crate::model::{DriverRunContext, ExecutionTarget};
use crate::model::AuvResult;

pub(crate) const DEFAULT_CLICK_INTERVAL_MS: u64 = 80;
pub(crate) const MAX_CLICK_INTERVAL_MS: u64 = 1000;

pub(crate) struct ClickPointCallOptions<'a> {
  pub(crate) x: f64,
  pub(crate) y: f64,
  pub(crate) button: &'a str,
  pub(crate) click_count: i64,
  pub(crate) click_interval_ms: Option<u64>,
  pub(crate) settle_ms: Option<u64>,
  pub(crate) app: Option<&'a str>,
}

pub(super) fn activate_app_if_needed(app: &str) -> AuvResult<()> {
  if !app.is_empty() {
    activate_target_app(app)?;
  }
  Ok(())
}

pub(crate) fn resolve_click_interval_ms(call: &DriverCall) -> AuvResult<u64> {
  let value =
    optional_positive_u64(call, "click_interval_ms")?.unwrap_or(DEFAULT_CLICK_INTERVAL_MS);
  Ok(value.min(MAX_CLICK_INTERVAL_MS))
}

pub(super) fn send_reveal_shortcut_if_needed(
  reveal_shortcut: Option<&str>,
  reveal_settle_ms: u64,
) -> AuvResult<()> {
  if let Some(shortcut) = reveal_shortcut {
    send_shortcut(shortcut)?;
    thread::sleep(Duration::from_millis(reveal_settle_ms));
  }
  Ok(())
}

pub(crate) fn build_click_point_call(
  target: &ExecutionTarget,
  working_directory: &std::path::Path,
  run_context: DriverRunContext,
  options: ClickPointCallOptions<'_>,
) -> DriverCall {
  let mut inputs = std::collections::BTreeMap::from([
    ("x".to_string(), format!("{:.3}", options.x)),
    ("y".to_string(), format!("{:.3}", options.y)),
    ("button".to_string(), options.button.to_string()),
    ("click_count".to_string(), options.click_count.to_string()),
  ]);
  if let Some(click_interval_ms) = options.click_interval_ms {
    inputs.insert(
      "click_interval_ms".to_string(),
      click_interval_ms.to_string(),
    );
  }
  if let Some(settle_ms) = options.settle_ms {
    inputs.insert("settle_ms".to_string(), settle_ms.to_string());
  }
  if let Some(app) = options.app.filter(|value| !value.is_empty()) {
    inputs.insert("app".to_string(), app.to_string());
  }

  DriverCall {
    operation: "click_point".to_string(),
    target: target.clone(),
    inputs,
    working_directory: working_directory.to_path_buf(),
    run_context,
  }
}

pub(super) fn build_ax_click_notes(
  query: &str,
  matched: &ObservedAxNode,
  center_x: f64,
  center_y: f64,
) -> Vec<String> {
  let mut notes = vec![
    format!("query={query}"),
    format!("matchedPath={}", matched.path),
    format!("matchedRole={}", matched.role),
    format!("matchedBounds={}", render_rect_compact(&matched.bounds)),
    format!("clickLogicalPoint={center_x:.3},{center_y:.3}"),
  ];
  if !matched.description.is_empty() {
    notes.push(format!("matchedDescription={}", matched.description));
  }
  if !matched.title.is_empty() {
    notes.push(format!("matchedTitle={}", matched.title));
  }
  notes
}
