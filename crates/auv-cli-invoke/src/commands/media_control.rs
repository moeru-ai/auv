use crate::{CommandGroup, InvokeCommandInput, InvokeCommandResult, arg::NO_ARGS, invoke_command};

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
fn media_control_now_playing(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-media-control-typed-api): media report population is deferred
  // with command enablement; Task 4 cannot activate this previously deferred
  // command before a typed media control API is accepted.
  Err(
    "mediaControl.nowPlaying requires a typed media control API moved out of the root driver and backed by auv_media_macos"
      .to_string(),
  )
}

#[invoke_command(
  id = "mediaControl.play",
  group = "mediaControl",
  summary = "Send a generic system media play command and read now-playing state for verification.",
  args = NO_ARGS,
)]
fn media_control_play(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-media-control-typed-api): media control still depends on root
  // driver/media crate routing. Move a typed API out of the root driver and
  // back it with `auv_media_macos` before enabling this invoke command.
  Err(
    "mediaControl.play requires a typed media control API moved out of the root driver and backed by auv_media_macos"
      .to_string(),
  )
}

#[invoke_command(
  id = "mediaControl.pause",
  group = "mediaControl",
  summary = "Send a generic system media pause command and read now-playing state for verification.",
  args = NO_ARGS,
)]
fn media_control_pause(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-media-control-typed-api): media control still depends on root
  // driver/media crate routing. Move a typed API out of the root driver and
  // back it with `auv_media_macos` before enabling this invoke command.
  Err(
    "mediaControl.pause requires a typed media control API moved out of the root driver and backed by auv_media_macos"
      .to_string(),
  )
}

#[invoke_command(
  id = "mediaControl.togglePlayPause",
  group = "mediaControl",
  summary = "Send a generic system media play/pause toggle command and compare now-playing state before and after.",
  args = NO_ARGS,
)]
fn media_control_toggle_play_pause(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-media-control-typed-api): media control still depends on root
  // driver/media crate routing. Move a typed API out of the root driver and
  // back it with `auv_media_macos` before enabling this invoke command.
  Err(
    "mediaControl.togglePlayPause requires a typed media control API moved out of the root driver and backed by auv_media_macos"
      .to_string(),
  )
}

#[invoke_command(
  id = "mediaControl.next",
  group = "mediaControl",
  summary = "Send a generic system media next-track command and compare now-playing identity before and after.",
  args = NO_ARGS,
)]
fn media_control_next(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-media-control-typed-api): media control still depends on root
  // driver/media crate routing. Move a typed API out of the root driver and
  // back it with `auv_media_macos` before enabling this invoke command.
  Err(
    "mediaControl.next requires a typed media control API moved out of the root driver and backed by auv_media_macos"
      .to_string(),
  )
}

#[invoke_command(
  id = "mediaControl.previous",
  group = "mediaControl",
  summary = "Send a generic system media previous-track command and compare now-playing identity before and after.",
  args = NO_ARGS,
)]
fn media_control_previous(_input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  // TODO(invoke-media-control-typed-api): media control still depends on root
  // driver/media crate routing. Move a typed API out of the root driver and
  // back it with `auv_media_macos` before enabling this invoke command.
  Err(
    "mediaControl.previous requires a typed media control API moved out of the root driver and backed by auv_media_macos"
      .to_string(),
  )
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use super::*;

  fn input<'a>(
    command_id: &'static str,
    inputs: &'a BTreeMap<String, String>,
  ) -> InvokeCommandInput<'a> {
    InvokeCommandInput {
      command_id,
      target_application_id: None,
      inputs,
      dry_run: false,
    }
  }

  #[test]
  fn media_control_commands_report_typed_api_migration_gap() {
    let inputs = BTreeMap::new();

    for (command_id, invoke) in [
      (
        "mediaControl.nowPlaying",
        media_control_now_playing as fn(InvokeCommandInput<'_>) -> InvokeCommandResult,
      ),
      (
        "mediaControl.play",
        media_control_play as fn(InvokeCommandInput<'_>) -> InvokeCommandResult,
      ),
      (
        "mediaControl.pause",
        media_control_pause as fn(InvokeCommandInput<'_>) -> InvokeCommandResult,
      ),
      (
        "mediaControl.togglePlayPause",
        media_control_toggle_play_pause as fn(InvokeCommandInput<'_>) -> InvokeCommandResult,
      ),
      (
        "mediaControl.next",
        media_control_next as fn(InvokeCommandInput<'_>) -> InvokeCommandResult,
      ),
      (
        "mediaControl.previous",
        media_control_previous as fn(InvokeCommandInput<'_>) -> InvokeCommandResult,
      ),
    ] {
      let error =
        invoke(input(command_id, &inputs)).expect_err("command should not route to root driver");

      assert!(
        error.contains("typed media control API"),
        "{command_id} returned unclear error: {error}"
      );
    }
  }
}
