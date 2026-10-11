use super::*;

fn playing(title: &str) -> NowPlayingState {
  NowPlayingState {
    present: true,
    is_playing: true,
    title: Some(title.to_string()),
    ..NowPlayingState::default()
  }
}

fn paused(title: &str) -> NowPlayingState {
  NowPlayingState {
    is_playing: false,
    ..playing(title)
  }
}

#[test]
fn play_and_pause_are_judged_by_the_state_after_the_command() {
  assert!(MediaControlOutcome::new(MediaCommand::Play, &paused("a"), &playing("a")).verified);
  assert!(MediaControlOutcome::new(MediaCommand::Play, &playing("a"), &playing("a")).verified);
  assert!(!MediaControlOutcome::new(MediaCommand::Play, &paused("a"), &paused("a")).verified);

  assert!(MediaControlOutcome::new(MediaCommand::Pause, &playing("a"), &paused("a")).verified);
  assert!(!MediaControlOutcome::new(MediaCommand::Pause, &playing("a"), &playing("a")).verified);
}

#[test]
fn a_command_is_not_verified_when_nothing_is_playing_after_it() {
  let idle = NowPlayingState::default();
  assert!(!MediaControlOutcome::new(MediaCommand::Pause, &playing("a"), &idle).verified);
  assert!(!MediaControlOutcome::new(MediaCommand::TogglePlayPause, &playing("a"), &idle).verified);
  assert!(!MediaControlOutcome::new(MediaCommand::NextTrack, &playing("a"), &idle).verified);
}

#[test]
fn a_toggle_needs_the_playing_state_to_change() {
  assert!(MediaControlOutcome::new(MediaCommand::TogglePlayPause, &playing("a"), &paused("a")).verified);
  assert!(MediaControlOutcome::new(MediaCommand::TogglePlayPause, &paused("a"), &playing("a")).verified);
  assert!(!MediaControlOutcome::new(MediaCommand::TogglePlayPause, &playing("a"), &playing("a")).verified);
}

#[test]
fn a_track_skip_needs_the_track_identity_to_change() {
  assert!(MediaControlOutcome::new(MediaCommand::NextTrack, &playing("a"), &playing("b")).verified);
  assert!(MediaControlOutcome::new(MediaCommand::PreviousTrack, &playing("b"), &playing("a")).verified);
  assert!(!MediaControlOutcome::new(MediaCommand::NextTrack, &playing("a"), &playing("a")).verified);

  let other_artist = NowPlayingState {
    artist: Some("someone else".to_string()),
    ..playing("a")
  };
  assert!(MediaControlOutcome::new(MediaCommand::NextTrack, &playing("a"), &other_artist).verified);
}

#[test]
fn the_outcome_carries_the_command_label_and_both_states_in_the_v0_schema() {
  let outcome = MediaControlOutcome::new(MediaCommand::NextTrack, &playing("a"), &paused("b"));
  assert_eq!(outcome.command, "next");
  assert_eq!(outcome.before.schema_version, SCHEMA_VERSION);
  assert_eq!(outcome.before.title.as_deref(), Some("a"));
  assert_eq!(outcome.after.title.as_deref(), Some("b"));
  assert!(!outcome.after.is_playing);
}
