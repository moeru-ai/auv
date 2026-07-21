pub mod cli;
pub mod commands;
pub mod driver;

pub use auv_driver::DriverResult;
pub use commands::document::{
  DEFAULT_APP_ID, DEFAULT_BODY_ROLE, DEFAULT_FOCUS_QUERY, DEFAULT_MARKER_TEXT, DEFAULT_SETTLE_MS, DocumentCommand, DocumentCommandReport,
  DocumentCompare, DocumentFocus, DocumentWrite, run_document_command,
};
pub use driver::{MacosTextEditDriver, StepOutcome, TextEditDriver, VerificationOutcome};
