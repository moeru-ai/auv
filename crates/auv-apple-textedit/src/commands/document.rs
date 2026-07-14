use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::driver::{OperationResult, StepOutcome, TextEditDriver, VerificationOutcome};

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
  pub outcomes: Vec<StepOutcome>,
  pub verification: Option<VerificationOutcome>,
}

pub fn run_document_command(command: &DocumentCommand, driver: &mut impl TextEditDriver) -> OperationResult<DocumentCommandReport> {
  match command {
    DocumentCommand::Write(command) => run_write(command, driver),
    DocumentCommand::Compare(command) => run_compare(command, driver),
    DocumentCommand::Focus(command) => run_focus(command, driver),
  }
}

fn run_write(command: &DocumentWrite, driver: &mut impl TextEditDriver) -> OperationResult<DocumentCommandReport> {
  let mut outcomes = vec![
    driver.activate_app(&command.app_id, Duration::from_millis(command.activate_settle_ms))?,
    driver.focus_text_input(&command.app_id, &command.focus_query, &command.focus_candidate)?,
    driver.paste_text_preserve_clipboard(
      &command.app_id,
      &command.content,
      command.replace,
      Duration::from_millis(command.input_settle_ms),
    )?,
  ];
  let verification = if command.verify {
    Some(driver.verify_ax_text(&command.app_id, &command.content, &command.compare_role)?)
  } else {
    None
  };
  normalize_write_step_ids(&mut outcomes);
  Ok(DocumentCommandReport {
    command: "document.write",
    outcomes,
    verification,
  })
}

fn run_compare(command: &DocumentCompare, driver: &mut impl TextEditDriver) -> OperationResult<DocumentCommandReport> {
  let verification = driver.verify_ax_text(&command.app_id, &command.content, &command.role)?;
  Ok(DocumentCommandReport {
    command: "document.compare",
    outcomes: Vec::new(),
    verification: Some(verification),
  })
}

fn run_focus(command: &DocumentFocus, driver: &mut impl TextEditDriver) -> OperationResult<DocumentCommandReport> {
  let outcome = driver.focus_text_input(&command.app_id, &command.query, &command.candidate)?;
  Ok(DocumentCommandReport {
    command: "document.focus",
    outcomes: vec![outcome],
    verification: None,
  })
}

fn normalize_write_step_ids(outcomes: &mut [StepOutcome]) {
  for (index, outcome) in outcomes.iter_mut().enumerate() {
    outcome.step_id = match index {
      0 => "document-write.activate",
      1 => "document-write.focus",
      2 => "document-write.paste",
      _ => outcome.step_id,
    };
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_driver::{InputActionResult, InputDeliveryPath};

  #[derive(Default)]
  struct RecordingTextEditDriver {
    calls: Vec<String>,
  }

  impl TextEditDriver for RecordingTextEditDriver {
    fn activate_app(&mut self, app_id: &str, settle: Duration) -> OperationResult<StepOutcome> {
      self.calls.push(format!("activate:{app_id}:{}", settle.as_millis()));
      Ok(StepOutcome {
        step_id: "activate",
        summary: "activated".to_string(),
        input_action_result: None,
      })
    }

    fn focus_text_input(&mut self, app_id: &str, query: &str, candidate: &str) -> OperationResult<StepOutcome> {
      self.calls.push(format!("focus:{app_id}:{query}:{candidate}"));
      Ok(StepOutcome {
        step_id: "focus",
        summary: "focused".to_string(),
        input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse)),
      })
    }

    fn paste_text_preserve_clipboard(
      &mut self,
      app_id: &str,
      text: &str,
      replace_existing: bool,
      settle: Duration,
    ) -> OperationResult<StepOutcome> {
      self.calls.push(format!("paste:{app_id}:{text}:{replace_existing}:{}", settle.as_millis()));
      Ok(StepOutcome {
        step_id: "paste",
        summary: "pasted".to_string(),
        input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::ClipboardPaste)),
      })
    }

    fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> OperationResult<VerificationOutcome> {
      self.calls.push(format!("compare:{app_id}:{target_text}:{target_role}"));
      Ok(VerificationOutcome {
        matched_role: target_role.to_string(),
        matched_text: format!("prefix {target_text} suffix"),
        artifact_count: 1,
        semantic_matched: true,
        observation_path: Some("0.1.2".to_string()),
        observation_pid: Some(1),
      })
    }
  }

  #[test]
  fn document_write_runs_focus_paste_and_optional_compare() {
    let command = DocumentCommand::Write(DocumentWrite::marker_defaults());
    let mut driver = RecordingTextEditDriver::default();

    let report = run_document_command(&command, &mut driver).expect("command should run");

    assert_eq!(report.command, "document.write");
    assert_eq!(
      driver.calls,
      vec![
        "activate:com.apple.TextEdit:250",
        "focus:com.apple.TextEdit:First Text View:",
        "paste:com.apple.TextEdit:AUV_TEXTEDIT_MARKER_2026_05_17:true:250",
        "compare:com.apple.TextEdit:AUV_TEXTEDIT_MARKER_2026_05_17:AXTextArea",
      ]
    );
    assert_eq!(
      report.outcomes.iter().map(|outcome| outcome.step_id).collect::<Vec<_>>(),
      vec![
        "document-write.activate",
        "document-write.focus",
        "document-write.paste"
      ]
    );
    assert!(report.verification.is_some());
  }

  #[test]
  fn document_compare_only_verifies_body_text() {
    let command = DocumentCommand::Compare(DocumentCompare {
      app_id: DEFAULT_APP_ID.to_string(),
      content: "hello".to_string(),
      role: DEFAULT_BODY_ROLE.to_string(),
    });
    let mut driver = RecordingTextEditDriver::default();

    let report = run_document_command(&command, &mut driver).expect("command should run");

    assert_eq!(report.command, "document.compare");
    assert_eq!(driver.calls, vec!["compare:com.apple.TextEdit:hello:AXTextArea"]);
    assert!(report.outcomes.is_empty());
    assert!(report.verification.is_some());
  }

  #[test]
  fn document_focus_is_a_debuggable_document_subcommand() {
    let command = DocumentCommand::Focus(DocumentFocus {
      app_id: DEFAULT_APP_ID.to_string(),
      query: DEFAULT_FOCUS_QUERY.to_string(),
      candidate: String::new(),
    });
    let mut driver = RecordingTextEditDriver::default();

    let report = run_document_command(&command, &mut driver).expect("command should run");

    assert_eq!(report.command, "document.focus");
    assert_eq!(driver.calls, vec!["focus:com.apple.TextEdit:First Text View:"]);
    assert_eq!(report.outcomes.len(), 1);
  }
}
