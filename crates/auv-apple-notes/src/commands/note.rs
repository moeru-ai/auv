use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::driver::{NoteActionResult, NotesDriver, VerificationOutcome};

pub const DEFAULT_APP_ID: &str = "com.apple.Notes";
pub const DEFAULT_NOTE_TEXT: &str = "AUV_NOTE_MARKER_2026_05_21_V2";
pub const DEFAULT_FOCUS_QUERY: &str = "Note Body Text View";
pub const DEFAULT_BODY_ROLE: &str = "AXTextArea";
pub const DEFAULT_SETTLE_MS: u64 = 250;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NoteCommand {
  New(NoteNew),
  Write(NoteWrite),
  Compare(NoteCompare),
  Focus(NoteFocus),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteNew {
  pub app_id: String,
  pub settle_ms: u64,
}

impl NoteNew {
  pub fn defaults() -> Self {
    Self {
      app_id: DEFAULT_APP_ID.to_string(),
      settle_ms: DEFAULT_SETTLE_MS,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteWrite {
  pub app_id: String,
  pub content: String,
  pub new_note: bool,
  pub replace: bool,
  pub verify: bool,
  pub focus_query: String,
  pub focus_candidate: String,
  pub compare_role: String,
  pub activate_settle_ms: u64,
  pub create_settle_ms: u64,
  pub input_settle_ms: u64,
}

impl NoteWrite {
  pub fn defaults_with_content(content: impl Into<String>) -> Self {
    Self {
      app_id: DEFAULT_APP_ID.to_string(),
      content: content.into(),
      new_note: false,
      replace: false,
      verify: false,
      focus_query: DEFAULT_FOCUS_QUERY.to_string(),
      focus_candidate: String::new(),
      compare_role: DEFAULT_BODY_ROLE.to_string(),
      activate_settle_ms: DEFAULT_SETTLE_MS,
      create_settle_ms: DEFAULT_SETTLE_MS,
      input_settle_ms: DEFAULT_SETTLE_MS,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteCompare {
  pub app_id: String,
  pub content: String,
  pub role: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteFocus {
  pub app_id: String,
  pub query: String,
  pub candidate: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoteCommandReport {
  pub command: &'static str,
  pub actions: Vec<NoteActionResult>,
  pub verification: Option<VerificationOutcome>,
}

pub fn run_note_command(command: &NoteCommand, driver: &mut impl NotesDriver) -> Result<NoteCommandReport, String> {
  match command {
    NoteCommand::New(command) => crate::tracing::note_new(|| run_new(command, driver)),
    NoteCommand::Write(command) => crate::tracing::note_write(|| run_write(command, driver)),
    NoteCommand::Compare(command) => crate::tracing::note_compare(|| run_compare(command, driver)),
    NoteCommand::Focus(command) => crate::tracing::note_focus(|| run_focus(command, driver)),
  }
}

fn run_new(command: &NoteNew, driver: &mut impl NotesDriver) -> Result<NoteCommandReport, String> {
  let actions = vec![
    driver.activate_app(&command.app_id, Duration::from_millis(command.settle_ms))?,
    driver.create_note(&command.app_id, Duration::from_millis(command.settle_ms))?,
  ];
  Ok(NoteCommandReport {
    command: "note.new",
    actions,
    verification: None,
  })
}

fn run_write(command: &NoteWrite, driver: &mut impl NotesDriver) -> Result<NoteCommandReport, String> {
  let mut actions = vec![driver.activate_app(&command.app_id, Duration::from_millis(command.activate_settle_ms))?];
  if command.new_note {
    actions.push(driver.create_note(&command.app_id, Duration::from_millis(command.create_settle_ms))?);
  }
  actions.push(driver.focus_note_body(&command.app_id, &command.focus_query, &command.focus_candidate)?);
  actions.push(driver.paste_text_preserve_clipboard(
    &command.app_id,
    &command.content,
    command.replace,
    Duration::from_millis(command.input_settle_ms),
  )?);
  let verification = if command.verify {
    Some(driver.verify_ax_text(&command.app_id, &command.content, &command.compare_role)?)
  } else {
    None
  };
  Ok(NoteCommandReport {
    command: "note.write",
    actions,
    verification,
  })
}

fn run_compare(command: &NoteCompare, driver: &mut impl NotesDriver) -> Result<NoteCommandReport, String> {
  let verification = driver.verify_ax_text(&command.app_id, &command.content, &command.role)?;
  Ok(NoteCommandReport {
    command: "note.compare",
    actions: Vec::new(),
    verification: Some(verification),
  })
}

fn run_focus(command: &NoteFocus, driver: &mut impl NotesDriver) -> Result<NoteCommandReport, String> {
  let action = driver.focus_note_body(&command.app_id, &command.query, &command.candidate)?;
  Ok(NoteCommandReport {
    command: "note.focus",
    actions: vec![action],
    verification: None,
  })
}
