use crate::*;

#[cfg(target_os = "macos")]
use crate::run_artifacts::{DailyRecommendedInputDelivered, DailyRecommendedPlayAllChecked};
#[cfg(target_os = "macos")]
use auv_driver::{InputActionResult, InputDeliveryPath};

#[cfg(target_os = "macos")]
#[derive(serde::Serialize)]
struct DailyRecommendedIconVerificationArtifact<'a> {
  verification: &'a DailyRecommendedVerification,
  window_scale_factor: f64,
  search_region_pixels: PixelRegion,
}

#[cfg(target_os = "macos")]
#[derive(serde::Serialize)]
struct PixelRegion {
  x: i64,
  y: i64,
  width: i64,
  height: i64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
enum DailyRecommendedClick {
  SelectSidebarRecommend,
  OpenDailyRecommendedCard,
  OpenDailyRecommendedTitleForegroundRetry,
  PlayAll,
  PlayAllForegroundRetry,
}

#[cfg(target_os = "macos")]
impl DailyRecommendedClick {
  fn action_id(self) -> &'static str {
    match self {
      Self::SelectSidebarRecommend => "select-sidebar-recommend",
      Self::OpenDailyRecommendedCard => "open-daily-recommended-card-body",
      Self::OpenDailyRecommendedTitleForegroundRetry => "open-daily-recommended-title-foreground-retry",
      Self::PlayAll => "click-play-all",
      Self::PlayAllForegroundRetry => "click-play-all-foreground-retry",
    }
  }

  fn capture_purpose(self) -> &'static str {
    match self {
      Self::SelectSidebarRecommend => "auv.netease.daily_recommended.select_sidebar_capture",
      Self::OpenDailyRecommendedCard => "auv.netease.daily_recommended.open_card_capture",
      Self::OpenDailyRecommendedTitleForegroundRetry => "auv.netease.daily_recommended.open_title_retry_capture",
      Self::PlayAll => "auv.netease.daily_recommended.play_all_capture",
      Self::PlayAllForegroundRetry => "auv.netease.daily_recommended.play_all_retry_capture",
    }
  }

  fn delivered(self, label: String, bounds: ViewBounds, delivery: InputActionResult) -> DailyRecommendedInputDelivered {
    match self {
      Self::SelectSidebarRecommend => DailyRecommendedInputDelivered::SelectSidebarRecommend {
        label,
        bounds,
        delivery,
      },
      Self::OpenDailyRecommendedCard => DailyRecommendedInputDelivered::OpenDailyRecommendedCard {
        label,
        bounds,
        delivery,
      },
      Self::OpenDailyRecommendedTitleForegroundRetry => DailyRecommendedInputDelivered::OpenDailyRecommendedTitleForegroundRetry {
        label,
        bounds,
        delivery,
      },
      Self::PlayAll => DailyRecommendedInputDelivered::PlayAll {
        label,
        bounds,
        delivery,
      },
      Self::PlayAllForegroundRetry => DailyRecommendedInputDelivered::PlayAllForegroundRetry {
        label,
        bounds,
        delivery,
      },
    }
  }
}

#[cfg(not(target_os = "macos"))]
pub fn run_daily_recommended_play(_inputs: &DailyRecommendedPlayInputs) -> Result<DailyRecommendedPlayResult, String> {
  Err("live NetEase daily recommended play is only supported on macOS".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn run_daily_recommended_songs_scan(_inputs: &SongListInputs) -> Result<SongListScanResult, String> {
  Err("live NetEase daily recommended song scan is only supported on macOS".to_string())
}

#[cfg(target_os = "macos")]
pub fn run_daily_recommended_songs_scan(inputs: &SongListInputs) -> Result<SongListScanResult, String> {
  let daily_inputs = DailyRecommendedPlayInputs {
    app_id: inputs.app_id.clone(),
    max_top_scrolls: LIVE_TOP_SEEK_MAX_SCROLL_INPUTS,
    top_scroll_amount: inputs.scroll_amount,
    settle_ms: inputs.scroll_settle_ms,
    play_icon_template: None,
    play_icon_threshold: 0.72,
    ocr_options: inputs.ocr_options.clone(),
  };
  let session = auv_driver::open_local().map_err(|error| format!("failed to open macOS driver: {error}"))?;
  let app = App::bundle(inputs.app_id.clone());
  let window =
    session.window().resolve(Window::main_visible().owned_by(app)).map_err(|error| format!("failed to resolve NetEase window: {error}"))?;

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

  let mut run = DailyRecommendedRun {
    session,
    window,
    inputs: &daily_inputs,
    diagnostics: Vec::new(),
    known_limits: Vec::new(),
  };
  run.scroll_sidebar_to_top();
  run.click_text(DailyRecommendedClick::SelectSidebarRecommend, "推荐", |bounds, size| bounds.x < size.width * 0.28)?;
  run.open_daily_recommended()?;

  let region_bounds = daily_song_list_bounds(Size::new(run.window.frame.size.width, run.window.frame.size.height));
  let song_list_region = ViewRegionRecord {
    id: None,
    name: Some("daily_recommended_song_list".to_string()),
    bounds: Some(region_bounds),
    coordinate_space: Some("window".to_string()),
  };
  let mut scanner = SongListScanner::new(run, inputs, region_bounds);
  scanner.seek_boundary(ScrollDirection::Up)?;
  scanner.scan_down()?;
  Ok(scanner.finish(app_context, window_context, song_list_region, "daily-recommended"))
}

#[cfg(target_os = "macos")]
pub fn run_daily_recommended_play(inputs: &DailyRecommendedPlayInputs) -> Result<DailyRecommendedPlayResult, String> {
  let session = auv_driver::open_local().map_err(|error| format!("failed to open macOS driver: {error}"))?;
  let app = App::bundle(inputs.app_id.clone());
  let window =
    session.window().resolve(Window::main_visible().owned_by(app)).map_err(|error| format!("failed to resolve NetEase window: {error}"))?;

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

  let mut run = DailyRecommendedRun {
    session,
    window,
    inputs,
    diagnostics: Vec::new(),
    known_limits: Vec::new(),
  };

  run.scroll_sidebar_to_top();
  run.click_text(DailyRecommendedClick::SelectSidebarRecommend, "推荐", |bounds, size| bounds.x < size.width * 0.28)?;
  run.open_daily_recommended()?;
  run.click_text(DailyRecommendedClick::PlayAll, "播放全部", |bounds, _| bounds.y > 0.0)?;
  let mut verification = run.verify_play_icon()?;
  if !verification.passed() {
    run.known_limits.push("window-targeted Play All click did not verify playback; retried with foreground click".to_string());
    run.click_text_foreground(DailyRecommendedClick::PlayAllForegroundRetry, "播放全部", |bounds, _| bounds.y > 0.0)?;
    verification = run.verify_play_icon()?;
  }

  Ok(DailyRecommendedPlayResult {
    command: "playlist.play.daily-recommended".to_string(),
    app: app_context,
    window: window_context,
    verification,
    diagnostics: run.diagnostics,
    known_limits: run.known_limits,
  })
}

#[cfg(target_os = "macos")]
struct DailyRecommendedRun<'a> {
  session: LocalDriverSession,
  window: auv_driver::Window,
  inputs: &'a DailyRecommendedPlayInputs,
  diagnostics: Vec<ParserDiagnostic>,
  known_limits: Vec<String>,
}

fn daily_song_list_bounds(window_size: Size) -> ViewBounds {
  let x = window_size.width * 0.30;
  let y = window_size.height * 0.23;
  let bottom = playlist_sidebar_bottom(window_size);
  ViewBounds::new(x, y, window_size.width - x - 24.0, (bottom - y).max(1.0))
}

fn parse_song_list_rows(observation_index: usize, bounds: ViewBounds, recognition: &TextRecognition) -> Vec<SongListItem> {
  let mut index_regions = recognition
    .regions
    .iter()
    .filter(|region| {
      let text = region.text.trim();
      let center_y = region.bounds.origin.y + region.bounds.size.height * 0.5;
      text.len() <= 3
        && text.chars().all(|ch| ch.is_ascii_digit())
        && viewport_contains_center(bounds, recognized_bounds(&region.bounds))
        && center_y > bounds.y + 36.0
    })
    .collect::<Vec<_>>();
  index_regions.sort_by(|left, right| left.bounds.origin.y.partial_cmp(&right.bounds.origin.y).unwrap_or(std::cmp::Ordering::Equal));

  let mut rows = Vec::new();
  for index_region in index_regions {
    let index = index_region.text.trim().parse::<u32>().ok();
    let row_center_y = index_region.bounds.origin.y + index_region.bounds.size.height * 0.5;
    let row_top = row_center_y - 32.0;
    let row_bottom = row_center_y + 32.0;
    let mut parts = recognition
      .regions
      .iter()
      .filter(|region| {
        let text = region.text.trim();
        let center_y = region.bounds.origin.y + region.bounds.size.height * 0.5;
        !text.is_empty()
          && center_y >= row_top
          && center_y <= row_bottom
          && viewport_contains_center(bounds, recognized_bounds(&region.bounds))
          && region.bounds.origin.x > index_region.bounds.origin.x + 16.0
      })
      .collect::<Vec<_>>();
    parts.sort_by(|left, right| left.bounds.origin.x.partial_cmp(&right.bounds.origin.x).unwrap_or(std::cmp::Ordering::Equal));
    let title = parts
      .iter()
      .find(|region| {
        let x = region.bounds.origin.x;
        x > bounds.x + 60.0 && x < bounds.x + bounds.width * 0.52
      })
      .map(|region| region.text.trim().to_string());
    let row_text = parts.iter().map(|region| region.text.trim()).filter(|text| !text.is_empty()).collect::<Vec<_>>().join(" | ");
    let Some(title) = title.filter(|title| !title.is_empty()) else {
      continue;
    };
    let row_bounds = ViewBounds::new(bounds.x, row_top, bounds.width, row_bottom - row_top);
    rows.push(SongListItem {
      id: format!("daily.song.obs{observation_index}.{}", index.map(|value| value.to_string()).unwrap_or_else(|| slug(&title))),
      index,
      title,
      row_text,
      bounds: Some(row_bounds),
    });
  }
  rows
}

fn song_item_key(row: &SongListItem) -> String {
  row.index.map(|index| format!("index:{index}")).unwrap_or_else(|| format!("text:{}", normalize_identity(&row.row_text)))
}

fn recognized_bounds(bounds: &auv_driver::Rect) -> ViewBounds {
  ViewBounds::new(bounds.origin.x, bounds.origin.y, bounds.size.width, bounds.size.height)
}

#[cfg(target_os = "macos")]
struct SongListScanner<'a> {
  run: DailyRecommendedRun<'a>,
  inputs: &'a SongListInputs,
  region_bounds: ViewBounds,
  observations: Vec<SongListObservation>,
  items: Vec<SongListItem>,
  seen_items: HashSet<String>,
  boundary: ScrollBoundarySummary,
  pending_scroll_delivery_path: Option<String>,
  previous_crop: Option<RgbaImage>,
  motion_policy: MotionDetectionPolicy,
}

#[cfg(target_os = "macos")]
impl<'a> SongListScanner<'a> {
  fn new(run: DailyRecommendedRun<'a>, inputs: &'a SongListInputs, region_bounds: ViewBounds) -> Self {
    Self {
      run,
      inputs,
      region_bounds,
      observations: Vec::new(),
      items: Vec::new(),
      seen_items: HashSet::new(),
      boundary: ScrollBoundarySummary::default(),
      pending_scroll_delivery_path: None,
      previous_crop: None,
      motion_policy: MotionDetectionPolicy::default(),
    }
  }

  fn finish(self, app: ScanAppContext, window: ScanWindowContext, song_list_region: ViewRegionRecord, target: &str) -> SongListScanResult {
    SongListScanResult {
      command: "playlist.songs.ls".to_string(),
      target: target.to_string(),
      app,
      window,
      song_list_region,
      items: self.items,
      observations: self.observations,
      boundary: self.boundary,
      diagnostics: self.run.diagnostics,
      known_limits: self.run.known_limits,
    }
  }

  fn seek_boundary(&mut self, direction: ScrollDirection) -> Result<(), String> {
    self.pending_scroll_delivery_path = None;
    self.previous_crop = Some(self.capture_region_crop()?);
    let delta = match direction {
      ScrollDirection::Up => self.inputs.scroll_amount * LIVE_TOP_SEEK_SCROLL_DELTA_MULTIPLIER,
      ScrollDirection::Down => -self.inputs.scroll_amount * LIVE_TOP_SEEK_SCROLL_DELTA_MULTIPLIER,
    };
    let mut no_motion_confirmations = 0usize;
    for _ in 0..LIVE_TOP_SEEK_MAX_SCROLL_INPUTS {
      self.scroll_region(delta, std::time::Duration::ZERO)?;
      std::thread::sleep(std::time::Duration::from_millis(LIVE_FAST_SEEK_SAMPLE_INTERVAL_MS));
      let crop = self.capture_region_crop()?;
      if let Some(previous) = self.previous_crop.as_ref() {
        let motion = self.motion_policy.compare(previous, &crop);
        if motion.no_motion && successful_scroll_delivery_path(self.pending_scroll_delivery_path.as_deref()) {
          no_motion_confirmations += 1;
          if no_motion_confirmations >= 2 {
            match direction {
              ScrollDirection::Up => self.boundary.top = BoundaryConfidence::Likely,
              ScrollDirection::Down => self.boundary.bottom = BoundaryConfidence::Likely,
            }
            self.pending_scroll_delivery_path = None;
            self.previous_crop = Some(crop);
            return Ok(());
          }
        } else {
          no_motion_confirmations = 0;
        }
      }
      self.previous_crop = Some(crop);
      self.pending_scroll_delivery_path = None;
    }
    self.run.known_limits.push(format!(
      "song list {:?} seek stopped after max_scrolls={} without boundary confirmation",
      direction, LIVE_TOP_SEEK_MAX_SCROLL_INPUTS
    ));
    Ok(())
  }

  fn scan_down(&mut self) -> Result<(), String> {
    self.pending_scroll_delivery_path = None;
    self.previous_crop = None;
    let mut consecutive_no_new = 0usize;
    let mut consecutive_no_motion = 0usize;
    for _ in 0..=self.inputs.max_scrolls {
      let observation_index = self.observations.len();
      let observation = self.observe_page(observation_index)?;
      let introduced_new = self.record_items(&observation.rows);
      if introduced_new {
        consecutive_no_new = 0;
      } else if observation.incoming_scroll_delivery_path.is_some() {
        consecutive_no_new += 1;
      }
      if observation.scroll_motion.as_ref().is_some_and(|motion| motion.no_motion) {
        consecutive_no_motion += 1;
      } else {
        consecutive_no_motion = 0;
      }
      self.observations.push(observation);

      if consecutive_no_new >= 2 || consecutive_no_motion >= 2 {
        self.seek_boundary(ScrollDirection::Down)?;
        let final_observation = self.observe_page(self.observations.len())?;
        self.record_items(&final_observation.rows);
        self.observations.push(final_observation);
        return Ok(());
      }

      if self.observations.len() > self.inputs.max_scrolls {
        self.run.known_limits.push(format!("song list scan stopped after max_scrolls={}", self.inputs.max_scrolls));
        return Ok(());
      }
      self.scroll_region(-self.inputs.scroll_amount, std::time::Duration::from_millis(self.inputs.scroll_settle_ms))?;
    }
    Ok(())
  }

  fn record_items(&mut self, rows: &[SongListItem]) -> bool {
    let mut introduced_new = false;
    for row in rows {
      let key = song_item_key(row);
      if self.seen_items.insert(key) {
        self.items.push(row.clone());
        introduced_new = true;
      }
    }
    introduced_new
  }

  fn observe_page(&mut self, observation_index: usize) -> Result<SongListObservation, String> {
    auv_tracing::in_span!("auv.netease.song_list.observe", || {
      let capture = self.run.session.window().capture(&self.run.window).map_err(|error| format!("song list capture failed: {error}"))?;
      crate::run_artifacts::emit_png("auv.netease.song_list.capture", &capture.image);
      let recognition = self
        .run
        .session
        .vision()
        .recognize_text_in_capture_with_options(&capture, bounds_to_ratio(self.region_bounds, &capture), self.inputs.ocr_options.clone())
        .map_err(|error| format!("song list OCR failed: {error}"))?;
      let recognition = recognition_in_window_space(recognition, &capture);
      let crop = crop_image(&capture.image, self.region_bounds, capture.scale_factor);
      let incoming_scroll_delivery_path = self.pending_scroll_delivery_path.take();
      let scroll_motion =
        incoming_scroll_delivery_path.as_ref().and(self.previous_crop.as_ref()).map(|previous| self.motion_policy.compare(previous, &crop));
      self.previous_crop = Some(crop);
      Ok(SongListObservation {
        observation_index,
        incoming_scroll_delivery_path,
        scroll_motion,
        rows: parse_song_list_rows(observation_index, self.region_bounds, &recognition),
      })
    })
  }

  fn capture_region_crop(&mut self) -> Result<RgbaImage, String> {
    let capture = self.run.session.window().capture(&self.run.window).map_err(|error| format!("song list seek capture failed: {error}"))?;
    Ok(crop_image(&capture.image, self.region_bounds, capture.scale_factor))
  }

  fn scroll_region(&mut self, vertical_delta: f64, settle: std::time::Duration) -> Result<(), String> {
    let point =
      WindowPoint::new(self.region_bounds.x + self.region_bounds.width * 0.5, self.region_bounds.y + self.region_bounds.height * 0.65);
    let result = self
      .run
      .session
      .window()
      .scroll(
        &self.run.window,
        point,
        Scroll::new(0.0, vertical_delta),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle,
          ..ScrollOptions::default()
        },
      )
      .map_err(|error| format!("song list scroll failed: {error}"))?;
    self.pending_scroll_delivery_path = Some(delivery_path_label(result.selected_path).to_string());
    Ok(())
  }
}

#[cfg(target_os = "macos")]
impl DailyRecommendedRun<'_> {
  fn scroll_sidebar_to_top(&mut self) {
    let window_size = Size::new(self.window.frame.size.width, self.window.frame.size.height);
    let bounds = broad_sidebar_probe_bounds(window_size);
    let anchor = WindowPoint::new(bounds.x + bounds.width * 0.5, bounds.y + bounds.height * 0.45);
    for index in 0..self.inputs.max_top_scrolls {
      match self.session.window().scroll(
        &self.window,
        anchor,
        Scroll::new(0.0, self.inputs.top_scroll_amount),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: std::time::Duration::from_millis(self.inputs.settle_ms),
          ..ScrollOptions::default()
        },
      ) {
        Ok(delivery) => auv_tracing::emit_event!(DailyRecommendedInputDelivered::SeekSidebarTop {
          attempt: index,
          bounds,
          delivery,
        }),
        Err(error) => {
          self.diagnostics.push(ParserDiagnostic {
            code: "daily_recommended_top_scroll_failed".to_string(),
            message: error.to_string(),
            node_id: None,
          });
          self.known_limits.push("top seek stopped early after a typed scroll failure".to_string());
          break;
        }
      }
    }
  }

  fn click_text(&mut self, action: DailyRecommendedClick, query: &str, guard: impl Fn(ViewBounds, Size) -> bool) -> Result<(), String> {
    let action_id = action.action_id();
    let capture = self.session.window().capture(&self.window).map_err(|error| format!("{action_id}: capture failed: {error}"))?;
    crate::run_artifacts::emit_png(action.capture_purpose(), &capture.image);
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(&capture, RatioRect::new(0.0, 0.0, 1.0, 1.0), self.inputs.ocr_options.clone())
      .map_err(|error| format!("{action_id}: OCR failed: {error}"))?;
    let recognition = recognition_in_window_space(recognition, &capture);
    let window_size = Size::new(self.window.frame.size.width, self.window.frame.size.height);
    let Some(target) = best_text_match(&recognition, query, window_size, guard) else {
      return Err(format!("{action_id}: text {query:?} was not found"));
    };
    let bounds = ViewBounds::new(target.bounds.origin.x, target.bounds.origin.y, target.bounds.size.width, target.bounds.size.height);
    let point = target.action_point();
    let result = self
      .session
      .window()
      .click(&self.window, WindowPoint::new(point.x, point.y), daily_recommended_window_click_options())
      .map_err(|error| format!("{action_id}: click failed: {error}"))?;
    if self.inputs.settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(self.inputs.settle_ms));
    }
    auv_tracing::emit_event!(action.delivered(target.text, bounds, result));
    Ok(())
  }

  fn click_text_foreground(
    &mut self,
    action: DailyRecommendedClick,
    query: &str,
    guard: impl Fn(ViewBounds, Size) -> bool,
  ) -> Result<(), String> {
    let action_id = action.action_id();
    let capture = self.session.window().capture(&self.window).map_err(|error| format!("{action_id}: capture failed: {error}"))?;
    crate::run_artifacts::emit_png(action.capture_purpose(), &capture.image);
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(&capture, RatioRect::new(0.0, 0.0, 1.0, 1.0), self.inputs.ocr_options.clone())
      .map_err(|error| format!("{action_id}: OCR failed: {error}"))?;
    let recognition = recognition_in_window_space(recognition, &capture);
    let window_size = Size::new(self.window.frame.size.width, self.window.frame.size.height);
    let Some(target) = best_text_match(&recognition, query, window_size, guard) else {
      return Err(format!("{action_id}: text {query:?} was not found"));
    };
    let bounds = ViewBounds::new(target.bounds.origin.x, target.bounds.origin.y, target.bounds.size.width, target.bounds.size.height);
    let point = target.action_point();
    let screen_point = self
      .session
      .window()
      .to_screen_point(&self.window, WindowPoint::new(point.x, point.y))
      .map_err(|error| format!("{action_id}: screen point projection failed: {error}"))?;
    let lease = self
      .session
      .window()
      .prepare_for_input(
        &self.window,
        PrepareForInputOptions {
          activation: ActivationPolicy::Foreground {
            settle: std::time::Duration::from_millis(self.inputs.settle_ms),
          },
          preserve_frontmost: false,
          install_focus_guard: false,
          settle: std::time::Duration::from_millis(0),
        },
      )
      .map_err(|error| format!("{action_id}: foreground preparation failed: {error}"))?;
    let click_result = self.session.input().click_at(screen_point.point(), Click::Single);
    let restore_result = self.session.window().restore_input(lease);
    click_result.map_err(|error| format!("{action_id}: foreground click failed: {error}"))?;
    restore_result.map_err(|error| format!("{action_id}: foreground restore failed: {error}"))?;
    if self.inputs.settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(self.inputs.settle_ms));
    }
    let delivery = InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents);
    auv_tracing::emit_event!(action.delivered(target.text, bounds, delivery));
    Ok(())
  }

  fn open_daily_recommended(&mut self) -> Result<(), String> {
    if self.play_all_is_visible(false)? {
      return Ok(());
    }

    self.click_daily_recommended_card_body()
  }

  fn click_daily_recommended_card_body(&mut self) -> Result<(), String> {
    let capture = self.session.window().capture(&self.window).map_err(|error| format!("daily recommended card capture failed: {error}"))?;
    crate::run_artifacts::emit_png(DailyRecommendedClick::OpenDailyRecommendedCard.capture_purpose(), &capture.image);
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(&capture, RatioRect::new(0.0, 0.0, 1.0, 1.0), self.inputs.ocr_options.clone())
      .map_err(|error| format!("daily recommended card OCR failed: {error}"))?;
    let recognition = recognition_in_window_space(recognition, &capture);
    let window_size = Size::new(self.window.frame.size.width, self.window.frame.size.height);
    let Some(target) = best_text_match(&recognition, "每日推荐", window_size, |bounds, size| {
      bounds.x > size.width * 0.18 && bounds.y < size.height * 0.35
    }) else {
      return Err("daily recommended card title was not found on recommendation home".to_string());
    };
    let bounds = ViewBounds::new(target.bounds.origin.x, target.bounds.origin.y, target.bounds.size.width, target.bounds.size.height);
    let point = daily_recommended_card_click_point(bounds);
    let result = self
      .session
      .window()
      .click(&self.window, WindowPoint::new(point.x, point.y), daily_recommended_window_click_options())
      .map_err(|error| format!("daily recommended card body click failed: {error}"))?;
    if self.inputs.settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(self.inputs.settle_ms));
    }
    auv_tracing::emit_event!(DailyRecommendedClick::OpenDailyRecommendedCard.delivered(target.text, bounds, result));
    if self.play_all_is_visible(false)? {
      Ok(())
    } else {
      self.click_text_foreground(DailyRecommendedClick::OpenDailyRecommendedTitleForegroundRetry, "每日推荐", |bounds, size| {
        bounds.x > size.width * 0.18 && bounds.y < size.height * 0.35
      })?;
      if self.play_all_is_visible(true)? {
        Ok(())
      } else {
        Err("daily recommended card body click did not reveal 播放全部".to_string())
      }
    }
  }

  fn play_all_is_visible(&mut self, record_absent_diagnostic: bool) -> Result<bool, String> {
    auv_tracing::in_span!("auv.netease.daily_recommended.play_all_visibility", || {
      let capture =
        self.session.window().capture(&self.window).map_err(|error| format!("daily recommended fallback capture failed: {error}"))?;
      crate::run_artifacts::emit_png("auv.netease.daily_recommended.play_all_visibility_capture", &capture.image);
      let recognition = self
        .session
        .vision()
        .recognize_text_in_capture_with_options(&capture, RatioRect::new(0.0, 0.0, 1.0, 1.0), self.inputs.ocr_options.clone())
        .map_err(|error| format!("daily recommended fallback OCR failed: {error}"))?;
      let recognition = recognition_in_window_space(recognition, &capture);
      let window_size = Size::new(self.window.frame.size.width, self.window.frame.size.height);
      let visible = best_text_match(&recognition, "播放全部", window_size, |bounds, size| bounds.x > size.width * 0.18).is_some();
      if visible {
        self.known_limits.push("Play All was visible while opening Daily Recommended".to_string());
      } else if record_absent_diagnostic {
        self.diagnostics.push(ParserDiagnostic {
          code: "daily_recommended_fallback_not_visible".to_string(),
          message: "neither 每日推荐 nor 播放全部 could be detected".to_string(),
          node_id: None,
        });
      }
      auv_tracing::emit_event!(DailyRecommendedPlayAllChecked { visible });
      Ok(visible)
    })
  }

  fn verify_play_icon(&mut self) -> Result<DailyRecommendedVerification, String> {
    let Some(template) = self.inputs.play_icon_template.as_ref() else {
      return self.verify_bottom_playback_control();
    };
    if !template.exists() {
      return Err(format!("icon template not found: {}", template.display()));
    }

    auv_tracing::in_span!("auv.netease.daily_recommended.icon_verification", || {
      let capture = self.session.window().capture(&self.window).map_err(|error| format!("post-click icon capture failed: {error}"))?;
      crate::run_artifacts::emit_png("auv.netease.daily_recommended.icon_verification_capture", &capture.image);
      let scale = if capture.scale_factor.is_finite() && capture.scale_factor > 0.0 {
        capture.scale_factor
      } else {
        1.0
      };
      let region = auv_driver_macos::types::ObservedRect {
        x: ((capture.image.width() as f64) * 0.30).round() as i64,
        y: ((capture.image.height() as f64) * 0.72).round() as i64,
        width: ((capture.image.width() as f64) * 0.40).round() as i64,
        height: ((capture.image.height() as f64) * 0.24).round() as i64,
      };
      let output =
        auv_driver_macos::support::template_match::match_template(&capture.image, template, Some(&region), self.inputs.play_icon_threshold)?;
      let best_score = output.matches.first().map(|item| item.score);
      let match_count = output.matches.len();
      let evidence = DailyRecommendedVerificationEvidence::IconMatch {
        threshold: self.inputs.play_icon_threshold,
        match_count,
        best_score,
      };
      let verification = if match_count > 0 {
        DailyRecommendedVerification::Passed { evidence }
      } else {
        DailyRecommendedVerification::Failed { evidence }
      };
      crate::run_artifacts::emit_json(
        "auv.netease.daily_recommended.icon_verification",
        &DailyRecommendedIconVerificationArtifact {
          verification: &verification,
          window_scale_factor: scale,
          search_region_pixels: PixelRegion {
            x: region.x,
            y: region.y,
            width: region.width,
            height: region.height,
          },
        },
      );
      Ok(verification)
    })
  }

  fn verify_bottom_playback_control(&mut self) -> Result<DailyRecommendedVerification, String> {
    auv_tracing::in_span!("auv.netease.daily_recommended.playback_verification", || {
      let capture =
        self.session.window().capture(&self.window).map_err(|error| format!("post-click playback-state capture failed: {error}"))?;
      crate::run_artifacts::emit_png("auv.netease.daily_recommended.playback_verification_capture", &capture.image);
      let control_state = classify_bottom_playback_control_state(&capture.image);
      let bottom_text = self
        .session
        .vision()
        .recognize_text_in_capture_with_options(&capture, RatioRect::new(0.0, 0.88, 0.46, 0.12), self.inputs.ocr_options.clone())
        .ok()
        .map(|recognition| recognition.text.trim().to_string())
        .filter(|text| !text.is_empty());
      let evidence = DailyRecommendedVerificationEvidence::BottomPlaybackControl {
        control_state,
        observed_bottom_text: bottom_text,
      };
      let verification = if control_state == PlaybackControlState::PauseVisible {
        DailyRecommendedVerification::Passed { evidence }
      } else {
        DailyRecommendedVerification::Failed { evidence }
      };
      crate::run_artifacts::emit_json("auv.netease.daily_recommended.playback_verification", &verification);
      Ok(verification)
    })
  }
}

#[cfg(target_os = "macos")]
pub(crate) fn best_text_match(
  recognition: &TextRecognition,
  query: &str,
  window_size: Size,
  guard: impl Fn(ViewBounds, Size) -> bool,
) -> Option<auv_driver::vision::RecognizedText> {
  recognition
    .regions
    .iter()
    .filter(|region| normalize_identity(&region.text).contains(&normalize_identity(query)))
    .filter(|region| {
      guard(
        ViewBounds::new(region.bounds.origin.x, region.bounds.origin.y, region.bounds.size.width, region.bounds.size.height),
        window_size,
      )
    })
    .min_by(|left, right| left.bounds.origin.y.partial_cmp(&right.bounds.origin.y).unwrap_or(std::cmp::Ordering::Equal))
    .cloned()
}

pub(crate) fn daily_recommended_card_click_point(title_bounds: ViewBounds) -> auv_driver::Point {
  // NOTICE(netease-daily-card-hit-target): live NetEase testing showed the
  // OCR title text and bottom title strip on the recommendation card may not
  // activate navigation reliably. Target the cover/body area derived from the
  // title anchor until an owner-approved card geometry detector replaces this
  // local product policy.
  if title_bounds.y < 180.0 {
    auv_driver::Point::new(title_bounds.x + 55.0, title_bounds.y + 80.0)
  } else {
    auv_driver::Point::new(title_bounds.x + 70.0, title_bounds.y - 95.0)
  }
}

fn daily_recommended_window_click_options() -> auv_driver::ClickOptions {
  auv_driver::ClickOptions {
    policy: auv_driver::InputPolicy::BackgroundPreferred,
    click: auv_driver::Click::Single,
    window_strategy: auv_driver::WindowClickStrategy::ChromiumCompatible,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn daily_recommended_card_click_point_targets_card_body_from_title_bounds() {
    let bounds = ViewBounds::new(430.0, 102.0, 72.0, 20.0);

    let point = daily_recommended_card_click_point(bounds);

    assert_eq!(point, auv_driver::Point::new(485.0, 182.0));
  }

  #[test]
  fn daily_recommended_card_click_point_handles_bottom_title_bounds() {
    let bounds = ViewBounds::new(430.0, 278.0, 145.0, 36.0);

    let point = daily_recommended_card_click_point(bounds);

    assert_eq!(point, auv_driver::Point::new(500.0, 183.0));
  }

  #[test]
  fn daily_recommended_uses_background_preferred_window_click_by_default() {
    let options = daily_recommended_window_click_options();

    assert_eq!(options.policy, auv_driver::InputPolicy::BackgroundPreferred);
    assert_eq!(options.window_strategy, auv_driver::WindowClickStrategy::ChromiumCompatible);
  }
}
