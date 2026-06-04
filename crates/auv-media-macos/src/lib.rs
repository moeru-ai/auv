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

mod error;

pub use error::MediaError;

/// A structured snapshot of the system now-playing state.
#[derive(Clone, Debug, PartialEq)]
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
}

impl NowPlayingState {
  /// The idle state: nothing owns the now-playing slot.
  fn idle() -> Self {
    NowPlayingState {
      present: false,
      source_bundle_id: None,
      title: None,
      artist: None,
      album: None,
      duration_seconds: None,
      elapsed_seconds: None,
      playback_rate: None,
      is_playing: false,
      content_item_id: None,
    }
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
  playback_rate: Option<f64>,
  content_item_identifier: Option<String>,
}

/// Parse the adapter's `get` output into a [`NowPlayingState`]. Pure and
/// platform-independent so it is unit-testable without macOS or perl.
fn parse_get(json: &str) -> Result<NowPlayingState, MediaError> {
  let parsed: Option<AdapterGet> = serde_json::from_str(json.trim())
    .map_err(|error| MediaError::native(format!("invalid adapter JSON: {error}"), None))?;
  let Some(item) = parsed else {
    return Ok(NowPlayingState::idle());
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
    assert_eq!(state.source_bundle_id.as_deref(), Some("com.netease.163music"));
    assert_eq!(state.title.as_deref(), Some("Song"));
    assert_eq!(state.duration_seconds, Some(200.0));
    assert_eq!(state.elapsed_seconds, Some(12.5));
    assert_eq!(state.playback_rate, Some(1.0));
    assert_eq!(state.content_item_id.as_deref(), Some("abc"));
  }

  #[test]
  fn parse_get_paused_track_is_present_not_playing() {
    let json = r#"{"bundleIdentifier":"com.google.Chrome","playing":false,"title":"X","playbackRate":0}"#;
    let state = parse_get(json).unwrap();
    assert!(state.present);
    assert!(!state.is_playing);
    assert_eq!(state.playback_rate, Some(0.0));
  }

  #[test]
  fn parse_get_rejects_garbage() {
    assert!(parse_get("not json").is_err());
  }
}
