use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{ErrorKind, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::model::{
  AuvResult, DriverCall, DriverDescriptor, DriverResponse, ProducedArtifact, now_millis,
};

const PROBE_ACCESSIBILITY_SCRIPT: &str = include_str!("driver/macos/probe_accessibility.swift");
const PROBE_SCREEN_RECORDING_SCRIPT: &str =
  include_str!("driver/macos/probe_screen_recording.swift");
const ENUMERATE_DISPLAYS_SCRIPT: &str = include_str!("driver/macos/enumerate_displays.swift");

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
      donor_boundary: "AUV-native fixture driver; validate the shared execution substrate before platform drivers land.",
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
        "Use it to validate implicit run creation, artifact plumbing, and inspect output."
          .to_string(),
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
        "observe.permissions",
        "observe.displays",
        "observe.coordinate-readiness",
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
      "probe_permissions" => probe_permissions(call),
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
struct ScreenshotDimensions {
  width: i64,
  height: i64,
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

  Ok(DriverResponse {
    summary: "Captured one desktop screenshot through the shared AUV runtime.".to_string(),
    backend: Some("macos.screencapture".to_string()),
    notes: vec![
      format!(
        "Temporary screenshot created at {} before artifact ingestion.",
        temporary_path.display()
      ),
      "This remains a driver-level primitive instead of an AIRI-style desktop tool wrapper."
        .to_string(),
    ],
    artifacts: vec![ProducedArtifact {
      kind: "screenshot".to_string(),
      source_path: temporary_path,
      preferred_name: format!("{}.png", sanitize_file_component(&label)),
      note: Some(
        "Phase-1 screenshot artifact captured through the macOS desktop driver.".to_string(),
      ),
    }],
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

fn require_macos() -> AuvResult<()> {
  if env::consts::OS != "macos" {
    return Err("macos.desktop is only available on macOS".to_string());
  }

  Ok(())
}

fn optional_string(call: &DriverCall, key: &str) -> Option<String> {
  call.inputs.get(key).cloned()
}

fn report_value<'a>(report: &'a str, prefix: &str) -> Option<&'a str> {
  report
    .lines()
    .find_map(|line| line.strip_prefix(prefix))
    .map(str::trim)
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
    Driver, FixtureObserveDriver, ScreenshotDimensions, assess_coordinate_readiness,
    parse_display_snapshot, read_png_dimensions,
  };
  use crate::model::{DriverCall, ExecutionTarget, now_millis};

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
  fn parse_display_snapshot_rejects_non_finite_scale_factor() {
    let report = "capturedAt=2026-05-13T05:06:06Z\n\
displayCount=1\n\
display\t1\t1\t1\t0\t0\t1512\t982\t0\t65\t1512\t884\tNaN\t3024\t1964\n";
    let error = parse_display_snapshot(report).expect_err("NaN scale factor should be rejected");
    assert!(error.contains("finite number"));
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
    use super::DriverRegistry;
    let registry = DriverRegistry::new(vec![Box::new(FixtureObserveDriver)]);
    assert!(registry.get("fixture.observe").is_some());
    assert!(registry.get("missing").is_none());
    assert_eq!(registry.descriptors().len(), 1);
    assert_eq!(registry.descriptors()[0].id, "fixture.observe");
  }

  #[test]
  fn fixture_driver_rejects_unknown_operations() {
    let driver = FixtureObserveDriver;
    let error = driver
      .invoke(&DriverCall {
        operation: "unknown".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        working_directory: PathBuf::from("."),
      })
      .expect_err("unknown operation should fail");
    assert!(error.contains("does not support operation"));
  }

  #[test]
  fn fixture_driver_produces_deterministic_summary() {
    let driver = FixtureObserveDriver;
    let mut inputs = BTreeMap::new();
    inputs.insert("label".to_string(), format!("fixture-{}", now_millis()));
    let response = driver
      .invoke(&DriverCall {
        operation: "observe_fixture_scene".to_string(),
        target: ExecutionTarget {
          application_id: Some("fixture://example".to_string()),
        },
        inputs,
        working_directory: PathBuf::from("."),
      })
      .expect("fixture call should succeed");

    assert!(response.summary.contains("fixture://example"));
    assert_eq!(response.backend.as_deref(), Some("fixture.static"));
    assert!(response.artifacts.is_empty());
  }

  fn sample_display_report() -> &'static str {
    "capturedAt=2026-05-13T05:06:06Z\n\
displayCount=2\n\
display\t1\t1\t1\t0\t0\t1512\t982\t0\t65\t1512\t884\t2.000\t3024\t1964\n\
display\t3\t0\t0\t-222\t-1080\t1920\t1080\t-222\t-1080\t1920\t1080\t1.000\t1920\t1080\n"
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
