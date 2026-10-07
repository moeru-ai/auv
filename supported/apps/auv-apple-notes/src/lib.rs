//! Apple Notes commands over the macOS driver. The app only exists on macOS,
//! so the library is empty elsewhere and the binary exits with a message.
#![cfg(target_os = "macos")]

pub mod app;
pub mod cli;
pub mod commands;
pub mod driver;

pub use app::{Note, NotesApp};
pub use commands::note::{
  DEFAULT_APP_ID, DEFAULT_BODY_ROLE, DEFAULT_FOCUS_QUERY, DEFAULT_NOTE_TEXT, DEFAULT_SETTLE_MS, NoteCommand, NoteCommandReport, NoteCompare,
  NoteFocus, NoteNew, NoteWrite,
};
pub use driver::{MacosNotesDriver, NoteAction, NoteActionResult, NotesDriver, VerificationOutcome};
