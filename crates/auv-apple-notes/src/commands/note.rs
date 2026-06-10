use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::driver::{NotesDriver, OperationResult, StepOutcome, VerificationOutcome};

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
  pub outcomes: Vec<StepOutcome>,
  pub verification: Option<VerificationOutcome>,
}

pub fn run_note_command(
  command: &NoteCommand,
  driver: &mut impl NotesDriver,
) -> OperationResult<NoteCommandReport> {
  match command {
    NoteCommand::New(command) => run_new(command, driver),
    NoteCommand::Write(command) => run_write(command, driver),
    NoteCommand::Compare(command) => run_compare(command, driver),
    NoteCommand::Focus(command) => run_focus(command, driver),
  }
}

fn run_new(command: &NoteNew, driver: &mut impl NotesDriver) -> OperationResult<NoteCommandReport> {
  let outcomes = vec![
    driver.activate_app(&command.app_id, Duration::from_millis(command.settle_ms))?,
    driver.create_note(&command.app_id, Duration::from_millis(command.settle_ms))?,
  ];
  Ok(NoteCommandReport {
    command: "note.new",
    outcomes,
    verification: None,
  })
}

fn run_write(
  command: &NoteWrite,
  driver: &mut impl NotesDriver,
) -> OperationResult<NoteCommandReport> {
  let mut outcomes = vec![driver.activate_app(
    &command.app_id,
    Duration::from_millis(command.activate_settle_ms),
  )?];
  if command.new_note {
    outcomes.push(driver.create_note(
      &command.app_id,
      Duration::from_millis(command.create_settle_ms),
    )?);
  }
  outcomes.push(driver.focus_note_body(
    &command.app_id,
    &command.focus_query,
    &command.focus_candidate,
  )?);
  outcomes.push(driver.paste_text_preserve_clipboard(
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
  normalize_write_step_ids(&mut outcomes);
  Ok(NoteCommandReport {
    command: "note.write",
    outcomes,
    verification,
  })
}

fn run_compare(
  command: &NoteCompare,
  driver: &mut impl NotesDriver,
) -> OperationResult<NoteCommandReport> {
  let verification = driver.verify_ax_text(&command.app_id, &command.content, &command.role)?;
  Ok(NoteCommandReport {
    command: "note.compare",
    outcomes: Vec::new(),
    verification: Some(verification),
  })
}

fn run_focus(
  command: &NoteFocus,
  driver: &mut impl NotesDriver,
) -> OperationResult<NoteCommandReport> {
  let outcome = driver.focus_note_body(&command.app_id, &command.query, &command.candidate)?;
  Ok(NoteCommandReport {
    command: "note.focus",
    outcomes: vec![outcome],
    verification: None,
  })
}

fn normalize_write_step_ids(outcomes: &mut [StepOutcome]) {
  for outcome in outcomes.iter_mut() {
    outcome.step_id = match outcome.step_id {
      "activate" => "note-write.activate",
      "note.create" => "note-write.new",
      "focus" => "note-write.focus",
      "paste" => "note-write.paste",
      _ => outcome.step_id,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_driver::{InputActionResult, InputDeliveryPath};

  #[derive(Default)]
  struct RecordingNotesDriver {
    calls: Vec<String>,
  }

  impl NotesDriver for RecordingNotesDriver {
    fn activate_app(&mut self, app_id: &str, settle: Duration) -> OperationResult<StepOutcome> {
      self
        .calls
        .push(format!("activate:{app_id}:{}", settle.as_millis()));
      Ok(StepOutcome {
        step_id: "activate",
        summary: "activated".to_string(),
        input_action_result: None,
      })
    }

    fn create_note(&mut self, app_id: &str, settle: Duration) -> OperationResult<StepOutcome> {
      self
        .calls
        .push(format!("new:{app_id}:{}", settle.as_millis()));
      Ok(StepOutcome {
        step_id: "note.create",
        summary: "created".to_string(),
        input_action_result: Some(InputActionResult::single_success(
          InputDeliveryPath::AxPress,
        )),
      })
    }

    fn focus_note_body(
      &mut self,
      app_id: &str,
      query: &str,
      candidate: &str,
    ) -> OperationResult<StepOutcome> {
      self
        .calls
        .push(format!("focus:{app_id}:{query}:{candidate}"));
      Ok(StepOutcome {
        step_id: "focus",
        summary: "focused".to_string(),
        input_action_result: Some(InputActionResult::single_success(
          InputDeliveryPath::AxFocus,
        )),
      })
    }

    fn paste_text_preserve_clipboard(
      &mut self,
      app_id: &str,
      text: &str,
      replace_existing: bool,
      settle: Duration,
    ) -> OperationResult<StepOutcome> {
      self.calls.push(format!(
        "paste:{app_id}:{text}:{replace_existing}:{}",
        settle.as_millis()
      ));
      Ok(StepOutcome {
        step_id: "paste",
        summary: "pasted".to_string(),
        input_action_result: Some(InputActionResult::single_success(
          InputDeliveryPath::ClipboardPaste,
        )),
      })
    }

    fn verify_ax_text(
      &mut self,
      app_id: &str,
      target_text: &str,
      target_role: &str,
    ) -> OperationResult<VerificationOutcome> {
      self
        .calls
        .push(format!("compare:{app_id}:{target_text}:{target_role}"));
      Ok(VerificationOutcome {
        matched_role: target_role.to_string(),
        matched_text: format!("prefix {target_text} suffix"),
        artifact_count: 1,
      })
    }
  }

  #[test]
  fn note_new_activates_and_creates_note() {
    let command = NoteCommand::New(NoteNew::defaults());
    let mut driver = RecordingNotesDriver::default();

    let report = run_note_command(&command, &mut driver).expect("command should run");

    assert_eq!(report.command, "note.new");
    assert_eq!(
      driver.calls,
      vec!["activate:com.apple.Notes:250", "new:com.apple.Notes:250"]
    );
  }

  #[test]
  fn note_write_can_create_focus_paste_and_verify() {
    let mut command = NoteWrite::defaults_with_content(DEFAULT_NOTE_TEXT);
    command.new_note = true;
    command.verify = true;
    let command = NoteCommand::Write(command);
    let mut driver = RecordingNotesDriver::default();

    let report = run_note_command(&command, &mut driver).expect("command should run");

    assert_eq!(report.command, "note.write");
    assert_eq!(
      driver.calls,
      vec![
        "activate:com.apple.Notes:250",
        "new:com.apple.Notes:250",
        "focus:com.apple.Notes:Note Body Text View:",
        "paste:com.apple.Notes:AUV_NOTE_MARKER_2026_05_21_V2:false:250",
        "compare:com.apple.Notes:AUV_NOTE_MARKER_2026_05_21_V2:AXTextArea",
      ]
    );
    assert_eq!(
      report
        .outcomes
        .iter()
        .map(|outcome| outcome.step_id)
        .collect::<Vec<_>>(),
      vec![
        "note-write.activate",
        "note-write.new",
        "note-write.focus",
        "note-write.paste"
      ]
    );
    assert!(report.verification.is_some());
  }

  #[test]
  fn note_write_without_new_or_verify_focuses_and_pastes_existing_note() {
    let command = NoteCommand::Write(NoteWrite::defaults_with_content("hello"));
    let mut driver = RecordingNotesDriver::default();

    let report = run_note_command(&command, &mut driver).expect("command should run");

    assert_eq!(
      driver.calls,
      vec![
        "activate:com.apple.Notes:250",
        "focus:com.apple.Notes:Note Body Text View:",
        "paste:com.apple.Notes:hello:false:250",
      ]
    );
    assert_eq!(
      report
        .outcomes
        .iter()
        .map(|outcome| outcome.step_id)
        .collect::<Vec<_>>(),
      vec![
        "note-write.activate",
        "note-write.focus",
        "note-write.paste"
      ]
    );
    assert!(report.verification.is_none());
  }

  #[test]
  fn note_compare_only_verifies_body_text() {
    let command = NoteCommand::Compare(NoteCompare {
      app_id: DEFAULT_APP_ID.to_string(),
      content: "hello".to_string(),
      role: DEFAULT_BODY_ROLE.to_string(),
    });
    let mut driver = RecordingNotesDriver::default();

    let report = run_note_command(&command, &mut driver).expect("command should run");

    assert_eq!(report.command, "note.compare");
    assert_eq!(
      driver.calls,
      vec!["compare:com.apple.Notes:hello:AXTextArea"]
    );
  }

  #[test]
  fn note_focus_is_a_debuggable_note_subcommand() {
    let command = NoteCommand::Focus(NoteFocus {
      app_id: DEFAULT_APP_ID.to_string(),
      query: DEFAULT_FOCUS_QUERY.to_string(),
      candidate: String::new(),
    });
    let mut driver = RecordingNotesDriver::default();

    let report = run_note_command(&command, &mut driver).expect("command should run");

    assert_eq!(report.command, "note.focus");
    assert_eq!(
      driver.calls,
      vec!["focus:com.apple.Notes:Note Body Text View:"]
    );
  }
}
