use std::time::Instant;

use super::*;

pub(super) fn capture_screen(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "desktop".to_string());
  let activated_app = maybe_activate_target_app_for_observation(call)?;
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
  if let Some(app) = activated_app {
    notes.push(format!("activatedTargetBeforeCapture={app}"));
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

pub(super) fn probe_coordinate_readiness(call: &DriverCall) -> AuvResult<DriverResponse> {
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

pub(super) fn probe_displays(_call: &DriverCall) -> AuvResult<DriverResponse> {
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

pub(super) fn observe_windows(call: &DriverCall) -> AuvResult<DriverResponse> {
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

pub(super) fn observe_windows_snapshot(
  limit: i64,
  app_filter: &str,
) -> AuvResult<ObservedWindowSnapshot> {
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

pub(super) fn observe_window_tree(call: &DriverCall) -> AuvResult<DriverResponse> {
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

pub(super) fn verify_now_playing_title(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let expected_title = required_non_empty_string(call, "target_title")?;
  let expected_artist = optional_non_empty_string(call, "target_artist");
  let scope_path_prefix = optional_non_empty_string(call, "scope_path_prefix");
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(24)
    .clamp(1, 60);
  if !app.is_empty() {
    activate_target_app(&app)?;
  }

  let tree_report = run_swift_script(&build_observe_window_tree_script(
    &app,
    max_depth,
    max_children,
  ))?;
  let snapshot = parse_observed_ax_tree(&tree_report)?;
  let matched = find_now_playing_ax_node(
    &snapshot,
    &expected_title,
    expected_artist.as_deref(),
    scope_path_prefix.as_deref(),
  )
  .ok_or_else(|| {
    let mut detail = format!(
      "no matching now-playing node found for target_title {}",
      expected_title
    );
    if let Some(artist) = expected_artist.as_deref() {
      detail.push_str(&format!(" and target_artist {}", artist));
    }
    detail
  })?;
  let report = render_ax_interaction_report(
    "verify-now-playing-title",
    &snapshot,
    matched,
    &expected_title,
  );
  let artifact = build_text_artifact(
    "verify-now-playing-title",
    "txt",
    &format!(
      "verify-now-playing-title-{}",
      sanitize_file_component(&expected_title)
    ),
    report,
    "Captured an AX tree snapshot and matched the current now-playing title without relying on screenshot OCR.",
  )?;

  let mut notes = vec![
    format!("targetTitle={expected_title}"),
    format!("matchedPath={}", matched.path),
    format!("matchedRole={}", matched.role),
    format!("matchedBounds={}", render_rect_compact(&matched.bounds)),
  ];
  if let Some(artist) = expected_artist.as_deref() {
    notes.push(format!("targetArtist={artist}"));
  }
  if let Some(scope) = scope_path_prefix.as_deref() {
    notes.push(format!("scopePathPrefix={scope}"));
  }
  if !matched.title.is_empty() {
    notes.push(format!("matchedTitle={}", matched.title));
  }
  if !matched.description.is_empty() {
    notes.push(format!("matchedDescription={}", matched.description));
  }
  if !matched.value.is_empty() {
    notes.push(format!("matchedValue={}", matched.value));
  }

  Ok(DriverResponse {
    summary: format!(
      "Verified now-playing title {} in {} through the AX tree.",
      expected_title,
      if snapshot.app_name.is_empty() {
        "target app"
      } else {
        &snapshot.app_name
      }
    ),
    backend: Some("macos.observe.verify-now-playing-title".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

pub(super) fn verify_ax_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let expected_text = required_non_empty_string(call, "target_text")?;
  let expected_role = optional_non_empty_string(call, "target_role");
  let expected_subrole = optional_non_empty_string(call, "target_subrole");
  let scope_path_prefix = optional_non_empty_string(call, "scope_path_prefix");
  let max_depth = optional_i64(call, "max_depth")?.unwrap_or(6).clamp(1, 10);
  let max_children = optional_i64(call, "max_children")?
    .unwrap_or(24)
    .clamp(1, 60);
  if !app.is_empty() {
    activate_target_app(&app)?;
  }

  let tree_report = run_swift_script(&build_observe_window_tree_script(
    &app,
    max_depth,
    max_children,
  ))?;
  let snapshot = parse_observed_ax_tree(&tree_report)?;
  let expected_text_lc = expected_text.trim().to_lowercase();
  let expected_role_lc = expected_role
    .as_deref()
    .map(|value| value.trim().to_lowercase())
    .filter(|value| !value.is_empty());
  let expected_subrole_lc = expected_subrole
    .as_deref()
    .map(|value| value.trim().to_lowercase())
    .filter(|value| !value.is_empty());
  let scope_path_prefix = scope_path_prefix
    .as_deref()
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());

  let matched = snapshot
    .nodes
    .iter()
    .filter(|node| node.bounds.width > 0 && node.bounds.height > 0)
    .filter(|node| {
      scope_path_prefix
        .as_ref()
        .is_none_or(|prefix| node.path.starts_with(prefix))
    })
    .filter(|node| {
      if let Some(role) = expected_role_lc.as_deref() {
        node.role.to_lowercase() == role
      } else {
        true
      }
    })
    .filter(|node| {
      if let Some(subrole) = expected_subrole_lc.as_deref() {
        node.subrole.to_lowercase() == subrole
      } else {
        true
      }
    })
    .filter_map(|node| {
      let searchable = ax_node_search_text(node);
      if searchable.contains(&expected_text_lc) {
        Some((100 - node.depth as i64, node))
      } else {
        None
      }
    })
    .max_by(|left, right| left.0.cmp(&right.0))
    .map(|(_, node)| node)
    .ok_or_else(|| {
      let mut detail = format!(
        "no matching ax text node found for target_text {}",
        expected_text
      );
      if let Some(role) = expected_role.as_deref() {
        detail.push_str(&format!(" and target_role {}", role));
      }
      if let Some(subrole) = expected_subrole.as_deref() {
        detail.push_str(&format!(" and target_subrole {}", subrole));
      }
      if let Some(scope) = scope_path_prefix.as_deref() {
        detail.push_str(&format!(" within scope_path_prefix {}", scope));
      }
      detail
    })?;

  let report = render_ax_interaction_report("verify-ax-text", &snapshot, matched, &expected_text);
  let artifact = build_text_artifact(
    "verify-ax-text",
    "txt",
    &format!("verify-ax-text-{}", sanitize_file_component(&expected_text)),
    report,
    "Captured an AX tree snapshot and matched a text-bearing node without relying on screenshot OCR.",
  )?;

  let mut notes = vec![
    format!("targetText={expected_text}"),
    format!("matchedPath={}", matched.path),
    format!("matchedRole={}", matched.role),
    format!("matchedBounds={}", render_rect_compact(&matched.bounds)),
  ];
  if let Some(role) = expected_role.as_deref() {
    notes.push(format!("targetRole={role}"));
  }
  if let Some(subrole) = expected_subrole.as_deref() {
    notes.push(format!("targetSubrole={subrole}"));
  }
  if let Some(scope) = scope_path_prefix.as_deref() {
    notes.push(format!("scopePathPrefix={scope}"));
  }
  if !matched.title.is_empty() {
    notes.push(format!("matchedTitle={}", matched.title));
  }
  if !matched.description.is_empty() {
    notes.push(format!("matchedDescription={}", matched.description));
  }
  if !matched.value.is_empty() {
    notes.push(format!("matchedValue={}", matched.value));
  }

  let mut summary_suffix = String::new();
  if let Some(role) = expected_role.as_deref() {
    summary_suffix.push_str(&format!(" as {role}"));
  }
  if let Some(subrole) = expected_subrole.as_deref() {
    summary_suffix.push_str(&format!(" ({subrole})"));
  }

  Ok(DriverResponse {
    summary: format!(
      "Verified AX text {} in {}{} through the AX tree.",
      expected_text,
      if snapshot.app_name.is_empty() {
        "target app"
      } else {
        &snapshot.app_name
      },
      summary_suffix
    ),
    backend: Some("macos.observe.verify-ax-text".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

pub(super) fn project_screenshot_point(call: &DriverCall) -> AuvResult<DriverResponse> {
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

pub(super) fn find_screen_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let label = format!("screen-text-{}", sanitize_file_component(&query));
  let activated_app = maybe_activate_target_app_for_observation(call)?;
  let screenshot_path = capture_screenshot_file(&label)?;
  let dimensions = read_png_dimensions(&screenshot_path)?;
  let snapshot = enumerate_displays()?;
  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(64)
    .clamp(1, 256);
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
  let ocr_report = run_swift_script(&build_ocr_find_text_script(
    screenshot_path.as_path(),
    &query,
    exact,
    case_sensitive,
    max_observations,
    region.as_ref(),
  ))?;
  let ocr_snapshot = parse_ocr_text_snapshot(&ocr_report)?;
  let filtered_matches = filter_ocr_matches(&ocr_snapshot.matches, min_confidence, region.as_ref());
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
    format!("filteredMatchCount={}", filtered_matches.len()),
    format!("caseSensitive={case_sensitive}"),
    format!("exact={exact}"),
    format!("minConfidence={min_confidence:.3}"),
    format!(
      "screenshotPixels={}x{}",
      ocr_snapshot.image_width, ocr_snapshot.image_height
    ),
  ];
  if let Some(region) = region.as_ref() {
    notes.push(render_ocr_region_note(region));
  }
  if let Some(app) = activated_app {
    notes.push(format!("activatedTargetBeforeCapture={app}"));
  }

  let summary = if let Some(best_match) = filtered_matches.first() {
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
      "Found {} OCR text match(es) for query {} after filtering; best anchor {} projects to logical point ({logical_x:.3}, {logical_y:.3}).",
      filtered_matches.len(),
      query,
      best_match.text
    )
  } else {
    "Found 0 OCR text matches in the current desktop screenshot after applying the active filters."
      .to_string()
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.vision.screen-text".to_string()),
    notes,
    artifacts: vec![screenshot_artifact, report_artifact],
  })
}

pub(super) fn wait_for_screen_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let label = format!("screen-text-wait-{}", sanitize_file_component(&query));
  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(64)
    .clamp(1, 256);
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let timeout_ms = optional_positive_u64(call, "timeout_ms")?.unwrap_or(3000);
  let poll_interval_ms = optional_positive_u64(call, "poll_interval_ms")?.unwrap_or(250);
  let started_at = Instant::now();
  let mut attempts = 0usize;
  let mut previous_screenshot_path: Option<PathBuf> = None;

  loop {
    attempts += 1;
    let attempt_label = format!("{label}-attempt-{attempts}");
    let activated_app = maybe_activate_target_app_for_observation(call)?;
    let screenshot_path = capture_screenshot_file(&attempt_label)?;
    let dimensions = read_png_dimensions(&screenshot_path)?;
    let snapshot = enumerate_displays()?;
    let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
    let ocr_report = run_swift_script(&build_ocr_find_text_script(
      screenshot_path.as_path(),
      &query,
      exact,
      case_sensitive,
      max_observations,
      region.as_ref(),
    ))?;
    let ocr_snapshot = parse_ocr_text_snapshot(&ocr_report)?;
    let filtered_matches =
      filter_ocr_matches(&ocr_snapshot.matches, min_confidence, region.as_ref())
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let timed_out = elapsed_ms >= timeout_ms;

    if !filtered_matches.is_empty() || timed_out {
      if let Some(previous_path) = previous_screenshot_path {
        let _ = fs::remove_file(previous_path);
      }

      let report_artifact = build_text_artifact(
        "screen-text-wait-report",
        "txt",
        &format!(
          "screen-text-wait-report-{}",
          sanitize_file_component(&query)
        ),
        ocr_report,
        "Captured Vision OCR text-anchor report from the final wait-for-screen-text polling attempt.",
      )?;
      let screenshot_artifact = ProducedArtifact {
        kind: "screenshot".to_string(),
        source_path: screenshot_path,
        preferred_name: format!("{}.png", sanitize_file_component(&label)),
        note: Some(
          "Final screenshot retained from waitForScreenText polling over the live desktop."
            .to_string(),
        ),
      };
      let mut notes = vec![
        format!("query={query}"),
        format!("attemptCount={attempts}"),
        format!("elapsedMs={elapsed_ms}"),
        format!("timeoutMs={timeout_ms}"),
        format!("pollIntervalMs={poll_interval_ms}"),
        format!("timedOut={timed_out}"),
        format!("matchCount={}", ocr_snapshot.matches.len()),
        format!("filteredMatchCount={}", filtered_matches.len()),
        format!("caseSensitive={case_sensitive}"),
        format!("exact={exact}"),
        format!("minConfidence={min_confidence:.3}"),
        format!(
          "screenshotPixels={}x{}",
          ocr_snapshot.image_width, ocr_snapshot.image_height
        ),
      ];
      if let Some(region) = region.as_ref() {
        notes.push(render_ocr_region_note(region));
      }
      if let Some(app) = activated_app {
        notes.push(format!("activatedTargetBeforeCapture={app}"));
      }

      let summary = if let Some(best_match) = filtered_matches.first() {
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
          "Observed OCR text anchor {} after {} polling attempt(s) over {} ms; best anchor projects to logical point ({logical_x:.3}, {logical_y:.3}).",
          best_match.text, attempts, elapsed_ms
        )
      } else {
        "Timed out while polling the live desktop for a filtered OCR text anchor.".to_string()
      };

      return Ok(DriverResponse {
        summary,
        backend: Some("macos.vision.wait-screen-text".to_string()),
        notes,
        artifacts: vec![screenshot_artifact, report_artifact],
      });
    }

    if let Some(previous_path) = previous_screenshot_path.replace(screenshot_path) {
      let _ = fs::remove_file(previous_path);
    }
    thread::sleep(Duration::from_millis(poll_interval_ms));
  }
}

pub(super) fn find_screen_rows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "screen-rows".to_string());
  let activated_app = maybe_activate_target_app_for_observation(call)?;
  let screenshot_path = capture_screenshot_file(&label)?;
  let dimensions = read_png_dimensions(&screenshot_path)?;
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(128)
    .clamp(1, 512);
  let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
  let detection = detect_screen_rows(
    screenshot_path.as_path(),
    min_confidence,
    max_observations,
    region.as_ref(),
  )?;
  let rows = detection.rows;
  let report_artifact = build_text_artifact(
    "screen-rows-report",
    "txt",
    &format!("screen-rows-report-{}", sanitize_file_component(&label)),
    detection.report,
    "Captured row-detection report used for visible-row grouping (OCR first, then visual-band fallback).",
  )?;
  let screenshot_artifact = ProducedArtifact {
    kind: "screenshot".to_string(),
    source_path: screenshot_path,
    preferred_name: format!("{}.png", sanitize_file_component(&label)),
    note: Some("Screenshot captured for OCR-based visible-row detection.".to_string()),
  };
  let mut notes = vec![
    format!("rowStrategy={}", detection.strategy),
    format!("rowCount={}", rows.len()),
    format!("matchCount={}", detection.raw_match_count),
    format!("filteredMatchCount={}", detection.filtered_match_count),
    format!("minConfidence={min_confidence:.3}"),
    format!(
      "screenshotPixels={}x{}",
      dimensions.width, dimensions.height
    ),
  ];
  if let Some(region) = region.as_ref() {
    notes.push(render_ocr_region_note(region));
  }
  if let Some(app) = activated_app {
    notes.push(format!("activatedTargetBeforeCapture={app}"));
  }
  for row in rows.iter().take(5) {
    notes.push(render_ocr_row_note(row));
  }

  let summary = if let Some(first_row) = rows.first() {
    let preview = if first_row.text_fragments.is_empty() {
      format!("bounds={}", render_rect_compact(&first_row.bounds))
    } else {
      first_row
        .text_fragments
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(" | ")
    };
    format!(
      "Detected {} visible row(s) with strategy {} in the constrained region; first row preview: {}.",
      rows.len(),
      detection.strategy,
      preview
    )
  } else {
    format!(
      "Detected 0 visible row(s) in the constrained region after strategy {}.",
      detection.strategy
    )
  };

  Ok(DriverResponse {
    summary,
    backend: Some(format!("macos.vision.screen-rows.{}", detection.strategy)),
    notes,
    artifacts: vec![screenshot_artifact, report_artifact],
  })
}

pub(super) fn wait_for_screen_rows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "screen-rows-wait".to_string());
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(128)
    .clamp(1, 512);
  let min_row_count = optional_i64(call, "min_row_count")?
    .unwrap_or(1)
    .clamp(1, 64) as usize;
  let timeout_ms = optional_positive_u64(call, "timeout_ms")?.unwrap_or(3000);
  let poll_interval_ms = optional_positive_u64(call, "poll_interval_ms")?.unwrap_or(250);
  let started_at = Instant::now();
  let mut attempts = 0usize;
  let mut previous_screenshot_path: Option<PathBuf> = None;

  loop {
    attempts += 1;
    let attempt_label = format!("{label}-attempt-{attempts}");
    let activated_app = maybe_activate_target_app_for_observation(call)?;
    let screenshot_path = capture_screenshot_file(&attempt_label)?;
    let dimensions = read_png_dimensions(&screenshot_path)?;
    let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
    let detection = detect_screen_rows(
      screenshot_path.as_path(),
      min_confidence,
      max_observations,
      region.as_ref(),
    )?;
    let rows = detection.rows;
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let timed_out = elapsed_ms >= timeout_ms;

    if rows.len() >= min_row_count || timed_out {
      if let Some(previous_path) = previous_screenshot_path {
        let _ = fs::remove_file(previous_path);
      }
      let report_artifact = build_text_artifact(
        "screen-rows-wait-report",
        "txt",
        &format!(
          "screen-rows-wait-report-{}",
          sanitize_file_component(&label)
        ),
        detection.report,
        "Captured row-detection report from the final wait-for-screen-rows polling attempt.",
      )?;
      let screenshot_artifact = ProducedArtifact {
        kind: "screenshot".to_string(),
        source_path: screenshot_path,
        preferred_name: format!("{}.png", sanitize_file_component(&label)),
        note: Some(
          "Final screenshot retained from waitForScreenRows polling over the live desktop."
            .to_string(),
        ),
      };
      let mut notes = vec![
        format!("rowStrategy={}", detection.strategy),
        format!("rowCount={}", rows.len()),
        format!("requiredRowCount={min_row_count}"),
        format!("attemptCount={attempts}"),
        format!("elapsedMs={elapsed_ms}"),
        format!("timeoutMs={timeout_ms}"),
        format!("pollIntervalMs={poll_interval_ms}"),
        format!("timedOut={timed_out}"),
        format!("matchCount={}", detection.raw_match_count),
        format!("filteredMatchCount={}", detection.filtered_match_count),
        format!("minConfidence={min_confidence:.3}"),
        format!(
          "screenshotPixels={}x{}",
          dimensions.width, dimensions.height
        ),
      ];
      if let Some(region) = region.as_ref() {
        notes.push(render_ocr_region_note(region));
      }
      if let Some(app) = activated_app {
        notes.push(format!("activatedTargetBeforeCapture={app}"));
      }
      for row in rows.iter().take(5) {
        notes.push(render_ocr_row_note(row));
      }

      let summary = if rows.len() >= min_row_count {
        format!(
          "Observed {} visible row(s) with strategy {} after {} polling attempt(s) over {} ms.",
          rows.len(),
          detection.strategy,
          attempts,
          elapsed_ms
        )
      } else {
        format!(
          "Timed out while polling the live desktop for visible rows after strategy {}.",
          detection.strategy
        )
      };

      return Ok(DriverResponse {
        summary,
        backend: Some(format!(
          "macos.vision.wait-screen-rows.{}",
          detection.strategy
        )),
        notes,
        artifacts: vec![screenshot_artifact, report_artifact],
      });
    }

    if let Some(previous_path) = previous_screenshot_path.replace(screenshot_path) {
      let _ = fs::remove_file(previous_path);
    }
    thread::sleep(Duration::from_millis(poll_interval_ms));
  }
}

pub(super) fn find_image_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = required_non_empty_string(call, "query")?;
  let image_path = PathBuf::from(required_non_empty_string(call, "image_path")?);
  if !image_path.exists() {
    return Err(format!(
      "image_path does not exist: {}",
      image_path.display()
    ));
  }

  let exact = optional_bool(call, "exact")?.unwrap_or(false);
  let case_sensitive = optional_bool(call, "case_sensitive")?.unwrap_or(false);
  let max_observations = optional_i64(call, "max_observations")?
    .unwrap_or(64)
    .clamp(1, 256);
  let dimensions = read_png_dimensions(&image_path)?;
  let min_confidence = optional_f64(call, "min_confidence")?.unwrap_or(0.0);
  if !(0.0..=1.0).contains(&min_confidence) {
    return Err(format!(
      "invalid --min_confidence value {:.3}: expected a ratio within 0.0..=1.0",
      min_confidence
    ));
  }
  let region = parse_ocr_region_constraint(call, dimensions.width, dimensions.height)?;
  let ocr_report = run_swift_script(&build_ocr_find_text_script(
    image_path.as_path(),
    &query,
    exact,
    case_sensitive,
    max_observations,
    region.as_ref(),
  ))?;
  let ocr_snapshot = parse_ocr_text_snapshot(&ocr_report)?;
  let filtered_matches = filter_ocr_matches(&ocr_snapshot.matches, min_confidence, region.as_ref());
  let report_artifact = build_text_artifact(
    "image-text-report",
    "txt",
    &format!("image-text-report-{}", sanitize_file_component(&query)),
    ocr_report,
    "Captured Vision OCR text-anchor report for a provided image artifact.",
  )?;

  let mut notes = vec![
    format!("query={query}"),
    format!("imagePath={}", image_path.display()),
    format!("matchCount={}", ocr_snapshot.matches.len()),
    format!("filteredMatchCount={}", filtered_matches.len()),
    format!("caseSensitive={case_sensitive}"),
    format!("exact={exact}"),
    format!("minConfidence={min_confidence:.3}"),
    format!(
      "imagePixels={}x{}",
      ocr_snapshot.image_width, ocr_snapshot.image_height
    ),
  ];
  if let Some(region) = region.as_ref() {
    notes.push(render_ocr_region_note(region));
  }

  let summary = if let Some(best_match) = filtered_matches.first() {
    notes.push(format!("bestMatchText={}", best_match.text));
    notes.push(format!(
      "bestMatchBounds={}",
      render_rect_compact(&best_match.bounds)
    ));
    notes.push(format!("bestMatchConfidence={:.3}", best_match.confidence));
    format!(
      "Found {} OCR text match(es) for query {} inside the provided image after filtering; best anchor is {}.",
      filtered_matches.len(),
      query,
      best_match.text
    )
  } else {
    "Found 0 OCR text matches in the provided image after applying the active filters.".to_string()
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.vision.image-text".to_string()),
    notes,
    artifacts: vec![report_artifact],
  })
}

pub(super) fn identify_point(call: &DriverCall) -> AuvResult<DriverResponse> {
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

pub(super) fn probe_permissions(_call: &DriverCall) -> AuvResult<DriverResponse> {
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
