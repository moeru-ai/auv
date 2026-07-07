use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::commands::document::{
  DEFAULT_APP_ID, DEFAULT_BODY_ROLE, DEFAULT_FOCUS_QUERY, DEFAULT_SETTLE_MS, DocumentCommand, DocumentCompare, DocumentFocus, DocumentWrite,
  run_document_command,
};
use crate::driver::MacosTextEditDriver;

#[derive(Clone, Debug, Parser)]
#[command(
  name = "auv-apple-textedit",
  disable_help_subcommand = true,
  about = "TextEdit app commands"
)]
struct CliArgs {
  #[command(subcommand)]
  command: CliCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum CliCommand {
  /// Operate on the current TextEdit document.
  Document(DocumentArgs),
}

#[derive(Clone, Debug, Args)]
struct DocumentArgs {
  #[command(subcommand)]
  command: DocumentSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum DocumentSubcommand {
  /// Write content into the document body.
  Write(DocumentWriteArgs),
  /// Compare document body text against expected content.
  Compare(DocumentCompareArgs),
  /// Focus the document body.
  Focus(DocumentFocusArgs),
}

#[derive(Clone, Debug, Args)]
struct DocumentWriteArgs {
  #[arg(value_name = "content")]
  content: String,
  #[arg(long = "replace")]
  replace: bool,
  #[arg(long = "verify")]
  verify: bool,
  #[arg(long = "app-id", default_value = DEFAULT_APP_ID)]
  app_id: String,
  #[arg(long = "focus-query", default_value = DEFAULT_FOCUS_QUERY)]
  focus_query: String,
  #[arg(long = "focus-candidate", default_value = "")]
  focus_candidate: String,
  #[arg(long = "role", default_value = DEFAULT_BODY_ROLE)]
  role: String,
  #[arg(long = "activate-settle-ms", default_value_t = DEFAULT_SETTLE_MS)]
  activate_settle_ms: u64,
  #[arg(long = "input-settle-ms", default_value_t = DEFAULT_SETTLE_MS)]
  input_settle_ms: u64,
}

#[derive(Clone, Debug, Args)]
struct DocumentCompareArgs {
  #[arg(value_name = "content")]
  content: String,
  #[arg(long = "role", default_value = DEFAULT_BODY_ROLE)]
  role: String,
  #[arg(long = "app-id", default_value = DEFAULT_APP_ID)]
  app_id: String,
}

#[derive(Clone, Debug, Args)]
struct DocumentFocusArgs {
  #[arg(long = "query", default_value = DEFAULT_FOCUS_QUERY)]
  query: String,
  #[arg(long = "candidate", default_value = "")]
  candidate: String,
  #[arg(long = "app-id", default_value = DEFAULT_APP_ID)]
  app_id: String,
}

pub fn parse_from<I, T>(args: I) -> Result<DocumentCommand, clap::Error>
where
  I: IntoIterator<Item = T>,
  T: Into<std::ffi::OsString> + Clone,
{
  let args = CliArgs::try_parse_from(args)?;
  Ok(match args.command {
    CliCommand::Document(document) => match document.command {
      DocumentSubcommand::Write(args) => DocumentCommand::Write(DocumentWrite {
        app_id: args.app_id,
        content: args.content,
        replace: args.replace,
        verify: args.verify,
        focus_query: args.focus_query,
        focus_candidate: args.focus_candidate,
        compare_role: args.role,
        activate_settle_ms: args.activate_settle_ms,
        input_settle_ms: args.input_settle_ms,
      }),
      DocumentSubcommand::Compare(args) => DocumentCommand::Compare(DocumentCompare {
        app_id: args.app_id,
        content: args.content,
        role: args.role,
      }),
      DocumentSubcommand::Focus(args) => DocumentCommand::Focus(DocumentFocus {
        app_id: args.app_id,
        query: args.query,
        candidate: args.candidate,
      }),
    },
  })
}

pub fn run() -> ExitCode {
  let command = match parse_from(std::env::args_os()) {
    Ok(command) => command,
    Err(error) => {
      let _ = error.print();
      return ExitCode::from(2);
    }
  };
  let mut driver = match MacosTextEditDriver::open_local() {
    Ok(driver) => driver,
    Err(error) => {
      eprintln!("{error}");
      return ExitCode::from(1);
    }
  };
  match run_document_command(&command, &mut driver) {
    Ok(report) => {
      println!("{}", report.command);
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("{error}");
      ExitCode::from(1)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_document_write_maps_content_and_flags() {
    let command = parse_from([
      "auv-apple-textedit",
      "document",
      "write",
      "hello",
      "--replace",
      "--verify",
    ])
    .expect("command should parse");

    assert_eq!(
      command,
      DocumentCommand::Write(DocumentWrite {
        app_id: DEFAULT_APP_ID.to_string(),
        content: "hello".to_string(),
        replace: true,
        verify: true,
        focus_query: DEFAULT_FOCUS_QUERY.to_string(),
        focus_candidate: String::new(),
        compare_role: DEFAULT_BODY_ROLE.to_string(),
        activate_settle_ms: DEFAULT_SETTLE_MS,
        input_settle_ms: DEFAULT_SETTLE_MS,
      })
    );
  }

  #[test]
  fn parse_document_compare_maps_role() {
    let command = parse_from([
      "auv-apple-textedit",
      "document",
      "compare",
      "hello",
      "--role",
      "AXTextArea",
    ])
    .expect("command should parse");

    assert_eq!(
      command,
      DocumentCommand::Compare(DocumentCompare {
        app_id: DEFAULT_APP_ID.to_string(),
        content: "hello".to_string(),
        role: DEFAULT_BODY_ROLE.to_string(),
      })
    );
  }

  #[test]
  fn parse_document_focus_maps_query() {
    let command = parse_from([
      "auv-apple-textedit",
      "document",
      "focus",
      "--query",
      "First Text View",
    ])
    .expect("command should parse");

    assert_eq!(
      command,
      DocumentCommand::Focus(DocumentFocus {
        app_id: DEFAULT_APP_ID.to_string(),
        query: DEFAULT_FOCUS_QUERY.to_string(),
        candidate: String::new(),
      })
    );
  }
}
