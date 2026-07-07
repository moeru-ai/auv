//! Transport control commands: play/pause, next track, previous track.
//!
//! ## Delivery mechanism
//!
//! Transport actions are sent as system-wide media key events via Win32
//! `SendInput`, using the virtual-key codes `VK_MEDIA_PLAY_PAUSE` (0xB3),
//! `VK_MEDIA_NEXT_TRACK` (0xB0), and `VK_MEDIA_PREV_TRACK` (0xB1). Media
//! keys are processed by the Windows shell and routed to whichever app has
//! registered as the current media session owner -- typically the last app
//! that started playback. Apple Music does not need to be in the foreground,
//! visible, or even have its window resolved; the OS routes the event correctly.
//!
//! ## Note on the coordinate-click approach
//!
//! A previous implementation attempted to click the UIA transport buttons by
//! converting WinUI3 virtual coordinates to screen coordinates. That approach
//! produced negative x values on multi-monitor setups where the primary monitor
//! is not the leftmost display. Media key injection avoids all coordinate
//! mapping entirely.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::app::ResolveOptions;

/// A transport action to perform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportAction {
  /// Toggle play/pause.
  PlayPause,
  /// Skip to the next track.
  Next,
  /// Skip to the previous track (or restart the current track).
  Previous,
}

impl fmt::Display for TransportAction {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      TransportAction::PlayPause => write!(f, "play_pause"),
      TransportAction::Next => write!(f, "next"),
      TransportAction::Previous => write!(f, "previous"),
    }
  }
}

/// Output produced by [`run_transport_action`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransportResult {
  pub command: String,
  pub action: TransportAction,
  /// Virtual-key name that was pressed.
  pub key: String,
  /// Non-fatal diagnostic notes.
  pub diagnostics: Vec<String>,
}

/// Inputs for a transport command.
#[derive(Clone, Debug)]
pub struct TransportInputs {
  /// Which transport action to perform.
  pub action: TransportAction,
  /// How long to wait after the key press for the app to react (ms).
  pub settle_ms: u64,
  /// Window resolution options (unused for media-key delivery, kept for API
  /// symmetry so callers can optionally verify the window exists first).
  pub resolve: ResolveOptions,
  /// When true, verify that Apple Music has a visible window before sending
  /// the key. Prevents accidentally controlling a different media app.
  pub require_window: bool,
}

impl TransportInputs {
  pub fn new(action: TransportAction) -> Self {
    Self {
      action,
      settle_ms: 150,
      resolve: ResolveOptions::default(),
      require_window: true,
    }
  }
}

/// Returns the key name string understood by `auv-driver-windows` press_key.
fn media_key_name(action: TransportAction) -> &'static str {
  match action {
    TransportAction::PlayPause => "media_play_pause",
    TransportAction::Next => "media_next",
    TransportAction::Previous => "media_prev",
  }
}

/// Sends the transport media key to Apple Music.
pub fn run_transport_action(inputs: &TransportInputs) -> Result<TransportResult, String> {
  platform::run(inputs)
}

#[cfg(not(target_os = "windows"))]
mod platform {
  use super::{TransportInputs, TransportResult};

  pub fn run(_inputs: &TransportInputs) -> Result<TransportResult, String> {
    Err("transport controls are only supported on Windows".to_string())
  }
}

#[cfg(target_os = "windows")]
mod platform {
  use std::time::Duration;

  use auv_driver::Driver;
  use auv_driver::input::KeyPressOptions;
  use auv_driver_windows::WindowsDriver;

  use super::{TransportInputs, TransportResult, media_key_name};
  use crate::app::resolve_window;

  pub fn run(inputs: &TransportInputs) -> Result<TransportResult, String> {
    let mut diagnostics: Vec<String> = Vec::new();
    let key = media_key_name(inputs.action);

    // Optional guard: confirm Apple Music has a visible window before sending
    // the media key, so we don't accidentally control Spotify or another player.
    if inputs.require_window {
      resolve_window(&inputs.resolve)?.ok_or_else(|| "Apple Music window not found -- is the app running?".to_string())?;
    }

    let session = WindowsDriver::new().open_local().map_err(|e| format!("driver open failed: {e}"))?;

    let opts = KeyPressOptions {
      key: key.to_string(),
      settle: Duration::from_millis(inputs.settle_ms),
    };

    session.input().press_key(opts).map_err(|e| format!("media key '{key}' failed: {e}"))?;

    diagnostics.push(format!("sent media key: {key}"));

    Ok(TransportResult {
      command: format!("transport.{}", inputs.action),
      action: inputs.action,
      key: key.to_string(),
      diagnostics,
    })
  }
}
