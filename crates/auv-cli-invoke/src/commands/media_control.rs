use crate::{CommandGroup, arg::NO_ARGS, invoke_command};

pub fn group() -> CommandGroup {
  CommandGroup::new("mediaControl", "MEDIA CONTROL")
    .command(media_control_now_playing_invoke_command())
    .command(media_control_play_invoke_command())
    .command(media_control_pause_invoke_command())
    .command(media_control_toggle_play_pause_invoke_command())
    .command(media_control_next_invoke_command())
    .command(media_control_previous_invoke_command())
}

#[invoke_command(
  id = "mediaControl.nowPlaying",
  group = "mediaControl",
  summary = "Read structured now-playing media state from the desktop backend.",
  args = NO_ARGS,
)]
fn media_control_now_playing() {}

#[invoke_command(
  id = "mediaControl.play",
  group = "mediaControl",
  summary = "Send a generic system media play command and read now-playing state for verification.",
  args = NO_ARGS,
)]
fn media_control_play() {}

#[invoke_command(
  id = "mediaControl.pause",
  group = "mediaControl",
  summary = "Send a generic system media pause command and read now-playing state for verification.",
  args = NO_ARGS,
)]
fn media_control_pause() {}

#[invoke_command(
  id = "mediaControl.togglePlayPause",
  group = "mediaControl",
  summary = "Send a generic system media play/pause toggle command and compare now-playing state before and after.",
  args = NO_ARGS,
)]
fn media_control_toggle_play_pause() {}

#[invoke_command(
  id = "mediaControl.next",
  group = "mediaControl",
  summary = "Send a generic system media next-track command and compare now-playing identity before and after.",
  args = NO_ARGS,
)]
fn media_control_next() {}

#[invoke_command(
  id = "mediaControl.previous",
  group = "mediaControl",
  summary = "Send a generic system media previous-track command and compare now-playing identity before and after.",
  args = NO_ARGS,
)]
fn media_control_previous() {}
