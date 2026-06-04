// TODO(driver-crates): temporary root compatibility while command handlers
// still depend on root `DriverCall`, `DriverResponse`, and artifact models.
pub(crate) mod artifacts;
pub(crate) mod call;
pub(crate) mod display;
pub(crate) mod geometry;
pub(crate) mod observation;
pub(crate) mod ocr;
pub(crate) mod ocr_commands;
pub(crate) mod overlay_evidence;
pub(crate) mod recognition;
pub(crate) mod runtime;
pub(crate) mod typed_capture;

pub(crate) mod template_match {
  pub(crate) use auv_driver_macos::support::template_match::*;
}

pub(crate) mod ax {
  pub(crate) use auv_driver_macos::support::{
    ax_node_center, find_ax_node_at_point, find_best_ax_node, find_now_playing_ax_node,
    no_matching_ax_node_error, render_ax_interaction_report,
  };
}

pub(crate) mod selector {
  pub(crate) use auv_driver_macos::support::{
    build_window_candidates, parse_app_selector, resolve_app_ref, resolve_window_candidate,
    resolve_window_candidate_for_input, retry_window_capture_operation,
    window_capture_readiness_diagnostic,
  };
}

#[cfg(test)]
pub(crate) use auv_driver_macos::support::{
  filter_windows_for_app, group_ocr_matches_into_rows, is_retryable_window_capture_error,
  parse_observed_ax_tree, parse_ocr_text_snapshot,
};

#[cfg(test)]
pub(crate) use self::artifacts::*;
#[cfg(test)]
pub(crate) use self::call::*;
#[cfg(test)]
pub(crate) use self::geometry::*;
#[cfg(test)]
pub(crate) use self::ocr::*;
