//! `auv-media-macos` binary entry point: read the system now-playing state and
//! send transport controls to whichever app owns the now-playing slot.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand};

use crate::output::{build_now_playing_output, render_human_summary};
use crate::{MediaCommand, now_playing, seek, send_command};

#[derive(Parser)]
#[command(
  name = "auv-media-macos",
  about = "macOS system media: now-playing read + transport controls"
)]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Subcommand)]
enum Command {
  /// Read the system now-playing state.
  #[command(name = "now-playing")]
  NowPlaying(NowPlayingArgs),
  /// Start playback.
  Play,
  /// Pause playback.
  Pause,
  /// Toggle between play and pause.
  Toggle,
  /// Skip to the next track.
  Next,
  /// Return to the previous track.
  Previous,
  /// Seek to a position, in seconds from the start of the track.
  Seek { seconds: f64 },
}

#[derive(Args)]
struct NowPlayingArgs {
  /// Emit the now-playing-v0 JSON object to stdout (default: human summary).
  #[arg(long, conflicts_with = "json_out")]
  json: bool,
  /// Write the now-playing-v0 JSON object to a file.
  #[arg(long, value_name = "path")]
  json_out: Option<PathBuf>,
}

/// Parse argv, dispatch, and return the process exit code.
pub fn run() -> ExitCode {
  match Cli::parse().command {
    Command::NowPlaying(args) => run_now_playing(args),
    Command::Play => run_send(MediaCommand::Play),
    Command::Pause => run_send(MediaCommand::Pause),
    Command::Toggle => run_send(MediaCommand::TogglePlayPause),
    Command::Next => run_send(MediaCommand::NextTrack),
    Command::Previous => run_send(MediaCommand::PreviousTrack),
    Command::Seek { seconds } => run_seek(seconds),
  }
}

/// Read the now-playing state and emit it as human text or `now-playing-v0` JSON.
///
/// Exit `0` on a successful read, including the nothing-playing case
/// (`present: false` is state, not an error). Non-zero only on a read failure.
fn run_now_playing(args: NowPlayingArgs) -> ExitCode {
  let state = match now_playing() {
    Ok(state) => state,
    Err(error) => {
      eprintln!("{error}");
      return ExitCode::FAILURE;
    }
  };

  if args.json || args.json_out.is_some() {
    let output = build_now_playing_output(&state);
    let json = match serde_json::to_string_pretty(&output) {
      Ok(json) => json,
      Err(error) => {
        eprintln!("failed to encode now-playing JSON: {error}");
        return ExitCode::FAILURE;
      }
    };
    if let Some(path) = args.json_out {
      if let Err(error) = std::fs::write(&path, format!("{json}\n")) {
        eprintln!("failed to write {}: {error}", path.display());
        return ExitCode::FAILURE;
      }
    } else {
      println!("{json}");
    }
  } else {
    println!("{}", render_human_summary(&state));
  }

  ExitCode::SUCCESS
}

/// Send a transport command; print a terse confirmation on success.
fn run_send(command: MediaCommand) -> ExitCode {
  match send_command(command) {
    Ok(()) => {
      println!("ok: {}", command_label(command));
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("{error}");
      ExitCode::FAILURE
    }
  }
}

/// Seek to `seconds` from the start of the track.
fn run_seek(seconds: f64) -> ExitCode {
  if !seconds.is_finite() || seconds < 0.0 {
    eprintln!("seek position must be a non-negative number of seconds");
    return ExitCode::FAILURE;
  }
  match seek(Duration::from_secs_f64(seconds)) {
    Ok(()) => {
      println!("ok: seek {seconds}s");
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("{error}");
      ExitCode::FAILURE
    }
  }
}

fn command_label(command: MediaCommand) -> &'static str {
  match command {
    MediaCommand::Play => "play",
    MediaCommand::Pause => "pause",
    MediaCommand::TogglePlayPause => "toggle",
    MediaCommand::NextTrack => "next",
    MediaCommand::PreviousTrack => "previous",
  }
}
