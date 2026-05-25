// File: src/driver/macos/ax_tree/mod.rs
use std::thread;
use std::time::Duration;

use super::*;

pub(crate) fn capture_ax_tree(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let reveal_shortcut = optional_non_empty_string(call, "reveal_shortcut");
  let reveal_settle_ms = optional_positive_u64(call, "reveal_settle_ms")?.unwrap_or(250);
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(5).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(12)
    .clamp(1, 50);
  if !app.is_empty() {
    activate_target_app(&app)?;
  }
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    send_shortcut(shortcut)?;
    thread::sleep(Duration::from_millis(reveal_settle_ms));
  }
  let capture =
    crate::driver::macos::native::ax_tree::capture_ax_tree_snapshot(&app, max_depth, max_children)?;
  let snapshot = &capture.snapshot;
  let report = crate::driver::macos::native::ax_tree::render_ax_tree_report(&capture);
  let app_name = snapshot.app_name.clone();
  let bundle_id = snapshot.bundle_id.clone();
  let window_title = snapshot.window_title.clone();
  let captured_at = snapshot.observed_at.clone();
  let node_count = snapshot.nodes.len();
  let artifact = build_text_artifact(
    "ax-tree",
    "txt",
    &format!(
      "ax-tree-{}",
      sanitize_file_component(if app_name.is_empty() {
        "app"
      } else {
        &app_name
      })
    ),
    report.clone(),
    "Captured an AX tree snapshot for the target macOS app window.",
  )?;
  let mut notes = vec![format!("capturedAt={captured_at}")];
  if !bundle_id.is_empty() {
    notes.push(format!("bundleId={bundle_id}"));
  }
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    notes.push(format!("revealShortcut={shortcut}"));
    notes.push(format!("revealSettleMs={reveal_settle_ms}"));
  }
  for line in report
    .lines()
    .filter(|line| line.starts_with("node\t"))
    .take(8)
  {
    notes.push(line.to_string());
  }

  let summary = if app_name.is_empty() {
    format!("Captured window AX tree with {} node(s).", node_count)
  } else if window_title.is_empty() {
    format!("Captured {} AX node(s) for app {}.", node_count, app_name)
  } else {
    format!(
      "Captured {} AX node(s) for app {} window {}.",
      node_count, app_name, window_title
    )
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.desktop.capture-ax-tree".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes,
    artifacts: vec![artifact],
  })
}
