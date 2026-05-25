use std::path::PathBuf;

pub type AuvResult<T> = Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedWindow {
  pub window_number: i64,
  pub app_name: String,
  pub owner_pid: i64,
  pub owner_bundle_id: String,
  pub layer: i64,
  pub title: String,
  pub bounds: ObservedRect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservedWindowSnapshot {
  pub frontmost_app_name: String,
  pub frontmost_app_bundle_id: String,
  pub frontmost_window_title: String,
  pub observed_at: String,
  pub windows: Vec<ObservedWindow>,
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
