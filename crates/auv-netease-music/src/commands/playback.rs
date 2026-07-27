use std::fmt;

use auv_cli_common::TableRow;
use auv_cli_common::outputs::cli::CliOutput;
use auv_cli_common::outputs::formats::table::{self, TableOptions};
#[cfg(target_os = "macos")]
use auv_driver::WindowInput as _;
use auv_driver::vision::TextRecognitionOptions;
use auv_view::{ParserDiagnostic, ScanAppContext, ScanWindowContext};
use serde::{Deserialize, Serialize};

use crate::DEFAULT_APP_ID;
use crate::views::player::PlaybackControlState;

// NOTICE(netease-playback-status-visual-fallback): this command is a
// NetEase-specific OCR/detail-screen probe, not the future `now-playing-v0`
// contract. Generic title/artist/album/rate reads are deferred to
// `auv-media-macos`; see
// `docs/ai/references/2026-06-04-media-macos-now-playing-design.md`.

#[derive(Clone, Debug, PartialEq)]
pub struct PlaybackStatusInputs {
  pub app_id: String,
  pub settle_ms: u64,
  pub ocr_options: TextRecognitionOptions,
}

impl PlaybackStatusInputs {
  pub fn with_defaults() -> Self {
    Self {
      app_id: DEFAULT_APP_ID.to_string(),
      settle_ms: 350,
      ocr_options: TextRecognitionOptions::default(),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaybackStatus {
  /// Local experimental shape for `auv-netease-music playback status`.
  ///
  /// TODO(media-now-playing-v0): do not extend this into a parallel generic
  /// now-playing contract. When `auv-media-macos` lands, add/delegate
  /// `auv-netease-music now-playing` to its `now-playing-v0` output instead.
  pub command: String,
  pub app: ScanAppContext,
  pub window: ScanWindowContext,
  pub playback_exists: bool,
  pub was_playing: bool,
  pub control_state: Option<PlaybackControlState>,
  pub click_point: Option<auv_driver::Point>,
  pub detail_screen_detected: bool,
  pub source: Option<String>,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
}

impl PlaybackStatus {
  pub fn to_human_readable(&self, wide: bool) -> PlaybackStatusHumanReadable<'_> {
    PlaybackStatusHumanReadable { result: self, wide }
  }

  pub fn to_json(&self) -> PlaybackStatusJson<'_> {
    PlaybackStatusJson {
      command: &self.command,
      app: &self.app,
      window: &self.window,
      playback_exists: self.playback_exists,
      was_playing: self.was_playing,
      control_state: self.control_state,
      click_point: self.click_point,
      detail_screen_detected: self.detail_screen_detected,
      source: self.source.as_deref(),
      diagnostics: &self.diagnostics,
      known_limits: &self.known_limits,
    }
  }
}

#[derive(Clone, Copy, Debug)]
pub struct PlaybackStatusHumanReadable<'a> {
  result: &'a PlaybackStatus,
  wide: bool,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct PlaybackStatusJson<'a> {
  pub command: &'a str,
  pub app: &'a ScanAppContext,
  pub window: &'a ScanWindowContext,
  pub playback_exists: bool,
  pub was_playing: bool,
  pub control_state: Option<PlaybackControlState>,
  pub click_point: Option<auv_driver::Point>,
  pub detail_screen_detected: bool,
  pub source: Option<&'a str>,
  pub diagnostics: &'a [ParserDiagnostic],
  pub known_limits: &'a [String],
}

impl fmt::Display for PlaybackStatusHumanReadable<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", CliOutput::to_human(self.result, TableOptions::default().wide(self.wide)))
  }
}

impl CliOutput for PlaybackStatus {
  fn to_json(&self) -> impl Serialize {
    PlaybackStatus::to_json(self)
  }

  fn to_table_print(&self, options: TableOptions<'_>) -> String {
    let status = [PlaybackStatusRow {
      playback: self.playback_exists,
      screen: self.detail_screen_detected,
      playing: self.was_playing,
      control: self.control_state,
      click: self.click_point.map(|point| format!("{:.1},{:.1}", point.x, point.y)).unwrap_or_else(|| "-".to_string()),
      source: self.source.as_deref().unwrap_or("-"),
    }];
    table::render(&status, options)
  }

  fn human_details(&self, _options: TableOptions<'_>) -> Option<String> {
    let mut sections = Vec::new();
    if !self.known_limits.is_empty() {
      let limits = self
        .known_limits
        .iter()
        .map(|limit| KnownLimitRow {
          known_limits: limit,
        })
        .collect::<Vec<_>>();
      sections.push(table::render(&limits, TableOptions::default()));
    }

    if !self.diagnostics.is_empty() {
      let diagnostics = self
        .diagnostics
        .iter()
        .map(|diagnostic| DiagnosticRow {
          diagnostic: &diagnostic.code,
          message: &diagnostic.message,
        })
        .collect::<Vec<_>>();
      sections.push(table::render(&diagnostics, TableOptions::default()));
    }
    (!sections.is_empty()).then(|| sections.join("\n\n"))
  }
}

#[derive(TableRow)]
struct PlaybackStatusRow<'a> {
  #[table(display_zero = "N/A", display_with = |_: &bool| "Detected")]
  playback: bool,
  #[table(display_zero = "N/A", display_with = |_: &bool| "Details")]
  screen: bool,
  #[table(
    wide,
    display_with = |is_playing: &bool| {
      if *is_playing {
        "Playing"
      } else if self.control == Some(PlaybackControlState::PlayVisible) {
        "Paused"
      } else {
        "N/A"
      }
    }
  )]
  playing: bool,
  #[table(
    wide,
    display_with = |value: &Option<PlaybackControlState>| match value {
      Some(PlaybackControlState::PlayVisible) => "play_visible",
      Some(PlaybackControlState::PauseVisible) => "pause_visible",
      Some(PlaybackControlState::Unknown) => "unknown",
      None => "-",
    }
  )]
  control: Option<PlaybackControlState>,
  #[table(wide)]
  click: String,
  source: &'a str,
}

#[derive(TableRow)]
struct KnownLimitRow<'a> {
  known_limits: &'a str,
}

#[derive(TableRow)]
struct DiagnosticRow<'a> {
  diagnostic: &'a str,
  message: &'a str,
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn run_playback_status_probe(_inputs: &PlaybackStatusInputs) -> Result<PlaybackStatus, String> {
  Err("live NetEase playback status probe is only supported on macOS and Windows".to_string())
}

#[cfg(target_os = "windows")]
pub fn run_playback_status_probe(inputs: &PlaybackStatusInputs) -> Result<PlaybackStatus, String> {
  use crate::commands::transport::{NodeEvidence, classify_playpause_state, find_playpause_control};
  use crate::views::player::PlayerView;
  use crate::windows::{ResolveOptions, resolve_window};
  use auv_view::ViewBounds;

  // NOTICE(netease-windows-playback-status-scope): unlike the macOS probe,
  // this reads UIA control state directly and does not capture screenshots,
  // run OCR, or click into the song-detail screen. `ocr_options` is part of the
  // shared `PlaybackStatusInputs` contract for the macOS branch and is
  // intentionally unused here.
  let _ = &inputs.ocr_options;

  let mut diagnostics = Vec::new();
  let mut known_limits = Vec::new();

  let Some(window) = resolve_window(&ResolveOptions::default())? else {
    known_limits.push("NetEase Cloud Music window not found; run `open-window` first".to_string());
    return Ok(PlaybackStatus {
      command: "playback.status".to_string(),
      app: ScanAppContext::default(),
      window: ScanWindowContext::default(),
      playback_exists: false,
      was_playing: false,
      control_state: None,
      click_point: None,
      detail_screen_detected: false,
      source: None,
      diagnostics,
      known_limits,
    });
  };

  let app = ScanAppContext {
    app_id: None,
    name: window.app_name.clone(),
    version: None,
  };
  let window_context = ScanWindowContext {
    id: Some(window.reference.id.clone()),
    title: window.title.clone(),
    bounds: Some(ViewBounds::new(0.0, 0.0, window.frame.size.width, window.frame.size.height)),
  };

  let session = auv_driver::open_local().map_err(|error| format!("failed to open Windows driver: {error}"))?;
  let snapshot = session.accessibility().snapshot_window(&window).map_err(|error| format!("failed to capture NetEase UIA tree: {error}"))?;

  let matched = match find_playpause_control(
    snapshot.nodes.iter().map(|node| NodeEvidence {
      path: &node.path,
      name: &node.name,
      value: node.value.as_deref(),
      automation_id: &node.automation_id,
      control_type: &node.control_type,
      bounds: node.bounds,
    }),
    window.frame,
  ) {
    Ok(matched) => matched,
    Err(error) => {
      known_limits.push(format!(
        "{error}; captured {} UIA nodes. If the tree contains only CEF containers, relaunch NetEase with `auv-netease-music open-window` so renderer accessibility is enabled",
        snapshot.nodes.len()
      ));
      return Ok(PlaybackStatus {
        command: "playback.status".to_string(),
        app,
        window: window_context,
        playback_exists: false,
        was_playing: false,
        control_state: None,
        click_point: None,
        detail_screen_detected: false,
        source: None,
        diagnostics,
        known_limits,
      });
    }
  };

  let control_state = classify_playpause_state(matched.name, matched.value);
  let was_playing = PlayerView::from_control_state(control_state).is_playing();
  if control_state == PlaybackControlState::Unknown {
    diagnostics.push(ParserDiagnostic {
      code: "windows_playpause_state_unknown".to_string(),
      message: format!("could not classify play/pause state from UIA control name {:?} value {:?}", matched.name, matched.value),
      node_id: Some(matched.path.to_string()),
    });
    known_limits.push(
      "NetEase's Windows play/pause control did not expose a distinguishable Name/Value; state detection needs a UIA TogglePattern read that auv-driver-windows does not yet expose".to_string(),
    );
  }
  known_limits.push("song-detail screen detection and `source` extraction are not implemented for Windows in this slice".to_string());

  Ok(PlaybackStatus {
    command: "playback.status".to_string(),
    app,
    window: window_context,
    playback_exists: true,
    was_playing,
    control_state: Some(control_state),
    click_point: None,
    detail_screen_detected: false,
    source: None,
    diagnostics,
    known_limits,
  })
}

#[cfg(target_os = "macos")]
pub fn run_playback_status_probe(inputs: &PlaybackStatusInputs) -> Result<PlaybackStatus, String> {
  use crate::views::player::PlayerView;
  use crate::views::player::classify_bottom_playback_control_state;
  use crate::views::screen;
  use auv_driver::selector::{App, Window};
  use auv_driver::{
    ActivationPolicy, Click, ClickOptions, InputPolicy, PrepareForInputOptions, RatioRect, Size, WindowClickStrategy, WindowPoint,
  };
  use auv_view::ViewBounds;

  let mut diagnostics = Vec::new();
  let mut known_limits = Vec::new();

  let session = auv_driver::open_local().map_err(|error| format!("failed to open macOS driver: {error}"))?;
  let window = session
    .window()
    .resolve(Window::main_visible().owned_by(App::bundle(inputs.app_id.clone())))
    .map_err(|error| format!("failed to resolve NetEase window: {error}"))?;
  let app = ScanAppContext {
    app_id: window.app_bundle_id.clone().or_else(|| Some(inputs.app_id.clone())),
    name: window.app_name.clone(),
    version: None,
  };
  let window_context = ScanWindowContext {
    id: Some(window.reference.id.clone()),
    title: window.title.clone(),
    bounds: Some(ViewBounds::new(0.0, 0.0, window.frame.size.width, window.frame.size.height)),
  };
  let window_size = Size::new(window.frame.size.width, window.frame.size.height);

  let before_capture = session.window().capture(&window).map_err(|error| format!("initial playback capture failed: {error}"))?;
  crate::telemetry::png_artifact("auv.netease.playback.before_capture", &before_capture.image);
  let before_recognition = session
    .vision()
    .recognize_text_in_capture_with_options(&before_capture, RatioRect::new(0.0, 0.0, 1.0, 1.0), inputs.ocr_options.clone())
    .map_err(|error| format!("initial playback OCR failed: {error}"))?;
  let before_recognition = recognition_in_window_space(before_recognition, &before_capture);
  let before_screen = screen::classify_screen(&before_recognition, window_size);
  let control_state = classify_bottom_playback_control_state(&before_capture.image);
  let mut player = PlayerView::from_control_state(control_state);
  if let Some(text) = bottom_bar_text(&before_recognition, window_size) {
    player = player.with_observed_text(text);
  }

  if before_screen.is_playing_song_detail() {
    let source = screen::song_detail_source(&before_recognition, window_size);
    if source.is_none() {
      known_limits.push("song detail screen was already open, but OCR did not identify the upper-right source label".to_string());
    }
    return Ok(PlaybackStatus {
      command: "playback.status".to_string(),
      app,
      window: window_context,
      playback_exists: player.exists() || source.is_some(),
      was_playing: player.is_playing(),
      control_state: Some(control_state),
      click_point: None,
      detail_screen_detected: true,
      source,
      diagnostics,
      known_limits,
    });
  }

  let Some(click_point) = player.song_detail_click_point(window_size) else {
    known_limits.push("bottom playback bar was not detected; status probe did not click".to_string());
    return Ok(PlaybackStatus {
      command: "playback.status".to_string(),
      app,
      window: window_context,
      playback_exists: false,
      was_playing: false,
      control_state: Some(control_state),
      click_point: None,
      detail_screen_detected: false,
      source: None,
      diagnostics,
      known_limits,
    });
  };

  let click_result = session
    .window()
    .click(
      &window,
      WindowPoint::new(click_point.x, click_point.y),
      ClickOptions {
        policy: InputPolicy::BackgroundPreferred,
        click: Click::Single,
        window_strategy: WindowClickStrategy::ChromiumCompatible,
      },
    )
    .map_err(|error| format!("playback bar click failed: {error}"))?;
  if let Some(reason) = click_result.fallback_reason() {
    known_limits.push(format!("playback bar click fallback: {reason}"));
  }
  if inputs.settle_ms > 0 {
    std::thread::sleep(std::time::Duration::from_millis(inputs.settle_ms));
  }

  let mut after_capture = session.window().capture(&window).map_err(|error| format!("post-click detail capture failed: {error}"))?;
  crate::telemetry::png_artifact("auv.netease.playback.after_click_capture", &after_capture.image);
  let mut recognition = session
    .vision()
    .recognize_text_in_capture_with_options(&after_capture, RatioRect::new(0.0, 0.0, 1.0, 1.0), inputs.ocr_options.clone())
    .map_err(|error| format!("post-click detail OCR failed: {error}"))?;
  recognition = recognition_in_window_space(recognition, &after_capture);
  let mut screen = screen::classify_screen(&recognition, window_size);
  let mut detail_screen_detected = screen.is_playing_song_detail();
  if !detail_screen_detected {
    known_limits.push("background playback bar click did not reveal song detail; retried with foreground click".to_string());
    let lease = session
      .window()
      .prepare_for_input(
        &window,
        PrepareForInputOptions {
          activation: ActivationPolicy::Foreground {
            settle: std::time::Duration::from_millis(inputs.settle_ms),
          },
          preserve_frontmost: false,
          install_focus_guard: false,
          settle: std::time::Duration::ZERO,
        },
      )
      .map_err(|error| format!("playback bar foreground preparation failed: {error}"))?;
    let click_result =
      auv_driver_macos::native::pointer::click_point(window.frame.origin.x + click_point.x, window.frame.origin.y + click_point.y, 0, 1, 80);
    let restore_result = session.window().restore_input(lease);
    click_result.map_err(|error| format!("playback bar foreground click failed: {error}"))?;
    restore_result.map_err(|error| format!("playback bar foreground restore failed: {error}"))?;
    if inputs.settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(inputs.settle_ms));
    }

    after_capture = session.window().capture(&window).map_err(|error| format!("post-foreground-click detail capture failed: {error}"))?;
    crate::telemetry::png_artifact("auv.netease.playback.after_foreground_click_capture", &after_capture.image);
    recognition = session
      .vision()
      .recognize_text_in_capture_with_options(&after_capture, RatioRect::new(0.0, 0.0, 1.0, 1.0), inputs.ocr_options.clone())
      .map_err(|error| format!("post-foreground-click detail OCR failed: {error}"))?;
    recognition = recognition_in_window_space(recognition, &after_capture);
    screen = screen::classify_screen(&recognition, window_size);
    detail_screen_detected = screen.is_playing_song_detail();
  }
  if !detail_screen_detected {
    diagnostics.push(ParserDiagnostic {
      code: "song_detail_not_detected".to_string(),
      message: "playback bar click did not reveal NetEase song detail markers".to_string(),
      node_id: None,
    });
  }
  let source = screen::song_detail_source(&recognition, window_size);
  if detail_screen_detected && source.is_none() {
    known_limits.push("song detail screen was detected, but OCR did not identify the upper-right source label".to_string());
  }

  Ok(PlaybackStatus {
    command: "playback.status".to_string(),
    app,
    window: window_context,
    playback_exists: true,
    was_playing: player.is_playing(),
    control_state: Some(control_state),
    click_point: Some(click_point),
    detail_screen_detected,
    source,
    diagnostics,
    known_limits,
  })
}

#[cfg(target_os = "macos")]
fn recognition_in_window_space(
  mut recognition: auv_driver::vision::TextRecognition,
  capture: &auv_driver::capture::Capture,
) -> auv_driver::vision::TextRecognition {
  for region in &mut recognition.regions {
    region.bounds.origin.x -= capture.bounds.origin.x;
    region.bounds.origin.y -= capture.bounds.origin.y;
  }
  recognition
}

#[cfg(target_os = "macos")]
fn bottom_bar_text(recognition: &auv_driver::vision::TextRecognition, window_size: auv_driver::Size) -> Option<String> {
  let mut regions = recognition
    .regions
    .iter()
    .filter(|region| region.bounds.origin.x <= window_size.width * 0.34 && region.bounds.origin.y >= window_size.height * 0.88)
    .collect::<Vec<_>>();
  regions.sort_by(|left, right| {
    left
      .bounds
      .origin
      .y
      .partial_cmp(&right.bounds.origin.y)
      .unwrap_or(std::cmp::Ordering::Equal)
      .then_with(|| left.bounds.origin.x.partial_cmp(&right.bounds.origin.x).unwrap_or(std::cmp::Ordering::Equal))
  });
  let text = regions.into_iter().map(|region| region.text.trim()).filter(|text| !text.is_empty()).collect::<Vec<_>>().join("\n");

  (!text.trim().is_empty()).then_some(text)
}
