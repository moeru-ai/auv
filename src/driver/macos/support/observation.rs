// File: src/driver/macos/support/observation.rs
use super::super::*;
use crate::driver::macos::capture::types::DisplayDescriptor;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct DisplaySelection {
  pub(crate) display_ref: Option<String>,
  pub(crate) native_display_id: Option<String>,
  pub(crate) main: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedScreenCaptureSource {
  pub(crate) display_ref: String,
  pub(crate) native_display_id: String,
  pub(crate) selection_reason: String,
}

pub(crate) fn parse_display_selection(call: &DriverCall) -> AuvResult<Option<DisplaySelection>> {
  let display_ref = optional_non_empty_string(call, "display_ref");
  let native_display_id = optional_non_empty_string(call, "native_display_id")
    .or_else(|| optional_non_empty_string(call, "display_id"));
  let main = optional_bool(call, "main")?.unwrap_or(false);
  if display_ref.is_none() && native_display_id.is_none() && !main {
    return Ok(None);
  }
  Ok(Some(DisplaySelection {
    display_ref,
    native_display_id,
    main,
  }))
}

pub(crate) fn resolve_screen_capture_source(
  displays: &[DisplayDescriptor],
  display_selection: Option<&DisplaySelection>,
  target_window: Option<&WindowCandidate>,
) -> AuvResult<ResolvedScreenCaptureSource> {
  if let Some(selection) = display_selection {
    if let Some(display_ref) = selection.display_ref.as_deref() {
      let display = displays
        .iter()
        .find(|display| display.display_ref == display_ref)
        .ok_or_else(|| {
          format!("display selector --display_ref {display_ref} did not match current displays")
        })?;
      return Ok(source_from_display(display, "explicit-display-ref"));
    }

    if let Some(native_display_id) = selection.native_display_id.as_deref() {
      let display = displays
        .iter()
        .find(|display| display.native_display_id == native_display_id)
        .ok_or_else(|| {
          format!(
            "display selector --native_display_id {native_display_id} did not match current displays"
          )
        })?;
      return Ok(source_from_display(display, "explicit-native-display-id"));
    }

    if selection.main {
      let display = main_or_first_display(displays)?;
      return Ok(source_from_display(display, "explicit-main-display"));
    }
  }

  if let Some(candidate) = target_window
    && let (Some(display_ref), Some(native_display_id)) = (
      candidate.display_ref.as_ref(),
      candidate.native_display_id.as_ref(),
    )
  {
    return Ok(ResolvedScreenCaptureSource {
      display_ref: display_ref.clone(),
      native_display_id: native_display_id.clone(),
      selection_reason: "target-window-display".to_string(),
    });
  }

  Ok(source_from_display(
    main_or_first_display(displays)?,
    "main-display-fallback",
  ))
}

fn main_or_first_display(displays: &[DisplayDescriptor]) -> AuvResult<&DisplayDescriptor> {
  displays
    .iter()
    .find(|display| display.is_main)
    .or_else(|| displays.first())
    .ok_or_else(|| "display list is empty".to_string())
}

fn source_from_display(
  display: &DisplayDescriptor,
  selection_reason: &str,
) -> ResolvedScreenCaptureSource {
  ResolvedScreenCaptureSource {
    display_ref: display.display_ref.clone(),
    native_display_id: display.native_display_id.clone(),
    selection_reason: selection_reason.to_string(),
  }
}
