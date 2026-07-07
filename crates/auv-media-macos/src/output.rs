//! The agent-facing now-playing contract (`now-playing-v0`), owned by this
//! crate so the `auv-now-playing` binary and any embedding CLI (e.g. the
//! `auv-netease-music now-playing` subcommand) emit one identical shape.

use crate::NowPlayingState;

/// Stable schema identifier for the JSON output.
pub const SCHEMA_VERSION: &str = "now-playing-v0";

/// The stable JSON output object.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct NowPlayingOutput {
  pub schema_version: &'static str,
  pub present: bool,
  pub is_playing: bool,
  pub source_bundle_id: Option<String>,
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub duration_seconds: Option<f64>,
  pub elapsed_seconds: Option<f64>,
  pub playback_rate: Option<f64>,
  pub content_item_id: Option<String>,
  // Usually `null`: in practice only Apple Music catalog tracks report like
  // state — never NetEase or local files. See `NowPlayingState::supports_like`.
  pub supports_like: Option<bool>,
  pub is_liked: Option<bool>,
}

/// Build the versioned output object from a [`NowPlayingState`].
pub fn build_now_playing_output(state: &NowPlayingState) -> NowPlayingOutput {
  NowPlayingOutput {
    schema_version: SCHEMA_VERSION,
    present: state.present,
    is_playing: state.is_playing,
    source_bundle_id: state.source_bundle_id.clone(),
    title: state.title.clone(),
    artist: state.artist.clone(),
    album: state.album.clone(),
    duration_seconds: state.duration_seconds,
    elapsed_seconds: state.elapsed_seconds,
    playback_rate: state.playback_rate,
    content_item_id: state.content_item_id.clone(),
    supports_like: state.supports_like,
    is_liked: state.is_liked,
  }
}

/// Render a one-line human summary.
pub fn render_human_summary(state: &NowPlayingState) -> String {
  if !state.present {
    return "Nothing playing".to_string();
  }
  let marker = if state.is_playing { "▶" } else { "⏸" };
  let title = state.title.as_deref().unwrap_or("(unknown title)");
  let mut line = format!("{marker} {title}");
  if let Some(artist) = non_empty(state.artist.as_deref()) {
    line.push_str(&format!(" — {artist}"));
  }
  if let Some(album) = non_empty(state.album.as_deref()) {
    line.push_str(&format!(" [{album}]"));
  }
  if let Some(bundle) = non_empty(state.source_bundle_id.as_deref()) {
    line.push_str(&format!("  ({bundle})"));
  }
  if state.is_liked == Some(true) {
    line.push_str("  ♥");
  }
  line
}

fn non_empty(value: Option<&str>) -> Option<&str> {
  value.filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn playing_state() -> NowPlayingState {
    NowPlayingState {
      present: true,
      source_bundle_id: Some("com.netease.163music".to_string()),
      title: Some("Song".to_string()),
      artist: Some("Artist".to_string()),
      album: Some("Album".to_string()),
      duration_seconds: Some(200.0),
      elapsed_seconds: Some(12.5),
      playback_rate: Some(1.0),
      is_playing: true,
      content_item_id: Some("abc".to_string()),
      supports_like: Some(true),
      is_liked: Some(false),
    }
  }

  #[test]
  fn output_carries_schema_version_and_fields() {
    let output = build_now_playing_output(&playing_state());
    assert_eq!(output.schema_version, "now-playing-v0");
    assert!(output.present && output.is_playing);
    assert_eq!(output.source_bundle_id.as_deref(), Some("com.netease.163music"));
    let json = serde_json::to_string(&output).unwrap();
    assert!(json.contains("\"schema_version\":\"now-playing-v0\""));
    assert!(json.contains("\"title\":\"Song\""));
  }

  #[test]
  fn human_summary_playing() {
    let line = render_human_summary(&playing_state());
    assert_eq!(line, "▶ Song — Artist [Album]  (com.netease.163music)");
  }

  #[test]
  fn human_summary_paused_marker() {
    let mut state = playing_state();
    state.is_playing = false;
    assert!(render_human_summary(&state).starts_with("⏸ Song"));
  }

  #[test]
  fn human_summary_omits_empty_album_and_missing_artist() {
    let mut state = playing_state();
    state.artist = None;
    state.album = Some(String::new());
    let line = render_human_summary(&state);
    assert_eq!(line, "▶ Song  (com.netease.163music)");
  }

  #[test]
  fn human_summary_idle() {
    assert_eq!(render_human_summary(&NowPlayingState::default()), "Nothing playing");
  }

  #[test]
  fn human_summary_marks_liked_track() {
    let mut state = playing_state();
    state.is_liked = Some(true);
    assert!(render_human_summary(&state).ends_with("♥"));
  }
}
