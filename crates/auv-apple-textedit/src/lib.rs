pub mod cli;
pub mod commands;
pub mod driver;

mod tracing {
  pub(super) fn document_write<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.apple_textedit.document.write", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(super) fn document_compare<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.apple_textedit.document.compare", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(super) fn document_focus<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.apple_textedit.document.focus", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }
}

pub use auv_driver::DriverResult;
pub use commands::document::{
  DEFAULT_APP_ID, DEFAULT_BODY_ROLE, DEFAULT_FOCUS_QUERY, DEFAULT_MARKER_TEXT, DEFAULT_SETTLE_MS, DocumentCommand, DocumentCommandReport,
  DocumentCompare, DocumentFocus, DocumentWrite, run_document_command, run_document_command_with_checkpoint,
};
pub use driver::{MacosTextEditDriver, MatchedAxNode, TextEditAction, TextEditActionResult, TextEditDriver, VerificationOutcome};
