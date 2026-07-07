use std::{fs, path::PathBuf, process::ExitCode};

use clap::{Parser, Subcommand, ValueEnum};

use crate::{
  app::query_local_library_apps,
  library::{LibraryDiagnostic, LibraryQuery, LibrarySource, LibraryStatus, SteamError, resolve_scope},
  output::{build_library_ls_json_output, render_library_summary},
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

  if let Some(path) = args.json_out {
    let output = build_library_ls_json_output(&result);
    let json = serde_json::to_string_pretty(&output)?;
    fs::write(path, format!("{json}\n"))?;
  } else if args.format == OutputFormat::Json {
    let output = build_library_ls_json_output(&result);
    println!("{}", serde_json::to_string_pretty(&output)?);
  } else {
    println!("{}", render_library_summary(&result));
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
  Json(serde_json::Error),
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
      Self::Json(error) => eprintln!("error: failed to render JSON output: {error}"),
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

impl From<serde_json::Error> for CliError {
  fn from(error: serde_json::Error) -> Self {
    Self::Json(error)
  }
}

impl From<std::io::Error> for CliError {
  fn from(error: std::io::Error) -> Self {
    Self::Io(error)
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn library_ls_defaults_to_installed_auto_summary() {
    let cli = Cli::try_parse_from(["auv-steam", "library", "ls"]).expect("valid command");

    let Command::Library(LibraryCommand::Ls(args)) = cli.command;
    assert_eq!(
      args.library_query(),
      LibraryQuery {
        name: None,
        status: LibraryStatus::Installed,
        source: LibrarySource::Auto,
      }
    );
    assert_eq!(args.format, OutputFormat::Summary);
    assert_eq!(args.json_out, None);
  }

  #[test]
  fn library_ls_parses_explicit_query_and_json_format() {
    let cli = Cli::try_parse_from([
      "auv-steam",
      "library",
      "ls",
      "--name",
      "Balatro",
      "--status",
      "installed",
      "--source",
      "local",
      "--format",
      "json",
    ])
    .expect("valid command");

    let Command::Library(LibraryCommand::Ls(args)) = cli.command;
    assert_eq!(
      args.library_query(),
      LibraryQuery {
        name: Some("Balatro".to_string()),
        status: LibraryStatus::Installed,
        source: LibrarySource::Local,
      }
    );
    assert_eq!(args.format, OutputFormat::Json);
    assert_eq!(args.json_out, None);
  }

  #[test]
  fn library_ls_has_no_json_flag() {
    assert!(Cli::try_parse_from(["auv-steam", "library", "ls", "--json"]).is_err());
  }

  #[test]
  fn library_ls_json_out_takes_output_precedence() {
    let cli = Cli::try_parse_from([
      "auv-steam",
      "library",
      "ls",
      "--format",
      "summary",
      "--json-out",
      "library.json",
    ])
    .expect("valid command");

    let Command::Library(LibraryCommand::Ls(args)) = cli.command;
    assert_eq!(args.json_out, Some(PathBuf::from("library.json")));
    assert_eq!(args.format, OutputFormat::Summary);
  }

  #[test]
  fn library_ls_uses_shared_local_query_entry() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Owned,
      source: LibrarySource::Auto,
    };

    let shared_result = query_local_library_apps(query.clone()).expect_err("owned status should fail");
    let scoped_result = resolve_scope(&query).expect_err("owned status should fail before discovery");

    assert_eq!(shared_result.code, scoped_result.code);
    assert_eq!(shared_result.message, scoped_result.message);
  }
}
