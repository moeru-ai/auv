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
  let parsed: Option<AdapterGet> = serde_json::from_str(json.trim())
    .map_err(|error| MediaError::native(format!("invalid adapter JSON: {error}"), None))?;
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
  parse_get(&adapter::run_now_playing_get()?)
}

/// Read the current system now-playing state (non-macOS stub).
#[cfg(not(target_os = "macos"))]
pub fn now_playing() -> Result<NowPlayingState, MediaError> {
  Err(MediaError::Unsupported)
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
  adapter::send_command(command.command_id())
}

/// Send a transport command to the current now-playing app (non-macOS stub).
#[cfg(not(target_os = "macos"))]
pub fn send_command(_command: MediaCommand) -> Result<(), MediaError> {
  Err(MediaError::Unsupported)
}

/// Seek the current now-playing app to `position` from the start of the track.
#[cfg(target_os = "macos")]
pub fn seek(position: std::time::Duration) -> Result<(), MediaError> {
  adapter::seek(position.as_micros())
}

/// Seek the current now-playing app (non-macOS stub).
#[cfg(not(target_os = "macos"))]
pub fn seek(_position: std::time::Duration) -> Result<(), MediaError> {
  Err(MediaError::Unsupported)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_get_null_is_idle() {
    let state = parse_get("null").unwrap();
    assert!(!state.present);
    assert!(!state.is_playing);
    assert_eq!(state.title, None);
  }

  #[test]
  fn parse_get_maps_playing_track() {
    let json = r#"{
      "bundleIdentifier": "com.netease.163music",
      "playing": true,
      "title": "Song",
      "artist": "Artist",
      "album": "Album",
      "duration": 200.0,
      "elapsedTime": 12.5,
      "playbackRate": 1.0,
      "contentItemIdentifier": "abc",
      "artworkData": "ignored"
    }"#;
    let state = parse_get(json).unwrap();
    assert!(state.present);
    assert!(state.is_playing);
    assert_eq!(
      state.source_bundle_id.as_deref(),
      Some("com.netease.163music")
    );
    assert_eq!(state.title.as_deref(), Some("Song"));
    assert_eq!(state.duration_seconds, Some(200.0));
    assert_eq!(state.elapsed_seconds, Some(12.5));
    assert_eq!(state.playback_rate, Some(1.0));
    assert_eq!(state.content_item_id.as_deref(), Some("abc"));
  }

  #[test]
  fn parse_get_paused_track_is_present_not_playing() {
    let json =
      r#"{"bundleIdentifier":"com.google.Chrome","playing":false,"title":"X","playbackRate":0}"#;
    let state = parse_get(json).unwrap();
    assert!(state.present);
    assert!(!state.is_playing);
    assert_eq!(state.playback_rate, Some(0.0));
  }

  #[test]
  fn parse_get_maps_like_fields_when_present() {
    // Apple Music catalog tracks report these; local files / NetEase omit them.
    let json = r#"{"bundleIdentifier":"com.apple.Music","playing":true,"title":"X",
      "supportsIsLiked":true,"isLiked":true}"#;
    let state = parse_get(json).unwrap();
    assert_eq!(state.supports_like, Some(true));
    assert_eq!(state.is_liked, Some(true));
  }

  #[test]
  fn parse_get_like_fields_default_to_none_when_absent() {
    let json = r#"{"bundleIdentifier":"com.netease.163music","playing":true,"title":"X"}"#;
    let state = parse_get(json).unwrap();
    assert_eq!(state.supports_like, None);
    assert_eq!(state.is_liked, None);
  }

  #[test]
  fn parse_get_rejects_garbage() {
    assert!(parse_get("not json").is_err());
  }

  #[test]
  fn media_command_ids_match_adapter_table() {
    assert_eq!(MediaCommand::Play.command_id(), 0);
    assert_eq!(MediaCommand::Pause.command_id(), 1);
    assert_eq!(MediaCommand::TogglePlayPause.command_id(), 2);
    assert_eq!(MediaCommand::NextTrack.command_id(), 4);
    assert_eq!(MediaCommand::PreviousTrack.command_id(), 5);
  }
}
