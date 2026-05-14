use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::model::{
  AuvResult, DriverCall, DriverDescriptor, DriverResponse, ProducedArtifact, now_millis,
};

const PROBE_ACCESSIBILITY_SCRIPT: &str = include_str!("driver/macos/probe_accessibility.swift");
const PROBE_SCREEN_RECORDING_SCRIPT: &str =
  include_str!("driver/macos/probe_screen_recording.swift");
const ENUMERATE_DISPLAYS_SCRIPT: &str = include_str!("driver/macos/enumerate_displays.swift");
const OBSERVE_WINDOWS_SCRIPT_TEMPLATE: &str = include_str!("driver/macos/observe_windows.swift");
const OBSERVE_WINDOW_TREE_SCRIPT_TEMPLATE: &str =
  include_str!("driver/macos/observe_window_tree.swift");
const OCR_FIND_TEXT_SCRIPT_TEMPLATE: &str = include_str!("driver/macos/ocr_find_text.swift");
const CLICK_POINT_SCRIPT_TEMPLATE: &str = include_str!("driver/macos/click_point.swift");
const SCROLL_POINT_SCRIPT_TEMPLATE: &str = include_str!("driver/macos/scroll_point.swift");

const XCRUN_BINARY: &str = "/usr/bin/xcrun";
const OSASCRIPT_BINARY: &str = "/usr/bin/osascript";
const SCREEN_CAPTURE_BINARY: &str = "/usr/sbin/screencapture";

pub trait Driver {
  fn descriptor(&self) -> DriverDescriptor;
  fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse>;
}

pub struct DriverRegistry {
  drivers: HashMap<String, Box<dyn Driver>>,
}

impl DriverRegistry {
  pub fn new(drivers: Vec<Box<dyn Driver>>) -> Self {
    let mut registry = HashMap::new();
    for driver in drivers {
      let descriptor = driver.descriptor();
      registry.insert(descriptor.id.to_string(), driver);
    }
    Self { drivers: registry }
  }

  pub fn get(&self, driver_id: &str) -> Option<&dyn Driver> {
    self.drivers.get(driver_id).map(Box::as_ref)
  }

  pub fn descriptors(&self) -> Vec<DriverDescriptor> {
    let mut descriptors = self
      .drivers
      .values()
      .map(|driver| driver.descriptor())
      .collect::<Vec<_>>();
    descriptors.sort_by(|left, right| left.id.cmp(right.id));
    descriptors
  }
}

pub fn default_driver_registry() -> DriverRegistry {
  DriverRegistry::new(vec![
    Box::new(FixtureObserveDriver),
    Box::new(MacOsObserveDriver),
  ])
}

struct FixtureObserveDriver;

impl Driver for FixtureObserveDriver {
  fn descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: "fixture.observe",
      summary: "Non-UI fixture driver that proves invoke -> run -> inspect without platform side effects.",
      capabilities: &["observe.fixture"],
      donor_boundary: "AUV-native fixture driver; useful for validating the shared execution substrate before real app drivers land.",
    }
  }

  fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
    if call.operation != "observe_fixture_scene" {
      return Err(format!(
        "driver fixture.observe does not support operation {}",
        call.operation
      ));
    }

    let target = call
      .target
      .application_id
      .clone()
      .unwrap_or_else(|| "fixture://default".to_string());
    let label = call
      .inputs
      .get("label")
      .cloned()
      .unwrap_or_else(|| "fixture-observation".to_string());

    Ok(DriverResponse {
      summary: format!(
        "Observed deterministic fixture scene for target {} with label {}.",
        target, label
      ),
      backend: Some("fixture.static".to_string()),
      notes: vec![
        "This command does not touch the real desktop.".to_string(),
        "Use it to verify that implicit run creation and inspect output stay stable.".to_string(),
      ],
      artifacts: Vec::new(),
    })
  }
}

struct MacOsObserveDriver;

impl Driver for MacOsObserveDriver {
  fn descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: "macos.observe",
      summary: "Observation-first desktop donor primitives extracted into the shared AUV driver protocol.",
      capabilities: &[
        "observe.screenshot",
        "observe.windows",
        "observe.window-tree",
        "observe.permissions",
        "observe.displays",
        "observe.identify-point",
        "observe.project-screenshot-point",
        "observe.coordinate-readiness",
        "observe.screen-text",
        "control.focus-text-input",
        "control.press-button",
        "control.type-text",
        "control.press-key",
        "control.click-point",
        "control.click-window-point",
        "control.click-screen-text",
        "control.scroll-point",
      ],
      donor_boundary: "Borrow host observation primitives from AIRI, but keep MCP tools, action executors, approval queues, and workflow shells out of AUV core.",
    }
  }

  fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
    require_macos()?;

    match call.operation.as_str() {
      "capture_screen" => capture_screen(call),
      "probe_coordinate_readiness" => probe_coordinate_readiness(call),
      "probe_displays" => probe_displays(call),
      "project_screenshot_point" => project_screenshot_point(call),
      "identify_point" => identify_point(call),
      "observe_windows" => observe_windows(call),
      "observe_window_tree" => observe_window_tree(call),
      "find_screen_text" => find_screen_text(call),
      "probe_permissions" => probe_permissions(call),
      "focus_text_input" => focus_text_input(call),
      "press_button" => press_button(call),
      "type_text" => type_text(call),
      "press_key" => press_key(call),
      "click_point" => click_point(call),
      "click_window_point" => click_window_point(call),
      "click_screen_text" => click_screen_text(call),
      "scroll_point" => scroll_point(call),
      other => Err(format!(
        "driver macos.observe does not support operation {}",
        other
      )),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ObservedRect {
  x: i64,
  y: i64,
  width: i64,
  height: i64,
}

#[derive(Clone, Debug, PartialEq)]
struct ObservedDisplay {
  display_id: u32,
  is_main: bool,
  is_built_in: bool,
  bounds: ObservedRect,
  visible_bounds: ObservedRect,
  scale_factor: f64,
  pixel_width: i64,
  pixel_height: i64,
}

#[derive(Clone, Debug, PartialEq)]
struct ObservedDisplaySnapshot {
  displays: Vec<ObservedDisplay>,
  combined_bounds: ObservedRect,
  captured_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ObservedWindow {
  app_name: String,
  owner_pid: i64,
  layer: i64,
  title: String,
  bounds: ObservedRect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ObservedWindowSnapshot {
  frontmost_app_name: String,
  frontmost_window_title: String,
  observed_at: String,
  windows: Vec<ObservedWindow>,
}

#[derive(Clone, Debug, PartialEq)]
struct OcrTextMatch {
  match_index: usize,
  text: String,
  confidence: f64,
  bounds: ObservedRect,
}

#[derive(Clone, Debug, PartialEq)]
struct OcrTextSnapshot {
  recognized_at: String,
  image_path: PathBuf,
  image_width: i64,
  image_height: i64,
  query: String,
  exact: bool,
  case_sensitive: bool,
  matches: Vec<OcrTextMatch>,
}

#[derive(Clone, Debug, PartialEq)]
struct ObservedPointResolution {
  display: ObservedDisplay,
  local_x: f64,
  local_y: f64,
  backing_pixel_x: i64,
  backing_pixel_y: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ScreenshotDimensions {
  width: i64,
  height: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ObservedAxNode {
  depth: usize,
  path: String,
  role: String,
  subrole: String,
  title: String,
  description: String,
  help: String,
  identifier: String,
  placeholder: String,
  value: String,
  bounds: ObservedRect,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ObservedAxTreeSnapshot {
  observed_at: String,
  app_name: String,
  bundle_id: String,
  window_title: String,
  nodes: Vec<ObservedAxNode>,
}

#[derive(Clone, Debug, PartialEq)]
struct CoordinateReadinessAssessment {
  ready_for_logical_input: bool,
  matches_main_logical: bool,
  matches_main_physical: bool,
  matches_combined_logical: bool,
  likely_retina_backing_mismatch: bool,
  reason: String,
}

fn capture_screen(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "desktop".to_string());
  let temporary_path = capture_screenshot_file(&label)?;
  let dimensions = read_png_dimensions(&temporary_path)?;
  let snapshot = enumerate_displays().ok();
  let contract_report =
    render_capture_contract_report(snapshot.as_ref(), &dimensions, temporary_path.as_path());
  let contract_artifact = build_text_artifact(
    "capture-contract",
    "txt",
    &format!("{}-contract", sanitize_file_component(&label)),
    contract_report,
    "Recorded screenshot dimensions and the current macOS coordinate contract.",
  )?;
  let mut notes = vec![
    format!(
      "Temporary screenshot created at {} before artifact ingestion.",
      temporary_path.display()
    ),
    format!(
      "screenshotPixels={}x{}",
      dimensions.width, dimensions.height
    ),
    "coordinateSpace=screenshot pixels from main-display physical backing pixels".to_string(),
    "This remains a driver-level primitive instead of an AIRI-style desktop tool wrapper."
      .to_string(),
  ];
  if let Some(snapshot) = &snapshot {
    if let Some(main_display) = snapshot
      .displays
      .iter()
      .find(|display| display.is_main)
      .or_else(|| snapshot.displays.first())
    {
      notes.push(render_display_note(main_display));
    }
  }

  Ok(DriverResponse {
    summary: format!(
      "Captured one desktop screenshot through the shared AUV runtime ({}x{} pixels).",
      dimensions.width, dimensions.height
    ),
    backend: Some("macos.screencapture".to_string()),
    notes,
    artifacts: vec![
      ProducedArtifact {
        kind: "screenshot".to_string(),
        source_path: temporary_path,
        preferred_name: format!("{}.png", sanitize_file_component(&label)),
        note: Some(
          "Phase-1 screenshot artifact captured through the macOS desktop driver.".to_string(),
        ),
      },
      contract_artifact,
    ],
  })
}

fn probe_coordinate_readiness(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "coordinate-readiness".to_string());
  let screenshot_path = capture_screenshot_file(&label)?;
  let screenshot = read_png_dimensions(&screenshot_path)?;
  let snapshot = enumerate_displays()?;
  let main_display = snapshot
    .displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| snapshot.displays.first())
    .ok_or_else(|| "display probe returned no connected displays".to_string())?;
  let assessment = assess_coordinate_readiness(&snapshot, &screenshot)?;
  let report = render_coordinate_readiness_report(&snapshot, &screenshot, &assessment);
  let report_artifact = build_text_artifact(
    "coordinate-readiness",
    "txt",
    "coordinate-readiness-report",
    report,
    "Captured screenshot-backed coordinate readiness report from the observation driver.",
  )?;
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some(
      "Screenshot captured while validating observation-side coordinate readiness.".to_string(),
    ),
  };

  let summary = if assessment.ready_for_logical_input {
    format!(
      "Coordinate readiness looks aligned for logical input; screenshot is {}x{} and matches the observed logical desktop space.",
      screenshot.width, screenshot.height
    )
  } else if assessment.likely_retina_backing_mismatch {
    format!(
      "Coordinate readiness is not aligned yet; screenshot is {}x{} physical pixels while main display #{} is {}x{} logical points at scale {:.3}.",
      screenshot.width,
      screenshot.height,
      main_display.display_id,
      main_display.bounds.width,
      main_display.bounds.height,
      main_display.scale_factor
    )
  } else {
    format!(
      "Coordinate readiness is not aligned yet; screenshot is {}x{} and does not match the observed logical desktop bounds.",
      screenshot.width, screenshot.height
    )
  };

  let mut notes = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!(
      "screenshotPixels={}x{}",
      screenshot.width, screenshot.height
    ),
    format!(
      "combinedBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
    format!(
      "readyForLogicalInput={}",
      assessment.ready_for_logical_input
    ),
    format!("reason={}", assessment.reason),
  ];
  notes.push(render_display_note(main_display));

  Ok(DriverResponse {
    summary,
    backend: Some("macos.observe.coordinate-readiness".to_string()),
    notes,
    artifacts: vec![screenshot_artifact, report_artifact],
  })
}

fn probe_displays(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let snapshot = enumerate_displays()?;
  let main_display = snapshot
    .displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| snapshot.displays.first())
    .ok_or_else(|| "display probe returned no connected displays".to_string())?;
  let report = render_display_snapshot_report(&snapshot);
  let artifact = build_text_artifact(
    "display-report",
    "txt",
    "display-report",
    report,
    "Captured macOS display enumeration report from the observation driver.",
  )?;

  let mut notes = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!(
      "combinedBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
  ];
  for display in snapshot.displays.iter().take(3) {
    notes.push(render_display_note(display));
  }

  Ok(DriverResponse {
    summary: format!(
      "Enumerated {} macOS display(s); main display is #{} at {}x{} logical / {}x{} pixels.",
      snapshot.displays.len(),
      main_display.display_id,
      main_display.bounds.width,
      main_display.bounds.height,
      main_display.pixel_width,
      main_display.pixel_height
    ),
    backend: Some("macos.swift.nsscreen".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

fn observe_windows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let limit = optional_i64(call, "limit")?.unwrap_or(12).max(1);
  let app_filter = app_identifier(call).unwrap_or_default();
  let report = run_swift_script(&build_observe_windows_script(limit, &app_filter))?;
  let window_count = report_value(&report, "windowCount=")
    .unwrap_or("0")
    .parse::<usize>()
    .unwrap_or(0);
  let frontmost_app = report_value(&report, "frontmostAppName=")
    .unwrap_or("")
    .to_string();
  let frontmost_window = report_value(&report, "frontmostWindowTitle=")
    .unwrap_or("")
    .to_string();
  let observed_at = report_value(&report, "observedAt=")
    .unwrap_or("")
    .to_string();
  let artifact = build_text_artifact(
    "observe-windows",
    "txt",
    &format!(
      "observe-windows-{}",
      sanitize_file_component(&frontmost_app)
    ),
    report.clone(),
    "Captured window observation report from the macOS desktop driver.",
  )?;
  let mut notes = vec![format!("observedAt={observed_at}")];
  for line in report
    .lines()
    .filter(|line| line.starts_with("window\t"))
    .take(5)
  {
    notes.push(line.to_string());
  }

  let summary = if frontmost_app.is_empty() {
    format!("Observed {} visible macOS window(s).", window_count)
  } else if frontmost_window.is_empty() {
    format!(
      "Observed {} visible macOS window(s); frontmost app is {}.",
      window_count, frontmost_app
    )
  } else {
    format!(
      "Observed {} visible macOS window(s); frontmost app is {} ({})",
      window_count, frontmost_app, frontmost_window
    )
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.swift.cgwindowlist".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

fn observe_windows_snapshot(limit: i64, app_filter: &str) -> AuvResult<ObservedWindowSnapshot> {
  let report = run_swift_script(&build_observe_windows_script(limit, app_filter))?;
  let windows = report
    .lines()
    .filter(|line| line.starts_with("window\t"))
    .map(parse_window_line)
    .collect::<AuvResult<Vec<_>>>()?;
  Ok(ObservedWindowSnapshot {
    frontmost_app_name: report_value(&report, "frontmostAppName=")
      .unwrap_or("")
      .to_string(),
    frontmost_window_title: report_value(&report, "frontmostWindowTitle=")
      .unwrap_or("")
      .to_string(),
    observed_at: report_value(&report, "observedAt=")
      .unwrap_or("")
      .to_string(),
    windows,
  })
}

fn observe_window_tree(call: &DriverCall) -> AuvResult<DriverResponse> {
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
  let report = run_swift_script(&build_observe_window_tree_script(
    &app,
    max_depth,
    max_children,
  ))?;
  let app_name = report_value(&report, "appName=").unwrap_or("").to_string();
  let bundle_id = report_value(&report, "bundleId=").unwrap_or("").to_string();
  let window_title = report_value(&report, "windowTitle=")
    .unwrap_or("")
    .to_string();
  let observed_at = report_value(&report, "observedAt=")
    .unwrap_or("")
    .to_string();
  let node_count = report_value(&report, "nodeCount=")
    .unwrap_or("0")
    .parse::<usize>()
    .unwrap_or(0);
  let artifact = build_text_artifact(
    "window-tree",
    "txt",
    &format!(
      "window-tree-{}",
      sanitize_file_component(if app_name.is_empty() {
        "app"
      } else {
        &app_name
      })
    ),
    report.clone(),
    "Captured an AX tree snapshot for the target macOS app window.",
  )?;
  let mut notes = vec![format!("observedAt={observed_at}")];
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
    format!("Observed window AX tree with {} node(s).", node_count)
  } else if window_title.is_empty() {
    format!("Observed {} AX node(s) for app {}.", node_count, app_name)
  } else {
    format!(
      "Observed {} AX node(s) for app {} window {}.",
      node_count, app_name, window_title
    )
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.swift.ax-tree".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

fn project_screenshot_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let snapshot = enumerate_displays()?;
  let main_display = snapshot
    .displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| snapshot.displays.first())
    .ok_or_else(|| "display probe returned no connected displays".to_string())?;

  if x < 0.0
    || y < 0.0
    || x >= main_display.pixel_width as f64
    || y >= main_display.pixel_height as f64
  {
    return Err(format!(
      "screenshot pixel point ({x:.3}, {y:.3}) is outside main display physical bounds {}x{}",
      main_display.pixel_width, main_display.pixel_height
    ));
  }

  let logical_x = main_display.bounds.x as f64 + (x / main_display.scale_factor);
  let logical_y = main_display.bounds.y as f64 + (y / main_display.scale_factor);
  let resolution = resolve_display_point(&snapshot, logical_x, logical_y)
    .ok_or_else(|| "projected logical point fell outside connected displays".to_string())?;
  let report = [
    format!("capturedAt={}", snapshot.captured_at),
    format!("screenshotPixelPoint={x:.3},{y:.3}"),
    format!("projectedLogicalPoint={logical_x:.3},{logical_y:.3}"),
    format!("displayId={}", resolution.display.display_id),
    format!(
      "displayLogicalBounds={}",
      render_rect_compact(&resolution.display.bounds)
    ),
    format!(
      "displayPixelSize={}x{}",
      resolution.display.pixel_width, resolution.display.pixel_height
    ),
    format!("displayScaleFactor={:.3}", resolution.display.scale_factor),
    "coordinateContract=debug.captureScreen uses main-display physical pixels".to_string(),
  ]
  .join("\n")
    + "\n";
  let artifact = build_text_artifact(
    "screenshot-point-projection",
    "txt",
    &format!(
      "screenshot-point-{}-{}",
      sanitize_file_component(&format!("{x:.3}")),
      sanitize_file_component(&format!("{y:.3}"))
    ),
    report,
    "Projected screenshot pixel coordinates back into AUV global logical coordinates.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Projected screenshot pixel ({x:.3}, {y:.3}) to global logical point ({logical_x:.3}, {logical_y:.3}) on display #{}.",
      resolution.display.display_id
    ),
    backend: Some("macos.observe.screenshot-point".to_string()),
    notes: vec![
      format!("capturedAt={}", snapshot.captured_at),
      "coordinateSpace=main-display-physical-screenshot-pixels".to_string(),
      format!("globalLogicalPoint={logical_x:.3},{logical_y:.3}"),
      format!(
        "backingPixelPoint={},{}",
        resolution.backing_pixel_x, resolution.backing_pixel_y
      ),
      render_display_note(&resolution.display),
    ],
    artifacts: vec![artifact],
  })
}

fn find_screen_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let label = format!("screen-text-{}", sanitize_file_component(&query));
  let screenshot_path = capture_screenshot_file(&label)?;
  let _dimensions = read_png_dimensions(&screenshot_path)?;
  let snapshot = enumerate_displays()?;
  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(64)
    .clamp(1, 256);
  let ocr_report = run_swift_script(&build_ocr_find_text_script(
    screenshot_path.as_path(),
    &query,
    exact,
    case_sensitive,
    max_observations,
  ))?;
  let ocr_snapshot = parse_ocr_text_snapshot(&ocr_report)?;
  let report_artifact = build_text_artifact(
    "screen-text-report",
    "txt",
    &format!("screen-text-report-{}", sanitize_file_component(&query)),
    ocr_report,
    "Captured Vision OCR text-anchor report for a desktop screenshot.",
  )?;
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some("Screenshot captured for OCR text-anchor detection.".to_string()),
  };
  let mut notes = vec![
    format!("query={query}"),
    format!("matchCount={}", ocr_snapshot.matches.len()),
    format!("caseSensitive={case_sensitive}"),
    format!("exact={exact}"),
    format!(
      "screenshotPixels={}x{}",
      ocr_snapshot.image_width, ocr_snapshot.image_height
    ),
  ];

  let summary = if let Some(best_match) = ocr_snapshot.matches.first() {
    let (screenshot_center_x, screenshot_center_y) = ocr_match_center(best_match);
    let (logical_x, logical_y) =
      project_main_screenshot_point(&snapshot, screenshot_center_x, screenshot_center_y)?;
    notes.push(format!("bestMatchText={}", best_match.text));
    notes.push(format!(
      "bestMatchBounds={}",
      render_rect_compact(&best_match.bounds)
    ));
    notes.push(format!("bestMatchConfidence={:.3}", best_match.confidence));
    notes.push(format!("bestLogicalPoint={logical_x:.3},{logical_y:.3}"));
    format!(
      "Found {} OCR text match(es) for query {}; best anchor {} projects to logical point ({logical_x:.3}, {logical_y:.3}).",
      ocr_snapshot.matches.len(),
      query,
      best_match.text
    )
  } else {
    "Found 0 OCR text matches in the current desktop screenshot.".to_string()
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.vision.screen-text".to_string()),
    notes,
    artifacts: vec![screenshot_artifact, report_artifact],
  })
}

fn identify_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let snapshot = enumerate_displays()?;
  let resolution = resolve_display_point(&snapshot, x, y);
  let report = render_point_identification_report(&snapshot, x, y, resolution.as_ref());
  let label = format!(
    "point-{}-{}",
    sanitize_file_component(&format!("{x:.3}")),
    sanitize_file_component(&format!("{y:.3}"))
  );
  let artifact = build_text_artifact(
    "point-resolution",
    "txt",
    &label,
    report,
    "Captured macOS point-to-display resolution report from the observation driver.",
  )?;

  let mut notes = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!(
      "combinedBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
  ];
  let summary = if let Some(resolution) = resolution {
    notes.push(render_display_note(&resolution.display));
    notes.push(format!(
      "localPoint={:.3},{:.3}",
      resolution.local_x, resolution.local_y
    ));
    notes.push(format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ));
    let role = if resolution.display.is_main {
      "main"
    } else {
      "secondary"
    };
    format!(
      "Point ({x:.3}, {y:.3}) is on {role} display #{}; local=({:.3}, {:.3}), backingPixel=({}, {}).",
      resolution.display.display_id,
      resolution.local_x,
      resolution.local_y,
      resolution.backing_pixel_x,
      resolution.backing_pixel_y
    )
  } else {
    format!("Point ({x:.3}, {y:.3}) is outside all connected macOS displays.")
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.observe.display-point".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

fn focus_text_input(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let query = required_non_empty_string(call, "query")?;
  let reveal_shortcut = optional_non_empty_string(call, "reveal_shortcut");
  let reveal_settle_ms = optional_positive_u64(call, "reveal_settle_ms")?.unwrap_or(250);
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(16)
    .clamp(1, 50);
  if !app.is_empty() {
    activate_target_app(&app)?;
  }
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    send_shortcut(shortcut)?;
    thread::sleep(Duration::from_millis(reveal_settle_ms));
  }
  let tree_report = run_swift_script(&build_observe_window_tree_script(
    &app,
    max_depth,
    max_children,
  ))?;
  let snapshot = parse_observed_ax_tree(&tree_report)?;
  let matched = find_best_ax_node(&snapshot, &query)
    .ok_or_else(|| no_matching_ax_node_error(&snapshot, &query, "text input-like"))?;
  let (center_x, center_y) = ax_node_center(matched);
  run_swift_script(&build_click_point_script(center_x, center_y, 0, 1))?;
  let report = render_ax_interaction_report("focus-text-input", &snapshot, matched, &query);
  let artifact = build_text_artifact(
    "focus-text-input",
    "txt",
    &format!("focus-text-input-{}", sanitize_file_component(&query)),
    report,
    "Focused a text input by matching the observed AX tree and clicking the resolved bounds.",
  )?;
  let mut notes = vec![
    format!("query={query}"),
    format!("matchedPath={}", matched.path),
    format!("matchedRole={}", matched.role),
    format!("matchedBounds={}", render_rect_compact(&matched.bounds)),
    format!("clickLogicalPoint={center_x:.3},{center_y:.3}"),
  ];
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    notes.push(format!("revealShortcut={shortcut}"));
    notes.push(format!("revealSettleMs={reveal_settle_ms}"));
  }
  if !matched.description.is_empty() {
    notes.push(format!("matchedDescription={}", matched.description));
  }
  if !matched.placeholder.is_empty() {
    notes.push(format!("matchedPlaceholder={}", matched.placeholder));
  }
  if !matched.title.is_empty() {
    notes.push(format!("matchedTitle={}", matched.title));
  }

  Ok(DriverResponse {
    summary: if matched.title.is_empty() && matched.description.is_empty() {
      format!(
        "Focused text input in {} using query {} (role {}).",
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query,
        matched.role
      )
    } else {
      format!(
        "Focused text input {} in {} using query {}.",
        if matched.title.is_empty() {
          matched.description.as_str()
        } else {
          matched.title.as_str()
        },
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query
      )
    },
    backend: Some("macos.observe.ax-tree-click-focus".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

fn press_button(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let query = required_non_empty_string(call, "query")?;
  let reveal_shortcut = optional_non_empty_string(call, "reveal_shortcut");
  let reveal_settle_ms = optional_positive_u64(call, "reveal_settle_ms")?.unwrap_or(250);
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(16)
    .clamp(1, 50);
  if !app.is_empty() {
    activate_target_app(&app)?;
  }
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    send_shortcut(shortcut)?;
    thread::sleep(Duration::from_millis(reveal_settle_ms));
  }
  let tree_report = run_swift_script(&build_observe_window_tree_script(
    &app,
    max_depth,
    max_children,
  ))?;
  let snapshot = parse_observed_ax_tree(&tree_report)?;
  let matched = find_best_ax_node(&snapshot, &query)
    .ok_or_else(|| no_matching_ax_node_error(&snapshot, &query, "button-like"))?;
  let (center_x, center_y) = ax_node_center(matched);
  run_swift_script(&build_click_point_script(center_x, center_y, 0, 1))?;
  let report = render_ax_interaction_report("press-button", &snapshot, matched, &query);
  let artifact = build_text_artifact(
    "press-button",
    "txt",
    &format!("press-button-{}", sanitize_file_component(&query)),
    report,
    "Pressed a known control by matching the observed AX tree and clicking the resolved bounds.",
  )?;
  let mut notes = vec![
    format!("query={query}"),
    format!("matchedPath={}", matched.path),
    format!("matchedRole={}", matched.role),
    format!("matchedBounds={}", render_rect_compact(&matched.bounds)),
    format!("clickLogicalPoint={center_x:.3},{center_y:.3}"),
  ];
  if let Some(shortcut) = reveal_shortcut.as_deref() {
    notes.push(format!("revealShortcut={shortcut}"));
    notes.push(format!("revealSettleMs={reveal_settle_ms}"));
  }
  if !matched.description.is_empty() {
    notes.push(format!("matchedDescription={}", matched.description));
  }
  if !matched.help.is_empty() {
    notes.push(format!("matchedHelp={}", matched.help));
  }
  if !matched.title.is_empty() {
    notes.push(format!("matchedTitle={}", matched.title));
  }

  Ok(DriverResponse {
    summary: if matched.title.is_empty() && matched.description.is_empty() {
      format!(
        "Pressed button-like control in {} using query {} (role {}).",
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query,
        matched.role
      )
    } else {
      format!(
        "Pressed {} in {} using query {}.",
        if matched.title.is_empty() {
          matched.description.as_str()
        } else {
          matched.title.as_str()
        },
        if snapshot.app_name.is_empty() {
          "target app"
        } else {
          &snapshot.app_name
        },
        query
      )
    },
    backend: Some("macos.observe.ax-tree-click-press".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

fn type_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let text = required_non_empty_string(call, "text")?;
  let replace_existing = optional_bool(call, "replace_existing")?.unwrap_or(false);
  let submit_key = optional_non_empty_string(call, "submit_key");
  let submit_settle_ms = optional_positive_u64(call, "submit_settle_ms")?.unwrap_or(0);
  if !app.is_empty() {
    activate_target_app(&app)?;
  }
  type_text_via_system_events(
    &text,
    replace_existing,
    submit_key.as_deref(),
    submit_settle_ms,
  )?;

  let report = render_type_text_report(&app, &text, replace_existing, submit_key.as_deref());
  let artifact = build_text_artifact(
    "type-text",
    "txt",
    &format!("type-text-{}", sanitize_file_component(&text)),
    report,
    "Typed text into the active macOS control through System Events.",
  )?;

  let mut notes = vec![
    format!("text={text}"),
    format!("textLength={}", text.chars().count()),
    format!("replaceExisting={replace_existing}"),
  ];
  if !app.is_empty() {
    notes.push(format!("app={app}"));
  }
  if let Some(submit_key) = submit_key.as_deref() {
    notes.push(format!("submitKey={submit_key}"));
  }
  if submit_settle_ms > 0 {
    notes.push(format!("submitSettleMs={submit_settle_ms}"));
  }

  Ok(DriverResponse {
    summary: match submit_key.as_deref() {
      Some(submit_key) => format!(
        "Typed {} character(s) into {} and submitted with {}.",
        text.chars().count(),
        if app.is_empty() {
          "the active app"
        } else {
          &app
        },
        submit_key
      ),
      None => format!(
        "Typed {} character(s) into {}.",
        text.chars().count(),
        if app.is_empty() {
          "the active app"
        } else {
          &app
        }
      ),
    },
    backend: Some("macos.system-events.type-text".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

fn press_key(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let key = required_non_empty_string(call, "key")?;
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);
  if !app.is_empty() {
    activate_target_app(&app)?;
  }
  send_key_input(&key, settle_ms)?;
  let artifact = build_text_artifact(
    "press-key",
    "txt",
    &format!("press-key-{}", sanitize_file_component(&key)),
    [
      format!("pressedAt={}", now_millis()),
      format!("app={app}"),
      format!("key={key}"),
      format!("settleMs={settle_ms}"),
    ]
    .join("\n"),
    "Pressed a keyboard key or shortcut through System Events.",
  )?;
  Ok(DriverResponse {
    summary: format!(
      "Pressed key {} in {}.",
      key,
      if app.is_empty() {
        "the active app"
      } else {
        &app
      }
    ),
    backend: Some("macos.system-events.press-key".to_string()),
    notes: vec![
      format!("key={key}"),
      format!("settleMs={settle_ms}"),
      format!("app={app}"),
    ],
    artifacts: vec![artifact],
  })
}

fn click_window_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| {
      "operation requires --target <application-id> or --app <application-id>".to_string()
    })?;
  activate_target_app(&app)?;
  let snapshot = observe_windows_snapshot(32, "")?;
  let mut candidate_windows = snapshot
    .windows
    .iter()
    .filter(|window| {
      app_contains_window(&app, &window.app_name)
        || (!snapshot.frontmost_app_name.is_empty()
          && snapshot.frontmost_app_name == window.app_name)
    })
    .collect::<Vec<_>>();
  candidate_windows.sort_by(|left, right| {
    let left_key = (left.layer != 0, -window_area(left));
    let right_key = (right.layer != 0, -window_area(right));
    left_key.cmp(&right_key)
  });
  let window = candidate_windows
    .into_iter()
    .next()
    .or_else(|| snapshot.windows.first())
    .ok_or_else(|| format!("could not find a visible window for app {}", app))?;

  let (logical_x, logical_y, coordinate_summary) = resolve_window_point(call, window)?;
  let button_label = optional_string(call, "button").unwrap_or_else(|| "left".to_string());
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let nested_call = DriverCall {
    operation: "click_point".to_string(),
    target: call.target.clone(),
    inputs: std::collections::BTreeMap::from([
      ("x".to_string(), format!("{logical_x:.3}")),
      ("y".to_string(), format!("{logical_y:.3}")),
      ("button".to_string(), button_label.clone()),
      ("click_count".to_string(), click_count.to_string()),
      ("app".to_string(), app.clone()),
    ]),
    working_directory: call.working_directory.clone(),
  };
  let _ = click_point(&nested_call)?;

  let artifact = build_text_artifact(
    "click-window-point",
    "txt",
    &format!("click-window-point-{}", sanitize_file_component(&app)),
    [
      format!("app={app}"),
      format!("windowTitle={}", window.title),
      format!("windowBounds={}", render_rect_compact(&window.bounds)),
      format!("resolvedLogicalPoint={logical_x:.3},{logical_y:.3}"),
      coordinate_summary.clone(),
      format!("button={button_label}"),
      format!("clickCount={click_count}"),
    ]
    .join("\n"),
    "Clicked a point relative to a resolved macOS app window.",
  )?;
  let mut notes = vec![
    format!("app={app}"),
    format!("windowBounds={}", render_rect_compact(&window.bounds)),
    format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
    coordinate_summary,
  ];
  if !window.title.is_empty() {
    notes.push(format!("windowTitle={}", window.title));
  }

  Ok(DriverResponse {
    summary: format!(
      "Clicked {} window-relative point in {} at global logical point ({logical_x:.3}, {logical_y:.3}).",
      button_label, app
    ),
    backend: Some("macos.observe.window-relative-click".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

fn click_screen_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let label = format!("screen-text-click-{}", sanitize_file_component(&query));
  let screenshot_path = capture_screenshot_file(&label)?;
  let _dimensions = read_png_dimensions(&screenshot_path)?;
  let snapshot = enumerate_displays()?;
  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(64)
    .clamp(1, 256);
  let match_index = optional_i64(call, "match_index")?.unwrap_or(0).max(0) as usize;
  let ocr_report = run_swift_script(&build_ocr_find_text_script(
    screenshot_path.as_path(),
    &query,
    exact,
    case_sensitive,
    max_observations,
  ))?;
  let ocr_snapshot = parse_ocr_text_snapshot(&ocr_report)?;
  let matched = ocr_snapshot.matches.get(match_index).ok_or_else(|| {
    format!(
      "no OCR text match at index {} for query {} (found {})",
      match_index,
      query,
      ocr_snapshot.matches.len()
    )
  })?;
  let (screenshot_center_x, screenshot_center_y) = ocr_match_center(matched);
  let (logical_x, logical_y) =
    project_main_screenshot_point(&snapshot, screenshot_center_x, screenshot_center_y)?;
  let button_label = optional_string(call, "button").unwrap_or_else(|| "left".to_string());
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let nested_call = DriverCall {
    operation: "click_point".to_string(),
    target: call.target.clone(),
    inputs: std::collections::BTreeMap::from([
      ("x".to_string(), format!("{logical_x:.3}")),
      ("y".to_string(), format!("{logical_y:.3}")),
      ("button".to_string(), button_label.clone()),
      ("click_count".to_string(), click_count.to_string()),
    ]),
    working_directory: call.working_directory.clone(),
  };
  let _ = click_point(&nested_call)?;

  let report_artifact = build_text_artifact(
    "screen-text-click",
    "txt",
    &format!("screen-text-click-{}", sanitize_file_component(&query)),
    [
      format!("query={query}"),
      format!("matchIndex={match_index}"),
      format!("matchText={}", matched.text),
      format!("matchBounds={}", render_rect_compact(&matched.bounds)),
      format!("matchConfidence={:.3}", matched.confidence),
      format!("screenshotCenter={screenshot_center_x:.3},{screenshot_center_y:.3}"),
      format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
      format!("button={button_label}"),
      format!("clickCount={click_count}"),
    ]
    .join("\n"),
    "Clicked an OCR text anchor projected from screenshot pixels to logical coordinates.",
  )?;
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some("Screenshot captured for OCR click-anchor detection.".to_string()),
  };

  Ok(DriverResponse {
    summary: format!(
      "Clicked OCR text anchor {} for query {} at logical point ({logical_x:.3}, {logical_y:.3}).",
      matched.text, query
    ),
    backend: Some("macos.vision.click-screen-text".to_string()),
    notes: vec![
      format!("query={query}"),
      format!("matchIndex={match_index}"),
      format!("matchText={}", matched.text),
      format!("matchBounds={}", render_rect_compact(&matched.bounds)),
      format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
    ],
    artifacts: vec![screenshot_artifact, report_artifact],
  })
}

fn click_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1).clamp(1, 4);
  let (button_name, button_code) = parse_mouse_button(call)?;
  let snapshot = enumerate_displays()?;
  let resolution = resolve_display_point(&snapshot, x, y)
    .ok_or_else(|| format!("logical point ({x:.3}, {y:.3}) is outside all connected displays"))?;
  if let Some(app) = app_identifier(call) {
    if !app.is_empty() {
      activate_target_app(&app)?;
    }
  }
  run_swift_script(&build_click_point_script(x, y, button_code, click_count))?;
  let report = [
    format!("capturedAt={}", snapshot.captured_at),
    format!("globalLogicalPoint={x:.3},{y:.3}"),
    format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ),
    format!("displayId={}", resolution.display.display_id),
    format!("button={button_name}"),
    format!("clickCount={click_count}"),
    "coordinateSpace=global-logical".to_string(),
    "cursorAfter=target".to_string(),
  ]
  .join("\n")
    + "\n";
  let artifact = build_text_artifact(
    "click-point",
    "txt",
    &format!(
      "click-point-{}-{}",
      sanitize_file_component(&format!("{x:.3}")),
      sanitize_file_component(&format!("{y:.3}"))
    ),
    report,
    "Clicked a macOS logical point through Quartz and recorded its coordinate contract.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Clicked {} at global logical point ({x:.3}, {y:.3}) on display #{}.",
      button_name, resolution.display.display_id
    ),
    backend: Some("macos.swift.quartz-click".to_string()),
    notes: vec![
      "coordinateSpace=global-logical".to_string(),
      format!("button={button_name}"),
      format!("clickCount={click_count}"),
      format!(
        "backingPixelPoint={},{}",
        resolution.backing_pixel_x, resolution.backing_pixel_y
      ),
      render_display_note(&resolution.display),
      "cursorAfter=target".to_string(),
    ],
    artifacts: vec![artifact],
  })
}

fn scroll_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let (delta_x, delta_y, normalized_scroll) = resolve_scroll_deltas(call)?;
  let snapshot = enumerate_displays()?;
  let resolution = resolve_display_point(&snapshot, x, y)
    .ok_or_else(|| format!("logical point ({x:.3}, {y:.3}) is outside all connected displays"))?;
  if let Some(app) = app_identifier(call) {
    if !app.is_empty() {
      activate_target_app(&app)?;
    }
  }
  run_swift_script(&build_scroll_point_script(x, y, delta_x, delta_y))?;
  let report = [
    format!("capturedAt={}", snapshot.captured_at),
    format!("globalLogicalPoint={x:.3},{y:.3}"),
    format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ),
    format!("displayId={}", resolution.display.display_id),
    format!("deltaX={delta_x:.0}"),
    format!("deltaY={delta_y:.0}"),
    format!("normalizedScroll={normalized_scroll}"),
    "coordinateSpace=global-logical".to_string(),
    "cursorAfter=target".to_string(),
  ]
  .join("\n")
    + "\n";
  let artifact = build_text_artifact(
    "scroll-point",
    "txt",
    &format!(
      "scroll-point-{}-{}",
      sanitize_file_component(&format!("{x:.3}")),
      sanitize_file_component(&format!("{y:.3}"))
    ),
    report,
    "Scrolled at a macOS logical point through Quartz and recorded its coordinate contract.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Scrolled at global logical point ({x:.3}, {y:.3}) on display #{} with {}.",
      resolution.display.display_id, normalized_scroll
    ),
    backend: Some("macos.swift.quartz-scroll".to_string()),
    notes: vec![
      "coordinateSpace=global-logical".to_string(),
      format!("deltaX={delta_x:.0}"),
      format!("deltaY={delta_y:.0}"),
      format!("normalizedScroll={normalized_scroll}"),
      format!(
        "backingPixelPoint={},{}",
        resolution.backing_pixel_x, resolution.backing_pixel_y
      ),
      render_display_note(&resolution.display),
      "cursorAfter=target".to_string(),
    ],
    artifacts: vec![artifact],
  })
}

fn probe_permissions(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let screen_recording = run_swift_script(PROBE_SCREEN_RECORDING_SCRIPT)?
    .trim()
    .to_string();
  let accessibility = run_swift_script(PROBE_ACCESSIBILITY_SCRIPT)?
    .trim()
    .to_string();
  let automation = probe_automation_to_system_events();
  let launch_host = launch_host_process();

  let report = [
    format!("screenRecording={screen_recording}"),
    format!("accessibility={accessibility}"),
    format!("automationToSystemEvents={automation}"),
    format!("launchHostProcess={launch_host}"),
  ]
  .join("\n")
    + "\n";

  let artifact = build_text_artifact(
    "probe-permissions",
    "txt",
    "permission-report",
    report.clone(),
    "Captured macOS permission probe report from the desktop driver.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Permission probe: screenRecording={}, accessibility={}, automationToSystemEvents={}.",
      screen_recording, accessibility, automation
    ),
    backend: Some("macos.swift-and-osascript".to_string()),
    notes: report.lines().map(str::to_string).collect(),
    artifacts: vec![artifact],
  })
}

fn enumerate_displays() -> AuvResult<ObservedDisplaySnapshot> {
  let report = run_swift_script(ENUMERATE_DISPLAYS_SCRIPT)?;
  parse_display_snapshot(&report)
}

fn capture_screenshot_file(label: &str) -> AuvResult<PathBuf> {
  let temporary_path = screenshot_temp_path(label);
  let args = vec!["-x".to_string(), temporary_path.display().to_string()];
  run_command(SCREEN_CAPTURE_BINARY, &args)?;

  if !temporary_path.exists() {
    return Err(format!(
      "screencapture reported success but no image was created at {}",
      temporary_path.display()
    ));
  }

  Ok(temporary_path)
}

fn build_observe_windows_script(limit: i64, app_filter: &str) -> String {
  OBSERVE_WINDOWS_SCRIPT_TEMPLATE
    .replace("__LIMIT__", &limit.to_string())
    .replace("__APP_FILTER__", &swift_string_literal(app_filter))
}

fn build_observe_window_tree_script(app: &str, max_depth: i64, max_children: i64) -> String {
  OBSERVE_WINDOW_TREE_SCRIPT_TEMPLATE
    .replace("__APP_QUERY__", &swift_string_literal(app))
    .replace("__MAX_DEPTH__", &max_depth.to_string())
    .replace("__MAX_CHILDREN__", &max_children.to_string())
}

fn build_ocr_find_text_script(
  image_path: &Path,
  query: &str,
  exact: bool,
  case_sensitive: bool,
  max_observations: i64,
) -> String {
  OCR_FIND_TEXT_SCRIPT_TEMPLATE
    .replace(
      "__IMAGE_PATH__",
      &swift_string_literal(&image_path.display().to_string()),
    )
    .replace("__QUERY__", &swift_string_literal(query))
    .replace("__EXACT__", if exact { "true" } else { "false" })
    .replace(
      "__CASE_SENSITIVE__",
      if case_sensitive { "true" } else { "false" },
    )
    .replace("__MAX_OBSERVATIONS__", &max_observations.to_string())
}

fn build_click_point_script(x: f64, y: f64, button_code: i32, click_count: i64) -> String {
  CLICK_POINT_SCRIPT_TEMPLATE
    .replace("__X__", &format!("{x:.3}"))
    .replace("__Y__", &format!("{y:.3}"))
    .replace("__BUTTON__", &button_code.to_string())
    .replace("__CLICK_COUNT__", &click_count.to_string())
}

fn build_scroll_point_script(x: f64, y: f64, delta_x: f64, delta_y: f64) -> String {
  SCROLL_POINT_SCRIPT_TEMPLATE
    .replace("__X__", &format!("{x:.3}"))
    .replace("__Y__", &format!("{y:.3}"))
    .replace("__DELTA_X__", &format!("{:.0}", delta_x.round()))
    .replace("__DELTA_Y__", &format!("{:.0}", delta_y.round()))
}

fn probe_automation_to_system_events() -> String {
  let args = vec![
    "-e".to_string(),
    "tell application \"System Events\"".to_string(),
    "-e".to_string(),
    "return name of first application process whose frontmost is true".to_string(),
    "-e".to_string(),
    "end tell".to_string(),
  ];

  match run_command(OSASCRIPT_BINARY, &args) {
    Ok(_) => "granted".to_string(),
    Err(_) => "missing".to_string(),
  }
}

fn parse_display_snapshot(report: &str) -> AuvResult<ObservedDisplaySnapshot> {
  let captured_at = report_value(report, "capturedAt=")
    .unwrap_or("")
    .to_string();
  let displays = report
    .lines()
    .filter(|line| line.starts_with("display\t"))
    .map(parse_display_line)
    .collect::<AuvResult<Vec<_>>>()?;

  if displays.is_empty() {
    return Err("display probe returned no connected displays".to_string());
  }

  if let Some(raw_count) = report_value(report, "displayCount=") {
    let parsed_count = raw_count
      .parse::<usize>()
      .map_err(|error| format!("invalid displayCount value {}: {}", raw_count, error))?;
    if parsed_count != displays.len() {
      return Err(format!(
        "display probe reported {} displays but parsed {}",
        parsed_count,
        displays.len()
      ));
    }
  }

  Ok(ObservedDisplaySnapshot {
    combined_bounds: compute_combined_bounds(&displays),
    displays,
    captured_at,
  })
}

fn parse_display_line(line: &str) -> AuvResult<ObservedDisplay> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 15 {
    return Err(format!(
      "invalid display report line; expected 15 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(ObservedDisplay {
    display_id: parse_u32(columns[1], "displayId")?,
    is_main: parse_bool_flag(columns[2], "isMain")?,
    is_built_in: parse_bool_flag(columns[3], "isBuiltIn")?,
    bounds: ObservedRect {
      x: parse_i64(columns[4], "bounds.x")?,
      y: parse_i64(columns[5], "bounds.y")?,
      width: parse_i64(columns[6], "bounds.width")?,
      height: parse_i64(columns[7], "bounds.height")?,
    },
    visible_bounds: ObservedRect {
      x: parse_i64(columns[8], "visibleBounds.x")?,
      y: parse_i64(columns[9], "visibleBounds.y")?,
      width: parse_i64(columns[10], "visibleBounds.width")?,
      height: parse_i64(columns[11], "visibleBounds.height")?,
    },
    scale_factor: parse_f64(columns[12], "scaleFactor")?,
    pixel_width: parse_i64(columns[13], "pixelWidth")?,
    pixel_height: parse_i64(columns[14], "pixelHeight")?,
  })
}

fn parse_window_line(line: &str) -> AuvResult<ObservedWindow> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 9 {
    return Err(format!(
      "invalid window report line; expected 9 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(ObservedWindow {
    app_name: columns[1].to_string(),
    owner_pid: parse_i64(columns[2], "window.ownerPid")?,
    layer: parse_i64(columns[3], "window.layer")?,
    title: columns[4].to_string(),
    bounds: ObservedRect {
      x: parse_i64(columns[5], "window.bounds.x")?,
      y: parse_i64(columns[6], "window.bounds.y")?,
      width: parse_i64(columns[7], "window.bounds.width")?,
      height: parse_i64(columns[8], "window.bounds.height")?,
    },
  })
}

fn parse_ocr_text_snapshot(report: &str) -> AuvResult<OcrTextSnapshot> {
  let recognized_at = report_value(report, "recognizedAt=")
    .unwrap_or("")
    .to_string();
  let image_path = PathBuf::from(report_value(report, "imagePath=").unwrap_or(""));
  let image_width = parse_i64(
    report_value(report, "imageWidth=").unwrap_or("0"),
    "ocr.imageWidth",
  )?;
  let image_height = parse_i64(
    report_value(report, "imageHeight=").unwrap_or("0"),
    "ocr.imageHeight",
  )?;
  let query = report_value(report, "query=").unwrap_or("").to_string();
  let exact = parse_bool_flag(
    report_value(report, "exact=").unwrap_or("false"),
    "ocr.exact",
  )?;
  let case_sensitive = parse_bool_flag(
    report_value(report, "caseSensitive=").unwrap_or("false"),
    "ocr.caseSensitive",
  )?;
  let matches = report
    .lines()
    .filter(|line| line.starts_with("match\t"))
    .map(parse_ocr_text_line)
    .collect::<AuvResult<Vec<_>>>()?;
  Ok(OcrTextSnapshot {
    recognized_at,
    image_path,
    image_width,
    image_height,
    query,
    exact,
    case_sensitive,
    matches,
  })
}

fn parse_ocr_text_line(line: &str) -> AuvResult<OcrTextMatch> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 8 {
    return Err(format!(
      "invalid OCR report line; expected 8 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(OcrTextMatch {
    match_index: columns[1]
      .parse::<usize>()
      .map_err(|error| format!("invalid ocr.matchIndex value {}: {}", columns[1], error))?,
    text: columns[2].to_string(),
    confidence: parse_f64(columns[3], "ocr.confidence")?,
    bounds: ObservedRect {
      x: parse_i64(columns[4], "ocr.bounds.x")?,
      y: parse_i64(columns[5], "ocr.bounds.y")?,
      width: parse_i64(columns[6], "ocr.bounds.width")?,
      height: parse_i64(columns[7], "ocr.bounds.height")?,
    },
  })
}

fn parse_bool_flag(raw: &str, label: &str) -> AuvResult<bool> {
  match raw {
    "1" | "true" => Ok(true),
    "0" | "false" => Ok(false),
    other => Err(format!("invalid {} value {}: expected 0/1", label, other)),
  }
}

fn parse_i64(raw: &str, label: &str) -> AuvResult<i64> {
  raw
    .parse::<i64>()
    .map_err(|error| format!("invalid {} value {}: {}", label, raw, error))
}

fn parse_u32(raw: &str, label: &str) -> AuvResult<u32> {
  raw
    .parse::<u32>()
    .map_err(|error| format!("invalid {} value {}: {}", label, raw, error))
}

fn parse_f64(raw: &str, label: &str) -> AuvResult<f64> {
  let value = raw
    .parse::<f64>()
    .map_err(|error| format!("invalid {} value {}: {}", label, raw, error))?;
  if !value.is_finite() {
    return Err(format!(
      "invalid {} value {}: expected a finite number",
      label, raw
    ));
  }
  Ok(value)
}

fn compute_combined_bounds(displays: &[ObservedDisplay]) -> ObservedRect {
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

fn app_contains_window(app_identifier: &str, app_name: &str) -> bool {
  let app_identifier = app_identifier.trim().to_ascii_lowercase();
  let app_name = app_name.trim().to_ascii_lowercase();
  app_identifier == app_name
    || app_identifier.contains(&app_name)
    || app_name.contains(&app_identifier)
}

fn window_area(window: &ObservedWindow) -> i64 {
  window.bounds.width.saturating_mul(window.bounds.height)
}

fn ocr_match_center(matched: &OcrTextMatch) -> (f64, f64) {
  (
    matched.bounds.x as f64 + (matched.bounds.width as f64 / 2.0),
    matched.bounds.y as f64 + (matched.bounds.height as f64 / 2.0),
  )
}

fn project_main_screenshot_point(
  snapshot: &ObservedDisplaySnapshot,
  screenshot_x: f64,
  screenshot_y: f64,
) -> AuvResult<(f64, f64)> {
  let main_display = snapshot
    .displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| snapshot.displays.first())
    .ok_or_else(|| "display probe returned no connected displays".to_string())?;
  if screenshot_x < 0.0
    || screenshot_y < 0.0
    || screenshot_x >= main_display.pixel_width as f64
    || screenshot_y >= main_display.pixel_height as f64
  {
    return Err(format!(
      "screenshot pixel point ({screenshot_x:.3}, {screenshot_y:.3}) is outside main display physical bounds {}x{}",
      main_display.pixel_width, main_display.pixel_height
    ));
  }
  Ok((
    main_display.bounds.x as f64 + (screenshot_x / main_display.scale_factor),
    main_display.bounds.y as f64 + (screenshot_y / main_display.scale_factor),
  ))
}

fn resolve_window_point(
  call: &DriverCall,
  window: &ObservedWindow,
) -> AuvResult<(f64, f64, String)> {
  let offset_x = optional_f64(call, "offset_x")?;
  let offset_y = optional_f64(call, "offset_y")?;
  let relative_x = optional_f64(call, "relative_x")?;
  let relative_y = optional_f64(call, "relative_y")?;

  match (offset_x, offset_y, relative_x, relative_y) {
    (Some(offset_x), Some(offset_y), None, None) => Ok((
      window.bounds.x as f64 + offset_x,
      window.bounds.y as f64 + offset_y,
      format!("windowOffset={offset_x:.3},{offset_y:.3}"),
    )),
    (None, None, Some(relative_x), Some(relative_y)) => {
      if !(0.0..=1.0).contains(&relative_x) || !(0.0..=1.0).contains(&relative_y) {
        return Err(
          "relative window coordinates must be within 0.0..=1.0 for both axes".to_string(),
        );
      }
      Ok((
        window.bounds.x as f64 + (window.bounds.width as f64 * relative_x),
        window.bounds.y as f64 + (window.bounds.height as f64 * relative_y),
        format!("windowRelative={relative_x:.3},{relative_y:.3}"),
      ))
    }
    (Some(_), None, _, _) | (None, Some(_), _, _) => {
      Err("window point offset mode requires both --offset_x and --offset_y".to_string())
    }
    (_, _, Some(_), None) | (_, _, None, Some(_)) => {
      Err("window point relative mode requires both --relative_x and --relative_y".to_string())
    }
    (Some(_), Some(_), Some(_), Some(_)) => {
      Err("use either --offset_x/--offset_y or --relative_x/--relative_y, not both".to_string())
    }
    _ => Err(
      "operation requires either --offset_x/--offset_y or --relative_x/--relative_y".to_string(),
    ),
  }
}

fn resolve_display_point(
  snapshot: &ObservedDisplaySnapshot,
  x: f64,
  y: f64,
) -> Option<ObservedPointResolution> {
  let display = snapshot.displays.iter().find(|display| {
    let left = display.bounds.x as f64;
    let top = display.bounds.y as f64;
    let right = left + display.bounds.width as f64;
    let bottom = top + display.bounds.height as f64;
    x >= left && x < right && y >= top && y < bottom
  })?;
  let local_x = x - display.bounds.x as f64;
  let local_y = y - display.bounds.y as f64;

  Some(ObservedPointResolution {
    display: display.clone(),
    local_x,
    local_y,
    backing_pixel_x: (local_x * display.scale_factor).round() as i64,
    backing_pixel_y: (local_y * display.scale_factor).round() as i64,
  })
}

fn render_display_snapshot_report(snapshot: &ObservedDisplaySnapshot) -> String {
  let mut lines = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!("displayCount={}", snapshot.displays.len()),
    format!(
      "combinedBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
  ];
  for display in &snapshot.displays {
    lines.push(render_display_report_line(display));
  }
  lines.join("\n") + "\n"
}

fn render_point_identification_report(
  snapshot: &ObservedDisplaySnapshot,
  x: f64,
  y: f64,
  resolution: Option<&ObservedPointResolution>,
) -> String {
  let mut lines = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!("queryPoint={x:.3},{y:.3}"),
    format!(
      "combinedBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
  ];

  if let Some(resolution) = resolution {
    lines.push(format!("result=display#{}", resolution.display.display_id));
    lines.push(format!(
      "localPoint={:.3},{:.3}",
      resolution.local_x, resolution.local_y
    ));
    lines.push(format!(
      "backingPixelPoint={},{}",
      resolution.backing_pixel_x, resolution.backing_pixel_y
    ));
  } else {
    lines.push("result=outside".to_string());
  }

  for display in &snapshot.displays {
    lines.push(render_display_report_line(display));
  }

  lines.join("\n") + "\n"
}

fn parse_observed_ax_tree(report: &str) -> AuvResult<ObservedAxTreeSnapshot> {
  let observed_at = report_value(report, "observedAt=")
    .unwrap_or("")
    .to_string();
  let app_name = report_value(report, "appName=").unwrap_or("").to_string();
  let bundle_id = report_value(report, "bundleId=").unwrap_or("").to_string();
  let window_title = report_value(report, "windowTitle=")
    .unwrap_or("")
    .to_string();
  let nodes = report
    .lines()
    .filter(|line| line.starts_with("node\t"))
    .map(parse_observed_ax_node_line)
    .collect::<AuvResult<Vec<_>>>()?;

  if nodes.is_empty() {
    return Err("AX tree report contained no nodes".to_string());
  }

  Ok(ObservedAxTreeSnapshot {
    observed_at,
    app_name,
    bundle_id,
    window_title,
    nodes,
  })
}

fn parse_observed_ax_node_line(line: &str) -> AuvResult<ObservedAxNode> {
  let columns = line.split('\t').collect::<Vec<_>>();
  if columns.len() != 15 {
    return Err(format!(
      "invalid AX node report line; expected 15 columns but got {}: {}",
      columns.len(),
      line
    ));
  }

  Ok(ObservedAxNode {
    depth: columns[1]
      .parse::<usize>()
      .map_err(|error| format!("invalid AX node depth {}: {}", columns[1], error))?,
    path: columns[2].to_string(),
    role: columns[3].to_string(),
    subrole: columns[4].to_string(),
    title: columns[5].to_string(),
    description: columns[6].to_string(),
    help: columns[7].to_string(),
    identifier: columns[8].to_string(),
    placeholder: columns[9].to_string(),
    value: columns[10].to_string(),
    bounds: ObservedRect {
      x: parse_i64(columns[11], "ax.bounds.x")?,
      y: parse_i64(columns[12], "ax.bounds.y")?,
      width: parse_i64(columns[13], "ax.bounds.width")?,
      height: parse_i64(columns[14], "ax.bounds.height")?,
    },
  })
}

fn find_best_ax_node<'a>(
  snapshot: &'a ObservedAxTreeSnapshot,
  query: &str,
) -> Option<&'a ObservedAxNode> {
  let query = query.trim().to_lowercase();
  snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter_map(|node| score_ax_node_match(node, &query).map(|score| (score, node)))
    .max_by(|left, right| left.0.cmp(&right.0))
    .map(|(_, node)| node)
}

fn no_matching_ax_node_error(
  snapshot: &ObservedAxTreeSnapshot,
  query: &str,
  expected_kind: &str,
) -> String {
  if snapshot.nodes.len() <= 1 {
    return format!(
      "no matching {expected_kind} node found for query {query}; observed only {} AX node(s), so the target UI may need to be revealed before retrying",
      snapshot.nodes.len()
    );
  }
  format!("no matching {expected_kind} node found for query {query}")
}

fn score_ax_node_match(node: &ObservedAxNode, query: &str) -> Option<i64> {
  if query.is_empty() {
    return None;
  }

  let fields = [
    ("title", node.title.as_str()),
    ("description", node.description.as_str()),
    ("help", node.help.as_str()),
    ("identifier", node.identifier.as_str()),
    ("placeholder", node.placeholder.as_str()),
    ("value", node.value.as_str()),
  ];

  let mut score = 0i64;
  for (label, raw_value) in fields {
    let value = raw_value.trim().to_lowercase();
    if value.is_empty() || !value.contains(query) {
      continue;
    }

    score += match label {
      "title" => 80,
      "description" => 72,
      "placeholder" => 64,
      "help" => 56,
      "identifier" => 40,
      _ => 24,
    };
    if value == query {
      score += 20;
    }
  }

  if score == 0 {
    return None;
  }

  if node.role == "AXTextField" || node.subrole == "AXSearchField" {
    score += 24;
  }
  if node.role == "AXButton" || node.role == "AXLink" {
    score += 18;
  }
  if node.role == "AXUnknown" {
    score += 8;
  }

  Some(score - node.depth as i64)
}

fn ax_node_center(node: &ObservedAxNode) -> (f64, f64) {
  (
    node.bounds.x as f64 + (node.bounds.width as f64 / 2.0),
    node.bounds.y as f64 + (node.bounds.height as f64 / 2.0),
  )
}

fn render_ax_interaction_report(
  kind: &str,
  snapshot: &ObservedAxTreeSnapshot,
  node: &ObservedAxNode,
  query: &str,
) -> String {
  [
    format!("kind={kind}"),
    format!("observedAt={}", snapshot.observed_at),
    format!("appName={}", snapshot.app_name),
    format!("bundleId={}", snapshot.bundle_id),
    format!("windowTitle={}", snapshot.window_title),
    format!("query={query}"),
    format!("matchedPath={}", node.path),
    format!("matchedRole={}", node.role),
    format!("matchedSubrole={}", node.subrole),
    format!("matchedTitle={}", node.title),
    format!("matchedDescription={}", node.description),
    format!("matchedHelp={}", node.help),
    format!("matchedIdentifier={}", node.identifier),
    format!("matchedPlaceholder={}", node.placeholder),
    format!("matchedValue={}", node.value),
    format!("matchedBounds={}", render_rect_compact(&node.bounds)),
  ]
  .join("\n")
    + "\n"
}

fn render_display_report_line(display: &ObservedDisplay) -> String {
  format!(
    "display\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:.3}\t{}\t{}",
    display.display_id,
    if display.is_main { 1 } else { 0 },
    if display.is_built_in { 1 } else { 0 },
    display.bounds.x,
    display.bounds.y,
    display.bounds.width,
    display.bounds.height,
    display.visible_bounds.x,
    display.visible_bounds.y,
    display.visible_bounds.width,
    display.visible_bounds.height,
    display.scale_factor,
    display.pixel_width,
    display.pixel_height
  )
}

fn render_display_note(display: &ObservedDisplay) -> String {
  format!(
    "display#{} main={} builtIn={} bounds={} scaleFactor={:.3} pixels={}x{}",
    display.display_id,
    display.is_main,
    display.is_built_in,
    render_rect_compact(&display.bounds),
    display.scale_factor,
    display.pixel_width,
    display.pixel_height
  )
}

fn render_rect_compact(rect: &ObservedRect) -> String {
  format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height)
}

fn assess_coordinate_readiness(
  snapshot: &ObservedDisplaySnapshot,
  screenshot: &ScreenshotDimensions,
) -> AuvResult<CoordinateReadinessAssessment> {
  let main_display = snapshot
    .displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| snapshot.displays.first())
    .ok_or_else(|| "display probe returned no connected displays".to_string())?;
  let matches_main_logical = main_display.bounds.width == screenshot.width
    && main_display.bounds.height == screenshot.height;
  let matches_main_physical =
    main_display.pixel_width == screenshot.width && main_display.pixel_height == screenshot.height;
  let matches_combined_logical = snapshot.combined_bounds.width == screenshot.width
    && snapshot.combined_bounds.height == screenshot.height;
  let likely_retina_backing_mismatch =
    matches_main_physical && !matches_main_logical && main_display.scale_factor > 1.0;
  let ready_for_logical_input = matches_main_logical || matches_combined_logical;
  let reason = if ready_for_logical_input {
    if matches_main_logical && matches_combined_logical {
      "screenshot dimensions match both the main display and the combined logical bounds"
        .to_string()
    } else if matches_main_logical {
      "screenshot dimensions match the main display logical bounds".to_string()
    } else {
      "screenshot dimensions match the combined logical desktop bounds".to_string()
    }
  } else if likely_retina_backing_mismatch {
    format!(
      "screenshot dimensions match main display physical pixels while logical input uses {}x{} points; align Retina/backing-scale assumptions before real input",
      main_display.bounds.width, main_display.bounds.height
    )
  } else {
    format!(
      "screenshot {}x{} does not match main logical {}x{}, main physical {}x{}, or combined logical {}x{}",
      screenshot.width,
      screenshot.height,
      main_display.bounds.width,
      main_display.bounds.height,
      main_display.pixel_width,
      main_display.pixel_height,
      snapshot.combined_bounds.width,
      snapshot.combined_bounds.height
    )
  };

  Ok(CoordinateReadinessAssessment {
    ready_for_logical_input,
    matches_main_logical,
    matches_main_physical,
    matches_combined_logical,
    likely_retina_backing_mismatch,
    reason,
  })
}

fn render_coordinate_readiness_report(
  snapshot: &ObservedDisplaySnapshot,
  screenshot: &ScreenshotDimensions,
  assessment: &CoordinateReadinessAssessment,
) -> String {
  let main_display = snapshot
    .displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| snapshot.displays.first());
  let mut lines = vec![
    format!("capturedAt={}", snapshot.captured_at),
    format!("displayCount={}", snapshot.displays.len()),
    format!(
      "screenshotPixels={}x{}",
      screenshot.width, screenshot.height
    ),
    format!(
      "combinedLogicalBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ),
    format!(
      "readyForLogicalInput={}",
      assessment.ready_for_logical_input
    ),
    format!("matchesMainLogical={}", assessment.matches_main_logical),
    format!("matchesMainPhysical={}", assessment.matches_main_physical),
    format!(
      "matchesCombinedLogical={}",
      assessment.matches_combined_logical
    ),
    format!(
      "likelyRetinaBackingMismatch={}",
      assessment.likely_retina_backing_mismatch
    ),
    format!("reason={}", assessment.reason),
  ];
  if let Some(main_display) = main_display {
    lines.push(format!("mainDisplayId={}", main_display.display_id));
    lines.push(format!(
      "mainDisplayLogicalSize={}x{}",
      main_display.bounds.width, main_display.bounds.height
    ));
    lines.push(format!(
      "mainDisplayPixelSize={}x{}",
      main_display.pixel_width, main_display.pixel_height
    ));
    lines.push(format!(
      "mainDisplayScaleFactor={:.3}",
      main_display.scale_factor
    ));
  }
  for display in &snapshot.displays {
    lines.push(render_display_report_line(display));
  }
  lines.join("\n") + "\n"
}

fn read_png_dimensions(path: &Path) -> AuvResult<ScreenshotDimensions> {
  let mut file = fs::File::open(path)
    .map_err(|error| format!("failed to open screenshot {}: {error}", path.display()))?;
  let mut header = [0u8; 24];
  file
    .read_exact(&mut header)
    .map_err(|error| format!("failed to read PNG header {}: {error}", path.display()))?;

  const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];
  if header[..8] != PNG_SIGNATURE {
    return Err(format!(
      "screenshot {} is not a PNG produced by screencapture",
      path.display()
    ));
  }
  if &header[12..16] != b"IHDR" {
    return Err(format!(
      "screenshot {} is missing a PNG IHDR chunk",
      path.display()
    ));
  }

  let width = u32::from_be_bytes([header[16], header[17], header[18], header[19]]) as i64;
  let height = u32::from_be_bytes([header[20], header[21], header[22], header[23]]) as i64;
  Ok(ScreenshotDimensions { width, height })
}

fn run_swift_script(source: &str) -> AuvResult<String> {
  let script_path = temp_file_path("swift-script", "swift");
  fs::write(&script_path, source).map_err(|error| {
    format!(
      "failed to write Swift script {}: {error}",
      script_path.display()
    )
  })?;

  let result = run_swift_script_with_fallback(&script_path);
  let _ = fs::remove_file(&script_path);
  result
}

fn run_swift_script_with_fallback(script_path: &PathBuf) -> AuvResult<String> {
  let xcrun_args = vec!["swift".to_string(), script_path.display().to_string()];

  match run_command(XCRUN_BINARY, &xcrun_args) {
    Ok(output) => Ok(output.stdout),
    Err(error) if error.contains("failed to spawn xcrun") => {
      let swift_args = vec![script_path.display().to_string()];
      Ok(run_command("swift", &swift_args)?.stdout)
    }
    Err(error) => Err(error),
  }
}

fn run_command(binary: &str, args: &[String]) -> AuvResult<CommandOutput> {
  let output = Command::new(binary)
    .args(args)
    .output()
    .map_err(|error| match error.kind() {
      ErrorKind::NotFound => format!("failed to spawn {}: command not found", binary),
      _ => format!("failed to spawn {}: {}", binary, error),
    })?;

  let stdout = String::from_utf8_lossy(&output.stdout).to_string();
  let stderr = String::from_utf8_lossy(&output.stderr).to_string();

  if !output.status.success() {
    let trimmed_stderr = stderr.trim();
    return Err(format!(
      "{} exited with status {}: {}",
      binary,
      output.status,
      if trimmed_stderr.is_empty() {
        "no stderr output"
      } else {
        trimmed_stderr
      }
    ));
  }

  Ok(CommandOutput { stdout })
}

fn temp_file_path(label: &str, extension: &str) -> PathBuf {
  env::temp_dir().join(format!(
    "auv-{}-{}-{}.{}",
    sanitize_file_component(label),
    now_millis(),
    std::process::id(),
    extension
  ))
}

fn build_text_artifact(
  kind: &str,
  extension: &str,
  label: &str,
  content: String,
  note: &str,
) -> AuvResult<ProducedArtifact> {
  let source_path = temp_file_path(label, extension);
  fs::write(&source_path, content).map_err(|error| {
    format!(
      "failed to write artifact source {}: {error}",
      source_path.display()
    )
  })?;

  Ok(ProducedArtifact {
    kind: kind.to_string(),
    source_path,
    preferred_name: format!("{}.{}", sanitize_file_component(label), extension),
    note: Some(note.to_string()),
  })
}

fn screenshot_temp_path(label: &str) -> PathBuf {
  temp_file_path(label, "png")
}

fn render_capture_contract_report(
  snapshot: Option<&ObservedDisplaySnapshot>,
  dimensions: &ScreenshotDimensions,
  path: &Path,
) -> String {
  let mut lines = vec![
    format!("screenshotPath={}", path.display()),
    format!(
      "screenshotPixels={}x{}",
      dimensions.width, dimensions.height
    ),
    "coordinateContract=debug.captureScreen emits main-display physical screenshot pixels"
      .to_string(),
  ];
  if let Some(snapshot) = snapshot {
    lines.push(format!("capturedAt={}", snapshot.captured_at));
    lines.push(format!(
      "combinedLogicalBounds={}",
      render_rect_compact(&snapshot.combined_bounds)
    ));
    if let Some(main_display) = snapshot
      .displays
      .iter()
      .find(|display| display.is_main)
      .or_else(|| snapshot.displays.first())
    {
      lines.push(format!("mainDisplayId={}", main_display.display_id));
      lines.push(format!(
        "mainDisplayLogicalSize={}x{}",
        main_display.bounds.width, main_display.bounds.height
      ));
      lines.push(format!(
        "mainDisplayPixelSize={}x{}",
        main_display.pixel_width, main_display.pixel_height
      ));
      lines.push(format!(
        "mainDisplayScaleFactor={:.3}",
        main_display.scale_factor
      ));
    }
  } else {
    lines.push("displaySnapshot=unavailable".to_string());
  }
  lines.join("\n") + "\n"
}

fn require_macos() -> AuvResult<()> {
  if env::consts::OS != "macos" {
    return Err("macos.desktop is only available on macOS".to_string());
  }

  Ok(())
}

fn app_identifier(call: &DriverCall) -> Option<String> {
  optional_string(call, "app").or_else(|| {
    call
      .target
      .application_id
      .clone()
      .filter(|value| !value.trim().is_empty())
  })
}

fn optional_string(call: &DriverCall, key: &str) -> Option<String> {
  call.inputs.get(key).cloned()
}

fn optional_non_empty_string(call: &DriverCall, key: &str) -> Option<String> {
  optional_string(call, key)
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

fn required_non_empty_string(call: &DriverCall, key: &str) -> AuvResult<String> {
  let value = optional_non_empty_string(call, key)
    .ok_or_else(|| format!("operation requires --{} <text>", key))?;
  Ok(value)
}

fn required_f64(call: &DriverCall, key: &str) -> AuvResult<f64> {
  optional_f64(call, key)?.ok_or_else(|| format!("operation requires --{} <number>", key))
}

fn optional_f64(call: &DriverCall, key: &str) -> AuvResult<Option<f64>> {
  match call.inputs.get(key) {
    Some(value) => {
      let parsed = value
        .parse::<f64>()
        .map_err(|error| format!("invalid --{} value {}: {}", key, value, error))?;
      if !parsed.is_finite() {
        return Err(format!(
          "invalid --{} value {}: expected a finite number",
          key, value
        ));
      }
      Ok(Some(parsed))
    }
    None => Ok(None),
  }
}

fn optional_i64(call: &DriverCall, key: &str) -> AuvResult<Option<i64>> {
  match call.inputs.get(key) {
    Some(value) => value
      .parse::<i64>()
      .map(Some)
      .map_err(|error| format!("invalid --{} value {}: {}", key, value, error)),
    None => Ok(None),
  }
}

fn optional_bool(call: &DriverCall, key: &str) -> AuvResult<Option<bool>> {
  match optional_non_empty_string(call, key) {
    Some(value) => match value.to_ascii_lowercase().as_str() {
      "1" | "true" | "yes" | "on" => Ok(Some(true)),
      "0" | "false" | "no" | "off" => Ok(Some(false)),
      _ => Err(format!(
        "invalid --{} value {}: expected true/false or 1/0",
        key, value
      )),
    },
    None => Ok(None),
  }
}

fn optional_positive_u64(call: &DriverCall, key: &str) -> AuvResult<Option<u64>> {
  match optional_i64(call, key)? {
    Some(value) if value < 0 => Err(format!(
      "invalid --{} value {}: expected a non-negative integer",
      key, value
    )),
    Some(value) => Ok(Some(value as u64)),
    None => Ok(None),
  }
}

fn parse_mouse_button(call: &DriverCall) -> AuvResult<(&'static str, i32)> {
  match optional_string(call, "button")
    .unwrap_or_else(|| "left".to_string())
    .trim()
    .to_ascii_lowercase()
    .as_str()
  {
    "left" => Ok(("left", 0)),
    "right" => Ok(("right", 1)),
    "middle" => Ok(("middle", 2)),
    other => Err(format!(
      "invalid --button value {}; expected left, right, or middle",
      other
    )),
  }
}

fn resolve_scroll_deltas(call: &DriverCall) -> AuvResult<(f64, f64, String)> {
  let explicit_delta_x = optional_f64(call, "delta_x")?;
  let explicit_delta_y = optional_f64(call, "delta_y")?;
  if explicit_delta_x.is_some() || explicit_delta_y.is_some() {
    let delta_x = explicit_delta_x.unwrap_or(0.0);
    let delta_y = explicit_delta_y.unwrap_or(0.0);
    return Ok((
      delta_x,
      delta_y,
      format!("delta_x={:.0},delta_y={:.0}", delta_x, delta_y),
    ));
  }

  let direction = required_non_empty_string(call, "direction")?.to_ascii_lowercase();
  let pages = optional_f64(call, "pages")?.unwrap_or(1.0);
  if !pages.is_finite() || pages <= 0.0 {
    return Err(format!(
      "invalid --pages value {:.3}: expected a positive finite number",
      pages
    ));
  }
  let magnitude = (pages * 480.0).round();
  let (delta_x, delta_y) = match direction.as_str() {
    "up" => (0.0, magnitude),
    "down" => (0.0, -magnitude),
    "left" => (magnitude, 0.0),
    "right" => (-magnitude, 0.0),
    other => {
      return Err(format!(
        "invalid --direction value {}; expected up, down, left, or right",
        other
      ));
    }
  };

  Ok((
    delta_x,
    delta_y,
    format!("direction={direction},pages={pages:.3}"),
  ))
}

fn report_value<'a>(report: &'a str, prefix: &str) -> Option<&'a str> {
  report
    .lines()
    .find_map(|line| line.strip_prefix(prefix))
    .map(str::trim)
}

fn activate_target_app(app: &str) -> AuvResult<()> {
  let command = if looks_like_bundle_identifier(app) {
    format!(
      "tell application id {} to activate",
      osascript_string_literal(app)
    )
  } else {
    format!(
      "tell application {} to activate",
      osascript_string_literal(app)
    )
  };
  let args = vec!["-e".to_string(), command];
  run_command(OSASCRIPT_BINARY, &args).map(|_| ())
}

fn type_text_via_system_events(
  text: &str,
  replace_existing: bool,
  submit_key: Option<&str>,
  submit_settle_ms: u64,
) -> AuvResult<()> {
  let mut lines = vec!["tell application \"System Events\"".to_string()];
  if replace_existing {
    lines.push("keystroke \"a\" using {command down}".to_string());
    lines.push("delay 0.05".to_string());
    lines.push("key code 51".to_string());
    lines.push("delay 0.05".to_string());
  }
  lines.push(format!("keystroke {}", osascript_string_literal(text)));
  if let Some(submit_key) = submit_key {
    let key_code = special_key_code(submit_key)?;
    lines.push("delay 0.05".to_string());
    lines.push(format!("key code {key_code}"));
  }
  lines.push("end tell".to_string());
  run_osascript_lines(&lines)?;
  if submit_settle_ms > 0 {
    thread::sleep(Duration::from_millis(submit_settle_ms));
  }
  Ok(())
}

fn special_key_code(raw: &str) -> AuvResult<u32> {
  match raw.trim().to_ascii_lowercase().as_str() {
    "return" => Ok(36),
    "enter" => Ok(76),
    "tab" => Ok(48),
    "escape" | "esc" => Ok(53),
    "space" => Ok(49),
    other => Err(format!(
      "invalid submit key {}; supported values are return, enter, tab, escape, and space",
      other
    )),
  }
}

fn run_osascript_lines(lines: &[String]) -> AuvResult<CommandOutput> {
  let mut args = Vec::with_capacity(lines.len() * 2);
  for line in lines {
    args.push("-e".to_string());
    args.push(line.clone());
  }
  run_command(OSASCRIPT_BINARY, &args)
}

fn send_key_input(key: &str, settle_ms: u64) -> AuvResult<()> {
  if key.contains('+') {
    send_shortcut(key)?;
  } else if let Ok(key_code) = special_key_code(key) {
    run_osascript_lines(&[
      "tell application \"System Events\"".to_string(),
      format!("key code {key_code}"),
      "end tell".to_string(),
    ])?;
  } else if key.chars().count() == 1 {
    run_osascript_lines(&[format!(
      "tell application \"System Events\" to keystroke {}",
      osascript_string_literal(key)
    )])?;
  } else {
    return Err(format!(
      "invalid key {}; use a special key like Return, a shortcut like cmd+f, or debug.typeText for multi-character text",
      key
    ));
  }

  if settle_ms > 0 {
    thread::sleep(Duration::from_millis(settle_ms));
  }
  Ok(())
}

fn send_shortcut(shortcut: &str) -> AuvResult<()> {
  let parsed = parse_shortcut(shortcut)?;
  let line = if parsed.modifiers.is_empty() {
    format!(
      "tell application \"System Events\" to keystroke {}",
      osascript_string_literal(&parsed.key)
    )
  } else {
    format!(
      "tell application \"System Events\" to keystroke {} using {{{}}}",
      osascript_string_literal(&parsed.key),
      parsed.modifiers.join(", ")
    )
  };
  run_osascript_lines(&[line]).map(|_| ())
}

#[derive(Debug)]
struct ParsedShortcut {
  key: String,
  modifiers: Vec<&'static str>,
}

fn parse_shortcut(shortcut: &str) -> AuvResult<ParsedShortcut> {
  let raw_parts = shortcut
    .split('+')
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>();
  if raw_parts.len() < 2 {
    return Err(format!(
      "invalid shortcut {}; expected a form like cmd+f or cmd+shift+p",
      shortcut
    ));
  }

  let key = raw_parts
    .last()
    .map(|value| value.to_ascii_lowercase())
    .ok_or_else(|| format!("invalid shortcut {}; missing key", shortcut))?;
  if key.chars().count() != 1 {
    return Err(format!(
      "invalid shortcut {}; only single-character keys are currently supported",
      shortcut
    ));
  }

  let mut modifiers = Vec::new();
  for raw_modifier in &raw_parts[..raw_parts.len() - 1] {
    let modifier = match raw_modifier.to_ascii_lowercase().as_str() {
      "cmd" | "command" => "command down",
      "shift" => "shift down",
      "alt" | "option" => "option down",
      "ctrl" | "control" => "control down",
      other => {
        return Err(format!(
          "invalid shortcut {}; unsupported modifier {}",
          shortcut, other
        ));
      }
    };
    if !modifiers.contains(&modifier) {
      modifiers.push(modifier);
    }
  }

  Ok(ParsedShortcut { key, modifiers })
}

fn render_type_text_report(
  app: &str,
  text: &str,
  replace_existing: bool,
  submit_key: Option<&str>,
) -> String {
  let mut lines = vec![
    format!("typedAt={}", now_millis()),
    format!("app={app}"),
    format!("text={text}"),
    format!("textLength={}", text.chars().count()),
    format!("replaceExisting={replace_existing}"),
  ];
  if let Some(submit_key) = submit_key {
    lines.push(format!("submitKey={submit_key}"));
  }
  lines.join("\n")
}

fn looks_like_bundle_identifier(raw: &str) -> bool {
  raw.contains('.')
    && raw
      .chars()
      .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_'))
}

fn osascript_string_literal(raw: &str) -> String {
  let mut escaped = String::from("\"");
  for character in raw.chars() {
    match character {
      '\\' => escaped.push_str("\\\\"),
      '"' => escaped.push_str("\\\""),
      _ => escaped.push(character),
    }
  }
  escaped.push('"');
  escaped
}

fn launch_host_process() -> String {
  env::args()
    .next()
    .map(PathBuf::from)
    .as_ref()
    .and_then(|value| value.file_name())
    .and_then(|value| value.to_str())
    .unwrap_or("auv-cli")
    .to_string()
}

fn swift_string_literal(raw: &str) -> String {
  let mut escaped = String::from("\"");
  for character in raw.chars() {
    match character {
      '\\' => escaped.push_str("\\\\"),
      '"' => escaped.push_str("\\\""),
      '\n' => escaped.push_str("\\n"),
      '\r' => escaped.push_str("\\r"),
      '\t' => escaped.push_str("\\t"),
      _ => escaped.push(character),
    }
  }
  escaped.push('"');
  escaped
}

fn sanitize_file_component(raw: &str) -> String {
  let sanitized = raw
    .chars()
    .map(|character| match character {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
      _ => '-',
    })
    .collect::<String>()
    .trim_matches('-')
    .to_string();

  if sanitized.is_empty() {
    "artifact".to_string()
  } else {
    sanitized
  }
}

pub fn copy_file(source: &PathBuf, destination: &PathBuf) -> AuvResult<()> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create artifact directory {}: {error}",
        parent.display()
      )
    })?;
  }

  fs::copy(source, destination).map_err(|error| {
    format!(
      "failed to copy artifact from {} to {}: {error}",
      source.display(),
      destination.display()
    )
  })?;

  Ok(())
}

pub fn sanitized_artifact_name(raw: &str) -> String {
  sanitize_file_component(raw)
}

struct CommandOutput {
  stdout: String,
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::fs;
  use std::path::PathBuf;

  use super::{
    ScreenshotDimensions, assess_coordinate_readiness, optional_bool, optional_f64,
    parse_display_snapshot, parse_mouse_button, parse_ocr_text_snapshot, parse_shortcut,
    project_main_screenshot_point, read_png_dimensions, resolve_display_point,
    resolve_scroll_deltas, special_key_code,
  };
  use crate::model::{DriverCall, ExecutionTarget, now_millis};

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
    assert_eq!(super::sanitize_file_component("My App!"), "My-App");
    assert_eq!(
      super::sanitize_file_component("../../etc/passwd"),
      "etc-passwd"
    );
    assert_eq!(super::sanitize_file_component(""), "artifact");
  }

  #[test]
  fn swift_string_literal_escapes_correctly() {
    assert_eq!(super::swift_string_literal("hello"), "\"hello\"");
    assert_eq!(super::swift_string_literal("a\"b"), "\"a\\\"b\"");
    assert_eq!(super::swift_string_literal("a\\b"), "\"a\\\\b\"");
    assert_eq!(super::swift_string_literal("a\nb"), "\"a\\nb\"");
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
    assert_eq!(snapshot.query, "I DRINK THE LIGHT");
    assert_eq!(snapshot.image_width, 3024);
    assert_eq!(snapshot.image_height, 1964);
    assert_eq!(snapshot.matches.len(), 2);
    assert_eq!(snapshot.matches[0].match_index, 0);
    assert_eq!(snapshot.matches[0].text, "I DRINK THE LIGHT (Jengi Remix)");
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
  fn read_png_dimensions_extracts_width_and_height() {
    let path = temp_png_path("png-dimensions");
    write_minimal_png(&path, 3024, 1964);
    let dimensions = read_png_dimensions(&path).expect("PNG dimensions should parse");
    assert_eq!(dimensions.width, 3024);
    assert_eq!(dimensions.height, 1964);
    let _ = fs::remove_file(path);
  }

  #[test]
  fn driver_registry_stores_and_retrieves_drivers() {
    use super::{DriverRegistry, FixtureObserveDriver};
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
    }
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
query=I DRINK THE LIGHT\n\
exact=false\n\
caseSensitive=false\n\
match\t0\tI DRINK THE LIGHT (Jengi Remix)\t0.998901\t741\t1286\t513\t51\n\
match\t1\tTHE GODS WE CAN TOUCH\t0.945678\t1604\t808\t300\t42\n\
matchCount=2\n"
  }

  fn temp_png_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv-{}-{}.png", label, now_millis()))
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
}
