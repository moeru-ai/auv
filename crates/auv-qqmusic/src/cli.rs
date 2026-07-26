use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, error::ErrorKind};

use crate::driver::MacosQqMusicDriver;
use crate::search::{
  DEFAULT_ANCHOR_TIMEOUT_MS, DEFAULT_APP_ID, DEFAULT_SETTLE_MS, SearchCommand, SearchResultsAction, SearchResultsClick, SearchResultsSelect,
  SearchSubmit, run_search_command,
};

#[derive(Clone, Debug, Parser)]
#[command(
  name = "auv-qqmusic",
  disable_help_subcommand = true,
  about = "QQMusic app commands"
)]
struct CliArgs {
  #[command(subcommand)]
  command: CliCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum CliCommand {
  /// Search QQMusic and operate on search results.
  Search(SearchArgs),
}

#[derive(Clone, Debug, Args)]
#[command(args_conflicts_with_subcommands = true, subcommand_negates_reqs = true)]
struct SearchArgs {
  #[arg(value_name = "query")]
  query: Option<String>,
  #[command(subcommand)]
  results: Option<SearchSubcommand>,
}

#[derive(Clone, Debug, Subcommand)]
enum SearchSubcommand {
  /// Operate on search results.
  Results(SearchResultsArgs),
}

#[derive(Clone, Debug, Args)]
struct SearchResultsArgs {
  #[command(subcommand)]
  command: SearchResultsSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum SearchResultsSubcommand {
  /// Select a result by visible text anchor.
  Select(SearchResultsSelectArgs),
  /// Click a result by anchor, row, or candidate ref.
  Click(SearchResultsClickArgs),
}

#[derive(Clone, Debug, Args)]
struct SearchResultsSelectArgs {
  #[arg(value_name = "query")]
  query: String,
  #[arg(long = "anchor")]
  anchor: String,
  #[arg(long = "app-id", default_value = DEFAULT_APP_ID)]
  app_id: String,
  #[arg(long = "settle-ms", default_value_t = DEFAULT_SETTLE_MS)]
  settle_ms: u64,
  #[arg(long = "anchor-timeout-ms", default_value_t = DEFAULT_ANCHOR_TIMEOUT_MS)]
  anchor_timeout_ms: u64,
}

#[derive(Clone, Debug, Args)]
struct SearchResultsClickArgs {
  #[arg(value_name = "query", required_unless_present_any = ["row", "candidate_ref"])]
  query: Option<String>,
  #[arg(long = "anchor", conflicts_with_all = ["row", "candidate_ref"])]
  anchor: Option<String>,
  #[arg(long = "row", conflicts_with_all = ["anchor", "candidate_ref"])]
  row: Option<usize>,
  #[arg(long = "candidate-ref", conflicts_with_all = ["anchor", "row"])]
  candidate_ref: Option<String>,
  #[arg(long = "app-id", default_value = DEFAULT_APP_ID)]
  app_id: String,
  #[arg(long = "settle-ms", default_value_t = DEFAULT_SETTLE_MS)]
  settle_ms: u64,
  #[arg(long = "anchor-timeout-ms", default_value_t = DEFAULT_ANCHOR_TIMEOUT_MS)]
  anchor_timeout_ms: u64,
}

pub fn parse_from<I, T>(args: I) -> Result<SearchCommand, clap::Error>
where
  I: IntoIterator<Item = T>,
  T: Into<std::ffi::OsString> + Clone,
{
  let args = CliArgs::try_parse_from(args)?;
  Ok(match args.command {
    CliCommand::Search(search) => match search.results {
      None => SearchCommand::Search(SearchSubmit::defaults_with_query(
        search.query.ok_or_else(|| clap::Error::raw(ErrorKind::MissingRequiredArgument, "auv-qqmusic search requires <query>"))?,
      )),
      Some(SearchSubcommand::Results(results)) => match results.command {
        SearchResultsSubcommand::Select(args) => SearchCommand::Results(SearchResultsAction::Select(SearchResultsSelect {
          app_id: args.app_id,
          query: args.query,
          anchor: args.anchor,
          settle_ms: args.settle_ms,
          anchor_timeout_ms: args.anchor_timeout_ms,
        })),
        SearchResultsSubcommand::Click(args) => {
          validate_click_args(&args)?;
          SearchCommand::Results(SearchResultsAction::Click(SearchResultsClick {
            app_id: args.app_id,
            query: args.query,
            anchor: args.anchor,
            row: args.row,
            candidate_ref_json: args.candidate_ref,
            settle_ms: args.settle_ms,
            anchor_timeout_ms: args.anchor_timeout_ms,
          }))
        }
      },
    },
  })
}

fn validate_click_args(args: &SearchResultsClickArgs) -> Result<(), clap::Error> {
  let selector_count = args.anchor.is_some() as usize + args.row.is_some() as usize + args.candidate_ref.is_some() as usize;
  if selector_count == 0 {
    return Err(clap::Error::raw(ErrorKind::MissingRequiredArgument, "search results click requires --anchor, --row, or --candidate-ref"));
  }
  if args.candidate_ref.is_some() {
    if args.query.is_some() {
      return Err(clap::Error::raw(ErrorKind::ArgumentConflict, "search results click --candidate-ref does not take <query>"));
    }
    return Ok(());
  }
  if args.query.is_none() {
    return Err(clap::Error::raw(ErrorKind::MissingRequiredArgument, "search results click --anchor and --row require <query>"));
  }
  Ok(())
}

pub fn run() -> ExitCode {
  let command = match parse_from(std::env::args_os()) {
    Ok(command) => command,
    Err(error) => {
      let _ = error.print();
      return ExitCode::from(2);
    }
  };
  let mut driver = match MacosQqMusicDriver::open_local() {
    Ok(driver) => driver,
    Err(error) => {
      eprintln!("{error}");
      return ExitCode::from(1);
    }
  };
  match run_search_command(&command, &mut driver) {
    Ok(report) => {
      if let Some(unsupported) = report.unsupported {
        eprintln!("{unsupported}");
        ExitCode::from(2)
      } else {
        println!("{}", report.command);
        ExitCode::SUCCESS
      }
    }
    Err(error) => {
      eprintln!("{error}");
      ExitCode::from(1)
    }
  }
}
