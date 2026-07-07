use crate::*;

#[cfg(not(target_os = "macos"))]
pub fn run_live_scan(_inputs: &Inputs) -> Result<PlaylistSidebarScan, String> {
  Err("live NetEase playlist sidebar scan is only supported on macOS".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn run_live_scan_until_query(_inputs: &Inputs, _query: &str) -> Result<PlaylistSidebarScan, String> {
  Err("live NetEase playlist sidebar scan is only supported on macOS".to_string())
}

#[cfg(target_os = "macos")]
pub fn run_live_scan(inputs: &Inputs) -> Result<PlaylistSidebarScan, String> {
  run_live_scan_inner(inputs, None)
}

#[cfg(target_os = "macos")]
pub fn run_live_scan_until_query(inputs: &Inputs, query: &str) -> Result<PlaylistSidebarScan, String> {
  run_live_scan_inner(inputs, Some(query))
}

// NOTICE(a6c-8b/a6c-10b): short numeric playlist labels get query custom_words
// plus probe default languages when the caller did not set recognition_languages.
// Full-window OCR fallback remains empty-sidebar-only in capture_observation.
pub(crate) fn sidebar_ls_scan_ocr_options(base: &TextRecognitionOptions, query: Option<&str>) -> TextRecognitionOptions {
  let Some(query) = query else {
    return base.clone();
  };
  let recognition_languages =
    if crate::view_parsers::sidebar::parse::is_single_ascii_digit_query(query) && base.recognition_languages.is_none() {
      crate::view_parsers::sidebar::target_probe::build_sidebar_target_probe_ocr_options(base, query, query).recognition_languages
    } else {
      base.recognition_languages.clone()
    };
  TextRecognitionOptions {
    custom_words: crate::view_parsers::sidebar::target_probe::merge_custom_words(&base.custom_words, &[query]),
    recognition_languages,
  }
}

#[cfg(target_os = "macos")]
fn run_live_scan_inner(inputs: &Inputs, query: Option<&str>) -> Result<PlaylistSidebarScan, String> {
  let driver = MacosDriver::new();
  let default_app_context = ScanAppContext {
    app_id: Some(inputs.app_id.clone()),
    name: None,
    version: None,
  };
  let mut session = match driver.open_local() {
    Ok(session) => session,
    Err(error) => {
      return Ok(PlaylistSidebarScan::empty_with_diagnostic(
        default_app_context,
        ScanWindowContext::default(),
        ViewRegionRecord::default(),
        ParserDiagnostic {
          code: "driver_open_failed".to_string(),
          message: error.to_string(),
          node_id: None,
        },
        "scan stopped before sidebar observation because the macOS driver could not be opened",
      ));
    }
  };
  let app = App::bundle(inputs.app_id.clone());
  let window = match session.window().resolve(Window::main_visible().owned_by(app)) {
    Ok(window) => window,
    Err(error) => {
      return Ok(PlaylistSidebarScan::empty_with_diagnostic(
        default_app_context,
        ScanWindowContext::default(),
        ViewRegionRecord::default(),
        ParserDiagnostic {
          code: "target_window_not_found".to_string(),
          message: error.to_string(),
          node_id: None,
        },
        "scan stopped before sidebar observation because the target window could not be resolved",
      ));
    }
  };
  let window_size = Size::new(window.frame.size.width, window.frame.size.height);
  let app_context = ScanAppContext {
    app_id: window.app_bundle_id.clone().or_else(|| Some(inputs.app_id.clone())),
    name: window.app_name.clone(),
    version: None,
  };
  let window_context = ScanWindowContext {
    id: Some(window.reference.id.clone()),
    title: window.title.clone(),
    bounds: Some(ViewBounds::new(0.0, 0.0, window.frame.size.width, window.frame.size.height)),
  };
  let mut pre_scan_diagnostics = Vec::new();
  let mut pre_scan_known_limits = Vec::new();

  let mut capture = match session.window().capture(&window) {
    Ok(capture) => capture,
    Err(error) => {
      return Ok(PlaylistSidebarScan::empty_with_diagnostic(
        app_context,
        window_context,
        ViewRegionRecord::default(),
        ParserDiagnostic {
          code: "window_capture_failed".to_string(),
          message: error.to_string(),
          node_id: None,
        },
        "scan stopped before sidebar observation because the target window could not be captured",
      ));
    }
  };
  let full_window = RatioRect::new(0.0, 0.0, 1.0, 1.0);
  let mut full_recognition = match session.vision().recognize_text_in_capture_with_options(&capture, full_window, inputs.ocr_options.clone())
  {
    Ok(recognition) => recognition_in_window_space(recognition, &capture),
    Err(error) => {
      return Ok(PlaylistSidebarScan::empty_with_diagnostic(
        app_context,
        window_context,
        ViewRegionRecord::default(),
        ParserDiagnostic {
          code: "full_window_ocr_failed".to_string(),
          message: error.to_string(),
          node_id: None,
        },
        "scan stopped before sidebar observation because full-window OCR failed",
      ));
    }
  };

  if let Some(diagnostic) = detect_blocking_modal(&full_recognition) {
    return Ok(PlaylistSidebarScan::empty_with_diagnostic(
      app_context,
      window_context,
      ViewRegionRecord::default(),
      diagnostic,
      "scan stopped before sidebar observation because a blocking modal was detected",
    ));
  }

  if inputs.sidebar_region.is_none() {
    if let Some(restore) = detect_default_screen_restore(&full_recognition, window_size) {
      if let Err(error) = click_default_screen_restore(&session, &window, restore.point) {
        return Ok(PlaylistSidebarScan::empty_with_diagnostic(
          app_context,
          window_context,
          ViewRegionRecord::default(),
          ParserDiagnostic {
            code: "default_screen_restore_failed".to_string(),
            message: format!("failed to restore NetEase default sidebar screen from {:?}: {error}", restore.reason),
            node_id: None,
          },
          "scan stopped before sidebar observation because the default screen restore click failed",
        ));
      }
      if inputs.scroll_settle_ms > 0 {
        std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
      }
      capture = match session.window().capture(&window) {
        Ok(capture) => capture,
        Err(error) => {
          return Ok(PlaylistSidebarScan::empty_with_diagnostic(
            app_context,
            window_context,
            ViewRegionRecord::default(),
            ParserDiagnostic {
              code: "window_capture_failed".to_string(),
              message: error.to_string(),
              node_id: None,
            },
            "scan stopped before sidebar observation because the target window could not be captured after default screen restore",
          ));
        }
      };
      full_recognition = match session.vision().recognize_text_in_capture_with_options(&capture, full_window, inputs.ocr_options.clone()) {
        Ok(recognition) => recognition_in_window_space(recognition, &capture),
        Err(error) => {
          return Ok(PlaylistSidebarScan::empty_with_diagnostic(
            app_context,
            window_context,
            ViewRegionRecord::default(),
            ParserDiagnostic {
              code: "full_window_ocr_failed".to_string(),
              message: error.to_string(),
              node_id: None,
            },
            "scan stopped before sidebar observation because full-window OCR failed after default screen restore",
          ));
        }
      };
    }
  }

  if inputs.sidebar_region.is_none() {
    let broad_sidebar_bounds = broad_sidebar_probe_bounds(window_size);
    let broad_sidebar_ratio = bounds_to_ratio(broad_sidebar_bounds, &capture);
    let mut top_probe = LiveSidebarObserver {
      session,
      window: window.clone(),
      sidebar_bounds: broad_sidebar_bounds,
      sidebar_ratio: broad_sidebar_ratio,
      ocr_options: inputs.ocr_options.clone(),
      ls_query: None,
      artifact_dir: inputs.artifact_dir.clone(),
      pending_artifacts: Vec::new(),
      scroll_amount: inputs.scroll_amount,
      scroll_settle_ms: inputs.scroll_settle_ms,
      pending_scroll_delivery_path: None,
      previous_sidebar_crop: None,
      motion_policy: MotionDetectionPolicy::default(),
    };
    let top_seek = scroll_to_top_by_motion(&mut top_probe, top_seek_scroll_budget(inputs.max_scrolls));
    pre_scan_diagnostics.extend(top_seek.diagnostics);
    pre_scan_known_limits.extend(top_seek.known_limits);
    let LiveSidebarObserver {
      session: probe_session,
      ..
    } = top_probe;
    session = probe_session;

    capture = match session.window().capture(&window) {
      Ok(capture) => capture,
      Err(error) => {
        return Ok(PlaylistSidebarScan::empty_with_diagnostic(
          app_context,
          window_context,
          ViewRegionRecord::default(),
          ParserDiagnostic {
            code: "window_capture_failed".to_string(),
            message: error.to_string(),
            node_id: None,
          },
          "scan stopped before sidebar observation because the target window could not be captured after top seek",
        ));
      }
    };
    full_recognition = match session.vision().recognize_text_in_capture_with_options(&capture, full_window, inputs.ocr_options.clone()) {
      Ok(recognition) => recognition_in_window_space(recognition, &capture),
      Err(error) => {
        return Ok(PlaylistSidebarScan::empty_with_diagnostic(
          app_context,
          window_context,
          ViewRegionRecord::default(),
          ParserDiagnostic {
            code: "full_window_ocr_failed".to_string(),
            message: error.to_string(),
            node_id: None,
          },
          "scan stopped before sidebar observation because full-window OCR failed after top seek",
        ));
      }
    };
  }

  let sidebar_region = match detect_sidebar_region(inputs.sidebar_region, window_size, &full_recognition) {
    Ok(sidebar_region) => sidebar_region,
    Err(diagnostic) => {
      if inputs.sidebar_region.is_none() && diagnostic.code == "sidebar_region_not_found" {
        // NOTICE(netease-default-screen-restore): song-detail and similar
        // transient NetEase surfaces hide the left sidebar. `playlist ls`
        // cannot proceed from that state, so this fallback uses the app's
        // top-left restore affordance once before giving up on sidebar
        // detection. Broader surface classification is deferred until the
        // NetEase preflight contract is owner-approved.
        let restore = DefaultScreenRestore {
          reason: DefaultScreenRestoreReason::MissingSidebarRegion,
          point: song_detail_restore_point(window_size),
        };
        if let Err(error) = click_default_screen_restore(&session, &window, restore.point) {
          return Ok(PlaylistSidebarScan::empty_with_diagnostic(
            app_context,
            window_context,
            ViewRegionRecord::default(),
            ParserDiagnostic {
              code: "default_screen_restore_failed".to_string(),
              message: format!("failed to restore NetEase default sidebar screen from {:?}: {error}", restore.reason),
              node_id: None,
            },
            "scan stopped before sidebar observation because the default screen restore click failed",
          ));
        }
        if inputs.scroll_settle_ms > 0 {
          std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
        }
        capture = match session.window().capture(&window) {
          Ok(capture) => capture,
          Err(error) => {
            return Ok(PlaylistSidebarScan::empty_with_diagnostic(
              app_context,
              window_context,
              ViewRegionRecord::default(),
              ParserDiagnostic {
                code: "window_capture_failed".to_string(),
                message: error.to_string(),
                node_id: None,
              },
              "scan stopped before sidebar observation because the target window could not be captured after sidebar restore fallback",
            ));
          }
        };
        full_recognition = match session.vision().recognize_text_in_capture_with_options(&capture, full_window, inputs.ocr_options.clone()) {
          Ok(recognition) => recognition_in_window_space(recognition, &capture),
          Err(error) => {
            return Ok(PlaylistSidebarScan::empty_with_diagnostic(
              app_context,
              window_context,
              ViewRegionRecord::default(),
              ParserDiagnostic {
                code: "full_window_ocr_failed".to_string(),
                message: error.to_string(),
                node_id: None,
              },
              "scan stopped before sidebar observation because full-window OCR failed after sidebar restore fallback",
            ));
          }
        };
        match detect_sidebar_region(None, window_size, &full_recognition) {
          Ok(sidebar_region) => sidebar_region,
          Err(diagnostic) => {
            let fallback = fallback_playlist_sidebar_region(window_size);
            pre_scan_diagnostics.push(diagnostic);
            pre_scan_diagnostics.push(ParserDiagnostic {
              code: "sidebar_region_fallback_used".to_string(),
              message: "sidebar markers were not recognized after default screen restore; using conservative playlist sidebar bounds"
                .to_string(),
              node_id: None,
            });
            fallback
          }
        }
      } else {
        return Ok(PlaylistSidebarScan::empty_with_diagnostic(
          app_context,
          window_context,
          ViewRegionRecord::default(),
          diagnostic,
          "scan stopped before sidebar observation because the sidebar region could not be detected",
        ));
      }
    }
  };
  let sidebar_bounds = sidebar_region.bounds.unwrap_or_default();
  let sidebar_ratio = bounds_to_ratio(sidebar_bounds, &capture);
  let mut observer = LiveSidebarObserver {
    session,
    window: window.clone(),
    sidebar_bounds,
    sidebar_ratio,
    ocr_options: sidebar_ls_scan_ocr_options(&inputs.ocr_options, query),
    ls_query: query.map(str::to_string),
    artifact_dir: inputs.artifact_dir.clone(),
    pending_artifacts: Vec::new(),
    scroll_amount: inputs.scroll_amount,
    scroll_settle_ms: inputs.scroll_settle_ms,
    pending_scroll_delivery_path: None,
    previous_sidebar_crop: None,
    motion_policy: MotionDetectionPolicy::default(),
  };
  let options = ScanOptions {
    // NOTICE: NetEase playlist listing no longer has a page completion
    // model. This shared `auv-view::ScanOptions` field remains for other
    // scan loops, but this crate's collection policy intentionally ignores
    // it and stops at section landmarks or scroll boundaries.
    max_pages: 0,
    max_scrolls: inputs.max_scrolls,
  };
  let mut scan = match query {
    Some(query) => {
      scan_sidebar_with_observer_until_query(&mut observer, options, inputs.category, inputs.scroll_amount, inputs.scroll_settle_ms, query)
    }
    None => scan_sidebar_with_observer(&mut observer, options, inputs.category, inputs.scroll_amount, inputs.scroll_settle_ms),
  };
  scan.diagnostics.extend(pre_scan_diagnostics);
  scan.known_limits.extend(pre_scan_known_limits);
  scan.diagnostics.extend(observer.finish_artifacts());
  scan.app = app_context;
  scan.window = window_context;
  scan.sidebar_region = sidebar_region;
  scan.reconstruction.root.bounds = sidebar_bounds;

  Ok(scan)
}

#[cfg(target_os = "macos")]
struct LiveSidebarObserver {
  session: MacosDriverSession,
  window: auv_driver::Window,
  sidebar_bounds: ViewBounds,
  sidebar_ratio: RatioRect,
  ocr_options: TextRecognitionOptions,
  ls_query: Option<String>,
  artifact_dir: PathBuf,
  pending_artifacts: Vec<std::thread::JoinHandle<Result<(), String>>>,
  scroll_amount: f64,
  scroll_settle_ms: u64,
  pending_scroll_delivery_path: Option<String>,
  previous_sidebar_crop: Option<RgbaImage>,
  motion_policy: MotionDetectionPolicy,
}

#[cfg(target_os = "macos")]
impl LiveSidebarObserver {
  fn capture_observation(
    &mut self,
    observation_index: usize,
  ) -> Result<(RgbaImage, f64, TextRecognition, SidebarViewportObservation), ParserDiagnostic> {
    let capture = self.session.window().capture(&self.window).map_err(|error| ParserDiagnostic {
      code: "window_capture_failed".to_string(),
      message: error.to_string(),
      node_id: None,
    })?;
    let sidebar_recognition =
      self.session.vision().recognize_text_in_capture_with_options(&capture, self.sidebar_ratio, self.ocr_options.clone()).map_err(
        |error| ParserDiagnostic {
          code: "sidebar_ocr_failed".to_string(),
          message: error.to_string(),
          node_id: None,
        },
      )?;
    let sidebar_region_count = sidebar_recognition.regions.len();
    let numeric_query = self.ls_query.as_deref().is_some_and(crate::view_parsers::sidebar::parse::is_single_ascii_digit_query);
    let recognition = if numeric_query && sidebar_region_count == 0 {
      let full_window = RatioRect::new(0.0, 0.0, 1.0, 1.0);
      self.session.vision().recognize_text_in_capture_with_options(&capture, full_window, self.ocr_options.clone()).map_err(|error| {
        ParserDiagnostic {
          code: "sidebar_ocr_full_window_failed".to_string(),
          message: error.to_string(),
          node_id: None,
        }
      })?
    } else {
      sidebar_recognition
    };

    let window_recognition = recognition_in_window_space(recognition, &capture);
    let parse_bounds = crate::view_parsers::sidebar::target_probe::ls_parse_viewport_bounds_for_sidebar_ocr(
      self.sidebar_bounds,
      sidebar_region_count,
      numeric_query,
    );
    let mut observation = parse_sidebar_viewport(observation_index, parse_bounds, &window_recognition);
    if numeric_query && sidebar_region_count == 0 {
      observation.parser_notes.push(ParserDiagnostic {
        code: crate::view_parsers::sidebar::target_probe::LS_OCR_FULL_WINDOW_FALLBACK_NOTE.to_string(),
        message: "sidebar crop OCR returned zero regions; retried with full-window capture".to_string(),
        node_id: None,
      });
    }

    Ok((capture.image.clone(), capture.scale_factor, window_recognition, observation))
  }

  fn write_observation_artifacts(
    &mut self,
    observation_index: usize,
    image: RgbaImage,
    recognition: TextRecognition,
    observation: SidebarViewportObservation,
  ) -> Vec<String> {
    let base = format!("obs-{observation_index:04}");
    let screenshot = self.artifact_dir.join(format!("{base}-window.png"));
    let overlay = self.artifact_dir.join(format!("{base}-overlay.png"));
    let recognition_json = self.artifact_dir.join(format!("{base}-recognition.json"));
    let observation_json = self.artifact_dir.join(format!("{base}-observation.json"));
    let paths = vec![
      screenshot.clone(),
      overlay.clone(),
      recognition_json.clone(),
      observation_json.clone(),
    ];
    let artifact_dir = self.artifact_dir.clone();
    let sidebar_bounds = self.sidebar_bounds;
    self.pending_artifacts.push(std::thread::spawn(move || {
      std::fs::create_dir_all(&artifact_dir).map_err(|error| format!("failed to create {}: {error}", artifact_dir.display()))?;
      image.save(&screenshot).map_err(|error| format!("failed to save {}: {error}", screenshot.display()))?;

      let mut overlay_image = image.clone();
      draw_overlay(&mut overlay_image, sidebar_bounds, &observation);
      overlay_image.save(&overlay).map_err(|error| format!("failed to save {}: {error}", overlay.display()))?;

      let recognition_payload =
        serde_json::to_string_pretty(&recognition).map_err(|error| format!("failed to serialize recognition: {error}"))?;
      std::fs::write(&recognition_json, recognition_payload)
        .map_err(|error| format!("failed to write {}: {error}", recognition_json.display()))?;

      let observation_payload =
        serde_json::to_string_pretty(&observation).map_err(|error| format!("failed to serialize observation: {error}"))?;
      std::fs::write(&observation_json, observation_payload)
        .map_err(|error| format!("failed to write {}: {error}", observation_json.display()))?;

      Ok(())
    }));

    paths.into_iter().map(|path| path.display().to_string()).collect()
  }

  fn finish_artifacts(self) -> Vec<ParserDiagnostic> {
    self
      .pending_artifacts
      .into_iter()
      .filter_map(|handle| match handle.join() {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(ParserDiagnostic {
          code: "artifact_write_failed".to_string(),
          message: error,
          node_id: None,
        }),
        Err(_) => Some(ParserDiagnostic {
          code: "artifact_write_panicked".to_string(),
          message: "background artifact writer panicked".to_string(),
          node_id: None,
        }),
      })
      .collect()
  }
}

#[cfg(target_os = "macos")]
impl ViewObserver for LiveSidebarObserver {
  type Observation = SidebarViewportObservation;

  fn observe(&mut self, observation_index: usize) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    let (image, scale_factor, window_recognition, mut observation) = self.capture_observation(observation_index)?;
    // NOTICE(netease-scroll-ax-window-targeting): corroboration currently asks
    // macOS for the app's focused/first AX window because the typed AX capture
    // API does not yet accept a concrete native window ref. Re-open this only
    // if NetEase starts surfacing multiple competing windows during playlist
    // scans.
    observation.ax_scrollbar_boundary = self.capture_ax_scrollbar_boundary();
    let sidebar_crop = crop_image(&image, self.sidebar_bounds, scale_factor);
    let incoming_scroll_delivery_path = self.pending_scroll_delivery_path.take();
    observation.scroll_motion = incoming_scroll_delivery_path
      .as_ref()
      .and(self.previous_sidebar_crop.as_ref())
      .map(|previous| self.motion_policy.compare(previous, &sidebar_crop));
    self.previous_sidebar_crop = Some(sidebar_crop);
    observation.incoming_scroll_delivery_path = incoming_scroll_delivery_path;
    observation.source_artifacts = self.write_observation_artifacts(observation_index, image, window_recognition, observation.clone());

    Ok(observation)
  }

  fn observe_probe(&mut self) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    let (_, _, _, observation) = self.capture_observation(0)?;
    Ok(observation)
  }

  fn scroll_up(&mut self) -> Result<(), ParserDiagnostic> {
    self.scroll_by(self.scroll_amount)
  }

  fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
    self.scroll_by(-self.scroll_amount)
  }
}

#[cfg(target_os = "macos")]
impl SidebarScanObserver for LiveSidebarObserver {
  fn reset_collection_phase(&mut self) {
    // NOTICE(netease-scroll-phase-state): top-seek rewind and collection reuse
    // the same observer instance. Clear transient scroll/crop state so the
    // first collected observation does not inherit rewind-phase motion
    // metadata.
    self.pending_scroll_delivery_path = None;
    self.previous_sidebar_crop = None;
  }

  fn scroll_seek_batch_size(&self) -> usize {
    LIVE_FAST_SEEK_BATCH_SCROLLS
  }

  fn scroll_seek_up(&mut self) -> Result<(), ParserDiagnostic> {
    self.scroll_by_with_settle(self.scroll_amount * LIVE_TOP_SEEK_SCROLL_DELTA_MULTIPLIER, std::time::Duration::ZERO)
  }

  fn observe_scroll_seek(&mut self, observation_index: usize) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    std::thread::sleep(std::time::Duration::from_millis(LIVE_FAST_SEEK_SAMPLE_INTERVAL_MS));
    let capture = self.session.window().capture(&self.window).map_err(|error| ParserDiagnostic {
      code: "window_capture_failed".to_string(),
      message: error.to_string(),
      node_id: None,
    })?;
    let mut observation = empty_scroll_seek_observation(observation_index, self.sidebar_bounds);
    let image = capture.image.clone();
    let scale_factor = capture.scale_factor;
    let sidebar_crop = crop_image(&image, self.sidebar_bounds, scale_factor);
    let incoming_scroll_delivery_path = self.pending_scroll_delivery_path.take();
    observation.scroll_motion = incoming_scroll_delivery_path
      .as_ref()
      .and(self.previous_sidebar_crop.as_ref())
      .map(|previous| self.motion_policy.compare(previous, &sidebar_crop));
    self.previous_sidebar_crop = Some(sidebar_crop);
    observation.incoming_scroll_delivery_path = incoming_scroll_delivery_path;
    Ok(observation)
  }

  fn scroll_down_for_query_recovery(&mut self) -> Result<(), ParserDiagnostic> {
    self.scroll_by_with_policy(
      -self.scroll_amount,
      std::time::Duration::from_millis(self.scroll_settle_ms),
      InputPolicy::ForegroundPreferred,
    )
  }
}

#[cfg(target_os = "macos")]
impl LiveSidebarObserver {
  fn capture_ax_scrollbar_boundary(&self) -> Option<SidebarScrollbarBoundary> {
    let app = self.window.app_bundle_id.as_deref().filter(|bundle_id| !bundle_id.trim().is_empty()).unwrap_or(DEFAULT_APP_ID);
    let snapshot = capture_ax_tree_snapshot(app, 8, 64).ok()?;
    sidebar_ax_scrollbar_boundary(&snapshot.snapshot.nodes, &self.window, self.sidebar_bounds)
  }

  fn scroll_anchor(&self) -> auv_driver::Point {
    crate::view_parsers::sidebar::sidebar_scroll_anchor(self.sidebar_bounds).0
  }

  fn scroll_by(&mut self, vertical_delta: f64) -> Result<(), ParserDiagnostic> {
    self.scroll_by_with_settle(vertical_delta, std::time::Duration::from_millis(self.scroll_settle_ms))
  }

  fn scroll_by_with_settle(&mut self, vertical_delta: f64, settle: std::time::Duration) -> Result<(), ParserDiagnostic> {
    self.scroll_by_with_policy(vertical_delta, settle, InputPolicy::BackgroundPreferred)
  }

  fn scroll_by_with_policy(
    &mut self,
    vertical_delta: f64,
    settle: std::time::Duration,
    policy: InputPolicy,
  ) -> Result<(), ParserDiagnostic> {
    let anchor = self.scroll_anchor();
    let result = self
      .session
      .window()
      .scroll(
        &self.window,
        WindowPoint::new(anchor.x, anchor.y),
        Scroll::new(0.0, vertical_delta),
        ScrollOptions {
          policy,
          settle,
          ..ScrollOptions::default()
        },
      )
      .map_err(|error| ParserDiagnostic {
        code: "sidebar_scroll_failed".to_string(),
        message: error.to_string(),
        node_id: None,
      })?;
    self.pending_scroll_delivery_path = Some(delivery_path_label(result.selected_path).to_string());
    Ok(())
  }
}

#[cfg(test)]
mod ls_scan_ocr_options_tests {
  use super::*;

  #[test]
  fn sidebar_ls_scan_ocr_options_merges_query_into_custom_words() {
    let base = TextRecognitionOptions::default().with_custom_words(["绚香"]);
    let options = sidebar_ls_scan_ocr_options(&base, Some("3"));

    assert_eq!(options.custom_words, vec!["绚香".to_string(), "3".to_string()]);
  }

  #[test]
  fn sidebar_ls_scan_ocr_options_leaves_base_untouched_without_query() {
    let base = TextRecognitionOptions::default().with_custom_words(["绚香"]);
    let options = sidebar_ls_scan_ocr_options(&base, None);

    assert_eq!(options, base);
  }

  #[test]
  fn sidebar_ls_scan_ocr_options_sets_default_languages_for_single_digit_query() {
    let base = TextRecognitionOptions::default();
    let options = sidebar_ls_scan_ocr_options(&base, Some("3"));

    assert_eq!(options.recognition_languages, Some(vec!["zh-Hans".to_string(), "en-US".to_string()]));
  }

  #[test]
  fn sidebar_ls_scan_ocr_options_leaves_languages_for_non_numeric_query() {
    let base = TextRecognitionOptions::default();
    let options = sidebar_ls_scan_ocr_options(&base, Some("My Playlist"));

    assert_eq!(options.recognition_languages, None);
  }

  #[test]
  fn sidebar_ls_scan_ocr_options_preserves_caller_recognition_languages() {
    let base = TextRecognitionOptions::default().with_recognition_languages(["ja-JP"]);
    let options = sidebar_ls_scan_ocr_options(&base, Some("3"));

    assert_eq!(options.recognition_languages, Some(vec!["ja-JP".to_string()]));
  }
}
