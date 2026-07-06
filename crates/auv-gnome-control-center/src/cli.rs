use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::commands::mouse::{
  NaturalScrollingToggleInputs, PointerSpeedRoundtripInputs, PointerSpeedSetInputs,
};
use crate::commands::system_details::CopySystemDetailsInputs;
use crate::commands::{OpenInputs, run_open};
use crate::output::print_json;
use crate::{
  run_copy_system_details, run_natural_scrolling_toggle, run_pointer_speed_roundtrip,
  run_pointer_speed_set,
};

#[derive(Debug, Parser)]
#[command(name = "auv-gnome-control-center")]
#[command(about = "AUV workflows for GNOME Control Center")]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
  /// Open or resolve GNOME Control Center.
  Open(CommonArgs),
  /// Navigate to System Details, press GNOME's Copy button, and print the clipboard payload.
  CopySystemDetails(CommonArgs),
  /// Mouse settings workflows.
  Mouse {
    #[command(subcommand)]
    command: MouseCommand,
  },
}

#[derive(Debug, Subcommand)]
enum MouseCommand {
  /// Set pointer speed by clicking the slider at a relative position from 0.0 to 1.0.
  SetPointerSpeed(SetPointerSpeedArgs),
  /// Change pointer speed, then restore to a second relative position.
  RoundtripPointerSpeed(RoundtripPointerSpeedArgs),
  /// Toggle the Natural Scrolling switch.
  ToggleNaturalScrolling(CommonArgs),
}

#[derive(Clone, Debug, Args)]
struct CommonArgs {
  #[arg(long)]
  json: bool,
  #[arg(long, default_value_t = 8_000)]
  settle_ms: u64,
}

#[derive(Clone, Debug, Args)]
struct SetPointerSpeedArgs {
  #[arg(long)]
  json: bool,
  #[arg(long, default_value_t = 8_000)]
  settle_ms: u64,
  #[arg(long, default_value_t = 0.5)]
  position: f64,
}

#[derive(Clone, Debug, Args)]
struct RoundtripPointerSpeedArgs {
  #[arg(long)]
  json: bool,
  #[arg(long, default_value_t = 8_000)]
  settle_ms: u64,
  #[arg(long, default_value_t = 0.75)]
  first_position: f64,
  #[arg(long, default_value_t = 0.5)]
  restore_position: f64,
}

pub fn run() -> ExitCode {
  match run_inner(Cli::parse()) {
    Ok(()) => ExitCode::SUCCESS,
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::from(1)
    }
  }
}

fn run_inner(cli: Cli) -> Result<(), String> {
  match cli.command {
    Command::Open(args) => {
      let result = run_open(&OpenInputs {
        settle_ms: args.settle_ms,
      })?;
      if args.json {
        print_json(&result)
      } else {
        println!(
          "opened: found={} title={:?}",
          result.window.window_found, result.window.window_title
        );
        Ok(())
      }
    }
    Command::CopySystemDetails(args) => {
      let result = run_copy_system_details(&CopySystemDetailsInputs {
        settle_ms: args.settle_ms,
      })?;
      if args.json {
        print_json(&result)
      } else {
        println!("{}", result.clipboard_text);
        Ok(())
      }
    }
    Command::Mouse { command } => match command {
      MouseCommand::SetPointerSpeed(args) => {
        let result = run_pointer_speed_set(&PointerSpeedSetInputs {
          position: args.position,
          settle_ms: args.settle_ms,
        })?;
        if args.json {
          print_json(&result)
        } else {
          println!(
            "pointer speed clicked at {:.2}: {:?}",
            result.requested_position, result.clicked_point
          );
          Ok(())
        }
      }
      MouseCommand::RoundtripPointerSpeed(args) => {
        let result = run_pointer_speed_roundtrip(&PointerSpeedRoundtripInputs {
          first_position: args.first_position,
          restore_position: args.restore_position,
          settle_ms: args.settle_ms,
        })?;
        if args.json {
          print_json(&result)
        } else {
          println!(
            "pointer speed roundtrip: {:.2} -> {:.2}",
            result.first.requested_position, result.restore.requested_position
          );
          Ok(())
        }
      }
      MouseCommand::ToggleNaturalScrolling(args) => {
        let result = run_natural_scrolling_toggle(&NaturalScrollingToggleInputs {
          settle_ms: args.settle_ms,
        })?;
        if args.json {
          print_json(&result)
        } else {
          println!(
            "natural scrolling toggled: {:?} -> {:?}",
            result.observed_value_before, result.observed_value_after
          );
          Ok(())
        }
      }
    },
  }
}
