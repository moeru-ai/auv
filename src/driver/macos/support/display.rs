use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use super::super::*;
use super::{
  activate_target_app, app_identifier, compute_combined_bounds, optional_bool, parse_bool_flag,
  parse_f64, parse_i64, parse_u32, render_rect_compact, report_value, run_command,
  run_swift_script, screenshot_temp_path,
};

pub(crate) fn enumerate_displays() -> AuvResult<ObservedDisplaySnapshot> {
  let report = run_swift_script(ENUMERATE_DISPLAYS_SCRIPT)?;
  parse_display_snapshot(&report)
}

pub(crate) fn capture_screenshot_file(label: &str) -> AuvResult<PathBuf> {
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

pub(crate) fn maybe_activate_target_app_for_observation(
  call: &DriverCall,
) -> AuvResult<Option<String>> {
  let Some(app) = app_identifier(call) else {
    return Ok(None);
  };
  if app.is_empty() || !optional_bool(call, "activate_target_before_capture")?.unwrap_or(false) {
    return Ok(None);
  }

  activate_target_app(&app)?;
  Ok(Some(app))
}

pub(crate) fn parse_display_snapshot(report: &str) -> AuvResult<ObservedDisplaySnapshot> {
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

pub(crate) fn parse_display_line(line: &str) -> AuvResult<ObservedDisplay> {
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

pub(crate) fn parse_window_line(line: &str) -> AuvResult<ObservedWindow> {
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

pub(crate) fn render_display_snapshot_report(snapshot: &ObservedDisplaySnapshot) -> String {
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

pub(crate) fn render_point_identification_report(
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

pub(crate) fn render_display_report_line(display: &ObservedDisplay) -> String {
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

pub(crate) fn render_display_note(display: &ObservedDisplay) -> String {
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

pub(crate) fn assess_coordinate_readiness(
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

pub(crate) fn render_coordinate_readiness_report(
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

pub(crate) fn read_png_dimensions(path: &Path) -> AuvResult<ScreenshotDimensions> {
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

pub(crate) fn render_capture_contract_report(
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
