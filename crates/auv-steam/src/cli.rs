use std::{fs, path::PathBuf, process::ExitCode};

use auv_cli_common::outputs::cli::{self as common_output, OutputFormat as CommonOutputFormat};
use auv_cli_common::outputs::formats::table::TableOptions;
use clap::{Parser, Subcommand, ValueEnum};

use crate::{
  app::query_local_library_apps,
  library::{LibraryDiagnostic, LibraryQuery, LibrarySource, LibraryStatus, SteamError, resolve_scope},
};

#[derive(Debug, Parser)]
#[command(name = "auv-steam")]
struct Cli {
  #[command(subcommand)]
  command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
  #[command(subcommand)]
  Library(LibraryCommand),
}

#[derive(Debug, Subcommand)]
enum LibraryCommand {
  Ls(LibraryLsArgs),
}

#[derive(Clone, Debug, Parser)]
struct LibraryLsArgs {
  #[arg(long)]
  name: Option<String>,

  #[arg(long, value_enum, default_value_t = LibraryStatus::Installed)]
  status: LibraryStatus,

  #[arg(long, value_enum, default_value_t = LibrarySource::Auto)]
  source: LibrarySource,

  #[arg(long, value_enum, default_value_t = OutputFormat::Summary)]
  format: OutputFormat,

  #[arg(long)]
  json_out: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum OutputFormat {
  #[default]
  Summary,
  Json,
}

pub fn run() -> ExitCode {
  match Cli::try_parse().map_err(CliError::from).and_then(dispatch) {
    Ok(()) => ExitCode::SUCCESS,
    Err(error) => {
      error.print();
      error.exit_code()
    }
  }
}

fn dispatch(cli: Cli) -> Result<(), CliError> {
  match cli.command {
    Command::Library(LibraryCommand::Ls(args)) => run_library_ls(args),
  }
}

fn run_library_ls(args: LibraryLsArgs) -> Result<(), CliError> {
  let query = args.library_query();
  resolve_scope(&query)?;
  let result = query_local_library_apps(query)?;

  let format = if args.json_out.is_some() || args.format == OutputFormat::Json {
    CommonOutputFormat::Json
  } else {
    CommonOutputFormat::Human
  };
  let rendered = common_output::render(&result, format, TableOptions::default())?;
  if let Some(path) = args.json_out {
    fs::write(path, format!("{rendered}\n"))?;
  } else {
    println!("{rendered}");
  }

  Ok(())
}

impl LibraryLsArgs {
  fn library_query(&self) -> LibraryQuery {
    LibraryQuery {
      name: self.name.clone(),
      status: self.status,
      source: self.source,
    }
  }
}

enum CliError {
  Clap(clap::Error),
  Steam(SteamError),
  Library(LibraryDiagnostic),
  Output(common_output::OutputError),
  Io(std::io::Error),
}

impl CliError {
  fn print(&self) {
    match self {
      Self::Clap(error) => {
        let _ = error.print();
      }
      Self::Steam(error) => eprintln!("error: {error}"),
      Self::Library(diagnostic) => {
        eprintln!("error[{}]: {}", diagnostic.code, diagnostic.message);
        if let Some(path) = &diagnostic.path {
          eprintln!("path: {path}");
        }
      }
      Self::Output(error) => eprintln!("error: {error}"),
      Self::Io(error) => eprintln!("error: failed to write output: {error}"),
    }
  }

  fn exit_code(&self) -> ExitCode {
    match self {
      Self::Clap(error) => ExitCode::from(error.exit_code() as u8),
      _ => ExitCode::FAILURE,
    }
  }
}

impl From<clap::Error> for CliError {
  fn from(error: clap::Error) -> Self {
    Self::Clap(error)
  }
}

impl From<SteamError> for CliError {
  fn from(error: SteamError) -> Self {
    Self::Steam(error)
  }
}

impl From<LibraryDiagnostic> for CliError {
  fn from(diagnostic: LibraryDiagnostic) -> Self {
    Self::Library(diagnostic)
  }
}

impl From<common_output::OutputError> for CliError {
  fn from(error: common_output::OutputError) -> Self {
    Self::Output(error)
  }
}

impl From<std::io::Error> for CliError {
  fn from(error: std::io::Error) -> Self {
    Self::Io(error)
  }
}
