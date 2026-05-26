use std::time::Duration;

use auv_driver::{Driver as TypedDriver, PasteTextOptions, TextSubmit};

use crate::model::AuvResult;

// Explicit compatibility shims from the legacy root driver into
// `auv-driver-macos`.
//
// The legacy `macos.desktop` command adapter remains the default runtime
// surface. When it needs to borrow typed crate behavior, keep that borrowing
// centralized here instead of letting command handlers talk to
// `auv-driver-macos` directly.

pub(crate) mod descriptor {
  pub(crate) fn legacy_descriptor_metadata() -> auv_driver_macos::MacosLegacyDescriptorMetadata {
    auv_driver_macos::macos_legacy_descriptor_metadata()
  }
}

pub(crate) mod observe {
  pub(crate) use auv_driver_macos::observe::{
    find_ax_text_node, ocr_detection_signals, permission_probe_report, preferred_ax_signal_text,
    render_window_list_json, render_window_snapshot_report, row_detection_signals,
    verify_ax_text_signals, verify_now_playing_title_signals, wait_ocr_detection_signals,
    wait_row_detection_signals,
  };
}

pub(crate) mod session {
  use super::*;

  pub(crate) fn try_paste_text_preserve_clipboard(
    text: &str,
    replace_existing: bool,
    submit: Option<TextSubmit>,
    submit_settle_ms: u64,
  ) -> AuvResult<bool> {
    let Some(submit) = submit else {
      return Ok(false);
    };
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    session
      .input()
      .paste_text(PasteTextOptions {
        text: text.to_string(),
        replace_existing,
        submit,
        settle: Duration::from_millis(submit_settle_ms),
      })
      .map_err(|error| format!("typed macOS paste_text adapter failed: {error}"))?;
    Ok(true)
  }
}
