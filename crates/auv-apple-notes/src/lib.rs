pub mod cli;
pub mod commands;
pub mod driver;

pub use commands::note::{
  DEFAULT_APP_ID, DEFAULT_BODY_ROLE, DEFAULT_FOCUS_QUERY, DEFAULT_NOTE_TEXT, DEFAULT_SETTLE_MS,
  NoteCommand, NoteCommandReport, NoteCompare, NoteFocus, NoteNew, NoteWrite, run_note_command,
};
pub use driver::{
  MacosNotesDriver, NotesDriver, OperationResult, StepOutcome, VerificationOutcome,
};
