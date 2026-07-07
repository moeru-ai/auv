use std::collections::BTreeMap;

use serde::Serialize;

use crate::support::ax_node_search_text;
use crate::types::{AuvResult, ObservedAxNode, ObservedWindowSnapshot, WindowCandidate};

#[derive(Serialize)]
struct WindowListJson<'a> {
  snapshot: &'a ObservedWindowSnapshot,
  candidates: &'a [WindowCandidate],
  candidate_resolution: Option<&'a str>,
}

pub fn render_window_list_json(
  snapshot: &ObservedWindowSnapshot,
  candidates: &[WindowCandidate],
  candidate_resolution: Option<&str>,
) -> AuvResult<String> {
  serde_json::to_string_pretty(&WindowListJson {
    snapshot,
    candidates,
    candidate_resolution,
  })
  .map(|mut rendered| {
    rendered.push('\n');
    rendered
  })
  .map_err(|error| format!("failed to encode window list JSON: {error}"))
}

pub fn render_window_snapshot_report(snapshot: &ObservedWindowSnapshot) -> String {
  let mut lines = vec![
    format!("observedAt={}", snapshot.observed_at),
    format!("frontmostAppName={}", snapshot.frontmost_app_name),
    format!("frontmostAppBundleId={}", snapshot.frontmost_app_bundle_id),
    format!("frontmostWindowTitle={}", snapshot.frontmost_window_title),
    format!("windowCount={}", snapshot.windows.len()),
  ];
  for window in &snapshot.windows {
    lines.push(format!(
      "window\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
      window.app_name,
      window.owner_pid,
      window.owner_bundle_id,
      window.window_number,
      window.layer,
      window.title,
      window.bounds.x,
      window.bounds.y,
      window.bounds.width,
      window.bounds.height
    ));
  }
  lines.join("\n") + "\n"
}

pub fn find_ax_text_node<'a>(
  nodes: &'a [ObservedAxNode],
  expected_text: &str,
  expected_role: Option<&str>,
  expected_subrole: Option<&str>,
  scope_path_prefix: Option<&str>,
) -> AuvResult<&'a ObservedAxNode> {
  let expected_text_lc = expected_text.trim().to_lowercase();
  let expected_role_lc = expected_role.map(|value| value.trim().to_lowercase()).filter(|value| !value.is_empty());
  let expected_subrole_lc = expected_subrole.map(|value| value.trim().to_lowercase()).filter(|value| !value.is_empty());
  let scope_path_prefix = scope_path_prefix.map(|value| value.trim().to_string()).filter(|value| !value.is_empty());

  nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter(|node| scope_path_prefix.as_ref().is_none_or(|prefix| node.path.starts_with(prefix)))
    .filter(|node| {
      if let Some(role) = expected_role_lc.as_deref() {
        node.role.to_lowercase() == role
      } else {
        true
      }
    })
    .filter(|node| {
      if let Some(subrole) = expected_subrole_lc.as_deref() {
        node.subrole.to_lowercase() == subrole
      } else {
        true
      }
    })
    .filter_map(|node| {
      let searchable = ax_node_search_text(node);
      if searchable.contains(&expected_text_lc) {
        Some((100 - node.depth as i64, node))
      } else {
        None
      }
    })
    .max_by(|left, right| left.0.cmp(&right.0))
    .map(|(_, node)| node)
    .ok_or_else(|| {
      let mut detail = format!("no matching ax text node found for target_text {expected_text}");
      if let Some(role) = expected_role {
        detail.push_str(&format!(" and target_role {role}"));
      }
      if let Some(subrole) = expected_subrole {
        detail.push_str(&format!(" and target_subrole {subrole}"));
      }
      if let Some(scope) = scope_path_prefix.as_deref() {
        detail.push_str(&format!(" within scope_path_prefix {scope}"));
      }
      detail
    })
}

pub fn permission_probe_report(
  screen_recording: &str,
  screen_capture_kit: &str,
  accessibility: &str,
  automation: &str,
  launch_host: &str,
) -> String {
  [
    format!("screenRecording={screen_recording}"),
    format!("screenCaptureKit={screen_capture_kit}"),
    format!("accessibility={accessibility}"),
    format!("automationToSystemEvents={automation}"),
    format!("launchHostProcess={launch_host}"),
  ]
  .join("\n")
    + "\n"
}

pub fn verify_now_playing_title_signals(matched_title: &str) -> BTreeMap<String, String> {
  let mut signals = BTreeMap::from([("ax.node_found".to_string(), "true".to_string())]);
  insert_optional_signal(&mut signals, "ax.now_playing_title", matched_title);
  signals
}

pub fn verify_ax_text_signals(matched_text: &str, matched_role: &str) -> BTreeMap<String, String> {
  let mut signals = BTreeMap::from([("ax.node_found".to_string(), "true".to_string())]);
  insert_optional_signal(&mut signals, "ax.matched_text", matched_text);
  insert_optional_signal(&mut signals, "ax.matched_role", matched_role);
  signals
}

pub fn ocr_detection_signals(filtered_match_count: usize, best_match_text: Option<&str>) -> BTreeMap<String, String> {
  let mut signals = BTreeMap::from([
    ("ocr.match_found".to_string(), (!filtered_match_count.eq(&0)).to_string()),
    ("ocr.filtered_match_count".to_string(), filtered_match_count.to_string()),
  ]);
  if let Some(best_match_text) = best_match_text {
    insert_optional_signal(&mut signals, "ocr.best_match_text", best_match_text);
  }
  signals
}

pub fn wait_ocr_detection_signals(filtered_match_count: usize, best_match_text: Option<&str>, timed_out: bool) -> BTreeMap<String, String> {
  let mut signals = ocr_detection_signals(filtered_match_count, best_match_text);
  signals.insert("ocr.timed_out".to_string(), timed_out.to_string());
  signals
}

pub fn row_detection_signals(row_count: usize) -> BTreeMap<String, String> {
  BTreeMap::from([
    ("rows.count".to_string(), row_count.to_string()),
    ("rows.visible".to_string(), (!row_count.eq(&0)).to_string()),
  ])
}

pub fn wait_row_detection_signals(row_count: usize, required_row_count: usize, timed_out: bool) -> BTreeMap<String, String> {
  let mut signals = row_detection_signals(row_count);
  signals.insert("rows.requirement_met".to_string(), (row_count >= required_row_count).to_string());
  signals.insert("rows.timed_out".to_string(), timed_out.to_string());
  signals
}

pub fn insert_optional_signal(signals: &mut BTreeMap<String, String>, key: &str, value: &str) {
  if !value.trim().is_empty() {
    signals.insert(key.to_string(), value.to_string());
  }
}

pub fn preferred_ax_signal_text(node: &ObservedAxNode) -> String {
  for value in [
    &node.value,
    &node.title,
    &node.description,
    &node.help,
    &node.placeholder,
  ] {
    if !value.trim().is_empty() {
      return value.clone();
    }
  }
  String::new()
}
