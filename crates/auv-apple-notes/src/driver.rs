use std::time::Duration;

use auv_driver::LocalDriverSession;
use auv_driver::{
  ActivationPolicy, App, InputActionResult, InputDeliveryPath, PasteTextOptions, PrepareForInputOptions, TextSubmit, Window, WindowSelector,
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

pub trait NotesDriver {
  fn activate_app(&mut self, app_id: &str, settle: Duration) -> OperationResult<StepOutcome>;

  fn create_note(&mut self, app_id: &str, settle: Duration) -> OperationResult<StepOutcome>;

  fn focus_note_body(&mut self, app_id: &str, query: &str, candidate: &str) -> OperationResult<StepOutcome>;

  fn paste_text_preserve_clipboard(
    &mut self,
    app_id: &str,
    text: &str,
    replace_existing: bool,
    settle: Duration,
  ) -> OperationResult<StepOutcome>;

  fn verify_ax_text(&mut self, app_id: &str, target_text: &str, target_role: &str) -> OperationResult<VerificationOutcome>;
}

pub struct MacosNotesDriver {
  session: LocalDriverSession,
}

impl MacosNotesDriver {
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

impl NotesDriver for MacosNotesDriver {
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
      step_id: "note.activate",
      summary: format!("activated foreground Notes window for {app_id}"),
      input_action_result: None,
    })
  }

  fn create_note(&mut self, _app_id: &str, _settle: Duration) -> OperationResult<StepOutcome> {
    // TODO(auv-driver-macos-ax-press): `note new` needs a typed AX button
    // press API before this crate can create a note without root legacy
    // debug.axPressButton behavior.
    Err("typed Notes AX note creation is not available yet".to_string())
  }

  fn focus_note_body(&mut self, _app_id: &str, _query: &str, _candidate: &str) -> OperationResult<StepOutcome> {
    // TODO(auv-driver-macos-ax-focus): `note focus` needs typed AX text-input
    // focus before app-local Notes commands can safely paste into the note
    // body.
    Err("typed Notes AX body focus is not available yet".to_string())
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
      step_id: "note-write.paste",
      summary: "pasted Notes body through auv-driver-macos clipboard input".to_string(),
      input_action_result: Some(InputActionResult::single_success(InputDeliveryPath::ClipboardPaste)),
    })
  }

  fn verify_ax_text(&mut self, _app_id: &str, _target_text: &str, _target_role: &str) -> OperationResult<VerificationOutcome> {
    // TODO(auv-driver-macos-ax-verify): `note compare` needs typed AX text
    // observation in `auv-driver-macos` before Notes can verify note body text
    // without root legacy verify.axText behavior.
    Err("typed Notes AX text verification is not available yet".to_string())
  }
}

fn main_window_selector(app_id: &str) -> WindowSelector {
  WindowSelector {
    app: Some(App::bundle_id(app_id)),
    title: None,
    main_visible: true,
  }
}
