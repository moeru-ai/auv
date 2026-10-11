use auv_driver_common::DriverError;

use crate::capture::types::{CaptureBackend, DisplayDescriptor, Rect, Scale2D, Size};
use crate::types::{AppSelector, ObservedRect, ObservedWindow, ObservedWindowSnapshot, ResolvedAppRef, WindowSelection};

use super::{AppLiveness, app_resolution_message, resolve_window_candidate, resolve_window_candidate_for_input};

#[test]
fn input_window_candidate_allows_partially_visible_default_window() {
  let displays = sample_displays();
  let snapshot = sample_snapshot_with_partial_main_window();
  let resolved = sample_resolved_app();

  let candidate = resolve_window_candidate_for_input(&snapshot, &resolved, &displays, &WindowSelection::default())
    .expect("scroll/input candidate should resolve");

  assert_eq!(candidate.window_ref.window_number, 42);
  assert!(!candidate.is_fully_contained_in_display);
  assert_eq!(candidate.selection_reason, "largest-visible-normal-window");
}

#[test]
fn capture_window_candidate_still_requires_fully_contained_default_window() {
  let displays = sample_displays();
  let snapshot = sample_snapshot_with_partial_main_window();
  let resolved = sample_resolved_app();

  let error = resolve_window_candidate(&snapshot, &resolved, &displays, &WindowSelection::default())
    .expect_err("capture candidate should reject partial windows");

  assert!(error.contains("fully contained visible window"));
}

// ROOT CAUSE:
//
// App resolution reads only the window snapshot, so an application that is not
// running and an application that is running without a visible window both
// failed with `visible application "..."`, which reads as "no such
// application". A caller could not tell whether to launch the application or
// to bring back one of its windows.
//
// Before the fix, both cases produced the same message.
// The fix keeps the NotFound error kind and names the liveness that was
// actually observed.
#[test]
fn app_resolution_failure_names_the_observed_liveness() {
  assert_eq!(app_resolution_message("com.apple.Preview", AppLiveness::NotRunning), "application \"com.apple.Preview\" (not running)");
  assert_eq!(
    app_resolution_message("com.apple.Preview", AppLiveness::Running),
    "application \"com.apple.Preview\" (running, but resolved no visible window)"
  );
  assert_eq!(
    app_resolution_message("com.apple.Preview", AppLiveness::MultipleInstances),
    "application \"com.apple.Preview\" (multiple running instances; select a window)"
  );
  // A selector without a bundle id cannot be probed, so the message stays as it
  // was rather than claiming a liveness nobody observed.
  assert_eq!(app_resolution_message("Preview", AppLiveness::Unknown), "visible application \"Preview\"");
}

// `NotFound` renders as `"{target} was not found"`, so the reason has to read as
// a qualifier on a noun phrase. Written as a sentence it produced
// `application "X" is not running was not found`, which no unit test of the
// message alone would catch — only the rendered form shows it.
#[test]
fn resolution_reason_reads_as_a_qualifier_after_the_shared_suffix() {
  let rendered = |liveness| {
    DriverError::NotFound {
      target: app_resolution_message("com.apple.Preview", liveness),
    }
    .to_string()
  };

  assert_eq!(rendered(AppLiveness::NotRunning), "application \"com.apple.Preview\" (not running) was not found");
  assert_eq!(rendered(AppLiveness::Running), "application \"com.apple.Preview\" (running, but resolved no visible window) was not found");
  assert_eq!(rendered(AppLiveness::Unknown), "visible application \"com.apple.Preview\" was not found");
}

#[test]
fn liveness_maps_the_running_application_pid_contract() {
  assert_eq!(AppLiveness::from_probe(Ok(0)), AppLiveness::NotRunning);
  assert_eq!(AppLiveness::from_probe(Ok(4242)), AppLiveness::Running);
  assert_eq!(AppLiveness::from_probe(Ok(-1)), AppLiveness::MultipleInstances);
  assert_eq!(AppLiveness::from_probe(Ok(-7)), AppLiveness::Unknown);
  assert_eq!(AppLiveness::from_probe(Err("application resolution is unavailable".to_string())), AppLiveness::Unknown);
}

fn sample_resolved_app() -> ResolvedAppRef {
  ResolvedAppRef {
    selector: AppSelector {
      raw: "com.example.music".to_string(),
      bundle_id: Some("com.example.music".to_string()),
      app_name_hint: None,
    },
    resolved_bundle_id: Some("com.example.music".to_string()),
    resolved_app_name: "ExampleMusic".to_string(),
    owner_pids: vec![10],
    match_strategy: "bundle-id-exact".to_string(),
  }
}

fn sample_snapshot_with_partial_main_window() -> ObservedWindowSnapshot {
  ObservedWindowSnapshot {
    frontmost_app_name: "ExampleMusic".to_string(),
    frontmost_app_bundle_id: "com.example.music".to_string(),
    frontmost_window_title: "Main".to_string(),
    observed_at: "test".to_string(),
    windows: vec![ObservedWindow {
      window_number: 42,
      app_name: "ExampleMusic".to_string(),
      owner_pid: 10,
      owner_bundle_id: "com.example.music".to_string(),
      layer: 0,
      title: "Main".to_string(),
      bounds: ObservedRect {
        x: 1500,
        y: 50,
        width: 1200,
        height: 800,
      },
    }],
  }
}

fn sample_displays() -> Vec<DisplayDescriptor> {
  vec![DisplayDescriptor {
    display_ref: "display_1".to_string(),
    is_main: true,
    is_builtin: true,
    global_logical_bounds: Rect {
      x: 0.0,
      y: 0.0,
      width: 1512.0,
      height: 982.0,
    },
    visible_logical_bounds: Rect {
      x: 0.0,
      y: 0.0,
      width: 1512.0,
      height: 982.0,
    },
    physical_pixel_size: Size {
      width: 3024.0,
      height: 1964.0,
    },
    scale_factor: 2.0,
    pixel_to_logical_scale: Scale2D { x: 0.5, y: 0.5 },
    logical_to_pixel_scale: Scale2D { x: 2.0, y: 2.0 },
    native_display_id: "1".to_string(),
    capture_backend: CaptureBackend::XcapMacos,
  }]
}
