use crate::{CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, arg::NO_ARGS, invoke_command};

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
  description = "Read structured now-playing media state from the desktop backend.",
  args = NO_ARGS,
)]
async fn media_control_now_playing(_input: InvokeCommandInput) -> InvokeCommandResult {
  InvokeCommandOutput::from_result(&read_now_playing().await?)
}

pub async fn read_now_playing() -> Result<auv_media_macos::output::NowPlayingOutput, String> {
  let state = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  Ok(auv_media_macos::output::build_now_playing_output(&state))
}

#[invoke_command(
  id = "mediaControl.play",
  group = "mediaControl",
  description = "Send a generic system media play command and read now-playing state for verification.",
  args = NO_ARGS,
)]
async fn media_control_play(_input: InvokeCommandInput) -> InvokeCommandResult {
  InvokeCommandOutput::from_result(&control_media(auv_media_macos::MediaCommand::Play).await?)
}

#[invoke_command(
  id = "mediaControl.pause",
  group = "mediaControl",
  description = "Send a generic system media pause command and read now-playing state for verification.",
  args = NO_ARGS,
)]
async fn media_control_pause(_input: InvokeCommandInput) -> InvokeCommandResult {
  InvokeCommandOutput::from_result(&control_media(auv_media_macos::MediaCommand::Pause).await?)
}

#[invoke_command(
  id = "mediaControl.togglePlayPause",
  group = "mediaControl",
  description = "Send a generic system media play/pause toggle command and compare now-playing state before and after.",
  args = NO_ARGS,
)]
async fn media_control_toggle_play_pause(_input: InvokeCommandInput) -> InvokeCommandResult {
  InvokeCommandOutput::from_result(&control_media(auv_media_macos::MediaCommand::TogglePlayPause).await?)
}

#[invoke_command(
  id = "mediaControl.next",
  group = "mediaControl",
  description = "Send a generic system media next-track command and compare now-playing identity before and after.",
  args = NO_ARGS,
)]
async fn media_control_next(_input: InvokeCommandInput) -> InvokeCommandResult {
  InvokeCommandOutput::from_result(&control_media(auv_media_macos::MediaCommand::NextTrack).await?)
}

#[invoke_command(
  id = "mediaControl.previous",
  group = "mediaControl",
  description = "Send a generic system media previous-track command and compare now-playing identity before and after.",
  args = NO_ARGS,
)]
async fn media_control_previous(_input: InvokeCommandInput) -> InvokeCommandResult {
  InvokeCommandOutput::from_result(&control_media(auv_media_macos::MediaCommand::PreviousTrack).await?)
}

pub async fn control_media(command: auv_media_macos::MediaCommand) -> Result<auv_media_macos::output::MediaControlOutcome, String> {
  auv_media_macos::control(command).map_err(|error| error.to_string())
}
