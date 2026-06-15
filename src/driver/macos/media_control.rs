use std::collections::BTreeMap;

use auv_media_macos::{MediaCommand, NowPlayingState};

use super::support::artifacts::build_text_artifact;
use crate::model::{AuvResult, DriverCall, DriverResponse};

pub(crate) fn media_control_now_playing(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let state = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  response_for_state("mediaControl.nowPlaying", state, "verified")
}

pub(crate) fn media_control_play(_call: &DriverCall) -> AuvResult<DriverResponse> {
  send_and_verify_playback(MediaCommand::Play, "mediaControl.play", Some(true))
}

pub(crate) fn media_control_pause(_call: &DriverCall) -> AuvResult<DriverResponse> {
  send_and_verify_playback(MediaCommand::Pause, "mediaControl.pause", Some(false))
}

pub(crate) fn media_control_toggle_play_pause(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let before = auv_media_macos::now_playing().ok();
  auv_media_macos::send_command(MediaCommand::TogglePlayPause)
    .map_err(|error| error.to_string())?;
  let after = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  let verification = before
    .as_ref()
    .map(|before| verify_toggle_transition(before, &after))
    .unwrap_or("inconclusive");
  response_for_state("mediaControl.togglePlayPause", after, verification)
}

pub(crate) fn media_control_next(_call: &DriverCall) -> AuvResult<DriverResponse> {
  send_and_verify_track_transition(MediaCommand::NextTrack, "mediaControl.next")
}

pub(crate) fn media_control_previous(_call: &DriverCall) -> AuvResult<DriverResponse> {
  send_and_verify_track_transition(MediaCommand::PreviousTrack, "mediaControl.previous")
}

fn send_and_verify_playback(
  command: MediaCommand,
  operation_id: &str,
  expected_is_playing: Option<bool>,
) -> AuvResult<DriverResponse> {
  auv_media_macos::send_command(command).map_err(|error| error.to_string())?;
  let after = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  let verification = verify_playback_state(expected_is_playing, &after);
  response_for_state(operation_id, after, verification)
}

fn send_and_verify_track_transition(
  command: MediaCommand,
  operation_id: &str,
) -> AuvResult<DriverResponse> {
  let before = auv_media_macos::now_playing().ok();
  auv_media_macos::send_command(command).map_err(|error| error.to_string())?;
  let after = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  let verification = before
    .as_ref()
    .map(|before| verify_track_transition(before, &after))
    .unwrap_or("inconclusive");
  response_for_state(operation_id, after, verification)
}

fn verify_playback_state(expected: Option<bool>, after: &NowPlayingState) -> &'static str {
  match expected {
    Some(expected) if after.present && after.is_playing == expected => "verified",
    Some(_) | None => "inconclusive",
  }
}

fn verify_toggle_transition(before: &NowPlayingState, after: &NowPlayingState) -> &'static str {
  if before.present && after.present && before.is_playing != after.is_playing {
    "verified"
  } else {
    "inconclusive"
  }
}

fn verify_track_transition(before: &NowPlayingState, after: &NowPlayingState) -> &'static str {
  if !after.present {
    return "inconclusive";
  }
  if before.content_item_id.is_some()
    && after.content_item_id.is_some()
    && before.content_item_id != after.content_item_id
  {
    return "verified";
  }
  if before.title.is_some() && after.title.is_some() && before.title != after.title {
    return "verified";
  }
  "inconclusive"
}

fn response_for_state(
  operation_id: &str,
  state: NowPlayingState,
  verification: &str,
) -> AuvResult<DriverResponse> {
  let json =
    serde_json::to_string_pretty(&auv_media_macos::output::build_now_playing_output(&state))
      .unwrap_or_else(|error| format!(r#"{{"error":"failed to encode now-playing: {error}"}}"#));
  let artifact = build_text_artifact(
    "now-playing-v0",
    "json",
    "now-playing",
    json,
    "System media now-playing state from auv-media-macos.",
  )?;
  let mut signals = BTreeMap::new();
  signals.insert(
    "mediaControl.present".to_string(),
    state.present.to_string(),
  );
  signals.insert(
    "mediaControl.isPlaying".to_string(),
    state.is_playing.to_string(),
  );
  signals.insert(
    "mediaControl.verification".to_string(),
    verification.to_string(),
  );
  if let Some(title) = state.title.as_deref() {
    signals.insert("mediaControl.title".to_string(), title.to_string());
  }
  if let Some(bundle_id) = state.source_bundle_id.as_deref() {
    signals.insert(
      "mediaControl.sourceBundleId".to_string(),
      bundle_id.to_string(),
    );
  }
  Ok(DriverResponse {
    summary: format!("{operation_id}: verification={verification}"),
    backend: Some("auv-media-macos".to_string()),
    signals,
    notes: Vec::new(),
    artifacts: vec![artifact],
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  fn state(title: &str, is_playing: bool, elapsed_seconds: f64) -> NowPlayingState {
    NowPlayingState {
      present: true,
      title: Some(title.to_string()),
      is_playing,
      elapsed_seconds: Some(elapsed_seconds),
      ..Default::default()
    }
  }

  #[test]
  fn playback_verification_reports_verified_state() {
    assert_eq!(
      verify_playback_state(Some(false), &state("Song", false, 10.0)),
      "verified"
    );
  }

  #[test]
  fn playback_verification_reports_inconclusive_when_state_is_not_target() {
    assert_eq!(
      verify_playback_state(Some(false), &state("Song", true, 10.0)),
      "inconclusive"
    );
  }

  #[test]
  fn toggle_verification_accepts_playing_state_change() {
    let before = state("Song", true, 10.0);
    let after = state("Song", false, 10.5);

    assert_eq!(verify_toggle_transition(&before, &after), "verified");
  }

  #[test]
  fn track_transition_verification_accepts_identity_change() {
    let before = state("First", true, 40.0);
    let after = state("Second", true, 2.0);

    assert_eq!(verify_track_transition(&before, &after), "verified");
  }

  #[test]
  fn track_transition_verification_reports_inconclusive_without_change() {
    let before = state("First", true, 40.0);
    let after = state("First", true, 40.5);

    assert_eq!(verify_track_transition(&before, &after), "inconclusive");
  }

  #[test]
  fn track_transition_verification_requires_identity_or_title_change() {
    let before = state("First", true, 40.0);
    let after = state("First", true, 2.0);

    assert_eq!(verify_track_transition(&before, &after), "inconclusive");
  }
}
