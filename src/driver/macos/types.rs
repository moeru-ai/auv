// File: src/driver/macos/types.rs
use std::path::PathBuf;

use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ObservedRect {
  pub(crate) x: i64,
  pub(crate) y: i64,
  pub(crate) width: i64,
  pub(crate) height: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObservedDisplay {
  pub(crate) display_id: u32,
  pub(crate) is_main: bool,
  pub(crate) is_built_in: bool,
  pub(crate) bounds: ObservedRect,
  pub(crate) visible_bounds: ObservedRect,
  pub(crate) scale_factor: f64,
  pub(crate) pixel_width: i64,
  pub(crate) pixel_height: i64,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObservedDisplaySnapshot {
  pub(crate) displays: Vec<ObservedDisplay>,
  pub(crate) combined_bounds: ObservedRect,
  pub(crate) captured_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ObservedWindow {
  pub(crate) window_number: i64,
  pub(crate) app_name: String,
  pub(crate) owner_pid: i64,
  pub(crate) owner_bundle_id: String,
  pub(crate) layer: i64,
  pub(crate) title: String,
  pub(crate) bounds: ObservedRect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct ObservedWindowSnapshot {
  pub(crate) frontmost_app_name: String,
  pub(crate) frontmost_app_bundle_id: String,
  pub(crate) frontmost_window_title: String,
  pub(crate) observed_at: String,
  pub(crate) windows: Vec<ObservedWindow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AppSelector {
  pub(crate) raw: String,
  pub(crate) bundle_id: Option<String>,
  pub(crate) app_name_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedAppRef {
  pub(crate) selector: AppSelector,
  pub(crate) resolved_bundle_id: Option<String>,
  pub(crate) resolved_app_name: String,
  pub(crate) owner_pids: Vec<i64>,
  pub(crate) match_strategy: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct WindowRef {
  pub(crate) window_number: i64,
  pub(crate) owner_pid: i64,
  pub(crate) owner_bundle_id: String,
  pub(crate) app_name: String,
  pub(crate) title: String,
  pub(crate) bounds: ObservedRect,
  pub(crate) layer: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct WindowCandidate {
  pub(crate) candidate_index: usize,
  pub(crate) window_ref: WindowRef,
  pub(crate) native_window_id: Option<String>,
  pub(crate) display_ref: Option<String>,
  pub(crate) native_display_id: Option<String>,
  pub(crate) is_main_candidate: bool,
  pub(crate) is_fully_contained_in_display: bool,
  pub(crate) area: i64,
  pub(crate) selection_reason: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(crate) struct WindowSelection {
  pub(crate) window_ref: Option<String>,
  pub(crate) native_window_id: Option<String>,
  pub(crate) title: Option<String>,
}

impl WindowSelection {
  pub(crate) fn has_selector(&self) -> bool {
    self.window_ref.is_some() || self.native_window_id.is_some() || self.title.is_some()
  }
}

impl ObservedWindow {
  pub(crate) fn to_window_ref(&self) -> WindowRef {
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
pub(crate) struct OcrTextMatch {
  pub(crate) match_index: usize,
  pub(crate) text: String,
  pub(crate) confidence: f64,
  pub(crate) bounds: ObservedRect,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OcrTextSnapshot {
  pub(crate) recognized_at: String,
  pub(crate) image_path: PathBuf,
  pub(crate) image_width: i64,
  pub(crate) image_height: i64,
  pub(crate) query: String,
  pub(crate) exact: bool,
  pub(crate) case_sensitive: bool,
  pub(crate) matches: Vec<OcrTextMatch>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservedOcrRow {
  pub(crate) row_index: usize,
  pub(crate) source: String,
  pub(crate) bounds: ObservedRect,
  pub(crate) text_fragments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DetectedScreenRows {
  pub(crate) strategy: String,
  pub(crate) raw_match_count: usize,
  pub(crate) filtered_match_count: usize,
  pub(crate) rows: Vec<ObservedOcrRow>,
  pub(crate) report: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ObservedPointResolution {
  pub(crate) display: ObservedDisplay,
  pub(crate) local_x: f64,
  pub(crate) local_y: f64,
  pub(crate) backing_pixel_x: i64,
  pub(crate) backing_pixel_y: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ScreenshotDimensions {
  pub(crate) width: i64,
  pub(crate) height: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservedAxNode {
  pub(crate) depth: usize,
  pub(crate) path: String,
  pub(crate) role: String,
  pub(crate) subrole: String,
  pub(crate) title: String,
  pub(crate) description: String,
  pub(crate) help: String,
  pub(crate) identifier: String,
  pub(crate) placeholder: String,
  pub(crate) value: String,
  pub(crate) bounds: ObservedRect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservedAxTreeSnapshot {
  pub(crate) observed_at: String,
  pub(crate) app_name: String,
  pub(crate) bundle_id: String,
  pub(crate) pid: i32,
  pub(crate) window_title: String,
  pub(crate) nodes: Vec<ObservedAxNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct CoordinateReadinessAssessment {
  pub(crate) ready_for_logical_input: bool,
  pub(crate) matches_main_logical: bool,
  pub(crate) matches_main_physical: bool,
  pub(crate) matches_combined_logical: bool,
  pub(crate) likely_retina_backing_mismatch: bool,
  pub(crate) reason: String,
}
