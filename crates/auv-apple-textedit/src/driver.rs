use std::time::Duration;

use auv_driver::{DriverResult, InputActionResult};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepOutcome {
  pub step_id: &'static str,
  pub summary: String,
  pub input_action_result: Option<InputActionResult>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationOutcome {
  pub matched_role: String,
  /// Observed AX text value (independent of the expected/target text).
  pub matched_text: String,
  pub artifact_count: usize,
  /// Whether observed text contains the requested target text.
  pub semantic_matched: bool,
  /// Optional AX path / pid metadata for product invoke evidence staging.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub observation_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub observation_pid: Option<i32>,
}

pub trait TextEditDriver {
  fn activate_app(&mut self, app_id: &str, settle: Duration) -> DriverResult<StepOutcome>;

  fn focus_text_input(&mut self, app_id: &str, query: &str, candidate: &str) -> DriverResult<StepOutcome>;

  fn paste_text_preserve_clipboard(
    &mut self,
    app_id: &str,
    text: &str,
    replace_existing: bool,
    settle: Duration,
  ) -> DriverResult<StepOutcome>;

  fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> DriverResult<VerificationOutcome>;
}

#[cfg(target_os = "macos")]
mod macos {
  use std::time::Duration;

  use auv_driver::LocalDriverSession;
  use auv_driver::{InputActionResult, InputDeliveryPath, PasteTextOptions, TextSubmit};
  use auv_driver_macos::{ApplicationControl, MacosDriverSession};

  use super::{DriverResult, StepOutcome, TextEditDriver, VerificationOutcome};

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

  impl TextEditDriver for MacosTextEditDriver {
    fn activate_app(&mut self, app_id: &str, settle: Duration) -> DriverResult<StepOutcome> {
      self.session.activate_bundle_id(app_id, settle)?;
      Ok(StepOutcome {
        step_id: "activate-target-app",
        summary: format!("activated foreground TextEdit application for {app_id} without requiring WindowServer discovery"),
        input_action_result: None,
      })
    }

    fn focus_text_input(&mut self, app_id: &str, query: &str, candidate: &str) -> DriverResult<StepOutcome> {
      let observation = self.session.accessibility().focus_text_by_query(app_id, query, Some("AXTextArea"), candidate)?;
      Ok(StepOutcome {
        step_id: "focus-text-input",
        summary: format!("focused TextEdit AX text input path={} role={}", observation.path, observation.role),
        input_action_result: Some(observation.input_action_result),
      })
    }

    fn paste_text_preserve_clipboard(
      &mut self,
      _app_id: &str,
      text: &str,
      replace_existing: bool,
      settle: Duration,
    ) -> DriverResult<StepOutcome> {
      self.session.input().paste_text(PasteTextOptions {
        text: text.to_string(),
        replace_existing,
        submit: TextSubmit::No,
        settle,
      })?;
      Ok(StepOutcome {
        step_id: "document-write",
        summary: "pasted TextEdit document body through auv-driver-macos clipboard input".to_string(),
        input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::ClipboardPaste)),
      })
    }

    fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> DriverResult<VerificationOutcome> {
      // Observation is independent of expected text; mismatch returns Ok with
      // semantic_matched=false so callers can persist a VerificationResult.
      let observation = self.session.accessibility().verify_text(app_id, target_text, target_role)?;
      Ok(VerificationOutcome {
        matched_role: observation.role,
        matched_text: observation.matched_text,
        artifact_count: 1,
        semantic_matched: observation.semantic_matched,
        observation_path: Some(observation.path),
        observation_pid: Some(observation.pid),
      })
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
  fn activate_app(&mut self, _app_id: &str, _settle: Duration) -> DriverResult<StepOutcome> {
    Err(auv_driver::DriverError::Unsupported {
      operation: "MacosTextEditDriver.activate_app",
    })
  }

  fn focus_text_input(&mut self, _app_id: &str, _query: &str, _candidate: &str) -> DriverResult<StepOutcome> {
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
  ) -> DriverResult<StepOutcome> {
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
