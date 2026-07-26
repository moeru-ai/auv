use std::time::Duration;

use auv_driver::{DriverError, DriverResult};
use serde::{Deserialize, Serialize};

use crate::driver::{TextEditActionResult, TextEditDriver, VerificationOutcome};

pub const DEFAULT_APP_ID: &str = "com.apple.TextEdit";
pub const DEFAULT_MARKER_TEXT: &str = "AUV_TEXTEDIT_MARKER_2026_05_17";
pub const DEFAULT_FOCUS_QUERY: &str = "First Text View";
pub const DEFAULT_BODY_ROLE: &str = "AXTextArea";
pub const DEFAULT_SETTLE_MS: u64 = 250;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DocumentCommand {
  Write(DocumentWrite),
  Compare(DocumentCompare),
  Focus(DocumentFocus),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentWrite {
  pub app_id: String,
  pub content: String,
  pub replace: bool,
  pub verify: bool,
  pub focus_query: String,
  pub focus_candidate: String,
  pub compare_role: String,
  pub activate_settle_ms: u64,
  pub input_settle_ms: u64,
}

impl DocumentWrite {
  pub fn defaults_with_content(content: impl Into<String>) -> Self {
    Self {
      app_id: DEFAULT_APP_ID.to_string(),
      content: content.into(),
      replace: true,
      verify: true,
      focus_query: DEFAULT_FOCUS_QUERY.to_string(),
      focus_candidate: String::new(),
      compare_role: DEFAULT_BODY_ROLE.to_string(),
      activate_settle_ms: DEFAULT_SETTLE_MS,
      input_settle_ms: DEFAULT_SETTLE_MS,
    }
  }

  pub fn marker_defaults() -> Self {
    Self::defaults_with_content(DEFAULT_MARKER_TEXT)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentCompare {
  pub app_id: String,
  pub content: String,
  pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentFocus {
  pub app_id: String,
  pub query: String,
  pub candidate: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentCommandReport {
  pub command: &'static str,
  pub actions: Vec<TextEditActionResult>,
  pub verification: Option<VerificationOutcome>,
}

pub fn run_document_command(command: &DocumentCommand, driver: &mut impl TextEditDriver) -> DriverResult<DocumentCommandReport> {
  run_document_command_with_checkpoint(command, driver, || Ok::<_, DriverError>(()))
}

/// Runs a document command while checking a caller-owned lifecycle boundary
/// immediately before each UI-facing driver phase.
pub fn run_document_command_with_checkpoint<E>(
  command: &DocumentCommand,
  driver: &mut impl TextEditDriver,
  mut checkpoint: impl FnMut() -> Result<(), E>,
) -> Result<DocumentCommandReport, E>
where
  E: From<DriverError>,
{
  match command {
    DocumentCommand::Write(command) => crate::tracing::document_write(|| run_write(command, driver, &mut checkpoint)),
    DocumentCommand::Compare(command) => crate::tracing::document_compare(|| run_compare(command, driver, &mut checkpoint)),
    DocumentCommand::Focus(command) => crate::tracing::document_focus(|| run_focus(command, driver, &mut checkpoint)),
  }
}

fn run_write<E>(
  command: &DocumentWrite,
  driver: &mut impl TextEditDriver,
  checkpoint: &mut impl FnMut() -> Result<(), E>,
) -> Result<DocumentCommandReport, E>
where
  E: From<DriverError>,
{
  checkpoint()?;
  let mut actions = vec![driver.activate_app(&command.app_id, Duration::from_millis(command.activate_settle_ms))?];
  checkpoint()?;
  actions.push(driver.focus_text_input(&command.app_id, &command.focus_query, &command.focus_candidate)?);
  checkpoint()?;
  actions.push(driver.paste_text_preserve_clipboard(
    &command.app_id,
    &command.content,
    command.replace,
    Duration::from_millis(command.input_settle_ms),
  )?);
  let verification = if command.verify {
    checkpoint()?;
    Some(driver.verify_ax_text(&command.app_id, &command.content, &command.compare_role)?)
  } else {
    None
  };
  Ok(DocumentCommandReport {
    command: "document.write",
    actions,
    verification,
  })
}

fn run_compare<E>(
  command: &DocumentCompare,
  driver: &mut impl TextEditDriver,
  checkpoint: &mut impl FnMut() -> Result<(), E>,
) -> Result<DocumentCommandReport, E>
where
  E: From<DriverError>,
{
  checkpoint()?;
  let verification = driver.verify_ax_text(&command.app_id, &command.content, &command.role)?;
  Ok(DocumentCommandReport {
    command: "document.compare",
    actions: Vec::new(),
    verification: Some(verification),
  })
}

fn run_focus<E>(
  command: &DocumentFocus,
  driver: &mut impl TextEditDriver,
  checkpoint: &mut impl FnMut() -> Result<(), E>,
) -> Result<DocumentCommandReport, E>
where
  E: From<DriverError>,
{
  checkpoint()?;
  let action = driver.focus_text_input(&command.app_id, &command.query, &command.candidate)?;
  Ok(DocumentCommandReport {
    command: "document.focus",
    actions: vec![action],
    verification: None,
  })
}
