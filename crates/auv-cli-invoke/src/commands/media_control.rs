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
  summary = "Read structured now-playing media state from the desktop backend.",
  args = NO_ARGS,
)]
async fn media_control_now_playing(_input: InvokeCommandInput) -> InvokeCommandResult {
  read_now_playing().await?;
  Ok(InvokeCommandOutput::new("read now-playing state"))
}

pub async fn read_now_playing() -> Result<(), String> {
  // TODO(invoke-media-control-typed-api): media report population is deferred
  // with command enablement; Task 4 cannot activate this previously deferred
  // command before a typed media control API is accepted.
  Err("mediaControl.nowPlaying requires a typed media control API moved out of the root driver and backed by auv_media_macos".to_string())
}

#[invoke_command(
  id = "mediaControl.play",
  group = "mediaControl",
  summary = "Send a generic system media play command and read now-playing state for verification.",
  args = NO_ARGS,
)]
async fn media_control_play(_input: InvokeCommandInput) -> InvokeCommandResult {
  play_media().await?;
  Ok(InvokeCommandOutput::new("played media"))
}

pub async fn play_media() -> Result<(), String> {
  // TODO(invoke-media-control-typed-api): media control still depends on root
  // driver/media crate routing. Move a typed API out of the root driver and
  // back it with `auv_media_macos` before enabling this invoke command.
  Err("mediaControl.play requires a typed media control API moved out of the root driver and backed by auv_media_macos".to_string())
}

#[invoke_command(
  id = "mediaControl.pause",
  group = "mediaControl",
  summary = "Send a generic system media pause command and read now-playing state for verification.",
  args = NO_ARGS,
)]
async fn media_control_pause(_input: InvokeCommandInput) -> InvokeCommandResult {
  pause_media().await?;
  Ok(InvokeCommandOutput::new("paused media"))
}

pub async fn pause_media() -> Result<(), String> {
  // TODO(invoke-media-control-typed-api): media control still depends on root
  // driver/media crate routing. Move a typed API out of the root driver and
  // back it with `auv_media_macos` before enabling this invoke command.
  Err("mediaControl.pause requires a typed media control API moved out of the root driver and backed by auv_media_macos".to_string())
}

#[invoke_command(
  id = "mediaControl.togglePlayPause",
  group = "mediaControl",
  summary = "Send a generic system media play/pause toggle command and compare now-playing state before and after.",
  args = NO_ARGS,
)]
async fn media_control_toggle_play_pause(_input: InvokeCommandInput) -> InvokeCommandResult {
  toggle_play_pause().await?;
  Ok(InvokeCommandOutput::new("toggled media playback"))
}

pub async fn toggle_play_pause() -> Result<(), String> {
  // TODO(invoke-media-control-typed-api): media control still depends on root
  // driver/media crate routing. Move a typed API out of the root driver and
  // back it with `auv_media_macos` before enabling this invoke command.
  Err(
    "mediaControl.togglePlayPause requires a typed media control API moved out of the root driver and backed by auv_media_macos".to_string(),
  )
}

#[invoke_command(
  id = "mediaControl.next",
  group = "mediaControl",
  summary = "Send a generic system media next-track command and compare now-playing identity before and after.",
  args = NO_ARGS,
)]
async fn media_control_next(_input: InvokeCommandInput) -> InvokeCommandResult {
  next_track().await?;
  Ok(InvokeCommandOutput::new("advanced to next track"))
}

pub async fn next_track() -> Result<(), String> {
  // TODO(invoke-media-control-typed-api): media control still depends on root
  // driver/media crate routing. Move a typed API out of the root driver and
  // back it with `auv_media_macos` before enabling this invoke command.
  Err("mediaControl.next requires a typed media control API moved out of the root driver and backed by auv_media_macos".to_string())
}

#[invoke_command(
  id = "mediaControl.previous",
  group = "mediaControl",
  summary = "Send a generic system media previous-track command and compare now-playing identity before and after.",
  args = NO_ARGS,
)]
async fn media_control_previous(_input: InvokeCommandInput) -> InvokeCommandResult {
  previous_track().await?;
  Ok(InvokeCommandOutput::new("returned to previous track"))
}

pub async fn previous_track() -> Result<(), String> {
  // TODO(invoke-media-control-typed-api): media control still depends on root
  // driver/media crate routing. Move a typed API out of the root driver and
  // back it with `auv_media_macos` before enabling this invoke command.
  Err("mediaControl.previous requires a typed media control API moved out of the root driver and backed by auv_media_macos".to_string())
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use super::*;

  fn input(command_id: &str, inputs: &BTreeMap<String, String>) -> InvokeCommandInput {
    InvokeCommandInput {
      command_id: command_id.to_string(),
      target_application_id: None,
      inputs: inputs.clone(),
      dry_run: false,
      cancellation: crate::InvokeCancellation::new(),
    }
  }

  #[test]
  fn media_control_commands_report_typed_api_migration_gap() {
    let inputs = BTreeMap::new();

    for command in [
      media_control_now_playing_invoke_command(),
      media_control_play_invoke_command(),
      media_control_pause_invoke_command(),
      media_control_toggle_play_pause_invoke_command(),
      media_control_next_invoke_command(),
      media_control_previous_invoke_command(),
    ] {
      let command_id = command.id;
      let error =
        futures_executor::block_on(command.invoke(input(command_id, &inputs))).expect_err("command should not route to root driver");

      assert!(error.contains("typed media control API"), "{command_id} returned unclear error: {error}");
    }
  }
}
