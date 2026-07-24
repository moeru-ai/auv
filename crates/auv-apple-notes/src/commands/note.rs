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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::driver::NoteAction;
  use auv_driver::{InputActionResult, InputDeliveryPath};

  #[derive(Default)]
  struct RecordingNotesDriver {
    calls: Vec<String>,
  }

  impl NotesDriver for RecordingNotesDriver {
    fn activate_app(&mut self, app_id: &str, settle: Duration) -> Result<NoteActionResult, String> {
      self.calls.push(format!("activate:{app_id}:{}", settle.as_millis()));
      Ok(NoteActionResult {
        action: NoteAction::Activate,
        input_action_result: None,
      })
    }

    fn create_note(&mut self, app_id: &str, settle: Duration) -> Result<NoteActionResult, String> {
      self.calls.push(format!("new:{app_id}:{}", settle.as_millis()));
      Ok(NoteActionResult {
        action: NoteAction::Create,
        input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::AxPress)),
      })
    }

    fn focus_note_body(&mut self, app_id: &str, query: &str, candidate: &str) -> Result<NoteActionResult, String> {
      self.calls.push(format!("focus:{app_id}:{query}:{candidate}"));
      Ok(NoteActionResult {
        action: NoteAction::FocusBody,
        input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::AxFocus)),
      })
    }

    fn paste_text_preserve_clipboard(
      &mut self,
      app_id: &str,
      text: &str,
      replace_existing: bool,
      settle: Duration,
    ) -> Result<NoteActionResult, String> {
      self.calls.push(format!("paste:{app_id}:{text}:{replace_existing}:{}", settle.as_millis()));
      Ok(NoteActionResult {
        action: NoteAction::PasteText,
        input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::ClipboardPaste)),
      })
    }

    fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> Result<VerificationOutcome, String> {
      self.calls.push(format!("compare:{app_id}:{target_text}:{target_role}"));
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
    assert_eq!(driver.calls, vec!["activate:com.apple.Notes:250", "new:com.apple.Notes:250"]);
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
      report.actions.iter().map(|action| action.action).collect::<Vec<_>>(),
      vec![
        NoteAction::Activate,
        NoteAction::Create,
        NoteAction::FocusBody,
        NoteAction::PasteText,
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
      report.actions.iter().map(|action| action.action).collect::<Vec<_>>(),
      vec![
        NoteAction::Activate,
        NoteAction::FocusBody,
        NoteAction::PasteText
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
    assert_eq!(driver.calls, vec!["compare:com.apple.Notes:hello:AXTextArea"]);
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
    assert_eq!(driver.calls, vec!["focus:com.apple.Notes:Note Body Text View:"]);
  }

  #[cfg(feature = "tracing")]
  #[test]
  fn command_uses_the_caller_context_without_owning_a_run() {
    use std::sync::Arc;

    use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let mut driver = RecordingNotesDriver::default();

    let result = root.in_scope(|| run_note_command(&NoteCommand::New(NoteNew::defaults()), &mut driver));

    assert!(result.is_ok());
    futures_executor::block_on(dispatch.flush()).expect("flush");
    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).expect("snapshot read").expect("run snapshot");
    let span = snapshot.spans().values().next().expect("command span");
    assert_eq!(span.started().name().as_str(), "auv.apple_notes.note.new");
    assert!(span.started().attributes().is_empty());
    assert!(span.ended().is_some());
  }
}
