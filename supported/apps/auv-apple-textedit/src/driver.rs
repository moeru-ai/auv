use std::time::Duration;

use auv_driver::{DriverResult, InputActionResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextEditAction {
  Activate,
  FocusTextInput,
  PasteText,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEditActionResult {
  pub action: TextEditAction,
  pub input_action_result: Option<InputActionResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationOutcome {
  pub matched_role: String,
  /// Observed AX text value (independent of the expected/target text).
  pub matched_text: String,
  pub artifact_count: usize,
  /// Whether observed text contains the requested target text.
  pub semantic_matched: bool,
  /// The AX node that supplied the matched text, when the backend exposes it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub matched_node: Option<MatchedAxNode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatchedAxNode {
  pub path: String,
  pub process_id: i32,
}

pub trait TextEditDriver {
  fn activate_app(&mut self, app_id: &str, settle: Duration) -> DriverResult<TextEditActionResult>;

  fn focus_text_input(&mut self, app_id: &str, query: &str, candidate: &str) -> DriverResult<TextEditActionResult>;

  fn paste_text_preserve_clipboard(
    &mut self,
    app_id: &str,
    text: &str,
    replace_existing: bool,
    settle: Duration,
  ) -> DriverResult<TextEditActionResult>;

  fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> DriverResult<VerificationOutcome>;
}

#[cfg(target_os = "macos")]
mod macos {
  use std::time::Duration;

  use auv_driver::LocalDriverSession;
  use auv_driver::{PasteTextOptions, TextSubmit};
  use auv_driver_macos::{ApplicationControl, AxTextRead, MacosDriverSession};

  use super::{DriverResult, MatchedAxNode, TextEditAction, TextEditActionResult, TextEditDriver, VerificationOutcome};

  pub struct MacosTextEditDriver {
    session: LocalDriverSession,
  }

  impl MacosTextEditDriver {
    pub fn open_local() -> DriverResult<Self> {
      let session = auv_driver::open_local()?;
      Ok(Self { session })
    }

    pub fn from_session(session: MacosDriverSession) -> Self {
      Self {
        session: LocalDriverSession::Macos(session),
      }
    }
  }

  fn verification_outcome(read: AxTextRead, target_text: &str) -> VerificationOutcome {
    let semantic_matched = read.matched_text.contains(target_text);
    VerificationOutcome {
      matched_role: read.role,
      matched_text: read.matched_text,
      artifact_count: 1,
      semantic_matched,
      matched_node: Some(MatchedAxNode {
        path: read.path,
        process_id: read.pid,
      }),
    }
  }

  impl TextEditDriver for MacosTextEditDriver {
    fn activate_app(&mut self, app_id: &str, settle: Duration) -> DriverResult<TextEditActionResult> {
      self.session.activate_bundle_id(app_id, settle)?;
      Ok(TextEditActionResult {
        action: TextEditAction::Activate,
        input_action_result: None,
      })
    }

    fn focus_text_input(&mut self, app_id: &str, query: &str, candidate: &str) -> DriverResult<TextEditActionResult> {
      let selector = if candidate.trim().is_empty() {
        auv_driver::AxTextSelector::Query(query.to_string())
      } else {
        auv_driver::AxTextSelector::Path(candidate.to_string())
      };
      let focus = self.session.accessibility().focus_text(auv_driver::FocusTextOptions {
        app: app_id.to_string(),
        selector,
        expected_role: Some("AXTextArea".to_string()),
      })?;
      Ok(TextEditActionResult {
        action: TextEditAction::FocusTextInput,
        input_action_result: Some(focus.input_action_result),
      })
    }

    fn paste_text_preserve_clipboard(
      &mut self,
      _app_id: &str,
      text: &str,
      replace_existing: bool,
      settle: Duration,
    ) -> DriverResult<TextEditActionResult> {
      let result = self.session.input().paste_text(PasteTextOptions {
        text: text.to_string(),
        replace_existing,
        submit: TextSubmit::No,
        settle,
      })?;
      Ok(TextEditActionResult {
        action: TextEditAction::PasteText,
        input_action_result: Some(result),
      })
    }

    fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> DriverResult<VerificationOutcome> {
      let read = self.session.accessibility().verify_text(app_id, target_text, target_role)?;
      Ok(verification_outcome(read, target_text))
    }
  }
}

#[cfg(target_os = "macos")]
pub use macos::MacosTextEditDriver;

/// Non-macOS stub so `auv-apple-textedit` remains checkable on Linux CI hosts.
#[cfg(not(target_os = "macos"))]
#[derive(Debug, Default)]
pub struct MacosTextEditDriver;

#[cfg(not(target_os = "macos"))]
impl MacosTextEditDriver {
  pub fn open_local() -> DriverResult<Self> {
    Err(auv_driver::DriverError::Unsupported {
      operation: "MacosTextEditDriver.open_local",
    })
  }
}

#[cfg(not(target_os = "macos"))]
impl TextEditDriver for MacosTextEditDriver {
  fn activate_app(&mut self, _app_id: &str, _settle: Duration) -> DriverResult<TextEditActionResult> {
    Err(auv_driver::DriverError::Unsupported {
      operation: "MacosTextEditDriver.activate_app",
    })
  }

  fn focus_text_input(&mut self, _app_id: &str, _query: &str, _candidate: &str) -> DriverResult<TextEditActionResult> {
    Err(auv_driver::DriverError::Unsupported {
      operation: "MacosTextEditDriver.focus_text_input",
    })
  }

  fn paste_text_preserve_clipboard(
    &mut self,
    _app_id: &str,
    _text: &str,
    _replace_existing: bool,
    _settle: Duration,
  ) -> DriverResult<TextEditActionResult> {
    Err(auv_driver::DriverError::Unsupported {
      operation: "MacosTextEditDriver.paste_text_preserve_clipboard",
    })
  }

  fn verify_ax_text(&mut self, _app_id: &str, _target_text: &str, _target_role: &str) -> DriverResult<VerificationOutcome> {
    Err(auv_driver::DriverError::Unsupported {
      operation: "MacosTextEditDriver.verify_ax_text",
    })
  }
}
