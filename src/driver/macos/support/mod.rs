// TODO(driver-crates): temporary root compatibility while command handlers
// still depend on root `DriverCall`, `DriverResponse`, and artifact models.
pub(crate) mod artifacts;
pub(crate) mod call;
pub(crate) mod display;
pub(crate) mod geometry;
pub(crate) mod observation;
pub(crate) mod ocr;
mod ocr_commands;
pub(crate) mod overlay_evidence;
mod recognition;
pub(crate) mod runtime;
mod scripts;
pub(crate) mod typed_capture;

pub(crate) mod template_match {
  pub(crate) use auv_driver_macos::support::template_match::*;
}

pub(crate) use auv_driver_macos::support::{
  ax_node_center, build_window_candidates, find_ax_node_at_point, find_best_ax_node,
  find_now_playing_ax_node, group_ocr_matches_into_rows, no_matching_ax_node_error,
  parse_app_selector, render_ax_interaction_report, resolve_app_ref, resolve_window_candidate,
  retry_window_capture_operation, window_capture_readiness_diagnostic,
};

#[cfg(test)]
pub(crate) use auv_driver_macos::support::{
  filter_windows_for_app, is_retryable_window_capture_error, parse_observed_ax_tree,
  parse_ocr_text_snapshot,
};

pub(crate) use self::artifacts::*;
pub(crate) use self::call::*;
pub(crate) use self::display::*;
pub(crate) use self::geometry::*;
pub(crate) use self::observation::*;
pub(crate) use self::ocr::*;
pub(crate) use self::ocr_commands::*;
pub(crate) use self::overlay_evidence::*;
pub(crate) use self::recognition::*;
pub(crate) use self::scripts::*;
pub(crate) use self::typed_capture::*;
