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

/// A structured snapshot of the system now-playing state.
///
/// [`Default`] is the idle state (nothing owns the slot) — useful for callers
/// that scope/filter the read to a specific app.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowPlayingState {
  /// Whether an app currently owns the now-playing slot with valid content.
  pub present: bool,
  /// Bundle identifier of the app that owns the now-playing slot.
  pub source_bundle_id: Option<String>,
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub duration_seconds: Option<f64>,
  pub elapsed_seconds: Option<f64>,
  pub playback_rate: Option<f64>,
  /// Whether playback is currently active (from the adapter's `playing` flag).
  pub is_playing: bool,
  pub content_item_id: Option<String>,
  /// Whether the now-playing app exposes a like/favorite affordance for this
  /// track (`None` when unreported).
  ///
  /// LIMITATION: in practice only Apple Music **catalog/streaming** tracks
  /// populate this. It is `None` for NetEase, for local files (verified even in
  /// Music.app, even after pressing Favorite), and for apps that don't integrate
  /// MediaRemote's like affordance. There is no general, free way to set a
  /// "like" via MediaRemote (the vendored adapter doesn't expose `kMRLikeTrack`,
  /// and it would need track/station identifiers). Verified empirically on
  /// macOS 26.2 — see the design spec's "like/favorite" finding.
  pub supports_like: Option<bool>,
  /// Whether this track is currently liked/favorited (`None` when unreported).
  /// Same limitation as [`Self::supports_like`] — effectively Apple Music
  /// catalog only; never set for NetEase or local tracks.
  pub is_liked: Option<bool>,
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
    elapsed_seconds: item.elapsed_time,
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

/// A transport command sent to whichever app owns the system now-playing slot.
///
/// Like the read, this is system-wide and app-agnostic — it acts on the
/// current now-playing app, not a specific one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
  Play,
  Pause,
  TogglePlayPause,
  NextTrack,
  PreviousTrack,
}

impl MediaCommand {
  /// A short stable label for this command (`"play"`, `"pause"`, `"toggle"`,
  /// `"next"`, `"previous"`).
  pub fn label(self) -> &'static str {
    match self {
      MediaCommand::Play => "play",
      MediaCommand::Pause => "pause",
      MediaCommand::TogglePlayPause => "toggle",
      MediaCommand::NextTrack => "next",
      MediaCommand::PreviousTrack => "previous",
    }
  }

  /// The numeric MRCommand id understood by mediaremote-adapter's `send`.
  /// (See `vendor/mediaremote-adapter/include/MediaRemoteAdapter.h`.)
  fn command_id(self) -> u8 {
    match self {
      MediaCommand::Play => 0,
      MediaCommand::Pause => 1,
      MediaCommand::TogglePlayPause => 2,
      MediaCommand::NextTrack => 4,
      MediaCommand::PreviousTrack => 5,
    }
  }
}

/// Send a transport command to the current now-playing app.
#[cfg(target_os = "macos")]
pub fn send_command(command: MediaCommand) -> Result<(), MediaError> {
  tracing::send_command(command, || adapter::send_command(command.command_id()))
}

/// Send a transport command to the current now-playing app (non-macOS stub).
#[cfg(not(target_os = "macos"))]
pub fn send_command(command: MediaCommand) -> Result<(), MediaError> {
  tracing::send_command(command, || Err(MediaError::Unsupported))
}

/// Send a transport command and observe the resulting now-playing state.
pub fn control(command: MediaCommand) -> Result<output::MediaControlOutcome, MediaError> {
  control_with(command, now_playing, send_command, || {
    #[cfg(target_os = "macos")]
    std::thread::sleep(std::time::Duration::from_millis(200));
  })
}

fn control_with(
  command: MediaCommand,
  mut read: impl FnMut() -> Result<NowPlayingState, MediaError>,
  send: impl FnOnce(MediaCommand) -> Result<(), MediaError>,
  settle: impl FnOnce(),
) -> Result<output::MediaControlOutcome, MediaError> {
  let before = read()?;
  send(command)?;
  settle();
  let after = read()?;
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
mod tests {
  use std::cell::{Cell, RefCell};

  use super::*;

  fn state(title: &str, is_playing: bool) -> NowPlayingState {
    NowPlayingState {
      present: true,
      title: Some(title.to_string()),
      artist: Some("Artist".to_string()),
      content_item_id: Some(title.to_string()),
      is_playing,
      ..Default::default()
    }
  }

  fn outcome(command: MediaCommand, before: NowPlayingState, after: NowPlayingState) -> output::MediaControlOutcome {
    let reads = RefCell::new(vec![before, after].into_iter());
    control_with(command, || Ok(reads.borrow_mut().next().expect("two reads")), |_| Ok(()), || {}).expect("control")
  }

  #[test]
  fn control_reads_sends_once_settles_and_reads_in_order() {
    let events = RefCell::new(Vec::new());
    let reads = RefCell::new(vec![state("Before", false), state("After", true)].into_iter());
    let result = control_with(
      MediaCommand::Play,
      || {
        events.borrow_mut().push("read");
        Ok(reads.borrow_mut().next().expect("two reads"))
      },
      |command| {
        assert_eq!(command, MediaCommand::Play);
        events.borrow_mut().push("send");
        Ok(())
      },
      || events.borrow_mut().push("settle"),
    )
    .expect("control");

    assert_eq!(*events.borrow(), ["read", "send", "settle", "read"]);
    assert_eq!(result.command, "play");
    assert!(result.verified);
  }

  #[test]
  fn control_does_not_send_when_the_before_read_fails() {
    let sent = Cell::new(false);
    let error = control_with(
      MediaCommand::NextTrack,
      || Err(MediaError::Unsupported),
      |_| {
        sent.set(true);
        Ok(())
      },
      || {},
    )
    .expect_err("before read");
    assert_eq!(error, MediaError::Unsupported);
    assert!(!sent.get());
  }

  #[test]
  fn verification_matches_each_command_postcondition() {
    assert!(outcome(MediaCommand::Play, state("A", false), state("A", true)).verified);
    assert!(outcome(MediaCommand::Pause, state("A", true), state("A", false)).verified);
    assert!(outcome(MediaCommand::TogglePlayPause, state("A", false), state("A", true)).verified);
    assert!(outcome(MediaCommand::NextTrack, state("A", true), state("B", true)).verified);
    assert!(outcome(MediaCommand::PreviousTrack, state("B", true), state("A", true)).verified);

    assert!(!outcome(MediaCommand::Play, state("A", false), state("A", false)).verified);
    assert!(!outcome(MediaCommand::Pause, state("A", true), state("A", true)).verified);
    assert!(!outcome(MediaCommand::TogglePlayPause, state("A", true), state("A", true)).verified);
    assert!(!outcome(MediaCommand::NextTrack, state("A", true), state("A", true)).verified);
  }
}
