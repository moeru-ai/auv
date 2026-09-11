//! The agent-facing now-playing contract (`now-playing-v0`), owned by this
//! crate so the `auv-now-playing` binary and any embedding CLI (e.g. the
//! `auv-netease-music now-playing` subcommand) emit one identical shape.

use crate::{MediaCommand, NowPlayingState};

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

/// Observable result of one media transport command.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MediaControlOutcome {
  pub command: &'static str,
  pub before: NowPlayingOutput,
  pub after: NowPlayingOutput,
  pub verified: bool,
}

impl MediaControlOutcome {
  pub(crate) fn new(command: MediaCommand, before: &NowPlayingState, after: &NowPlayingState) -> Self {
    Self {
      command: command.label(),
      before: build_now_playing_output(before),
      after: build_now_playing_output(after),
      verified: command_verified(command, before, after),
    }
  }
}

fn command_verified(command: MediaCommand, before: &NowPlayingState, after: &NowPlayingState) -> bool {
  match command {
    MediaCommand::Play => after.present && after.is_playing,
    MediaCommand::Pause => after.present && !after.is_playing,
    MediaCommand::TogglePlayPause => before.present && after.present && before.is_playing != after.is_playing,
    MediaCommand::NextTrack | MediaCommand::PreviousTrack => {
      before.present
        && after.present
        && (before.content_item_id != after.content_item_id || before.title != after.title || before.artist != after.artist)
    }
  }
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
  if let Some(artist) = state.artist.as_deref().filter(|text| !text.is_empty()) {
    line.push_str(&format!(" — {artist}"));
  }
  if let Some(album) = state.album.as_deref().filter(|text| !text.is_empty()) {
    line.push_str(&format!(" [{album}]"));
  }
  if let Some(bundle) = state.source_bundle_id.as_deref().filter(|text| !text.is_empty()) {
    line.push_str(&format!("  ({bundle})"));
  }
  if state.is_liked == Some(true) {
    line.push_str("  ♥");
  }
  line
}
