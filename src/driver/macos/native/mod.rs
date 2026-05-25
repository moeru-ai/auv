// TODO(driver-crates): temporary root-native compatibility while the macOS
// driver and overlay crates are split across follow-up tasks. These modules
// adapt the root driver's old local types to the moved `auv-driver-macos`
// native implementation without restoring the root Swift bridge build.
use super::types as root;

#[cfg(target_os = "macos")]
use auv_driver_macos::native::types as moved;

#[cfg(target_os = "macos")]
fn to_moved_rect(rect: &root::ObservedRect) -> moved::ObservedRect {
  moved::ObservedRect {
    x: rect.x,
    y: rect.y,
    width: rect.width,
    height: rect.height,
  }
}

#[cfg(target_os = "macos")]
fn from_moved_rect(rect: moved::ObservedRect) -> root::ObservedRect {
  root::ObservedRect {
    x: rect.x,
    y: rect.y,
    width: rect.width,
    height: rect.height,
  }
}

#[cfg(target_os = "macos")]
fn from_moved_display(display: moved::ObservedDisplay) -> root::ObservedDisplay {
  root::ObservedDisplay {
    display_id: display.display_id,
    is_main: display.is_main,
    is_built_in: display.is_built_in,
    bounds: from_moved_rect(display.bounds),
    visible_bounds: from_moved_rect(display.visible_bounds),
    scale_factor: display.scale_factor,
    pixel_width: display.pixel_width,
    pixel_height: display.pixel_height,
  }
}

#[cfg(target_os = "macos")]
fn from_moved_window(window: moved::ObservedWindow) -> root::ObservedWindow {
  root::ObservedWindow {
    window_number: window.window_number,
    app_name: window.app_name,
    owner_pid: window.owner_pid,
    owner_bundle_id: window.owner_bundle_id,
    layer: window.layer,
    title: window.title,
    bounds: from_moved_rect(window.bounds),
  }
}

#[cfg(target_os = "macos")]
fn from_moved_ocr_match(matched: moved::OcrTextMatch) -> root::OcrTextMatch {
  root::OcrTextMatch {
    match_index: matched.match_index,
    text: matched.text,
    confidence: matched.confidence,
    bounds: from_moved_rect(matched.bounds),
  }
}

#[cfg(target_os = "macos")]
fn from_moved_ocr_row(row: moved::ObservedOcrRow) -> root::ObservedOcrRow {
  root::ObservedOcrRow {
    row_index: row.row_index,
    source: row.source,
    bounds: from_moved_rect(row.bounds),
    text_fragments: row.text_fragments,
  }
}

#[cfg(target_os = "macos")]
fn from_moved_ocr_snapshot(snapshot: moved::OcrTextSnapshot) -> root::OcrTextSnapshot {
  root::OcrTextSnapshot {
    recognized_at: snapshot.recognized_at,
    image_path: snapshot.image_path,
    image_width: snapshot.image_width,
    image_height: snapshot.image_height,
    query: snapshot.query,
    exact: snapshot.exact,
    case_sensitive: snapshot.case_sensitive,
    matches: snapshot
      .matches
      .into_iter()
      .map(from_moved_ocr_match)
      .collect(),
  }
}

#[cfg(target_os = "macos")]
fn from_moved_rows(rows: moved::DetectedScreenRows) -> root::DetectedScreenRows {
  root::DetectedScreenRows {
    strategy: rows.strategy,
    raw_match_count: rows.raw_match_count,
    filtered_match_count: rows.filtered_match_count,
    rows: rows.rows.into_iter().map(from_moved_ocr_row).collect(),
    report: rows.report,
  }
}

#[cfg(target_os = "macos")]
fn from_moved_ax_node(node: moved::ObservedAxNode) -> root::ObservedAxNode {
  root::ObservedAxNode {
    depth: node.depth,
    path: node.path,
    role: node.role,
    subrole: node.subrole,
    title: node.title,
    description: node.description,
    help: node.help,
    identifier: node.identifier,
    placeholder: node.placeholder,
    value: node.value,
    bounds: from_moved_rect(node.bounds),
  }
}

#[cfg(target_os = "macos")]
fn from_moved_ax_snapshot(snapshot: moved::ObservedAxTreeSnapshot) -> root::ObservedAxTreeSnapshot {
  root::ObservedAxTreeSnapshot {
    observed_at: snapshot.observed_at,
    app_name: snapshot.app_name,
    bundle_id: snapshot.bundle_id,
    pid: snapshot.pid,
    window_title: snapshot.window_title,
    nodes: snapshot.nodes.into_iter().map(from_moved_ax_node).collect(),
  }
}

pub(crate) mod ax_tree {
  use crate::model::AuvResult;

  #[cfg(target_os = "macos")]
  use auv_driver_macos::native::ax_tree as moved_ax_tree;

  #[derive(Clone, Debug, PartialEq, Eq)]
  pub(crate) struct NativeAxTreeCapture {
    pub(crate) snapshot: super::root::ObservedAxTreeSnapshot,
    pub(crate) pid: i64,
    pub(crate) root_role: String,
  }

  #[derive(Clone, Debug, PartialEq, Eq)]
  pub(crate) struct NativeAxAction {
    pub(crate) performed_action: String,
    pub(crate) available_actions: String,
  }

  #[derive(Clone, Debug, PartialEq, Eq)]
  pub(crate) struct NativeAxFocus {
    pub(crate) set_attribute: String,
    pub(crate) was_already_focused: bool,
    pub(crate) role: String,
    pub(crate) subrole: String,
    pub(crate) title: String,
    pub(crate) description: String,
    pub(crate) identifier: String,
    pub(crate) placeholder: String,
    pub(crate) bounds: super::root::ObservedRect,
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn capture_ax_tree_snapshot(
    app: &str,
    max_depth: i64,
    max_children: i64,
  ) -> AuvResult<NativeAxTreeCapture> {
    let capture = moved_ax_tree::capture_ax_tree_snapshot(app, max_depth, max_children)?;
    Ok(NativeAxTreeCapture {
      snapshot: super::from_moved_ax_snapshot(capture.snapshot),
      pid: capture.pid,
      root_role: capture.root_role,
    })
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn capture_ax_tree_snapshot(
    _app: &str,
    _max_depth: i64,
    _max_children: i64,
  ) -> AuvResult<NativeAxTreeCapture> {
    Err("macOS native AX tree capture is unsupported on this target".to_string())
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn perform_ax_path_action(
    pid: i32,
    path: &str,
    expected_role: &str,
    action_name: &str,
  ) -> AuvResult<NativeAxAction> {
    let action = moved_ax_tree::perform_ax_path_action(pid, path, expected_role, action_name)?;
    Ok(NativeAxAction {
      performed_action: action.performed_action,
      available_actions: action.available_actions,
    })
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn perform_ax_path_action(
    _pid: i32,
    _path: &str,
    _expected_role: &str,
    _action_name: &str,
  ) -> AuvResult<NativeAxAction> {
    Err("macOS native AX action dispatch is unsupported on this target".to_string())
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn set_ax_focused_path(
    pid: i32,
    path: &str,
    expected_role: &str,
  ) -> AuvResult<NativeAxFocus> {
    let focus = moved_ax_tree::set_ax_focused_path(pid, path, expected_role)?;
    Ok(NativeAxFocus {
      set_attribute: focus.set_attribute,
      was_already_focused: focus.was_already_focused,
      role: focus.role,
      subrole: focus.subrole,
      title: focus.title,
      description: focus.description,
      identifier: focus.identifier,
      placeholder: focus.placeholder,
      bounds: super::from_moved_rect(focus.bounds),
    })
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn set_ax_focused_path(
    _pid: i32,
    _path: &str,
    _expected_role: &str,
  ) -> AuvResult<NativeAxFocus> {
    Err("macOS native AX focus dispatch is unsupported on this target".to_string())
  }

  pub(crate) fn render_ax_tree_report(capture: &NativeAxTreeCapture) -> String {
    let snapshot = &capture.snapshot;
    let mut lines = vec![
      format!("observedAt={}", snapshot.observed_at),
      format!("appName={}", snapshot.app_name),
      format!("bundleId={}", snapshot.bundle_id),
      format!("pid={}", capture.pid),
      format!("windowTitle={}", snapshot.window_title),
      format!("rootRole={}", capture.root_role),
    ];
    for node in &snapshot.nodes {
      lines.push(format!(
        "node\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        node.depth,
        node.path,
        node.role,
        node.subrole,
        node.title,
        node.description,
        node.help,
        node.identifier,
        node.placeholder,
        node.value,
        node.bounds.x,
        node.bounds.y,
        node.bounds.width,
        node.bounds.height
      ));
    }
    lines.push(format!("nodeCount={}", snapshot.nodes.len()));
    lines.join("\n") + "\n"
  }
}

pub(crate) mod clipboard {
  use crate::model::AuvResult;

  #[cfg(target_os = "macos")]
  pub(crate) fn capture_clipboard_snapshot() -> AuvResult<String> {
    auv_driver_macos::native::clipboard::capture_clipboard_snapshot()
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn capture_clipboard_snapshot() -> AuvResult<String> {
    Err("macOS native clipboard capture is unsupported on this target".to_string())
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn restore_clipboard_snapshot(snapshot_payload: &str) -> AuvResult<()> {
    auv_driver_macos::native::clipboard::restore_clipboard_snapshot(snapshot_payload)
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn restore_clipboard_snapshot(_snapshot_payload: &str) -> AuvResult<()> {
    Err("macOS native clipboard restore is unsupported on this target".to_string())
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn set_clipboard_text(text: &str) -> AuvResult<()> {
    auv_driver_macos::native::clipboard::set_clipboard_text(text)
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn set_clipboard_text(_text: &str) -> AuvResult<()> {
    Err("macOS native clipboard set text is unsupported on this target".to_string())
  }
}

pub(crate) mod error {
  #[allow(unused_imports)]
  pub(crate) use auv_driver_macos::native::error::*;
}

pub(crate) mod ocr {
  use std::path::{Path, PathBuf};

  use crate::model::AuvResult;

  #[cfg(target_os = "macos")]
  use auv_driver_macos::native::ocr as moved_ocr;

  #[derive(Clone, Debug, PartialEq)]
  pub(crate) struct NativeOcrTextCapture {
    pub(crate) snapshot: super::root::OcrTextSnapshot,
    pub(crate) normalized_query: String,
    pub(crate) crop_rect: Option<super::root::ObservedRect>,
    pub(crate) ocr_scale_factor: f64,
  }

  #[derive(Clone, Debug, PartialEq)]
  pub(crate) struct NativeVisualRowsCapture {
    pub(crate) rows: super::root::DetectedScreenRows,
    pub(crate) detected_at: String,
    pub(crate) image_path: PathBuf,
    pub(crate) image_width: i64,
    pub(crate) image_height: i64,
    pub(crate) crop_rect: Option<super::root::ObservedRect>,
    pub(crate) analysis_strip: super::root::ObservedRect,
    pub(crate) peak_densities: Vec<f64>,
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn find_text(
    image_path: &Path,
    query: &str,
    exact: bool,
    case_sensitive: bool,
    max_observations: i64,
    crop_region: Option<&super::root::ObservedRect>,
  ) -> AuvResult<NativeOcrTextCapture> {
    let moved_crop = crop_region.map(super::to_moved_rect);
    let capture = moved_ocr::find_text(
      image_path,
      query,
      exact,
      case_sensitive,
      max_observations,
      moved_crop.as_ref(),
    )?;
    Ok(NativeOcrTextCapture {
      snapshot: super::from_moved_ocr_snapshot(capture.snapshot),
      normalized_query: capture.normalized_query,
      crop_rect: capture.crop_rect.map(super::from_moved_rect),
      ocr_scale_factor: capture.ocr_scale_factor,
    })
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn find_text(
    _image_path: &Path,
    _query: &str,
    _exact: bool,
    _case_sensitive: bool,
    _max_observations: i64,
    _crop_region: Option<&super::root::ObservedRect>,
  ) -> AuvResult<NativeOcrTextCapture> {
    Err("macOS native OCR text detection is unsupported on this target".to_string())
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn find_rows(
    image_path: &Path,
    crop_region: Option<&super::root::ObservedRect>,
  ) -> AuvResult<NativeVisualRowsCapture> {
    let moved_crop = crop_region.map(super::to_moved_rect);
    let capture = moved_ocr::find_rows(image_path, moved_crop.as_ref())?;
    Ok(NativeVisualRowsCapture {
      rows: super::from_moved_rows(capture.rows),
      detected_at: capture.detected_at,
      image_path: capture.image_path,
      image_width: capture.image_width,
      image_height: capture.image_height,
      crop_rect: capture.crop_rect.map(super::from_moved_rect),
      analysis_strip: super::from_moved_rect(capture.analysis_strip),
      peak_densities: capture.peak_densities,
    })
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn find_rows(
    _image_path: &Path,
    _crop_region: Option<&super::root::ObservedRect>,
  ) -> AuvResult<NativeVisualRowsCapture> {
    Err("macOS native visual row detection is unsupported on this target".to_string())
  }

  pub(crate) fn render_ocr_text_report(capture: &NativeOcrTextCapture) -> String {
    let snapshot = &capture.snapshot;
    let mut lines = vec![
      format!("recognizedAt={}", snapshot.recognized_at),
      format!("imagePath={}", snapshot.image_path.display()),
      format!("imageWidth={}", snapshot.image_width),
      format!("imageHeight={}", snapshot.image_height),
      format!("query={}", snapshot.query),
      format!("exact={}", snapshot.exact),
      format!("caseSensitive={}", snapshot.case_sensitive),
      format!("normalizedQuery={}", capture.normalized_query),
    ];
    if let Some(crop) = capture.crop_rect.as_ref() {
      lines.push(format!(
        "cropRect={},{},{},{}",
        crop.x, crop.y, crop.width, crop.height
      ));
      lines.push(format!("ocrScaleFactor={:.3}", capture.ocr_scale_factor));
    }
    for matched in &snapshot.matches {
      lines.push(format!(
        "match\t{}\t{}\t{:.6}\t{}\t{}\t{}\t{}",
        matched.match_index,
        matched.text,
        matched.confidence,
        matched.bounds.x,
        matched.bounds.y,
        matched.bounds.width,
        matched.bounds.height
      ));
    }
    lines.push(format!("matchCount={}", snapshot.matches.len()));
    lines.join("\n") + "\n"
  }
}

pub(crate) mod overlay {
  use crate::model::AuvResult;

  fn unsupported() -> AuvResult<()> {
    Err(
      "macOS native overlay is temporarily unsupported while overlay bridge moves to its crate"
        .to_string(),
    )
  }

  pub(crate) fn show_cursor(_x: f64, _y: f64, _label: &str) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn show_dual_cursor(
    _x: f64,
    _y: f64,
    _label: &str,
    _user_label: &str,
  ) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn set_cursor(
    _cursor_id: &str,
    _x: f64,
    _y: f64,
    _label: &str,
    _variant: &str,
  ) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn move_cursor(
    _cursor_id: &str,
    _x: f64,
    _y: f64,
    _label: &str,
    _variant: &str,
    _duration_ms: u64,
  ) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn move_dual_cursor(
    _x: f64,
    _y: f64,
    _label: &str,
    _user_label: &str,
    _duration_ms: u64,
  ) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn flash_cursor(_x: f64, _y: f64, _label: &str, _duration_ms: u64) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn flash_cursor_id(
    _cursor_id: &str,
    _x: f64,
    _y: f64,
    _label: &str,
    _duration_ms: u64,
  ) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn hide_cursor_id(_cursor_id: &str) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn hide_cursor() -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn pump_events(_duration_ms: u64) -> AuvResult<()> {
    unsupported()
  }

  pub(crate) fn shutdown() -> AuvResult<()> {
    Ok(())
  }
}

pub(crate) mod permission {
  use crate::model::AuvResult;

  #[derive(Clone, Debug, PartialEq, Eq)]
  pub(crate) struct NativePermissionProbe {
    pub(crate) screen_recording: &'static str,
    pub(crate) accessibility: &'static str,
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn probe_native_permissions() -> AuvResult<NativePermissionProbe> {
    let probe = auv_driver_macos::native::permission::probe_native_permissions()?;
    Ok(NativePermissionProbe {
      screen_recording: probe.screen_recording,
      accessibility: probe.accessibility,
    })
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn probe_native_permissions() -> AuvResult<NativePermissionProbe> {
    Err("macOS native permission probe is unsupported on this target".to_string())
  }
}

pub(crate) mod pointer {
  use crate::model::AuvResult;

  #[cfg(target_os = "macos")]
  pub(crate) fn click_point(
    x: f64,
    y: f64,
    button_code: i32,
    click_count: i64,
    click_interval_ms: u64,
  ) -> AuvResult<()> {
    auv_driver_macos::native::pointer::click_point(
      x,
      y,
      button_code,
      click_count,
      click_interval_ms,
    )
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn click_point(
    _x: f64,
    _y: f64,
    _button_code: i32,
    _click_count: i64,
    _click_interval_ms: u64,
  ) -> AuvResult<()> {
    Err("macOS native pointer click is unsupported on this target".to_string())
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn scroll_point(x: f64, y: f64, delta_x: f64, delta_y: f64) -> AuvResult<()> {
    auv_driver_macos::native::pointer::scroll_point(x, y, delta_x, delta_y)
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn scroll_point(_x: f64, _y: f64, _delta_x: f64, _delta_y: f64) -> AuvResult<()> {
    Err("macOS native pointer scroll is unsupported on this target".to_string())
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn current_mouse_logical_point() -> AuvResult<(f64, f64)> {
    auv_driver_macos::native::pointer::current_mouse_logical_point()
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn current_mouse_logical_point() -> AuvResult<(f64, f64)> {
    Err("macOS native mouse location is unsupported on this target".to_string())
  }
}

pub(crate) mod window {
  use std::collections::{HashMap, HashSet};

  use crate::model::AuvResult;

  #[cfg(target_os = "macos")]
  pub(crate) fn enumerate_displays() -> AuvResult<super::root::ObservedDisplaySnapshot> {
    let snapshot = auv_driver_macos::native::window::enumerate_displays()?;
    Ok(super::root::ObservedDisplaySnapshot {
      displays: snapshot
        .displays
        .into_iter()
        .map(super::from_moved_display)
        .collect(),
      combined_bounds: super::from_moved_rect(snapshot.combined_bounds),
      captured_at: snapshot.captured_at,
    })
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn enumerate_displays() -> AuvResult<super::root::ObservedDisplaySnapshot> {
    Err("macOS native display enumeration is unsupported on this target".to_string())
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn observe_windows_snapshot(
    limit: i64,
    app_filter: &str,
  ) -> AuvResult<super::root::ObservedWindowSnapshot> {
    let snapshot = auv_driver_macos::native::window::observe_windows_snapshot(limit, app_filter)?;
    Ok(super::root::ObservedWindowSnapshot {
      frontmost_app_name: snapshot.frontmost_app_name,
      frontmost_app_bundle_id: snapshot.frontmost_app_bundle_id,
      frontmost_window_title: snapshot.frontmost_window_title,
      observed_at: snapshot.observed_at,
      windows: snapshot
        .windows
        .into_iter()
        .map(super::from_moved_window)
        .collect(),
    })
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn observe_windows_snapshot(
    _limit: i64,
    _app_filter: &str,
  ) -> AuvResult<super::root::ObservedWindowSnapshot> {
    Err("macOS native window listing is unsupported on this target".to_string())
  }

  #[cfg(target_os = "macos")]
  pub(crate) fn bundle_ids_by_pid(pids: &HashSet<u32>) -> AuvResult<HashMap<u32, String>> {
    auv_driver_macos::native::window::bundle_ids_by_pid(pids)
  }

  #[cfg(not(target_os = "macos"))]
  pub(crate) fn bundle_ids_by_pid(_pids: &HashSet<u32>) -> AuvResult<HashMap<u32, String>> {
    Err("macOS native bundle id lookup is unsupported on this target".to_string())
  }
}
