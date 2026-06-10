use std::time::Duration;

use auv_driver::{
  ActivationPolicy, App, Driver, InputActionResult, InputDeliveryPath, PasteTextOptions,
  PrepareForInputOptions, TextSubmit, Window, WindowSelector,
};
use auv_driver_macos::MacosDriverSession;
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
  pub matched_text: String,
  pub artifact_count: usize,
}

pub trait TextEditDriver {
  fn activate_app(&mut self, app_id: &str, settle: Duration) -> OperationResult<StepOutcome>;

  fn focus_text_input(
    &mut self,
    app_id: &str,
    query: &str,
    candidate: &str,
  ) -> OperationResult<StepOutcome>;

  fn paste_text_preserve_clipboard(
    &mut self,
    app_id: &str,
    text: &str,
    replace_existing: bool,
    settle: Duration,
  ) -> OperationResult<StepOutcome>;

  fn verify_ax_text(
    &mut self,
    app_id: &str,
    target_text: &str,
    target_role: &str,
  ) -> OperationResult<VerificationOutcome>;
}

pub struct MacosTextEditDriver {
  session: MacosDriverSession,
}

impl MacosTextEditDriver {
  pub fn open_local() -> OperationResult<Self> {
    let session = auv_driver_macos::MacosDriver::new()
      .open_local()
      .map_err(|error| error.to_string())?;
    Ok(Self { session })
  }

  pub fn from_session(session: MacosDriverSession) -> Self {
    Self { session }
  }

  fn main_window(&self, app_id: &str) -> OperationResult<Window> {
    self
      .session
      .window()
      .resolve(main_window_selector(app_id))
      .map_err(|error| error.to_string())
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

  fn focus_text_input(
    &mut self,
    _app_id: &str,
    _query: &str,
    _candidate: &str,
  ) -> OperationResult<StepOutcome> {
    // TODO(auv-driver-macos-ax-focus): `document focus` needs a typed
    // AX-query focus API in `auv-driver-macos`; implement this adapter method
    // once it is available without routing through root runtime.rs.
    Err("typed TextEdit AX text-input focus is not available yet".to_string())
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
      input_action_result: Some(InputActionResult::single_success(
        InputDeliveryPath::ClipboardPaste,
      )),
    })
  }

  fn verify_ax_text(
    &mut self,
    _app_id: &str,
    _target_text: &str,
    _target_role: &str,
  ) -> OperationResult<VerificationOutcome> {
    // TODO(auv-driver-macos-ax-verify): `document compare` needs typed AX text
    // observation in `auv-driver-macos` before this app-local command can
    // replace legacy verify.axText end-to-end.
    Err("typed TextEdit AX text verification is not available yet".to_string())
  }
}

fn main_window_selector(app_id: &str) -> WindowSelector {
  WindowSelector {
    app: Some(App::bundle_id(app_id)),
    title: None,
    main_visible: true,
  }
}
