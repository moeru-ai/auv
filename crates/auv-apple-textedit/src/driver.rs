use std::time::Duration;

use auv_driver::InputActionResult;
use serde::{Deserialize, Serialize};

pub type OperationResult<T> = Result<T, String>;

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
  fn activate_app(&mut self, app_id: &str, settle: Duration) -> OperationResult<StepOutcome>;

  fn focus_text_input(&mut self, app_id: &str, query: &str, candidate: &str) -> OperationResult<StepOutcome>;

  fn paste_text_preserve_clipboard(
    &mut self,
    app_id: &str,
    text: &str,
    replace_existing: bool,
    settle: Duration,
  ) -> OperationResult<StepOutcome>;

  fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> OperationResult<VerificationOutcome>;
}

#[cfg(target_os = "macos")]
mod macos {
  use std::time::Duration;

  use auv_driver::LocalDriverSession;
  use auv_driver::{
    ActivationPolicy, App, InputActionResult, InputDeliveryPath, PasteTextOptions, PrepareForInputOptions, TextSubmit, Window,
    WindowSelector,
  };
  use auv_driver_macos::MacosDriverSession;

  use super::{OperationResult, StepOutcome, TextEditDriver, VerificationOutcome};

  pub struct MacosTextEditDriver {
    session: LocalDriverSession,
  }

  impl MacosTextEditDriver {
    pub fn open_local() -> OperationResult<Self> {
      let session = auv_driver::open_local().map_err(|error| error.to_string())?;
      Ok(Self { session })
    }

    pub fn from_session(session: MacosDriverSession) -> Self {
      Self {
        session: LocalDriverSession::Macos(session),
      }
    }

    fn main_window(&self, app_id: &str) -> OperationResult<Window> {
      self.session.window().resolve(main_window_selector(app_id)).map_err(|error| error.to_string())
    }
  }

  impl TextEditDriver for MacosTextEditDriver {
    fn activate_app(&mut self, app_id: &str, settle: Duration) -> OperationResult<StepOutcome> {
      let window = self.main_window(app_id)?;
      self
        .session
        .window()
        .prepare_for_input(
          &window,
          PrepareForInputOptions {
            activation: ActivationPolicy::Foreground { settle },
            preserve_frontmost: false,
            install_focus_guard: false,
            settle: Duration::ZERO,
          },
        )
        .map_err(|error| error.to_string())?;
      Ok(StepOutcome {
        step_id: "activate-target-app",
        summary: format!("activated foreground TextEdit window for {app_id}"),
        input_action_result: None,
      })
    }

    fn focus_text_input(&mut self, app_id: &str, query: &str, candidate: &str) -> OperationResult<StepOutcome> {
      let observation = self
        .session
        .accessibility()
        .focus_text_by_query(app_id, query, Some("AXTextArea"), candidate)
        .map_err(|error| format!("TextEdit AX focus failed: {error}"))?;
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
    ) -> OperationResult<StepOutcome> {
      self
        .session
        .input()
        .paste_text(PasteTextOptions {
          text: text.to_string(),
          replace_existing,
          submit: TextSubmit::No,
          settle,
        })
        .map_err(|error| error.to_string())?;
      Ok(StepOutcome {
        step_id: "document-write",
        summary: "pasted TextEdit document body through auv-driver-macos clipboard input".to_string(),
        input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::ClipboardPaste)),
      })
    }

    fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> OperationResult<VerificationOutcome> {
      // Observation is independent of expected text; mismatch returns Ok with
      // semantic_matched=false so callers can persist a VerificationResult.
      let observation = self
        .session
        .accessibility()
        .verify_text(app_id, target_text, target_role)
        .map_err(|error| format!("TextEdit AX text observation failed: {error}"))?;
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

  fn main_window_selector(app_id: &str) -> WindowSelector {
    WindowSelector {
      app: Some(App::bundle_id(app_id)),
      title: None,
      main_visible: true,
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
  pub fn open_local() -> OperationResult<Self> {
    Err("MacosTextEditDriver requires macOS".to_string())
  }
}

#[cfg(not(target_os = "macos"))]
impl TextEditDriver for MacosTextEditDriver {
  fn activate_app(&mut self, _app_id: &str, _settle: Duration) -> OperationResult<StepOutcome> {
    Err("MacosTextEditDriver requires macOS".to_string())
  }

  fn focus_text_input(&mut self, _app_id: &str, _query: &str, _candidate: &str) -> OperationResult<StepOutcome> {
    Err("MacosTextEditDriver requires macOS".to_string())
  }

  fn paste_text_preserve_clipboard(
    &mut self,
    _app_id: &str,
    _text: &str,
    _replace_existing: bool,
    _settle: Duration,
  ) -> OperationResult<StepOutcome> {
    Err("MacosTextEditDriver requires macOS".to_string())
  }

  fn verify_ax_text(&mut self, _app_id: &str, _target_text: &str, _target_role: &str) -> OperationResult<VerificationOutcome> {
    Err("MacosTextEditDriver requires macOS".to_string())
  }
}
