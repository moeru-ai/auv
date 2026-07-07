//! `playback status` command: read play/pause state, track title, and artist
//! from the running Apple Music window on Windows.
//!
//! ## Detection strategy
//!
//! **Play/pause state** — read from the UI Automation (UIA) accessibility tree.
//! Apple Music (WinUI/UWP) exposes the transport bar buttons by name. When the
//! app is playing the button's UIA name is "Pause"; when paused it is "Play".
//! This is fast and does not require a pixel capture.
//!
//! **Track title and artist** — read from the UIA tree when the nodes carry
//! a usable name, otherwise fall back to OCR on a bottom-bar strip of the
//! window capture. The strip is the bottom ~18 % of the window where Apple
//! Music renders the now-playing metadata.
//!
//! ## Artifacts
//!
//! When `artifact_dir` is set the full window capture is saved as a PNG so the
//! caller can inspect the raw pixels used for OCR. The file is named
//! `apple-music-playback-<timestamp>.png`.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::PathBuf;

use crate::app::ResolveOptions;

// NOTICE: bottom-bar strip covers the now-playing bar at the bottom of the
// Apple Music window. 0.82–1.0 captures the transport area on typical window
// heights without including the main content pane.
const BOTTOM_BAR_TOP: f64 = 0.82;

/// Play/pause state inferred from the accessibility tree or OCR.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaybackState {
  /// The play/pause button name is "Pause" → app is currently playing.
  Playing,
  /// The play/pause button name is "Play" → app is currently paused.
  Paused,
  /// Could not determine state (button not found or name was unexpected).
  Unknown,
}

impl fmt::Display for PlaybackState {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      PlaybackState::Playing => write!(f, "playing"),
      PlaybackState::Paused => write!(f, "paused"),
      PlaybackState::Unknown => write!(f, "unknown"),
    }
  }
}

/// Which probe path produced the track metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetadataSource {
  /// Title/artist came from UIA node names (preferred).
  UiAutomation,
  /// Title/artist came from OCR on the bottom-bar capture strip.
  Ocr,
  /// No metadata was found.
  NotFound,
}

impl fmt::Display for MetadataSource {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      MetadataSource::UiAutomation => write!(f, "ui_automation"),
      MetadataSource::Ocr => write!(f, "ocr"),
      MetadataSource::NotFound => write!(f, "not_found"),
    }
  }
}

/// Output produced by [`run_playback_status`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaybackStatus {
  pub command: String,
  /// Title of the resolved window (e.g. "Apple Music").
  pub window_title: Option<String>,
  /// Play/pause state.
  pub state: PlaybackState,
  /// Track title, if detected.
  pub track_title: Option<String>,
  /// Artist name, if detected.
  pub artist: Option<String>,
  /// Which path produced track metadata.
  pub metadata_source: MetadataSource,
  /// Path to the saved window capture artifact, if any.
  pub artifact: Option<String>,
  /// Non-fatal diagnostic notes.
  pub diagnostics: Vec<String>,
}

/// Inputs for the `playback status` command.
#[derive(Clone, Debug)]
pub struct PlaybackStatusInputs {
  /// Directory to write PNG artifacts into. `None` skips capture saving.
  pub artifact_dir: Option<PathBuf>,
  /// Window resolution options.
  pub resolve: ResolveOptions,
}

impl Default for PlaybackStatusInputs {
  fn default() -> Self {
    Self {
      artifact_dir: None,
      resolve: ResolveOptions::default(),
    }
  }
}

/// Probes the Apple Music window for playback state and now-playing metadata.
///
/// On non-Windows targets this always returns an error.
pub fn run_playback_status(inputs: &PlaybackStatusInputs) -> Result<PlaybackStatus, String> {
  platform::run(inputs)
}

#[cfg(not(target_os = "windows"))]
mod platform {
  use super::{PlaybackStatus, PlaybackStatusInputs};

  pub fn run(_inputs: &PlaybackStatusInputs) -> Result<PlaybackStatus, String> {
    Err("playback status is only supported on Windows".to_string())
  }
}

#[cfg(target_os = "windows")]
mod platform {
  use auv_driver::Driver;
  use auv_driver::geometry::RatioRect;
  use auv_driver::vision::TextRecognitionOptions;
  use auv_driver_windows::WindowsDriver;

  use super::{BOTTOM_BAR_TOP, MetadataSource, PlaybackState, PlaybackStatus, PlaybackStatusInputs};
  use crate::app::resolve_window;

  pub fn run(inputs: &PlaybackStatusInputs) -> Result<PlaybackStatus, String> {
    let mut diagnostics: Vec<String> = Vec::new();

    // --- Resolve window ---
    let apple_window = resolve_window(&inputs.resolve)?.ok_or_else(|| "Apple Music window not found — is the app running?".to_string())?;

    let window = &apple_window.window;
    let window_title = window.title.clone();

    // --- Open driver session ---
    let session = WindowsDriver::new().open_local().map_err(|e| format!("windows driver open failed: {e}"))?;

    // --- Playback state via UIA ---
    let (state, mut ax_track, mut ax_artist) = probe_via_ax(&session, window, &mut diagnostics);

    // --- Capture window (always, for OCR fallback + optional artifact) ---
    let capture = session.window().capture(window).map_err(|e| format!("window capture failed: {e}"))?;

    // --- Artifact save ---
    let artifact = if let Some(dir) = &inputs.artifact_dir {
      match save_artifact(dir, &capture) {
        Ok(path) => Some(path),
        Err(e) => {
          diagnostics.push(format!("artifact save failed: {e}"));
          None
        }
      }
    } else {
      None
    };

    // --- OCR fallback for track metadata ---
    let (ocr_track, ocr_artist) = if ax_track.is_none() || ax_artist.is_none() {
      probe_via_ocr(&session, &capture, &mut diagnostics)
    } else {
      (None, None)
    };

    let metadata_source = if ax_track.is_some() || ax_artist.is_some() {
      MetadataSource::UiAutomation
    } else if ocr_track.is_some() || ocr_artist.is_some() {
      MetadataSource::Ocr
    } else {
      MetadataSource::NotFound
    };

    let track_title = ax_track.take().or(ocr_track);
    let artist = ax_artist.take().or(ocr_artist);

    Ok(PlaybackStatus {
      command: "playback.status".to_string(),
      window_title,
      state,
      track_title,
      artist,
      metadata_source,
      artifact,
      diagnostics,
    })
  }

  /// Reads playback state and track metadata from the UIA accessibility tree.
  ///
  /// Returns `(PlaybackState, Option<title>, Option<artist>)`.
  ///
  /// From the live UIA tree:
  ///  - Play/pause button: `automation_id = "TransportControl_PlayPauseStop"`,
  ///    `control_type = "app bar button"`, name = "Pause" (playing) | "Play"
  ///    (paused).
  ///  - Now-playing metadata: group `automation_id = "LCD"` whose `name`
  ///    attribute is `"{title} By {artist} — {album}"`. The child
  ///    `ScrollingText` TextBlock nodes repeat the text many times for the
  ///    marquee ticker, so we read from the LCD group instead.
  fn probe_via_ax(
    session: &auv_driver_windows::WindowsDriverSession,
    window: &auv_driver::window::Window,
    diagnostics: &mut Vec<String>,
  ) -> (PlaybackState, Option<String>, Option<String>) {
    let snapshot = match session.accessibility().snapshot_window(window) {
      Ok(snap) => snap,
      Err(e) => {
        diagnostics.push(format!("UIA snapshot failed: {e}"));
        return (PlaybackState::Unknown, None, None);
      }
    };

    let mut state = PlaybackState::Unknown;
    let mut track: Option<String> = None;
    let mut artist: Option<String> = None;

    for node in &snapshot.nodes {
      // Play/pause: identified by stable automation_id regardless of locale.
      // control_type is "app bar button" (not "Button"), so we match by id.
      if node.automation_id == "TransportControl_PlayPauseStop" {
        state = match node.name.as_str() {
          "Pause" => PlaybackState::Playing,
          "Play" => PlaybackState::Paused,
          other => {
            diagnostics.push(format!("unexpected TransportControl_PlayPauseStop name: {other:?}"));
            PlaybackState::Unknown
          }
        };
      }

      // LCD group carries the full now-playing string:
      // "{title} By {artist} — {performer} — {album}"
      // This is the non-repeated version; child ScrollingText nodes repeat
      // the text many times for the marquee animation.
      if node.automation_id == "LCD" && !node.name.is_empty() {
        let (t, a) = parse_lcd_name(&node.name);
        track = t;
        artist = a;
      }
    }

    (state, track, artist)
  }

  /// Parses the LCD group name into `(track_title, artist)`.
  ///
  /// Apple Music formats the name as `"{title} By {artist} — {album}"`.
  /// When no song is loaded the LCD name may be empty or absent.
  fn parse_lcd_name(name: &str) -> (Option<String>, Option<String>) {
    // The marquee widget can duplicate text: "TITLE By ARTIST TITLE By ARTIST"
    // Use only the portion up to the first repetition boundary.
    let name = first_unique_segment(name);
    if let Some((title, rest)) = name.split_once(" By ") {
      let title = title.trim().to_string();
      // rest = "ARTIST — PERFORMER — ALBUM"; take only the first segment.
      let artist = rest
        .split(" \u{2014} ") // em-dash separator " — "
        .next()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
      (Some(title), artist)
    } else {
      // No "By" separator — return name as title only.
      (Some(name.trim().to_string()), None)
    }
  }

  /// Returns the shortest non-empty prefix `P` such that `text` starts with
  /// `"P P"` (space-separated duplicate), indicating a marquee repeat.
  /// Falls back to the full string when no repeat is detected.
  fn first_unique_segment(text: &str) -> &str {
    // Try each split point to find the smallest repeating prefix.
    // Cap the search at half the string length for efficiency.
    let bytes = text.as_bytes();
    let limit = text.len() / 2;
    for i in 1..=limit {
      // The separator between repetitions is a single space.
      if bytes.get(i) == Some(&b' ') {
        let prefix = &text[..i];
        let rest = &text[i + 1..];
        if rest.starts_with(prefix) {
          return prefix;
        }
      }
    }
    text
  }

  /// OCR fallback: recognizes text in the bottom-bar strip and heuristically
  /// assigns the first two non-trivial lines to title and artist.
  fn probe_via_ocr(
    session: &auv_driver_windows::WindowsDriverSession,
    capture: &auv_driver::capture::Capture,
    diagnostics: &mut Vec<String>,
  ) -> (Option<String>, Option<String>) {
    let region = RatioRect::new(0.0, BOTTOM_BAR_TOP, 1.0, 1.0 - BOTTOM_BAR_TOP);
    let recognition = match session.vision().recognize_text_in_capture_with_options(capture, region, TextRecognitionOptions::default()) {
      Ok(r) => r,
      Err(e) => {
        diagnostics.push(format!("bottom-bar OCR failed: {e}"));
        return (None, None);
      }
    };

    // The bottom bar on Apple Music typically shows the title on the first
    // line and the artist on the second. Filter out very short strings (icons,
    // single-char labels) and time stamps that look like "0:00".
    let lines: Vec<String> =
      recognition.regions.iter().filter(|r| r.text.len() > 2 && !looks_like_timestamp(&r.text)).map(|r| r.text.clone()).collect();

    let track = lines.first().cloned();
    let artist = lines.get(1).cloned();
    (track, artist)
  }

  /// Returns true for strings that look like `M:SS` or `MM:SS` timestamps.
  fn looks_like_timestamp(s: &str) -> bool {
    let parts: Vec<&str> = s.splitn(2, ':').collect();
    if parts.len() != 2 {
      return false;
    }
    parts[0].chars().all(|c| c.is_ascii_digit()) && parts[1].chars().all(|c| c.is_ascii_digit())
  }

  /// Saves the capture image to `dir` and returns the file path string.
  fn save_artifact(dir: &std::path::Path, capture: &auv_driver::capture::Capture) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create artifact dir failed: {e}"))?;

    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let path = dir.join(format!("apple-music-playback-{ts}.png"));
    capture.image.save(&path).map_err(|e| format!("save PNG failed: {e}"))?;
    Ok(path.to_string_lossy().into_owned())
  }

  use std::time::{SystemTime, UNIX_EPOCH};
}
