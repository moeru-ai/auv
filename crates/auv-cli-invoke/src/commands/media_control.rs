use auv_driver::media::{MediaCommand, MediaControlOutcome, NowPlayingOutput, NowPlayingState, build_now_playing_output};

use crate::{CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, invoke_command};
use clap::Args;

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke mediaControl.nowPlaying")]
struct NowPlayingArgs {}

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
  support = { macos: Supported, linux: Unsupported, windows: Partial("no duration, elapsed time or playback rate") },
  runner = Direct,
  group = "mediaControl",
  description = "Read structured now-playing media state from the desktop backend.",
  input = NowPlayingArgs,
)]
async fn media_control_now_playing(input: InvokeCommandInput, _args: NowPlayingArgs) -> InvokeCommandResult {
  if input.target.is_some() {
    return Err("mediaControl.nowPlaying cannot use --target; the now-playing state is system-wide".to_string());
  }
  let result = read_now_playing().await?;
  Ok(InvokeCommandOutput::from_result(&result)?.with_report(now_playing_report(&result)))
}

pub fn now_playing_state_output(state: &NowPlayingState) -> InvokeCommandResult {
  let result = build_now_playing_output(state);
  Ok(InvokeCommandOutput::from_result(&result)?.with_report(now_playing_report(&result)))
}

/// Reads the system now-playing state from this platform's media provider: MediaRemote on macOS,
/// the current SMTC session on Windows. Linux has none and reports unsupported.
pub async fn read_now_playing() -> Result<NowPlayingOutput, String> {
  #[cfg(target_os = "windows")]
  let state = auv_driver_windows::media::now_playing().map_err(|error| error.to_string())?;
  // TODO(invoke-driver-failure-migration): on Linux, `MediaError::Unsupported` reaches the caller
  // of this and `control_media` as `command_failed`, not `unsupported`. Both helpers also back the
  // Runner's media service, so typing them belongs with the Runner route's refusals
  // (`TODO(invoke-support-runner-route)`), not with the local-handler refusals.
  #[cfg(not(target_os = "windows"))]
  let state = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  Ok(build_now_playing_output(&state))
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke mediaControl.play")]
struct PlayArgs {}

#[invoke_command(
  id = "mediaControl.play",
  support = { macos: Supported, linux: Unsupported, windows: Supported },
  runner = Direct,
  group = "mediaControl",
  description = "Send a generic system media play command and read now-playing state for verification.",
  input = PlayArgs,
)]
async fn media_control_play(input: InvokeCommandInput, _args: PlayArgs) -> InvokeCommandResult {
  reject_media_target(&input, "mediaControl.play")?;
  media_control_output(&control_media(MediaCommand::Play).await?)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke mediaControl.pause")]
struct PauseArgs {}

#[invoke_command(
  id = "mediaControl.pause",
  support = { macos: Supported, linux: Unsupported, windows: Supported },
  runner = Direct,
  group = "mediaControl",
  description = "Send a generic system media pause command and read now-playing state for verification.",
  input = PauseArgs,
)]
async fn media_control_pause(input: InvokeCommandInput, _args: PauseArgs) -> InvokeCommandResult {
  reject_media_target(&input, "mediaControl.pause")?;
  media_control_output(&control_media(MediaCommand::Pause).await?)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke mediaControl.togglePlayPause")]
struct TogglePlayPauseArgs {}

#[invoke_command(
  id = "mediaControl.togglePlayPause",
  support = { macos: Supported, linux: Unsupported, windows: Supported },
  runner = Direct,
  group = "mediaControl",
  description = "Send a generic system media play/pause toggle command and compare now-playing state before and after.",
  input = TogglePlayPauseArgs,
)]
async fn media_control_toggle_play_pause(input: InvokeCommandInput, _args: TogglePlayPauseArgs) -> InvokeCommandResult {
  reject_media_target(&input, "mediaControl.togglePlayPause")?;
  media_control_output(&control_media(MediaCommand::TogglePlayPause).await?)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke mediaControl.next")]
struct NextArgs {}

#[invoke_command(
  id = "mediaControl.next",
  support = { macos: Supported, linux: Unsupported, windows: Supported },
  runner = Direct,
  group = "mediaControl",
  description = "Send a generic system media next-track command and compare now-playing identity before and after.",
  input = NextArgs,
)]
async fn media_control_next(input: InvokeCommandInput, _args: NextArgs) -> InvokeCommandResult {
  reject_media_target(&input, "mediaControl.next")?;
  media_control_output(&control_media(MediaCommand::NextTrack).await?)
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke mediaControl.previous")]
struct PreviousArgs {}

#[invoke_command(
  id = "mediaControl.previous",
  support = { macos: Supported, linux: Unsupported, windows: Supported },
  runner = Direct,
  group = "mediaControl",
  description = "Send a generic system media previous-track command and compare now-playing identity before and after.",
  input = PreviousArgs,
)]
async fn media_control_previous(input: InvokeCommandInput, _args: PreviousArgs) -> InvokeCommandResult {
  reject_media_target(&input, "mediaControl.previous")?;
  media_control_output(&control_media(MediaCommand::PreviousTrack).await?)
}

/// Sends `command` through this platform's media provider and reports the state before and after.
pub async fn control_media(command: MediaCommand) -> Result<MediaControlOutcome, String> {
  #[cfg(target_os = "windows")]
  let outcome = auv_driver_windows::media::control(command);
  #[cfg(not(target_os = "windows"))]
  let outcome = auv_media_macos::control(command);
  outcome.map_err(|error| error.to_string())
}

pub fn media_control_output(result: &MediaControlOutcome) -> InvokeCommandResult {
  Ok(InvokeCommandOutput::from_result(result)?.with_report(InvokeReport::new(
    vec![
      InvokeReportField::new("Command", result.command),
      InvokeReportField::new("Verified", if result.verified { "yes" } else { "no" }),
      InvokeReportField::new("Before", now_playing_summary(&result.before)),
      InvokeReportField::new("After", now_playing_summary(&result.after)),
    ],
    Vec::new(),
  )))
}

fn reject_media_target(input: &InvokeCommandInput, command_id: &str) -> Result<(), String> {
  if input.target.is_some() {
    return Err(format!("{command_id} cannot use --target; media controls are system-wide"));
  }
  Ok(())
}

fn now_playing_report(result: &NowPlayingOutput) -> InvokeReport {
  let mut fields = vec![InvokeReportField::new("State", playback_state(result))];
  if result.present {
    fields.push(InvokeReportField::new("Title", report_text(result.title.as_deref(), "(unknown title)")));
    push_optional_field(&mut fields, "Artist", result.artist.as_deref());
    push_optional_field(&mut fields, "Album", result.album.as_deref());
    push_optional_field(&mut fields, "Source", result.source_bundle_id.as_deref());
    if let Some(elapsed_seconds) = result.elapsed_seconds {
      fields.push(InvokeReportField::new("Elapsed", format_seconds(elapsed_seconds)));
    }
    if let Some(duration_seconds) = result.duration_seconds {
      fields.push(InvokeReportField::new("Duration", format_seconds(duration_seconds)));
    }
  }
  InvokeReport::new(fields, Vec::new())
}

fn now_playing_summary(result: &NowPlayingOutput) -> String {
  if !result.present {
    return "nothing playing".to_string();
  }

  let mut summary = format!("{}: {}", playback_state(result), report_text(result.title.as_deref(), "(unknown title)"));
  if let Some(artist) = non_empty(result.artist.as_deref()) {
    summary.push_str(" — ");
    summary.push_str(artist);
  }
  if let Some(source) = non_empty(result.source_bundle_id.as_deref()) {
    summary.push_str(" (");
    summary.push_str(source);
    summary.push(')');
  }
  summary
}

fn playback_state(result: &NowPlayingOutput) -> &'static str {
  if !result.present {
    "nothing playing"
  } else if result.is_playing {
    "playing"
  } else {
    "paused"
  }
}

fn push_optional_field(fields: &mut Vec<InvokeReportField>, label: &str, value: Option<&str>) {
  if let Some(value) = non_empty(value) {
    fields.push(InvokeReportField::new(label, value));
  }
}

fn report_text<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
  non_empty(value).unwrap_or(fallback)
}

fn non_empty(value: Option<&str>) -> Option<&str> {
  value.filter(|text| !text.trim().is_empty())
}

fn format_seconds(seconds: f64) -> String {
  format!("{seconds:.3} s")
}

#[cfg(test)]
#[path = "media_control_test.rs"]
mod tests;
