use auv_driver::{OperationDisturbance, OperationNamespace};

use crate::{
  CommandGroup, InvokeCommand, InvokeContext, InvokeDriverDispatch,
  arg::NO_ARGS,
  command::{MEDIA_TRANSPORT, NONE},
  default_driver_dispatch, invoke_command,
};

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
  driver = "macos.desktop",
  operation = "media_control_now_playing",
  args = NO_ARGS,
  disturbance = NONE,
  max_disturbance = OperationDisturbance::None,
  artifacts = ["now-playing-v0"],
  signals = ["mediaControl.present", "mediaControl.title", "mediaControl.isPlaying", "mediaControl.sourceBundleId", "mediaControl.verification"],
  verification = "read-only; verified means the backend returned a structured state",
)]
pub fn media_control_now_playing(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "mediaControl.play",
  group = "mediaControl",
  operation_namespace = OperationNamespace::Action,
  summary = "Send a generic system media play command and read now-playing state for verification.",
  driver = "macos.desktop",
  operation = "media_control_play",
  args = NO_ARGS,
  disturbance = MEDIA_TRANSPORT,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["now-playing-v0"],
  signals = ["mediaControl.present", "mediaControl.title", "mediaControl.isPlaying", "mediaControl.sourceBundleId", "mediaControl.verification"],
  verification = "verified when playback is present and playing after the command; otherwise inconclusive",
)]
pub fn media_control_play(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "mediaControl.pause",
  group = "mediaControl",
  operation_namespace = OperationNamespace::Action,
  summary = "Send a generic system media pause command and read now-playing state for verification.",
  driver = "macos.desktop",
  operation = "media_control_pause",
  args = NO_ARGS,
  disturbance = MEDIA_TRANSPORT,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["now-playing-v0"],
  signals = ["mediaControl.present", "mediaControl.title", "mediaControl.isPlaying", "mediaControl.sourceBundleId", "mediaControl.verification"],
  verification = "verified when playback is present and paused after the command; otherwise inconclusive",
)]
pub fn media_control_pause(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "mediaControl.togglePlayPause",
  group = "mediaControl",
  operation_namespace = OperationNamespace::Action,
  summary = "Send a generic system media play/pause toggle command and compare now-playing state before and after.",
  driver = "macos.desktop",
  operation = "media_control_toggle_play_pause",
  args = NO_ARGS,
  disturbance = MEDIA_TRANSPORT,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["now-playing-v0"],
  signals = ["mediaControl.present", "mediaControl.title", "mediaControl.isPlaying", "mediaControl.sourceBundleId", "mediaControl.verification"],
  verification = "verified when observed playback state changes after the command; otherwise inconclusive",
)]
pub fn media_control_toggle_play_pause(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "mediaControl.next",
  group = "mediaControl",
  operation_namespace = OperationNamespace::Action,
  summary = "Send a generic system media next-track command and compare now-playing identity before and after.",
  driver = "macos.desktop",
  operation = "media_control_next",
  args = NO_ARGS,
  disturbance = MEDIA_TRANSPORT,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["now-playing-v0"],
  signals = ["mediaControl.present", "mediaControl.title", "mediaControl.isPlaying", "mediaControl.sourceBundleId", "mediaControl.verification"],
  verification = "verified when content identity or title changes after the command; otherwise inconclusive",
)]
pub fn media_control_next(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}

#[invoke_command(
  id = "mediaControl.previous",
  group = "mediaControl",
  operation_namespace = OperationNamespace::Action,
  summary = "Send a generic system media previous-track command and compare now-playing identity before and after.",
  driver = "macos.desktop",
  operation = "media_control_previous",
  args = NO_ARGS,
  disturbance = MEDIA_TRANSPORT,
  max_disturbance = OperationDisturbance::Keyboard,
  artifacts = ["now-playing-v0"],
  signals = ["mediaControl.present", "mediaControl.title", "mediaControl.isPlaying", "mediaControl.sourceBundleId", "mediaControl.verification"],
  verification = "verified when content identity or title changes after the command; otherwise inconclusive",
)]
pub fn media_control_previous(
  context: InvokeContext<'_>,
  command: &InvokeCommand,
) -> InvokeDriverDispatch {
  default_driver_dispatch(context, command)
}
