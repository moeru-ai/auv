// File: src/driver/macos/tests.rs
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use auv_driver_macos::types::ObservedWindow;

use super::{
  OcrTextMatch, ScreenshotDimensions,
  control::common::{ClickPointCallOptions, build_click_point_call},
  observation::DisplaySelection,
  support::runtime::{
    parse_shortcut, process_is_alive, push_text_keystroke_lines, read_lock_owner_pid,
    special_key_code,
  },
  support::{
    app_contains_window, build_window_candidates, filter_ocr_matches, filter_windows_for_app,
    find_ax_node_at_point, find_now_playing_ax_node, group_ocr_matches_into_rows,
    optional_bool, optional_f64, parse_app_selector, parse_mouse_button,
    parse_observed_ax_tree, parse_ocr_region_constraint, parse_ocr_text_snapshot,
    parse_visual_rows_snapshot, parse_window_selection, project_main_screenshot_point,
    render_rect_compact, resolve_app_ref, resolve_display_point, resolve_scroll_deltas,
    resolve_window_candidate, resolve_window_point, sanitize_file_component,
    swift_string_literal, temp_file_path, window_area,
  },
  support::display::{assess_coordinate_readiness, parse_display_snapshot, read_png_dimensions},
  support::ocr_commands::{TextMatchCommandReport, render_text_match_command_json},
  support::observation::{parse_display_selection, resolve_screen_capture_source},
};
use crate::{
  driver::{Driver, DriverRegistry, fixture::FixtureObserveDriver},
  model::{DriverCall, DriverRunContext, ExecutionTarget, now_millis},
};

#[test]
fn macos_driver_descriptor_uses_desktop_namespace() {
  let driver = super::LegacyMacosCommandDriver;
  let descriptor = driver.descriptor();

  assert_eq!(descriptor.id, "macos.desktop");
  assert!(
    descriptor
      .capabilities
      .iter()
      .any(|capability| *capability == "desktop.capture-window")
  );
  assert!(
    !descriptor
      .capabilities
      .iter()
      .any(|capability| capability.starts_with("observe."))
  );
}

#[test]
fn dispatch_rejects_removed_ax_tree_operation_name() {
  let call = build_call([]);
  let mut call = call;
  call.operation = ["observe", "ax", "tree"].join("_");

  let error = super::dispatch::invoke_legacy_command_operation(&call).unwrap_err();

  assert!(error.contains("does not support operation"));
  assert!(error.contains(&["observe", "ax", "tree"].join("_")));
}

#[test]
fn optional_f64_rejects_non_finite_numbers() {
  let call = build_call([("x", "NaN")]);
  let error = optional_f64(&call, "x").expect_err("NaN should be rejected");
  assert!(error.contains("finite number"));
}

#[test]
fn parse_mouse_button_defaults_to_left() {
  let call = build_call([]);
  assert_eq!(
    parse_mouse_button(&call).expect("button should parse"),
    ("left", 0)
  );
}

#[test]
fn parse_shortcut_accepts_common_modifier_forms() {
  let shortcut = parse_shortcut("cmd+shift+f").expect("shortcut should parse");
  assert_eq!(shortcut.key, "f");
  assert_eq!(shortcut.modifiers, vec!["command down", "shift down"]);
}

#[test]
fn parse_shortcut_rejects_missing_key() {
  let error = parse_shortcut("cmd").expect_err("shortcut should fail");
  assert!(error.contains("expected a form like"));
}

#[test]
fn optional_bool_accepts_true_false_forms() {
  let call = build_call([("replace_existing", "true")]);
  assert_eq!(
    optional_bool(&call, "replace_existing").expect("bool should parse"),
    Some(true)
  );
  let call = build_call([("replace_existing", "0")]);
  assert_eq!(
    optional_bool(&call, "replace_existing").expect("bool should parse"),
    Some(false)
  );
}

#[test]
fn special_key_code_maps_return() {
  assert_eq!(special_key_code("return").expect("return should map"), 36);
}

#[test]
fn special_key_code_maps_delete_aliases() {
  assert_eq!(special_key_code("delete").expect("delete should map"), 51);
  assert_eq!(
    special_key_code("backspace").expect("backspace should map"),
    51
  );
}

#[test]
fn push_text_keystroke_lines_keeps_spaces_as_separate_events() {
  let mut lines = Vec::new();
  push_text_keystroke_lines(&mut lines, "For Me");

  assert_eq!(
    lines,
    vec![
      "keystroke \"F\"",
      "delay 0.02",
      "keystroke \"o\"",
      "delay 0.02",
      "keystroke \"r\"",
      "delay 0.02",
      "keystroke \" \"",
      "delay 0.02",
      "keystroke \"M\"",
      "delay 0.02",
      "keystroke \"e\"",
      "delay 0.02",
    ]
  );
}

#[test]
fn resolve_scroll_deltas_accepts_direction_and_pages() {
  let call = build_call([("direction", "down"), ("pages", "0.5")]);
  let (delta_x, delta_y, summary) =
    resolve_scroll_deltas(&call).expect("scroll delta should resolve");
  assert_eq!(delta_x, 0.0);
  assert_eq!(delta_y, -240.0);
  assert!(summary.contains("direction=down"));
}

#[test]
fn resolve_scroll_deltas_accepts_explicit_deltas() {
  let call = build_call([("delta_x", "40"), ("delta_y", "-120")]);
  let (delta_x, delta_y, summary) =
    resolve_scroll_deltas(&call).expect("scroll delta should resolve");
  assert_eq!(delta_x, 40.0);
  assert_eq!(delta_y, -120.0);
  assert!(summary.contains("delta_x=40"));
}

#[test]
fn sanitize_file_component_removes_invalid_characters() {
  assert_eq!(sanitize_file_component("My App!"), "My-App");
  assert_eq!(sanitize_file_component("../../etc/passwd"), "etc-passwd");
  assert_eq!(sanitize_file_component(""), "artifact");
}

#[test]
fn swift_string_literal_escapes_correctly() {
  assert_eq!(swift_string_literal("hello"), "\"hello\"");
  assert_eq!(swift_string_literal("a\"b"), "\"a\\\"b\"");
  assert_eq!(swift_string_literal("a\\b"), "\"a\\\\b\"");
  assert_eq!(swift_string_literal("a\nb"), "\"a\\nb\"");
}

#[test]
fn parse_display_snapshot_computes_combined_bounds() {
  let snapshot = parse_display_snapshot(sample_display_report()).expect("snapshot should parse");
  assert_eq!(snapshot.displays.len(), 2);
  assert_eq!(snapshot.combined_bounds.x, -222);
  assert_eq!(snapshot.combined_bounds.y, -1080);
  assert_eq!(snapshot.combined_bounds.width, 1920);
  assert_eq!(snapshot.combined_bounds.height, 2062);
  assert_eq!(snapshot.displays[0].pixel_width, 3024);
  assert_eq!(snapshot.displays[1].scale_factor, 1.0);
}

#[test]
fn resolve_display_point_maps_to_local_and_backing_pixel_coords() {
  let snapshot = parse_display_snapshot(sample_display_report()).expect("snapshot should parse");
  let resolution = resolve_display_point(&snapshot, 120.0, 80.0).expect("point should resolve");
  assert_eq!(resolution.display.display_id, 1);
  assert_eq!(resolution.local_x, 120.0);
  assert_eq!(resolution.local_y, 80.0);
  assert_eq!(resolution.backing_pixel_x, 240);
  assert_eq!(resolution.backing_pixel_y, 160);
}

#[test]
fn resolve_display_point_returns_none_outside_all_displays() {
  let snapshot = parse_display_snapshot(sample_display_report()).expect("snapshot should parse");
  assert!(resolve_display_point(&snapshot, 4000.0, 4000.0).is_none());
}

#[test]
fn assess_coordinate_readiness_accepts_matching_logical_dimensions() {
  let snapshot = parse_display_snapshot(sample_display_report()).expect("snapshot should parse");
  let assessment = assess_coordinate_readiness(
    &snapshot,
    &ScreenshotDimensions {
      width: 1512,
      height: 982,
    },
  )
  .expect("assessment should succeed");
  assert!(assessment.ready_for_logical_input);
  assert!(assessment.matches_main_logical);
  assert!(!assessment.matches_main_physical);
}

#[test]
fn assess_coordinate_readiness_flags_retina_backing_mismatch() {
  let snapshot = parse_display_snapshot(sample_display_report()).expect("snapshot should parse");
  let assessment = assess_coordinate_readiness(
    &snapshot,
    &ScreenshotDimensions {
      width: 3024,
      height: 1964,
    },
  )
  .expect("assessment should succeed");
  assert!(!assessment.ready_for_logical_input);
  assert!(assessment.matches_main_physical);
  assert!(assessment.likely_retina_backing_mismatch);
}

#[test]
fn parse_ocr_text_snapshot_parses_matches() {
  let snapshot = parse_ocr_text_snapshot(sample_ocr_report()).expect("OCR report should parse");
  assert_eq!(snapshot.query, "Primary Track");
  assert_eq!(snapshot.image_width, 3024);
  assert_eq!(snapshot.image_height, 1964);
  assert_eq!(snapshot.matches.len(), 2);
  assert_eq!(snapshot.matches[0].match_index, 0);
  assert_eq!(snapshot.matches[0].text, "Primary Track Remix");
  assert_eq!(snapshot.matches[0].bounds.x, 741);
  assert_eq!(snapshot.matches[1].match_index, 1);
  assert!((snapshot.matches[1].confidence - 0.945678).abs() < f64::EPSILON);
}

#[test]
fn project_main_screenshot_point_maps_retina_pixels_to_logical() {
  let snapshot = parse_display_snapshot(sample_display_report()).expect("snapshot should parse");
  let (logical_x, logical_y) =
    project_main_screenshot_point(&snapshot, 997.5, 1311.5).expect("projection should succeed");
  assert!((logical_x - 498.75).abs() < f64::EPSILON);
  assert!((logical_y - 655.75).abs() < f64::EPSILON);
}

#[test]
fn parse_ocr_region_constraint_accepts_normalized_bounds() {
  let call = build_call([
    ("region_left_ratio", "0.1"),
    ("region_top_ratio", "0.2"),
    ("region_right_ratio", "0.9"),
    ("region_bottom_ratio", "0.8"),
  ]);
  let region = parse_ocr_region_constraint(&call, 1000, 500)
    .expect("region should parse")
    .unwrap();
  assert_eq!(render_rect_compact(&region), "100,100,800,300");
}

#[test]
fn filter_ocr_matches_applies_confidence_and_region() {
  let snapshot = parse_ocr_text_snapshot(sample_ocr_report()).expect("OCR report should parse");
  let region = super::ObservedRect {
    x: 700,
    y: 1200,
    width: 700,
    height: 200,
  };
  let filtered = filter_ocr_matches(&snapshot.matches, 0.95, Some(&region));
  assert_eq!(filtered.len(), 1);
  assert_eq!(filtered[0].text, "Primary Track Remix");
}

#[test]
fn group_ocr_matches_into_rows_merges_nearby_vertical_observations() {
  let matches = [
    OcrTextMatch {
      match_index: 0,
      text: "Song Title".to_string(),
      confidence: 0.99,
      bounds: super::ObservedRect {
        x: 100,
        y: 100,
        width: 180,
        height: 30,
      },
    },
    OcrTextMatch {
      match_index: 1,
      text: "Artist".to_string(),
      confidence: 0.98,
      bounds: super::ObservedRect {
        x: 110,
        y: 138,
        width: 90,
        height: 24,
      },
    },
    OcrTextMatch {
      match_index: 2,
      text: "Next Row".to_string(),
      confidence: 0.97,
      bounds: super::ObservedRect {
        x: 100,
        y: 260,
        width: 120,
        height: 28,
      },
    },
  ];
  let refs = matches.iter().collect::<Vec<_>>();
  let rows = group_ocr_matches_into_rows(&refs);
  assert_eq!(rows.len(), 2);
  assert_eq!(rows[0].source, "ocr-text");
  assert_eq!(rows[0].text_fragments.len(), 2);
  assert_eq!(rows[1].text_fragments, vec!["Next Row".to_string()]);
}

#[test]
fn find_now_playing_ax_node_matches_title_and_artist() {
  let snapshot = parse_observed_ax_tree(sample_ax_report()).expect("AX report should parse");
  let node = find_now_playing_ax_node(&snapshot, "TrackAlpha", Some("ArtistAlpha"), Some("0.4.4"))
    .expect("now-playing node should match");
  assert_eq!(node.title, "Track: TrackAlpha - Artist: ArtistAlpha");
}

#[test]
fn parse_observed_ax_tree_extracts_pid_for_action_dispatch() {
  let snapshot = parse_observed_ax_tree(sample_ax_report()).expect("AX report should parse");
  assert_eq!(
    snapshot.pid, 1495,
    "pid must be parsed so ax_press_path can re-resolve the AX element by PID + path"
  );
}

#[test]
fn find_ax_node_at_point_prefers_deepest_pressable_container() {
  let report = "observedAt=2026-05-20T00:00:00Z\n\
appName=Demo\n\
bundleId=com.example.demo\n\
pid=42\n\
windowTitle=\n\
rootRole=AXWindow\n\
node\t0\t0\tAXWindow\t\t\t\t\t\t\t\t0\t0\t800\t600\n\
node\t1\t0.0\tAXGroup\t\t\t\t\t\t\t\t100\t100\t200\t40\n\
node\t2\t0.0.0\tAXButton\t\tStart return\t\t\t\t\t\t110\t108\t180\t24\n";
  let snapshot = parse_observed_ax_tree(report).expect("report should parse");
  // Point (200, 120) is inside both the window (0..800, 0..600),
  // the AXGroup container (100..300, 100..140), and the AXButton (110..290, 108..132).
  // The resolver must land on the button (deepest + pressable role bonus),
  // otherwise OCR→AX would press the wrapping group with no AXPress action.
  let node = find_ax_node_at_point(&snapshot, 200.0, 120.0).expect("a node should contain point");
  assert_eq!(node.role, "AXButton", "expected to land on the button");
  assert_eq!(node.title, "Start return");
}

#[test]
fn find_ax_node_at_point_returns_none_when_outside_all_bounds() {
  let snapshot = parse_observed_ax_tree(sample_ax_report()).expect("AX report should parse");
  // Point (-1, -1) is outside every node bound; resolver must return None
  // so callers can surface "no AX node at OCR anchor" instead of pressing
  // some unrelated container that happens to span the screen.
  assert!(find_ax_node_at_point(&snapshot, -1.0, -1.0).is_none());
}

#[test]
fn parse_visual_rows_snapshot_parses_visual_band_rows() {
  let snapshot =
    parse_visual_rows_snapshot(sample_visual_row_report()).expect("visual row report should parse");
  assert_eq!(snapshot.strategy, "visual-bands");
  assert_eq!(snapshot.rows.len(), 2);
  assert_eq!(snapshot.rows[0].source, "visual-bands");
  assert_eq!(snapshot.rows[0].bounds.x, 423);
  assert!(snapshot.rows[0].text_fragments.is_empty());
}

#[test]
fn read_png_dimensions_extracts_width_and_height() {
  let path = temp_png_path("png-dimensions");
  write_minimal_png(&path, 3024, 1964);
  let dimensions = read_png_dimensions(&path).expect("PNG dimensions should parse");
  assert_eq!(dimensions.width, 3024);
  assert_eq!(dimensions.height, 1964);
  let _ = fs::remove_file(path);
}

#[test]
fn temp_file_path_is_unique_within_process() {
  let first = temp_file_path("artifact", "txt");
  let second = temp_file_path("artifact", "txt");
  assert_ne!(first, second);
}

#[test]
fn read_lock_owner_pid_parses_pid_field() {
  let path = temp_txt_path("lock-owner");
  fs::write(&path, "pid=4242\nacquiredAt=123\n").expect("lock file should write");
  let pid = read_lock_owner_pid(&path).expect("pid should parse");
  assert_eq!(pid, Some(4242));
  let _ = fs::remove_file(path);
}

#[test]
fn process_is_alive_matches_current_process() {
  assert!(process_is_alive(std::process::id()));
}

#[test]
fn build_click_point_call_populates_required_inputs() {
  let target = ExecutionTarget::default();
  let working_directory = PathBuf::from("/tmp/auv");
  let call = build_click_point_call(
    &target,
    &working_directory,
    DriverRunContext::default(),
    ClickPointCallOptions {
      x: 12.5,
      y: 48.25,
      button: "left",
      click_count: 2,
      click_interval_ms: Some(80),
      settle_ms: Some(300),
      app: Some("com.example.editor"),
    },
  );
  assert_eq!(call.operation, "click_point");
  assert_eq!(call.working_directory, working_directory);
  assert_eq!(call.inputs.get("x"), Some(&"12.500".to_string()));
  assert_eq!(call.inputs.get("y"), Some(&"48.250".to_string()));
  assert_eq!(call.inputs.get("button"), Some(&"left".to_string()));
  assert_eq!(call.inputs.get("click_count"), Some(&"2".to_string()));
  assert_eq!(
    call.inputs.get("click_interval_ms"),
    Some(&"80".to_string())
  );
  assert_eq!(call.inputs.get("settle_ms"), Some(&"300".to_string()));
  assert_eq!(
    call.inputs.get("app"),
    Some(&"com.example.editor".to_string())
  );
}

#[test]
fn build_click_point_call_omits_optional_inputs_when_absent() {
  let call = build_click_point_call(
    &ExecutionTarget::default(),
    std::path::Path::new("."),
    DriverRunContext::default(),
    ClickPointCallOptions {
      x: 1.0,
      y: 2.0,
      button: "right",
      click_count: 1,
      click_interval_ms: None,
      settle_ms: None,
      app: None,
    },
  );
  assert!(!call.inputs.contains_key("click_interval_ms"));
  assert!(!call.inputs.contains_key("settle_ms"));
  assert!(!call.inputs.contains_key("app"));
}

#[test]
fn app_contains_window_matches_bundleish_identifiers() {
  assert!(app_contains_window("com.example.editor", "example.editor"));
  assert!(app_contains_window("ExampleMusic", "ExampleMusic"));
  assert!(!app_contains_window("ExampleEditor", "OtherApp"));
}

#[test]
fn window_area_uses_window_bounds() {
  let window = ObservedWindow {
    window_number: 7,
    app_name: "ExampleEditor".to_string(),
    owner_pid: 1,
    owner_bundle_id: "com.example.editor".to_string(),
    layer: 0,
    title: "Untitled".to_string(),
    bounds: super::ObservedRect {
      x: 0,
      y: 0,
      width: 640,
      height: 480,
    },
  };
  assert_eq!(window_area(&window), 307200);
}

#[test]
fn resolve_window_point_supports_offset_mode() {
  let call = build_call([("offset_x", "16"), ("offset_y", "24")]);
  let window = sample_window_ref();
  let (x, y, summary) = resolve_window_point(&call, &window).expect("offset mode should resolve");
  assert_eq!(x, 116.0);
  assert_eq!(y, 224.0);
  assert_eq!(summary, "windowOffset=16.000,24.000");
}

#[test]
fn resolve_window_point_supports_relative_mode() {
  let call = build_call([("relative_x", "0.5"), ("relative_y", "0.25")]);
  let window = sample_window_ref();
  let (x, y, summary) = resolve_window_point(&call, &window).expect("relative mode should resolve");
  assert_eq!(x, 420.0);
  assert_eq!(y, 320.0);
  assert_eq!(summary, "windowRelative=0.500,0.250");
}

#[test]
fn resolve_window_point_rejects_mixed_modes() {
  let call = build_call([
    ("offset_x", "16"),
    ("offset_y", "24"),
    ("relative_x", "0.5"),
    ("relative_y", "0.25"),
  ]);
  let window = sample_window_ref();
  let error = resolve_window_point(&call, &window).expect_err("mixed modes should fail");
  assert!(error.contains("either --offset_x/--offset_y or --relative_x/--relative_y"));
}

#[test]
fn parse_window_selection_accepts_ref_native_id_and_title() {
  let call = build_call([
    ("window_ref", "42"),
    ("native_window_id", "42"),
    ("title", "Main Window"),
  ]);

  let selection = parse_window_selection(&call).expect("selection should parse");

  assert_eq!(selection.window_ref.as_deref(), Some("42"));
  assert_eq!(selection.native_window_id.as_deref(), Some("42"));
  assert_eq!(selection.title.as_deref(), Some("Main Window"));
}

#[test]
fn parse_window_selection_rejects_window_index() {
  let call = build_call([("window_index", "1")]);

  let error = parse_window_selection(&call).expect_err("window_index should be rejected");

  assert!(error.contains("--window_index is not supported"));
}

#[test]
fn parse_app_selector_recognizes_bundle_id() {
  let selector = parse_app_selector("com.example.music").expect("bundle id selector should parse");
  assert_eq!(selector.bundle_id.as_deref(), Some("com.example.music"));
  assert!(selector.app_name_hint.is_none());
}

#[test]
fn resolve_app_ref_prefers_exact_bundle_id_matches() {
  let selector = parse_app_selector("com.example.music").expect("bundle id selector should parse");
  let snapshot = super::ObservedWindowSnapshot {
    frontmost_app_name: "ExampleMusic".to_string(),
    frontmost_app_bundle_id: "com.example.music".to_string(),
    frontmost_window_title: "Main Window".to_string(),
    observed_at: "2026-05-18T00:00:00Z".to_string(),
    windows: vec![
      ObservedWindow {
        window_number: 2,
        app_name: "StatusIndicator".to_string(),
        owner_pid: 20,
        owner_bundle_id: "com.status.helper".to_string(),
        layer: 0,
        title: "StatusIndicator".to_string(),
        bounds: super::ObservedRect {
          x: 10,
          y: 10,
          width: 28,
          height: 28,
        },
      },
      ObservedWindow {
        window_number: 9,
        app_name: "ExampleMusic".to_string(),
        owner_pid: 30,
        owner_bundle_id: "com.example.music".to_string(),
        layer: 0,
        title: "Main Window".to_string(),
        bounds: super::ObservedRect {
          x: 100,
          y: 100,
          width: 1200,
          height: 900,
        },
      },
    ],
  };

  let resolved = resolve_app_ref(&snapshot, &selector).expect("app ref should resolve");
  assert_eq!(
    resolved.resolved_bundle_id.as_deref(),
    Some("com.example.music")
  );
  assert_eq!(resolved.resolved_app_name, "ExampleMusic");
  assert_eq!(resolved.match_strategy, "bundle-id-exact");
}

#[test]
fn build_window_candidates_marks_main_and_containment() {
  let displays = sample_display_descriptors_for_windows();
  let snapshot = sample_multi_window_snapshot();
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");

  let candidates =
    build_window_candidates(&snapshot, &resolved, &displays).expect("candidates should build");

  assert_eq!(candidates.len(), 2);
  assert_eq!(candidates[0].window_ref.window_number, 42);
  assert!(candidates[0].is_main_candidate);
  assert!(candidates[0].is_fully_contained_in_display);
  assert_eq!(candidates[0].display_ref.as_deref(), Some("display_1"));
  assert_eq!(
    candidates[0].selection_reason,
    "largest-visible-normal-window"
  );
  assert_eq!(candidates[0].candidate_index, 0);
}

#[test]
fn resolve_window_candidate_rejects_ambiguous_title() {
  let displays = sample_display_descriptors_for_windows();
  let snapshot = sample_ambiguous_title_window_snapshot();
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");
  let requested = super::WindowSelection {
    window_ref: None,
    native_window_id: None,
    title: Some("Main Window".to_string()),
  };

  let error = resolve_window_candidate(&snapshot, &resolved, &displays, &requested)
    .expect_err("ambiguous title should fail");

  assert!(error.contains("multiple window candidates matched title"));
  assert!(error.contains("debug.listWindows"));
}

#[test]
fn resolve_window_candidate_rejects_stale_explicit_selector() {
  let displays = sample_display_descriptors_for_windows();
  let snapshot = sample_multi_window_snapshot();
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");
  let requested = super::WindowSelection {
    window_ref: Some("window_999".to_string()),
    native_window_id: None,
    title: None,
  };

  let error = resolve_window_candidate(&snapshot, &resolved, &displays, &requested)
    .expect_err("stale selector should fail");

  assert!(error.contains("no window candidate matched the explicit selector"));
  assert!(error.contains("debug.listWindows"));
}

#[test]
fn resolve_screen_capture_source_prefers_explicit_display() {
  let displays = sample_display_descriptors_for_windows();
  let selection = DisplaySelection {
    display_ref: Some("display_1".to_string()),
    native_display_id: None,
    main: false,
  };

  let source = resolve_screen_capture_source(&displays, Some(&selection), None)
    .expect("source should resolve");

  assert_eq!(source.display_ref, "display_1");
  assert_eq!(source.selection_reason, "explicit-display-ref");
}

#[test]
fn resolve_screen_capture_source_uses_target_window_display() {
  let displays = sample_display_descriptors_for_windows();
  let snapshot = sample_multi_window_snapshot();
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");
  let candidate = resolve_window_candidate(
    &snapshot,
    &resolved,
    &displays,
    &super::WindowSelection::default(),
  )
  .expect("candidate should resolve");

  let source = resolve_screen_capture_source(&displays, None, Some(&candidate))
    .expect("source should resolve");

  assert_eq!(source.display_ref, "display_1");
  assert_eq!(source.selection_reason, "target-window-display");
}

#[test]
fn parse_display_selection_accepts_native_display_id() {
  let call = build_call([("native_display_id", "2")]);

  let selection = parse_display_selection(&call)
    .expect("selection should parse")
    .expect("selection should exist");

  assert_eq!(selection.native_display_id.as_deref(), Some("2"));
  assert!(!selection.main);
}

#[test]
fn window_candidate_json_contains_stable_selector_fields() {
  let displays = sample_display_descriptors_for_windows();
  let snapshot = sample_multi_window_snapshot();
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");
  let candidates =
    build_window_candidates(&snapshot, &resolved, &displays).expect("candidates should build");

  let json = serde_json::to_value(&candidates[0]).expect("candidate should encode");

  assert_eq!(json["candidate_index"], 0);
  assert_eq!(json["window_ref"]["window_number"], 42);
  assert_eq!(json["native_window_id"], "42");
  assert_eq!(json["display_ref"], "display_1");
  assert_eq!(json["native_display_id"], "2");
}

#[test]
fn render_window_list_json_includes_snapshot_and_candidates() {
  let displays = sample_display_descriptors_for_windows();
  let snapshot = sample_multi_window_snapshot();
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");
  let candidates =
    build_window_candidates(&snapshot, &resolved, &displays).expect("candidates should build");

  let json = super::observe::render_window_list_json(&snapshot, &candidates, None)
    .expect("window list JSON should render");
  let parsed: serde_json::Value = serde_json::from_str(&json).expect("json should parse");

  assert_eq!(parsed["snapshot"]["frontmost_app_name"], "ExampleMusic");
  assert_eq!(parsed["snapshot"]["windows"][0]["window_number"], 42);
  assert_eq!(parsed["candidates"][0]["native_window_id"], "42");
  assert!(parsed["candidate_resolution"].is_null());
}

#[test]
fn window_capture_readiness_diagnostic_names_partial_window_and_display_bounds() {
  let displays = sample_display_descriptors_for_windows();
  let mut snapshot = sample_multi_window_snapshot();
  snapshot.windows[0].bounds = super::ObservedRect {
    x: 1500,
    y: 50,
    width: 1200,
    height: 800,
  };
  let selector = parse_app_selector("com.example.music").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("app should resolve");
  let candidate = build_window_candidates(&snapshot, &resolved, &displays)
    .expect("candidates should build")
    .remove(0);

  let diagnostic = super::support::window_capture_readiness_diagnostic(&candidate, &displays);

  assert!(diagnostic.contains("window_42"));
  assert!(diagnostic.contains("not fully contained"));
  assert!(diagnostic.contains("windowBounds=1500,50,1200,800"));
  assert!(diagnostic.contains("display_1=1512.000,0.000,1643.000,1053.000"));
}

#[test]
fn retryable_window_capture_error_only_matches_transient_containment_failures() {
  assert!(super::support::is_retryable_window_capture_error(
    "could not resolve a fully contained visible window; inspect `debug.listWindows`"
  ));
  assert!(super::support::is_retryable_window_capture_error(
    "stale-window-ref: refreshed window is not fully contained by one display"
  ));
  assert!(!super::support::is_retryable_window_capture_error(
    "operation requires --target <application-id>"
  ));
}

#[test]
fn render_text_match_json_records_scope_and_point() {
  let report = TextMatchCommandReport {
    scope: "window".to_string(),
    capture_source: "window_42".to_string(),
    query: "Primary Track".to_string(),
    match_count: 1,
    filtered_match_count: 1,
    region: Some(super::ObservedRect {
      x: 10,
      y: 20,
      width: 300,
      height: 200,
    }),
    best_match_bounds: Some(super::ObservedRect {
      x: 40,
      y: 60,
      width: 120,
      height: 24,
    }),
    screenshot_point: Some((100.0, 72.0)),
    logical_point: Some((1650.0, 112.0)),
  };

  let json = render_text_match_command_json(&report).expect("json should render");

  assert!(json.contains("\"scope\": \"window\""));
  assert!(json.contains("\"capture_source\": \"window_42\""));
  assert!(json.contains("\"logical_point\""));
}

/// Bundle-id selector must match windows whose owner name is the localized
/// (non-ASCII) app name.  This is the core NetEaseMusic regression: the selector
/// is `com.netease.163music` but the CGWindowList owner name is `网易云音乐`.
#[test]
fn filter_windows_for_app_bundle_id_matches_localized_owner_name() {
  let selector =
    parse_app_selector("com.netease.163music").expect("bundle id selector should parse");
  let snapshot = super::ObservedWindowSnapshot {
    frontmost_app_name: "网易云音乐".to_string(),
    frontmost_app_bundle_id: "com.netease.163music".to_string(),
    frontmost_window_title: "网易云音乐".to_string(),
    observed_at: "2026-05-20T00:00:00Z".to_string(),
    windows: vec![
      ObservedWindow {
        window_number: 1,
        app_name: "Dock".to_string(),
        owner_pid: 100,
        owner_bundle_id: "com.apple.dock".to_string(),
        layer: 0,
        title: "".to_string(),
        bounds: super::ObservedRect {
          x: 0,
          y: 1340,
          width: 1470,
          height: 80,
        },
      },
      // Localized owner name — bundle id is what we should match on.
      ObservedWindow {
        window_number: 42,
        app_name: "网易云音乐".to_string(),
        owner_pid: 9999,
        owner_bundle_id: "com.netease.163music".to_string(),
        layer: 0,
        title: "网易云音乐".to_string(),
        bounds: super::ObservedRect {
          x: 200,
          y: 100,
          width: 1200,
          height: 900,
        },
      },
    ],
  };

  let resolved = resolve_app_ref(&snapshot, &selector).expect("app ref should resolve");
  assert_eq!(resolved.match_strategy, "bundle-id-exact");
  let windows = filter_windows_for_app(&snapshot.windows, &resolved);
  assert_eq!(
    windows.len(),
    1,
    "exactly one window should survive the filter"
  );
  assert_eq!(windows[0].window_number, 42);
  assert_eq!(windows[0].app_name, "网易云音乐");
}

/// `observe_windows` selector-filtered report must have:
///   • `windowCount` equal to the number of matching windows (not total)
///   • metadata lines `appSelector`, `matchStrategy`, `resolvedAppBundleId`, `resolvedAppName`
///   • only `window\t…` lines for matched windows
///
/// We test the report-building helper directly to avoid needing a live desktop.
#[test]
fn selector_filtered_report_window_count_equals_matched_windows() {
  let selector =
    parse_app_selector("com.netease.163music").expect("bundle id selector should parse");
  let snapshot = super::ObservedWindowSnapshot {
    frontmost_app_name: "网易云音乐".to_string(),
    frontmost_app_bundle_id: "com.netease.163music".to_string(),
    frontmost_window_title: "网易云音乐".to_string(),
    observed_at: "2026-05-20T00:00:00Z".to_string(),
    windows: vec![
      ObservedWindow {
        window_number: 1,
        app_name: "Dock".to_string(),
        owner_pid: 100,
        owner_bundle_id: "com.apple.dock".to_string(),
        layer: 0,
        title: "".to_string(),
        bounds: super::ObservedRect {
          x: 0,
          y: 1340,
          width: 1470,
          height: 80,
        },
      },
      ObservedWindow {
        window_number: 42,
        app_name: "网易云音乐".to_string(),
        owner_pid: 9999,
        owner_bundle_id: "com.netease.163music".to_string(),
        layer: 0,
        title: "网易云音乐".to_string(),
        bounds: super::ObservedRect {
          x: 200,
          y: 100,
          width: 1200,
          height: 900,
        },
      },
    ],
  };

  let resolved = resolve_app_ref(&snapshot, &selector).expect("app ref should resolve");
  let filtered = filter_windows_for_app(&snapshot.windows, &resolved);

  // Simulate the raw Swift report header.
  let raw_report = format!(
    "frontmostAppName={}\nfrontmostAppBundleId={}\nfrontmostWindowTitle={}\nobservedAt={}\nwindowCount=2\nwindow\tDock\t100\tcom.apple.dock\t1\t0\t\t0\t1340\t1470\t80\nwindow\t网易云音乐\t9999\tcom.netease.163music\t42\t0\t网易云音乐\t200\t100\t1200\t900",
    snapshot.frontmost_app_name,
    snapshot.frontmost_app_bundle_id,
    snapshot.frontmost_window_title,
    snapshot.observed_at,
  );

  let report =
    super::observe::build_selector_filtered_report_for_test(&raw_report, &filtered, &resolved);

  // windowCount must reflect only the matched windows.
  let window_count_line = report
    .lines()
    .find(|line| line.starts_with("windowCount="))
    .expect("report must contain windowCount");
  assert_eq!(window_count_line, "windowCount=1");

  // Required metadata lines must be present.
  assert!(
    report.contains("matchStrategy=bundle-id-exact"),
    "report must contain matchStrategy"
  );
  assert!(
    report.contains("resolvedAppBundleId=com.netease.163music"),
    "report must contain resolvedAppBundleId"
  );
  assert!(
    report.contains("appSelector=com.netease.163music"),
    "report must contain appSelector"
  );

  // Only the NetEaseMusic window should survive — not the Dock window.
  let window_lines: Vec<&str> = report
    .lines()
    .filter(|line| line.starts_with("window\t"))
    .collect();
  assert_eq!(window_lines.len(), 1);
  assert!(window_lines[0].contains("网易云音乐"));
  assert!(!window_lines[0].contains("Dock"));
}

/// The filtering helper should keep only the windows belonging to the resolved
/// app; the unfiltered observe path is covered by preserving the no-selector
/// branch in `observe_windows`.
#[test]
fn filter_windows_for_app_keeps_resolved_app_subset() {
  let all_windows = vec![
    ObservedWindow {
      window_number: 1,
      app_name: "TextEdit".to_string(),
      owner_pid: 10,
      owner_bundle_id: "com.apple.TextEdit".to_string(),
      layer: 0,
      title: "Untitled".to_string(),
      bounds: super::ObservedRect {
        x: 0,
        y: 0,
        width: 800,
        height: 600,
      },
    },
    ObservedWindow {
      window_number: 2,
      app_name: "Notes".to_string(),
      owner_pid: 20,
      owner_bundle_id: "com.apple.Notes".to_string(),
      layer: 0,
      title: "Quick Note".to_string(),
      bounds: super::ObservedRect {
        x: 100,
        y: 100,
        width: 700,
        height: 500,
      },
    },
  ];
  let snapshot = super::ObservedWindowSnapshot {
    frontmost_app_name: "TextEdit".to_string(),
    frontmost_app_bundle_id: "com.apple.TextEdit".to_string(),
    frontmost_window_title: "Untitled".to_string(),
    observed_at: "2026-05-20T00:00:00Z".to_string(),
    windows: all_windows.clone(),
  };

  // Narrow selector — should match only TextEdit.
  let selector = parse_app_selector("com.apple.TextEdit").expect("selector should parse");
  let resolved = resolve_app_ref(&snapshot, &selector).expect("should resolve");
  let filtered = filter_windows_for_app(&snapshot.windows, &resolved);
  assert_eq!(filtered.len(), 1);
  assert_eq!(filtered[0].window_number, 1);

  // A selector for Notes should not return the TextEdit window.
  let notes_selector = parse_app_selector("com.apple.Notes").expect("notes selector should parse");
  let notes_resolved = resolve_app_ref(&snapshot, &notes_selector);
  // Notes is not the frontmost app, but it does have windows in the snapshot.
  if let Ok(ref r) = notes_resolved {
    let notes_filtered = filter_windows_for_app(&snapshot.windows, r);
    assert!(
      notes_filtered.iter().all(|w| w.app_name == "Notes"),
      "only Notes windows should be returned"
    );
  }
}

#[test]
fn driver_registry_stores_and_retrieves_drivers() {
  let registry = DriverRegistry::new(vec![Box::new(FixtureObserveDriver)]);
  assert!(registry.get("fixture.observe").is_some());
  assert!(registry.get("missing").is_none());
  assert_eq!(registry.descriptors().len(), 1);
  assert_eq!(registry.descriptors()[0].id, "fixture.observe");
}

fn build_call<const N: usize>(entries: [(&str, &str); N]) -> DriverCall {
  let mut inputs = BTreeMap::new();
  for (key, value) in entries {
    inputs.insert(key.to_string(), value.to_string());
  }

  DriverCall {
    operation: "test".to_string(),
    target: ExecutionTarget::default(),
    inputs,
    working_directory: PathBuf::from("."),
    run_context: DriverRunContext::default(),
  }
}

fn sample_window_ref() -> super::WindowRef {
  super::WindowRef {
    window_number: 7,
    owner_pid: 1,
    owner_bundle_id: "com.example.editor".to_string(),
    app_name: "ExampleEditor".to_string(),
    title: "Untitled".to_string(),
    bounds: super::ObservedRect {
      x: 100,
      y: 200,
      width: 640,
      height: 480,
    },
    layer: 0,
  }
}

fn sample_display_descriptors_for_windows() -> Vec<super::capture::types::DisplayDescriptor> {
  vec![
    super::capture::types::DisplayDescriptor {
      display_ref: "display_0".to_string(),
      is_main: true,
      is_builtin: true,
      global_logical_bounds: super::capture::types::Rect {
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
      },
      visible_logical_bounds: super::capture::types::Rect {
        x: 0.0,
        y: 0.0,
        width: 1512.0,
        height: 982.0,
      },
      physical_pixel_size: super::capture::types::Size {
        width: 3024.0,
        height: 1964.0,
      },
      scale_factor: 2.0,
      pixel_to_logical_scale: super::capture::types::Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: super::capture::types::Scale2D { x: 2.0, y: 2.0 },
      native_display_id: "3".to_string(),
      capture_backend: super::capture::types::CaptureBackend::XcapMacos,
    },
    super::capture::types::DisplayDescriptor {
      display_ref: "display_1".to_string(),
      is_main: false,
      is_builtin: false,
      global_logical_bounds: super::capture::types::Rect {
        x: 1512.0,
        y: 0.0,
        width: 1643.0,
        height: 1053.0,
      },
      visible_logical_bounds: super::capture::types::Rect {
        x: 1512.0,
        y: 0.0,
        width: 1643.0,
        height: 1053.0,
      },
      physical_pixel_size: super::capture::types::Size {
        width: 3286.0,
        height: 2106.0,
      },
      scale_factor: 2.0,
      pixel_to_logical_scale: super::capture::types::Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: super::capture::types::Scale2D { x: 2.0, y: 2.0 },
      native_display_id: "2".to_string(),
      capture_backend: super::capture::types::CaptureBackend::XcapMacos,
    },
  ]
}

fn sample_multi_window_snapshot() -> super::ObservedWindowSnapshot {
  super::ObservedWindowSnapshot {
    frontmost_app_name: "ExampleMusic".to_string(),
    frontmost_app_bundle_id: "com.example.music".to_string(),
    frontmost_window_title: "Main Window".to_string(),
    observed_at: "2026-05-20T00:00:00Z".to_string(),
    windows: vec![
      ObservedWindow {
        window_number: 42,
        app_name: "ExampleMusic".to_string(),
        owner_pid: 100,
        owner_bundle_id: "com.example.music".to_string(),
        layer: 0,
        title: "Main Window".to_string(),
        bounds: super::ObservedRect {
          x: 1600,
          y: 50,
          width: 1200,
          height: 800,
        },
      },
      ObservedWindow {
        window_number: 43,
        app_name: "ExampleMusic".to_string(),
        owner_pid: 100,
        owner_bundle_id: "com.example.music".to_string(),
        layer: 0,
        title: "Secondary Window".to_string(),
        bounds: super::ObservedRect {
          x: 100,
          y: 100,
          width: 320,
          height: 180,
        },
      },
    ],
  }
}

fn sample_ambiguous_title_window_snapshot() -> super::ObservedWindowSnapshot {
  let mut snapshot = sample_multi_window_snapshot();
  snapshot.windows[1].title = "Main Window".to_string();
  snapshot.windows[1].bounds = super::ObservedRect {
    x: 100,
    y: 100,
    width: 900,
    height: 600,
  };
  snapshot
}

fn sample_display_report() -> &'static str {
  "capturedAt=2026-05-13T05:06:06Z\n\
displayCount=2\n\
display\t1\t1\t1\t0\t0\t1512\t982\t0\t65\t1512\t884\t2.000\t3024\t1964\n\
display\t3\t0\t0\t-222\t-1080\t1920\t1080\t-222\t-1080\t1920\t1080\t1.000\t1920\t1080\n"
}

fn sample_ocr_report() -> &'static str {
  "recognizedAt=2026-05-14T10:00:00Z\n\
imagePath=/tmp/auv-screen.png\n\
imageWidth=3024\n\
imageHeight=1964\n\
query=Primary Track\n\
exact=false\n\
caseSensitive=false\n\
match\t0\tPrimary Track Remix\t0.998901\t741\t1286\t513\t51\n\
match\t1\tSecondary Track\t0.945678\t1604\t808\t300\t42\n\
matchCount=2\n"
}

fn sample_visual_row_report() -> &'static str {
  "detectedAt=2026-05-15T22:00:00Z\n\
imagePath=/tmp/auv-screen.png\n\
imageWidth=3024\n\
imageHeight=1964\n\
rowStrategy=visual-bands\n\
cropRect=423,668,2298,1198\n\
analysisStrip=46,0,552,1198\n\
row\t0\t423\t712\t2120\t88\t0.423100\n\
row\t1\t423\t826\t2120\t86\t0.401200\n\
rowCount=2\n"
}

fn sample_ax_report() -> &'static str {
  "observedAt=2026-05-16T07:00:00Z\n\
appName=ExampleMusic\n\
bundleId=com.example.music\n\
pid=1495\n\
windowTitle=\n\
rootRole=AXWindow\n\
node\t0\t0\tAXWindow\tAXStandardWindow\tMainWindow\t\t\t\t\t\t66\t33\t1280\t857\n\
node\t1\t0.4\tAXUnknown\t\tPlayback Controls\t\t\t\t\t\t298\t800\t1036\t78\n\
node\t2\t0.4.4\tAXUnknown\t\tTrack: TrackAlpha - Artist: ArtistAlpha\t\t\t\t\t\t375\t812\t264\t24\n\
node\t2\t0.4.9\tAXUnknown\t\tPlaylist\t\t\t\t\t\t1284\t824\t30\t30\n"
}

fn temp_png_path(label: &str) -> PathBuf {
  std::env::temp_dir().join(format!("auv-{}-{}.png", label, now_millis()))
}

fn temp_txt_path(label: &str) -> PathBuf {
  std::env::temp_dir().join(format!("auv-{}-{}.txt", label, now_millis()))
}

fn write_minimal_png(path: &PathBuf, width: u32, height: u32) {
  let mut bytes = Vec::new();
  bytes.extend_from_slice(&[137, 80, 78, 71, 13, 10, 26, 10]);
  bytes.extend_from_slice(&13u32.to_be_bytes());
  bytes.extend_from_slice(b"IHDR");
  bytes.extend_from_slice(&width.to_be_bytes());
  bytes.extend_from_slice(&height.to_be_bytes());
  bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
  bytes.extend_from_slice(&0u32.to_be_bytes());
  fs::write(path, bytes).expect("minimal png should be writable");
}
