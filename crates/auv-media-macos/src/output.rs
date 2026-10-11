//! The agent-facing now-playing contract (`now-playing-v0`). The contract itself is the shared
//! media contract in `auv-driver-common`, so the macOS and Windows providers emit one shape;
//! it is re-exported here so the `auv-now-playing` binary and any embedding CLI (e.g. the
//! `auv-netease-music now-playing` subcommand) keep their paths. The human summary is this
//! crate's own rendering.

pub use auv_driver_common::media::{MediaControlOutcome, NowPlayingOutput, SCHEMA_VERSION, build_now_playing_output};

use crate::NowPlayingState;

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
