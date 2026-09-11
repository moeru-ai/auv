use crate::*;

#[cfg(target_os = "macos")]
use crate::telemetry::{DailyRecommendedInputDelivered, DailyRecommendedPlayAllChecked};
#[cfg(target_os = "macos")]
use auv_driver::{InputActionResult, InputDeliveryPath, WindowInput as _};

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
pub fn run_daily_recommended_songs_scan(_inputs: &Inputs) -> Result<SongListScanResult, String> {
  Err("live NetEase daily recommended song scan is only supported on macOS".to_string())
}

#[cfg(target_os = "macos")]
pub fn run_daily_recommended_songs_scan(inputs: &Inputs) -> Result<SongListScanResult, String> {
  crate::app::run_songs_scan(inputs, DailyRecommendedRef)
}

#[cfg(target_os = "macos")]
pub(crate) fn scan_daily_recommended_songs(inputs: &Inputs) -> Result<SongListScanResult, String> {
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
  crate::commands::song::scan_open_song_list(
    run.session,
    run.window,
    inputs,
    "daily-recommended",
    "daily_recommended_song_list",
    run.diagnostics,
    run.known_limits,
  )
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
    crate::telemetry::png_artifact(action.capture_purpose(), &capture.image);
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
    crate::telemetry::png_artifact(action.capture_purpose(), &capture.image);
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
    crate::telemetry::png_artifact(DailyRecommendedClick::OpenDailyRecommendedCard.capture_purpose(), &capture.image);
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
      crate::telemetry::png_artifact("auv.netease.daily_recommended.play_all_visibility_capture", &capture.image);
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
      crate::telemetry::png_artifact("auv.netease.daily_recommended.icon_verification_capture", &capture.image);
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
      crate::telemetry::json_artifact(
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
      crate::telemetry::png_artifact("auv.netease.daily_recommended.playback_verification_capture", &capture.image);
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
      crate::telemetry::json_artifact("auv.netease.daily_recommended.playback_verification", &verification);
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

fn daily_recommended_card_click_point(title_bounds: ViewBounds) -> auv_driver::Point {
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
#[path = "daily_recommended_test.rs"]
mod tests;
