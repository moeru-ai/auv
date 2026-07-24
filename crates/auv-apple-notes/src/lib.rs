pub mod cli;
pub mod commands;
pub mod driver;

mod tracing {
  pub(super) fn note_new<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.apple_notes.note.new", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(super) fn note_write<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.apple_notes.note.write", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(super) fn note_compare<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.apple_notes.note.compare", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(super) fn note_focus<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.apple_notes.note.focus", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }
}

pub use commands::note::{
  DEFAULT_APP_ID, DEFAULT_BODY_ROLE, DEFAULT_FOCUS_QUERY, DEFAULT_NOTE_TEXT, DEFAULT_SETTLE_MS, NoteCommand, NoteCommandReport, NoteCompare,
  NoteFocus, NoteNew, NoteWrite, run_note_command,
};
pub use driver::{MacosNotesDriver, NoteAction, NoteActionResult, NotesDriver, VerificationOutcome};
