//! macOS system now-playing capability.
//!
//! Reads whatever app currently owns the system Now Playing slot (NetEase,
//! Spotify, Music, a browser tab — all identical) via the vendored
//! mediaremote-adapter, driven through Apple's `/usr/bin/perl` so the read
//! works on macOS 15.4+ where in-process MediaRemote access is gated. The read
//! is system-wide and app-agnostic; the owning app is reported in
//! [`NowPlayingState::source_bundle_id`] rather than filtered.

#[cfg(target_os = "macos")]
mod adapter;

pub mod cli;
mod error;
pub mod output;

// The now-playing state and transport commands are the shared media contract in
// `auv-driver-common`, so every platform's `mediaControl.*` reports one shape. They are
// re-exported here at their original paths.
pub use auv_driver_common::media::{MediaCommand, NowPlayingState};
pub use cli::OutputFormat;
pub use error::MediaError;

#[cfg(feature = "tracing")]
mod tracing {
  use std::time::Duration;

  use auv_tracing::{AttributeValue, Attributes, SpanSpec};

  use super::MediaCommand;

  struct NowPlayingSpan;

  impl SpanSpec for NowPlayingSpan {
    const NAME: &'static str = "auv.media.now_playing";

    fn attributes(&self) -> Attributes {
      Attributes::empty()
    }
  }

  struct SendCommandSpan {
    command: MediaCommand,
  }

  impl SpanSpec for SendCommandSpan {
    const NAME: &'static str = "auv.media.send_command";

    fn attributes(&self) -> Attributes {
      Attributes::from_iter([("auv.media.command", AttributeValue::string(self.command.label()))])
    }
  }

  struct SeekSpan {
    position: Duration,
  }

  impl SpanSpec for SeekSpan {
    const NAME: &'static str = "auv.media.seek";

    fn attributes(&self) -> Attributes {
      let Ok(position_millis) = i64::try_from(self.position.as_millis()) else {
        return Attributes::empty();
      };
      Attributes::from_iter([("auv.media.position_millis", AttributeValue::integer(position_millis))])
    }
  }

  pub(super) fn now_playing<T>(operation: impl FnOnce() -> T) -> T {
    auv_tracing::start_span(NowPlayingSpan).in_scope(operation)
  }

  pub(super) fn send_command<T>(command: MediaCommand, operation: impl FnOnce() -> T) -> T {
    auv_tracing::start_span(SendCommandSpan { command }).in_scope(operation)
  }

  pub(super) fn seek<T>(position: Duration, operation: impl FnOnce() -> T) -> T {
    auv_tracing::start_span(SeekSpan { position }).in_scope(operation)
  }
}

#[cfg(not(feature = "tracing"))]
mod tracing {
  use std::time::Duration;

  use super::MediaCommand;

  pub(super) fn now_playing<T>(operation: impl FnOnce() -> T) -> T {
    operation()
  }

  pub(super) fn send_command<T>(_command: MediaCommand, operation: impl FnOnce() -> T) -> T {
    operation()
  }

  pub(super) fn seek<T>(_position: Duration, operation: impl FnOnce() -> T) -> T {
    operation()
  }
}

/// The subset of the mediaremote-adapter `get` JSON we consume. The adapter
/// emits the bare literal `null` when nothing valid is playing; otherwise an
/// object whose mandatory keys are `bundleIdentifier`, `playing`, `title`.
/// `artworkData` and other keys are intentionally ignored.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct AdapterGet {
  bundle_identifier: Option<String>,
  #[serde(default)]
  playing: bool,
  title: Option<String>,
  artist: Option<String>,
  album: Option<String>,
  duration: Option<f64>,
  elapsed_time: Option<f64>,
  elapsed_time_now: Option<f64>,
  playback_rate: Option<f64>,
  content_item_identifier: Option<String>,
  supports_is_liked: Option<bool>,
  is_liked: Option<bool>,
}

/// Parse the adapter's `get` output into a [`NowPlayingState`]. Pure and
/// platform-independent so it is unit-testable without macOS or perl.
fn parse_get(json: &str) -> Result<NowPlayingState, MediaError> {
  let parsed: Option<AdapterGet> =
    serde_json::from_str(json.trim()).map_err(|error| MediaError::native(format!("invalid adapter JSON: {error}"), None))?;
  let Some(item) = parsed else {
    return Ok(NowPlayingState::default());
  };
  Ok(NowPlayingState {
    present: true,
    source_bundle_id: item.bundle_identifier,
    title: item.title,
    artist: item.artist,
    album: item.album,
    duration_seconds: item.duration,
    elapsed_seconds: item.elapsed_time_now.or(item.elapsed_time),
    playback_rate: item.playback_rate,
    is_playing: item.playing,
    content_item_id: item.content_item_identifier,
    supports_like: item.supports_is_liked,
    is_liked: item.is_liked,
  })
}

/// Read the current system now-playing state.
#[cfg(target_os = "macos")]
pub fn now_playing() -> Result<NowPlayingState, MediaError> {
  tracing::now_playing(|| parse_get(&adapter::run_now_playing_get()?))
}

/// Read the current system now-playing state (non-macOS stub).
#[cfg(not(target_os = "macos"))]
pub fn now_playing() -> Result<NowPlayingState, MediaError> {
  tracing::now_playing(|| Err(MediaError::Unsupported))
}

/// The numeric MRCommand id understood by mediaremote-adapter's `send`.
/// (See `vendor/mediaremote-adapter/include/MediaRemoteAdapter.h`.)
#[cfg(target_os = "macos")]
fn command_id(command: MediaCommand) -> u8 {
  match command {
    MediaCommand::Play => 0,
    MediaCommand::Pause => 1,
    MediaCommand::TogglePlayPause => 2,
    MediaCommand::NextTrack => 4,
    MediaCommand::PreviousTrack => 5,
  }
}

/// Send a transport command to the current now-playing app.
#[cfg(target_os = "macos")]
pub fn send_command(command: MediaCommand) -> Result<(), MediaError> {
  tracing::send_command(command, || adapter::send_command(command_id(command)))
}

/// Send a transport command to the current now-playing app (non-macOS stub).
#[cfg(not(target_os = "macos"))]
pub fn send_command(command: MediaCommand) -> Result<(), MediaError> {
  tracing::send_command(command, || Err(MediaError::Unsupported))
}

/// Send a transport command and observe the resulting now-playing state.
pub fn control(command: MediaCommand) -> Result<output::MediaControlOutcome, MediaError> {
  let before = now_playing()?;
  send_command(command)?;
  #[cfg(target_os = "macos")]
  std::thread::sleep(std::time::Duration::from_millis(200));
  let after = now_playing()?;
  Ok(output::MediaControlOutcome::new(command, &before, &after))
}

/// Seek the current now-playing app to `position` from the start of the track.
#[cfg(target_os = "macos")]
pub fn seek(position: std::time::Duration) -> Result<(), MediaError> {
  tracing::seek(position, || adapter::seek(position.as_micros()))
}

/// Seek the current now-playing app (non-macOS stub).
#[cfg(not(target_os = "macos"))]
pub fn seek(position: std::time::Duration) -> Result<(), MediaError> {
  tracing::seek(position, || Err(MediaError::Unsupported))
}

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
