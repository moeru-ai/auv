//! CLI entry point for `auv-apple-music`.
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::{DEFAULT_ACTIVATE_SETTLE_MS, DEFAULT_MUSIC_APP_BUNDLE_ID, ProbeInputs};
use crate::{
  DEFAULT_RESULT_SELECTION_TIMEOUT_MS, DEFAULT_SEARCH_SETTLE_MS, DEFAULT_SEARCH_VERIFICATION_TIMEOUT_MS, SearchInputs,
  SearchResultSelectInputs,
};
use crate::{LaunchEvent, OpenWindowInputs, PlaybackStatusInputs};
use crate::{TransportAction, TransportInputs};
use crate::{run_open_window, run_playback_status, run_probe, run_search, run_search_result_select, run_transport_action};

#[derive(Clone, Debug, Parser)]
#[command(
  name = "auv-apple-music",
  disable_help_subcommand = true,
  about = "Apple Music app commands (Windows)"
)]
struct CliArgs {
  #[command(subcommand)]
  command: CliCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum CliCommand {
  /// Ensure Apple Music is running and its window is visible.
  OpenWindow(OpenWindowArgs),
  /// Read playback state, track title, and artist from Apple Music.
  Playback(PlaybackArgs),
  /// Search Apple Music and verify the submitted query.
  Search(SearchArgs),
  /// Send a transport control action (play/pause, next, previous).
  Transport(TransportArgs),
  /// Probe Music.app's AX surface for search field candidates (macOS).
  ProbeMacos(ProbeMacosArgs),
}

#[derive(Clone, Debug, Args)]
struct OpenWindowArgs {
  /// How long to wait (ms) for the window to appear after launching.
  /// Set to 0 to disable waiting.
  #[arg(long = "settle-ms", default_value_t = 8000)]
  settle_ms: u64,

  /// Output as JSON instead of human-readable text.
  #[arg(long)]
  json: bool,
}

#[derive(Clone, Debug, Args)]
struct PlaybackArgs {
  /// Output as JSON instead of human-readable text.
  #[arg(long)]
  json: bool,
}

#[derive(Clone, Debug, Args)]
struct SearchArgs {
  /// Search query to submit.
  #[arg(value_name = "query")]
  query: String,

  /// Select one result whose full accessible name contains this unique anchor.
  #[arg(long = "select", value_name = "ANCHOR")]
  select: Option<String>,

  /// How long to wait after each input action (ms).
  #[arg(long = "settle-ms", default_value_t = DEFAULT_SEARCH_SETTLE_MS)]
  settle_ms: u64,

  /// How long to wait for OCR verification (ms).
  #[arg(
    long = "verification-timeout-ms",
    default_value_t = DEFAULT_SEARCH_VERIFICATION_TIMEOUT_MS
  )]
  verification_timeout_ms: u64,

  /// How long to wait for a matching UIA result item (ms).
  #[arg(
    long = "selection-timeout-ms",
    default_value_t = DEFAULT_RESULT_SELECTION_TIMEOUT_MS
  )]
  selection_timeout_ms: u64,

  /// Output as JSON instead of human-readable text.
  #[arg(long)]
  json: bool,
}

#[derive(Clone, Debug, Args)]
struct TransportArgs {
  /// Action to perform.
  #[command(subcommand)]
  action: TransportSubcommand,

  /// How long to wait after the click for the app to react (ms).
  #[arg(long = "settle-ms", default_value_t = 150, global = true)]
  settle_ms: u64,

  /// Output as JSON instead of human-readable text.
  #[arg(long, global = true)]
  json: bool,
}

#[derive(Clone, Debug, Args)]
struct ProbeMacosArgs {
  /// Bundle id to activate and probe.
  #[arg(long = "bundle-id", default_value = DEFAULT_MUSIC_APP_BUNDLE_ID)]
  bundle_id: String,

  /// How long to wait after activation before capturing the AX tree (ms).
  #[arg(long = "activate-settle-ms", default_value_t = DEFAULT_ACTIVATE_SETTLE_MS)]
  activate_settle_ms: u64,

  /// Output as JSON instead of human-readable text.
  #[arg(long)]
  json: bool,
}

#[derive(Clone, Debug, Subcommand)]
enum TransportSubcommand {
  /// Toggle play/pause.
  PlayPause,
  /// Skip to the next track.
  Next,
  /// Skip to the previous track (or restart the current one).
  Previous,
}

pub fn run() -> ExitCode {
  let args = CliArgs::parse();

  match args.command {
    CliCommand::OpenWindow(args) => run_open_window_cmd(args),
    CliCommand::Playback(args) => run_playback_cmd(args),
    CliCommand::Search(args) => run_search_cmd(args),
    CliCommand::Transport(args) => run_transport_cmd(args),
    CliCommand::ProbeMacos(args) => run_probe_macos_cmd(args),
  }
}

fn run_open_window_cmd(args: OpenWindowArgs) -> ExitCode {
  let inputs = OpenWindowInputs {
    settle_ms: args.settle_ms,
    ..OpenWindowInputs::default()
  };

  match run_open_window(&inputs) {
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::FAILURE
    }
    Ok(result) => {
      if args.json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
      } else {
        let status = if result.window_found {
          "found"
        } else {
          "not found"
        };
        let title = result.window_title.as_deref().unwrap_or("<no title>");
        println!("window: {status}");
        if result.window_found {
          println!("  title: {title}");
        }
        for event in &result.events {
          match event {
            LaunchEvent::WindowResolved => println!("  window resolved"),
            LaunchEvent::WindowNotFound => println!("  window not found"),
            LaunchEvent::LaunchTargetsDiscovered { app_ids } => println!("  launch targets: {}", app_ids.join(", ")),
            LaunchEvent::LaunchTargetsMissing => println!("  no registered launch target found"),
            LaunchEvent::LaunchTargetDiscoveryFailed { message } => println!("  launch target discovery failed: {message}"),
            LaunchEvent::LaunchSucceeded { app_id } => println!("  launched: {app_id}"),
            LaunchEvent::LaunchFailed { app_id, message } => println!("  launch failed for {app_id}: {message}"),
            LaunchEvent::WindowAppeared => println!("  window appeared"),
            LaunchEvent::WindowWaitTimedOut { timeout_ms } => println!("  window did not appear within {timeout_ms}ms"),
            LaunchEvent::UnsupportedPlatform => println!("  launch unsupported on this platform"),
          }
        }
      }
      if result.window_found {
        ExitCode::SUCCESS
      } else {
        ExitCode::FAILURE
      }
    }
  }
}

fn run_playback_cmd(args: PlaybackArgs) -> ExitCode {
  let inputs = PlaybackStatusInputs::default();

  match run_playback_status(&inputs) {
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::FAILURE
    }
    Ok(result) => {
      if args.json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
      } else {
        println!("state:   {}", result.state);
        println!("title:   {}", result.track_title.as_deref().unwrap_or("-"));
        println!("artist:  {}", result.artist.as_deref().unwrap_or("-"));
        println!("source:  {}", result.metadata_source);
        for note in &result.diagnostics {
          println!("note:    {note}");
        }
      }
      ExitCode::SUCCESS
    }
  }
}

fn run_transport_cmd(args: TransportArgs) -> ExitCode {
  let action = match args.action {
    TransportSubcommand::PlayPause => TransportAction::PlayPause,
    TransportSubcommand::Next => TransportAction::Next,
    TransportSubcommand::Previous => TransportAction::Previous,
  };

  let inputs = TransportInputs {
    action,
    settle_ms: args.settle_ms,
    ..TransportInputs::new(action)
  };

  match run_transport_action(&inputs) {
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::FAILURE
    }
    Ok(result) => {
      if args.json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
      } else {
        println!("action:  {}", result.action);
        println!("key:     {}", result.key);
        for note in &result.diagnostics {
          println!("note:    {note}");
        }
      }
      ExitCode::SUCCESS
    }
  }
}

fn run_search_cmd(args: SearchArgs) -> ExitCode {
  let mut inputs = SearchInputs::with_query(args.query);
  inputs.settle_ms = args.settle_ms;
  inputs.verification_timeout_ms = args.verification_timeout_ms;

  if let Some(anchor) = args.select {
    let selection_inputs = SearchResultSelectInputs {
      search: inputs,
      anchor,
      selection_timeout_ms: args.selection_timeout_ms,
    };
    return match run_search_result_select(&selection_inputs) {
      Err(error) => {
        eprintln!("error: {error}");
        ExitCode::FAILURE
      }
      Ok(result) => {
        if args.json {
          println!("{}", serde_json::to_string_pretty(&result).unwrap());
        } else {
          println!("query:          {}", result.search.query);
          println!("verification:   {}", result.search.verification.status);
          println!("selected:       {}", result.selected.name);
          println!("selection path: {:?}", result.selection_input.selected_path);
          println!("selection verified: {}", result.verification.status);
        }
        if result.is_verified() {
          ExitCode::SUCCESS
        } else {
          ExitCode::FAILURE
        }
      }
    };
  }

  match run_search(&inputs) {
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::FAILURE
    }
    Ok(result) => {
      if args.json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
      } else {
        println!("query:        {}", result.query);
        println!("verification: {}", result.verification.status);
        println!("input path:   {:?}", result.query_input.selected_path);
        for note in &result.diagnostics {
          println!("note:         {note}");
        }
      }
      if result.is_verified() {
        ExitCode::SUCCESS
      } else {
        ExitCode::FAILURE
      }
    }
  }
}

fn run_probe_macos_cmd(args: ProbeMacosArgs) -> ExitCode {
  let inputs = ProbeInputs {
    bundle_id: args.bundle_id,
    activate_settle_ms: args.activate_settle_ms,
  };

  match run_probe(&inputs) {
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::FAILURE
    }
    Ok(result) => {
      if args.json {
        println!("{}", serde_json::to_string_pretty(&result).unwrap());
      } else {
        println!("bundle_id:        {}", result.bundle_id);
        println!("activated:        {}", result.activated);
        println!("ax_captured:      {}", result.ax_snapshot_captured);
        println!("node_count:       {}", result.node_count);
        println!("search_fields:    {}", result.search_field_candidates.len());
        for node in &result.search_field_candidates {
          println!("  path={} role={} subrole={} title={:?}", node.path, node.role, node.subrole, node.title);
        }
        println!("toolbars_inspected: {}", result.toolbar_inspections.len());
        for inspection in &result.toolbar_inspections {
          let counts = &inspection.child_counts;
          println!(
            "  path={} role={} children={} visible={} contents={} navigation={} actions={:?}",
            inspection.path,
            inspection.role,
            counts.children_count,
            counts.visible_children_count,
            counts.contents_count,
            counts.navigation_children_count,
            inspection.available_actions,
          );
        }
        for note in &result.diagnostics {
          println!("note:             {note}");
        }
      }
      ExitCode::SUCCESS
    }
  }
}

#[cfg(test)]
mod tests {
  use clap::Parser;

  use super::{CliArgs, CliCommand};

  #[test]
  fn parse_search_maps_query_and_timing_options() {
    let args = CliArgs::try_parse_from([
      "auv-apple-music",
      "search",
      "AURORA Cure For Me",
      "--settle-ms",
      "125",
      "--verification-timeout-ms",
      "900",
    ])
    .expect("search command should parse");

    let CliCommand::Search(search) = args.command else {
      panic!("expected search command");
    };
    assert_eq!(search.query, "AURORA Cure For Me");
    assert_eq!(search.select, None);
    assert_eq!(search.settle_ms, 125);
    assert_eq!(search.verification_timeout_ms, 900);
  }

  #[test]
  fn parse_search_selection_maps_unique_anchor() {
    let args = CliArgs::try_parse_from([
      "auv-apple-music",
      "search",
      "Chopin ballade no. 1",
      "--select",
      "Ballade No. 1 in G Minor, Op. 23 YUNDI",
      "--selection-timeout-ms",
      "1200",
    ])
    .expect("search selection should parse");

    let CliCommand::Search(search) = args.command else {
      panic!("expected search command");
    };
    assert_eq!(search.select.as_deref(), Some("Ballade No. 1 in G Minor, Op. 23 YUNDI"));
    assert_eq!(search.selection_timeout_ms, 1200);
  }

  #[test]
  fn parse_search_requires_query() {
    assert!(CliArgs::try_parse_from(["auv-apple-music", "search"]).is_err());
  }
}
