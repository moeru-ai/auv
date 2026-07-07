//! `auv-media-macos` binary entry point: read the system now-playing state and
//! send transport controls to whichever app owns the now-playing slot.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::output::{build_now_playing_output, render_human_summary};
use crate::{MediaCommand, now_playing, seek, send_command};

/// Output format for the now-playing read. Shared with embedding CLIs (e.g.
/// `auv-netease-music`) via the crate root re-export.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
  /// One-line human summary.
  #[default]
  Summary,
  /// The `now-playing-v0` JSON object.
  Json,
}

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
  /// Output format on stdout.
  #[arg(long, value_enum, default_value_t = OutputFormat::Summary)]
  format: OutputFormat,
  /// Write the now-playing-v0 JSON object to a file (overrides --format).
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

  if args.format == OutputFormat::Json || args.json_out.is_some() {
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
      println!("ok: {}", command.label());
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
  let duration = match seek_duration_from_seconds(seconds) {
    Ok(duration) => duration,
    Err(message) => {
      eprintln!("{message}");
      return ExitCode::FAILURE;
    }
  };
  match seek(duration) {
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

/// Convert a CLI-supplied f64 seconds value into a [`Duration`] without
/// panicking. `Duration::from_secs_f64` panics not only on NaN/Inf/negative
/// (which the prior `is_finite` + `< 0` guard caught) but also on overflow
/// past `Duration::MAX` (~5.85e11 years), and that overflow path was
/// unguarded.
fn seek_duration_from_seconds(seconds: f64) -> Result<Duration, &'static str> {
  Duration::try_from_secs_f64(seconds)
    .map_err(|_| "seek position must be a non-negative finite number of seconds within the representable range")
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn seek_duration_accepts_zero_and_normal_values() {
    assert_eq!(seek_duration_from_seconds(0.0).unwrap(), Duration::ZERO);
    assert_eq!(seek_duration_from_seconds(1.5).unwrap(), Duration::from_secs_f64(1.5));
  }

  #[test]
  fn seek_duration_rejects_negative() {
    assert!(seek_duration_from_seconds(-1.0).is_err());
  }

  #[test]
  fn seek_duration_rejects_nan() {
    assert!(seek_duration_from_seconds(f64::NAN).is_err());
  }

  #[test]
  fn seek_duration_rejects_infinity() {
    assert!(seek_duration_from_seconds(f64::INFINITY).is_err());
  }

  #[test]
  fn seek_duration_rejects_overflow_past_duration_max() {
    // `Duration::from_secs_f64` panics on values above `Duration::MAX`
    // (~1.84e19 seconds). 1e20 is comfortably past that and would have
    // panicked the CLI under the old code path.
    assert!(seek_duration_from_seconds(1e20).is_err());
  }
}
