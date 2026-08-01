use auv_media_macos::output::{MediaControlOutcome, NowPlayingOutput, SCHEMA_VERSION};
use auv_tracing::RunId;

use super::*;
use crate::{InvokeOutputOptions, InvokeResult};

fn now_playing(is_playing: bool, title: &str) -> NowPlayingOutput {
  NowPlayingOutput {
    schema_version: SCHEMA_VERSION,
    present: true,
    is_playing,
    source_bundle_id: Some("com.apple.Music".to_string()),
    title: Some(title.to_string()),
    artist: Some("The Artist".to_string()),
    album: Some("The Album".to_string()),
    duration_seconds: Some(245.5),
    elapsed_seconds: Some(61.25),
    playback_rate: Some(if is_playing { 1.0 } else { 0.0 }),
    content_item_id: Some("track-42".to_string()),
    supports_like: Some(true),
    is_liked: Some(false),
  }
}

#[test]
fn now_playing_human_output_exposes_the_current_media_state() {
  let result = now_playing(true, "Current Song");
  let output =
    InvokeCommandOutput::from_result(&result).expect("now-playing output should serialize").with_report(now_playing_report(&result));
  assert_eq!(output.result(), Some(&serde_json::to_value(&result).expect("fixture should serialize")));

  let invoke_result = InvokeResult::from_command_result(RunId::new(), &media_control_now_playing_invoke_command(), Ok(output));
  let human = invoke_result.render_to_string(InvokeOutputOptions::default()).expect("human output should render");

  assert!(human.contains("State: playing"));
  assert!(human.contains("Title: Current Song"));
  assert!(human.contains("Artist: The Artist"));
  assert!(human.contains("Album: The Album"));
  assert!(human.contains("Source: com.apple.Music"));
  assert!(human.contains("Elapsed: 61.250 s"));
  assert!(human.contains("Duration: 245.500 s"));
}

#[test]
fn now_playing_rejects_target_before_platform_access() {
  let input = crate::InvokeCommandInput {
    command_id: "mediaControl.nowPlaying".to_string(),
    target_application_id: Some("com.example.Player".to_string()),
    inputs: Default::default(),
    typed_args: None,
    dry_run: false,
    cancellation: crate::InvokeCancellation::new(),
  };
  let error = futures_executor::block_on(media_control_now_playing_invoke_command().invoke(input))
    .expect_err("target must fail before reading MediaRemote");
  assert_eq!(error, "mediaControl.nowPlaying cannot use --target; the macOS now-playing state is system-wide");
}

#[test]
fn media_commands_reject_target_before_platform_access() {
  for (command_id, command) in [
    ("mediaControl.play", media_control_play_invoke_command()),
    ("mediaControl.pause", media_control_pause_invoke_command()),
    ("mediaControl.togglePlayPause", media_control_toggle_play_pause_invoke_command()),
    ("mediaControl.next", media_control_next_invoke_command()),
    ("mediaControl.previous", media_control_previous_invoke_command()),
  ] {
    let input = crate::InvokeCommandInput {
      command_id: command_id.to_string(),
      target_application_id: Some("com.example.Player".to_string()),
      inputs: Default::default(),
      typed_args: None,
      dry_run: false,
      cancellation: crate::InvokeCancellation::new(),
    };
    let error = futures_executor::block_on(command.invoke(input)).expect_err("target must fail before MediaRemote");
    assert_eq!(error, format!("{command_id} cannot use --target; macOS media controls are system-wide"));
  }
}

#[test]
fn media_control_human_output_exposes_command_verification_and_before_after_state() {
  for (command, invoke_command) in [
    ("play", media_control_play_invoke_command()),
    ("pause", media_control_pause_invoke_command()),
    ("toggle", media_control_toggle_play_pause_invoke_command()),
    ("next", media_control_next_invoke_command()),
    ("previous", media_control_previous_invoke_command()),
  ] {
    let result = MediaControlOutcome {
      command,
      before: now_playing(false, "Before Song"),
      after: now_playing(true, "After Song"),
      verified: true,
    };
    let output = media_control_output(&result).expect("media-control output should serialize");
    assert_eq!(output.result(), Some(&serde_json::to_value(&result).expect("fixture should serialize")));

    let invoke_result = InvokeResult::from_command_result(RunId::new(), &invoke_command, Ok(output));
    let human = invoke_result.render_to_string(InvokeOutputOptions::default()).expect("human output should render");

    assert!(human.contains(&format!("Command: {command}")), "{command} output was missing the command: {human}");
    assert!(human.contains("Verified: yes"), "{command} output was missing verification: {human}");
    assert!(
      human.contains("Before: paused: Before Song — The Artist (com.apple.Music)"),
      "{command} output was missing the before state: {human}"
    );
    assert!(
      human.contains("After: playing: After Song — The Artist (com.apple.Music)"),
      "{command} output was missing the after state: {human}"
    );
  }
}

#[test]
fn now_playing_report_still_has_a_meaningful_field_when_nothing_is_playing() {
  let result = NowPlayingOutput {
    schema_version: SCHEMA_VERSION,
    present: false,
    is_playing: false,
    source_bundle_id: None,
    title: None,
    artist: None,
    album: None,
    duration_seconds: None,
    elapsed_seconds: None,
    playback_rate: None,
    content_item_id: None,
    supports_like: None,
    is_liked: None,
  };

  assert_eq!(now_playing_report(&result).fields, [InvokeReportField::new("State", "nothing playing")]);
}
