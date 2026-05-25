use std::path::PathBuf;

use serde::Serialize;

pub type AuvResult<T> = Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservedRect {
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedDisplay {
  pub display_id: u32,
  pub is_main: bool,
  pub is_built_in: bool,
  pub bounds: ObservedRect,
  pub visible_bounds: ObservedRect,
  pub scale_factor: f64,
  pub pixel_width: i64,
  pub pixel_height: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedDisplaySnapshot {
  pub displays: Vec<ObservedDisplay>,
  pub combined_bounds: ObservedRect,
  pub captured_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservedWindow {
  pub window_number: i64,
  pub app_name: String,
  pub owner_pid: i64,
  pub owner_bundle_id: String,
  pub layer: i64,
  pub title: String,
  pub bounds: ObservedRect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ObservedWindowSnapshot {
  pub frontmost_app_name: String,
  pub frontmost_app_bundle_id: String,
  pub frontmost_window_title: String,
  pub observed_at: String,
  pub windows: Vec<ObservedWindow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppSelector {
  pub raw: String,
  pub bundle_id: Option<String>,
  pub app_name_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedAppRef {
  pub selector: AppSelector,
  pub resolved_bundle_id: Option<String>,
  pub resolved_app_name: String,
  pub owner_pids: Vec<i64>,
  pub match_strategy: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WindowRef {
  pub window_number: i64,
  pub owner_pid: i64,
  pub owner_bundle_id: String,
  pub app_name: String,
  pub title: String,
  pub bounds: ObservedRect,
  pub layer: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WindowCandidate {
  pub candidate_index: usize,
  pub window_ref: WindowRef,
  pub native_window_id: Option<String>,
  pub display_ref: Option<String>,
  pub native_display_id: Option<String>,
  pub is_main_candidate: bool,
  pub is_fully_contained_in_display: bool,
  pub area: i64,
  pub selection_reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct WindowSelection {
  pub window_ref: Option<String>,
  pub native_window_id: Option<String>,
  pub title: Option<String>,
}

impl WindowSelection {
  pub fn has_selector(&self) -> bool {
    self.window_ref.is_some() || self.native_window_id.is_some() || self.title.is_some()
  }
}

impl ObservedWindow {
  pub fn to_window_ref(&self) -> WindowRef {
    WindowRef {
      window_number: self.window_number,
      owner_pid: self.owner_pid,
      owner_bundle_id: self.owner_bundle_id.clone(),
      app_name: self.app_name.clone(),
      title: self.title.clone(),
      bounds: self.bounds.clone(),
      layer: self.layer,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct OcrTextMatch {
  pub match_index: usize,
  pub text: String,
  pub confidence: f64,
  pub bounds: ObservedRect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OcrTextSnapshot {
  pub recognized_at: String,
  pub image_path: PathBuf,
  pub image_width: i64,
  pub image_height: i64,
  pub query: String,
  pub exact: bool,
  pub case_sensitive: bool,
  pub matches: Vec<OcrTextMatch>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedOcrRow {
  pub row_index: usize,
  pub source: String,
  pub bounds: ObservedRect,
  pub text_fragments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DetectedScreenRows {
  pub strategy: String,
  pub raw_match_count: usize,
  pub filtered_match_count: usize,
  pub rows: Vec<ObservedOcrRow>,
  pub report: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservedPointResolution {
  pub display: ObservedDisplay,
  pub local_x: f64,
  pub local_y: f64,
  pub backing_pixel_x: i64,
  pub backing_pixel_y: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScreenshotDimensions {
  pub width: i64,
  pub height: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedAxNode {
  pub depth: usize,
  pub path: String,
  pub role: String,
  pub subrole: String,
  pub title: String,
  pub description: String,
  pub help: String,
  pub identifier: String,
  pub placeholder: String,
  pub value: String,
  pub bounds: ObservedRect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedAxTreeSnapshot {
  pub observed_at: String,
  pub app_name: String,
  pub bundle_id: String,
  pub pid: i32,
  pub window_title: String,
  pub nodes: Vec<ObservedAxNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoordinateReadinessAssessment {
  pub ready_for_logical_input: bool,
  pub matches_main_logical: bool,
  pub matches_main_physical: bool,
  pub matches_combined_logical: bool,
  pub likely_retina_backing_mismatch: bool,
  pub reason: String,
}

pub fn compute_combined_bounds(displays: &[ObservedDisplay]) -> ObservedRect {
  let min_x = displays
    .iter()
    .map(|display| display.bounds.x)
    .min()
    .unwrap_or(0);
  let min_y = displays
    .iter()
    .map(|display| display.bounds.y)
    .min()
    .unwrap_or(0);
  let max_x = displays
    .iter()
    .map(|display| display.bounds.x + display.bounds.width)
    .max()
    .unwrap_or(0);
  let max_y = displays
    .iter()
    .map(|display| display.bounds.y + display.bounds.height)
    .max()
    .unwrap_or(0);

  ObservedRect {
    x: min_x,
    y: min_y,
    width: max_x - min_x,
    height: max_y - min_y,
  }
}
