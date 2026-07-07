use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::commands::note::{
  DEFAULT_APP_ID, DEFAULT_BODY_ROLE, DEFAULT_FOCUS_QUERY, DEFAULT_SETTLE_MS, NoteCommand, NoteCompare, NoteFocus, NoteNew, NoteWrite,
  run_note_command,
};
use crate::driver::MacosNotesDriver;

#[derive(Clone, Debug, Parser)]
#[command(
  name = "auv-apple-notes",
  disable_help_subcommand = true,
  about = "Notes app commands"
)]
struct CliArgs {
  #[command(subcommand)]
  command: CliCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum CliCommand {
  /// Operate on Notes notes.
  Note(NoteArgs),
}

#[derive(Clone, Debug, Args)]
struct NoteArgs {
  #[command(subcommand)]
  command: NoteSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum NoteSubcommand {
  /// Create a new note.
  New(NoteNewArgs),
  /// Write content into the current note body.
  Write(NoteWriteArgs),
  /// Compare note body text against expected content.
  Compare(NoteCompareArgs),
  /// Focus the note body.
  Focus(NoteFocusArgs),
}

#[derive(Clone, Debug, Args)]
struct NoteNewArgs {
  #[arg(long = "app-id", default_value = DEFAULT_APP_ID)]
  app_id: String,
  #[arg(long = "settle-ms", default_value_t = DEFAULT_SETTLE_MS)]
  settle_ms: u64,
}

#[derive(Clone, Debug, Args)]
struct NoteWriteArgs {
  #[arg(value_name = "content")]
  content: String,
  #[arg(long = "new")]
  new_note: bool,
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
  #[arg(long = "create-settle-ms", default_value_t = DEFAULT_SETTLE_MS)]
  create_settle_ms: u64,
  #[arg(long = "input-settle-ms", default_value_t = DEFAULT_SETTLE_MS)]
  input_settle_ms: u64,
}

#[derive(Clone, Debug, Args)]
struct NoteCompareArgs {
  #[arg(value_name = "content")]
  content: String,
  #[arg(long = "role", default_value = DEFAULT_BODY_ROLE)]
  role: String,
  #[arg(long = "app-id", default_value = DEFAULT_APP_ID)]
  app_id: String,
}

#[derive(Clone, Debug, Args)]
struct NoteFocusArgs {
  #[arg(long = "query", default_value = DEFAULT_FOCUS_QUERY)]
  query: String,
  #[arg(long = "candidate", default_value = "")]
  candidate: String,
  #[arg(long = "app-id", default_value = DEFAULT_APP_ID)]
  app_id: String,
}

pub fn parse_from<I, T>(args: I) -> Result<NoteCommand, clap::Error>
where
  I: IntoIterator<Item = T>,
  T: Into<std::ffi::OsString> + Clone,
{
  let args = CliArgs::try_parse_from(args)?;
  Ok(match args.command {
    CliCommand::Note(note) => match note.command {
      NoteSubcommand::New(args) => NoteCommand::New(NoteNew {
        app_id: args.app_id,
        settle_ms: args.settle_ms,
      }),
      NoteSubcommand::Write(args) => NoteCommand::Write(NoteWrite {
        app_id: args.app_id,
        content: args.content,
        new_note: args.new_note,
        replace: args.replace,
        verify: args.verify,
        focus_query: args.focus_query,
        focus_candidate: args.focus_candidate,
        compare_role: args.role,
        activate_settle_ms: args.activate_settle_ms,
        create_settle_ms: args.create_settle_ms,
        input_settle_ms: args.input_settle_ms,
      }),
      NoteSubcommand::Compare(args) => NoteCommand::Compare(NoteCompare {
        app_id: args.app_id,
        content: args.content,
        role: args.role,
      }),
      NoteSubcommand::Focus(args) => NoteCommand::Focus(NoteFocus {
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
  let mut driver = match MacosNotesDriver::open_local() {
    Ok(driver) => driver,
    Err(error) => {
      eprintln!("{error}");
      return ExitCode::from(1);
    }
  };
  match run_note_command(&command, &mut driver) {
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
  fn parse_note_new_maps_defaults() {
    let command = parse_from(["auv-apple-notes", "note", "new"]).expect("command should parse");

    assert_eq!(command, NoteCommand::New(NoteNew::defaults()));
  }

  #[test]
  fn parse_note_write_maps_content_and_flags() {
    let command = parse_from([
      "auv-apple-notes",
      "note",
      "write",
      "hello",
      "--new",
      "--replace",
      "--verify",
    ])
    .expect("command should parse");

    assert_eq!(
      command,
      NoteCommand::Write(NoteWrite {
        app_id: DEFAULT_APP_ID.to_string(),
        content: "hello".to_string(),
        new_note: true,
        replace: true,
        verify: true,
        focus_query: DEFAULT_FOCUS_QUERY.to_string(),
        focus_candidate: String::new(),
        compare_role: DEFAULT_BODY_ROLE.to_string(),
        activate_settle_ms: DEFAULT_SETTLE_MS,
        create_settle_ms: DEFAULT_SETTLE_MS,
        input_settle_ms: DEFAULT_SETTLE_MS,
      })
    );
  }

  #[test]
  fn parse_note_compare_maps_role() {
    let command = parse_from([
      "auv-apple-notes",
      "note",
      "compare",
      "hello",
      "--role",
      "AXStaticText",
    ])
    .expect("command should parse");

    assert_eq!(
      command,
      NoteCommand::Compare(NoteCompare {
        app_id: DEFAULT_APP_ID.to_string(),
        content: "hello".to_string(),
        role: "AXStaticText".to_string(),
      })
    );
  }

  #[test]
  fn parse_note_focus_maps_query() {
    let command = parse_from([
      "auv-apple-notes",
      "note",
      "focus",
      "--query",
      "First Text View",
    ])
    .expect("command should parse");

    assert_eq!(
      command,
      NoteCommand::Focus(NoteFocus {
        app_id: DEFAULT_APP_ID.to_string(),
        query: "First Text View".to_string(),
        candidate: String::new(),
      })
    );
  }
}
