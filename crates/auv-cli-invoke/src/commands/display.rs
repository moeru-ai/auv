use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportTable,
  InvokeReportValue, invoke_command,
};
use auv_cli_common::{TableRow, outputs::formats::table::TableOptions};
use clap::Args;

use auv_tracing::ArtifactMetadata;

use crate::artifact::emit_capture_with_receipt;
#[cfg(target_os = "macos")]
use auv_driver::overlay::{Overlay, components::CaptureFrame};
#[cfg(target_os = "macos")]
use std::time::Duration;

pub fn group() -> CommandGroup {
  // TODO(invoke-display-stubs): point commands stay intentionally unregistered
  // until an owner-approved implementation has behavioral evidence.
  CommandGroup::new("display", "DISPLAY").command(capture_display_invoke_command()).command(list_displays_invoke_command())
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke display.capture")]
struct CaptureDisplayArgs {}

#[invoke_command(
  id = "display.capture",
  group = "display",
  description = "Capture the primary display with its screenshot-to-logical coordinate contract.",
  input = CaptureDisplayArgs,
)]
async fn capture_display(input: InvokeCommandInput, _args: CaptureDisplayArgs) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  #[cfg(target_os = "macos")]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    let (result, artifact) = capture_primary_display_recorded_with_session(&session).await?;
    let capture_overlay = Overlay::new().with_layer(
      CaptureFrame::new(result.display.frame)
        .with_label(result.display.name.clone().unwrap_or_else(|| format!("display {}", result.display.id))),
    );
    let overlay = super::overlay::show_overlay(
      &input,
      &session,
      capture_overlay,
      auv_driver::overlay::ShowOptions::new()
        .with_motion_ease(Duration::from_millis(120), auv_driver::overlay::Easing::EaseInOutExpo)
        .with_auto_removal_after(Duration::from_millis(180)),
    )?;
    let mut output = display_capture_output(&result.display, super::capture_result(&result.capture), artifact)?;
    output.report.as_mut().expect("display capture output always has a report").fields.push(overlay.report_field());
    Ok(output)
  }
  #[cfg(any(target_os = "linux", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    let (result, artifact) = capture_primary_display_recorded_with_session(&session).await?;
    display_capture_output(&result.display, super::capture_result(&result.capture), artifact)
  }
  #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
  {
    Err("display.capture is not available on this platform".to_string())
  }
}

/// Records and projects a Runner-held capture. `evidence` is its pixels when
/// this call records artifacts; without them the result reports metadata only.
pub async fn recorded_display_capture_output(
  display: &auv_driver::Display,
  capture: super::CaptureResult<'_>,
  evidence: Option<&auv_driver::Capture>,
) -> InvokeCommandResult {
  let artifact = match evidence {
    Some(capture) => emit_capture_with_receipt("auv.driver.display_capture", capture).await,
    None => None,
  };
  display_capture_output(display, capture, artifact)
}

fn display_capture_output(
  display: &auv_driver::Display,
  capture: super::CaptureResult<'_>,
  artifact: Option<ArtifactMetadata>,
) -> InvokeCommandResult {
  let report = display_capture_report(display, &capture);
  Ok(InvokeCommandOutput::from_result(&super::display_capture_result(display, capture))?.with_report(report).with_artifacts(artifact))
}

pub async fn capture_primary_display() -> Result<auv_driver::DisplayCapture, String> {
  capture_primary_display_recorded().await.map(|(capture, _)| capture)
}

async fn capture_primary_display_recorded() -> Result<(auv_driver::DisplayCapture, Option<ArtifactMetadata>), String> {
  #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    capture_primary_display_recorded_with_session(&session).await
  }
  #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
  {
    Err("display.capture is not available on this platform".to_string())
  }
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
async fn capture_primary_display_recorded_with_session(
  session: &auv_driver::LocalDriverSession,
) -> Result<(auv_driver::DisplayCapture, Option<ArtifactMetadata>), String> {
  let result = session.display().capture(auv_driver::CaptureOptions::default()).map_err(|error| error.to_string())?;
  let artifact = emit_capture_with_receipt("auv.driver.display_capture", &result.capture).await;
  Ok((result, artifact))
}

#[derive(Clone, Debug, Args, serde::Serialize, serde::Deserialize)]
#[command(after_long_help = "Examples:\n  auv invoke display.list --json")]
struct ListDisplaysArgs {}

#[invoke_command(
  id = "display.list",
  group = "display",
  description = "List connected displays using the normalized AUV coordinate contract.",
  input = ListDisplaysArgs,
)]
async fn list_displays(input: InvokeCommandInput, _args: ListDisplaysArgs) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let displays = observe_displays().await?;
  list_displays_output(&displays)
}

/// Builds the transport-independent direct result for `display.list`.
///
/// Local and daemon-backed frontends use this same projection so selecting a
/// Device changes placement without creating a second command result schema.
pub fn list_displays_output(displays: &auv_driver::ObservedDisplays) -> InvokeCommandResult {
  Ok(InvokeCommandOutput::from_result(displays)?.with_report(display_list_report(&displays.displays)))
}

pub async fn observe_displays() -> Result<auv_driver::ObservedDisplays, String> {
  #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
  {
    let session = auv::local::open().map_err(|error| error.to_string())?;
    session.display().list().map_err(|error| error.to_string())
  }
  #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
  {
    Err("display.list is not available on this platform".to_string())
  }
}

fn display_capture_report(display: &auv_driver::Display, capture: &super::CaptureResult<'_>) -> InvokeReport {
  let mut fields = vec![
    InvokeReportField::new("Display", display.name.clone().unwrap_or_else(|| format!("display {}", display.id))),
    InvokeReportField::new("Display ID", display.id.clone()),
    InvokeReportField::new("Display frame", display.frame.report_value()),
    InvokeReportField::new("Capture bounds", capture.bounds.report_value()),
    InvokeReportField::new("Pixel size", capture.pixel_dimensions.report_value()),
    InvokeReportField::new("Scale factor", format!("{:.3}", capture.scale_factor)),
  ];
  if let Some(reason) = capture.fallback_reason {
    fields.push(InvokeReportField::new("Fallback reason", reason));
  }
  InvokeReport::new(fields, Vec::new())
}

#[derive(TableRow)]
struct DisplayRow {
  #[table(header = "REF")]
  reference: String,
  role: &'static str,
  name: String,
  frame: String,
  #[table(display_with = |scale: &f64| format!("{scale:.3}"))]
  scale: f64,
  #[table(wide)]
  kind: &'static str,
}

fn display_list_report(displays: &[auv_driver::Display]) -> InvokeReport {
  let rows = displays
    .iter()
    .map(|display| DisplayRow {
      reference: display.id.clone(),
      role: if display.is_primary {
        "primary"
      } else {
        "secondary"
      },
      name: display.name.clone().unwrap_or_else(|| format!("display {}", display.id)),
      frame: display.frame.report_value(),
      scale: display.scale_factor,
      kind: match display.is_builtin {
        Some(true) => "built-in",
        Some(false) => "external",
        None => "unknown",
      },
    })
    .collect::<Vec<_>>();
  InvokeReport {
    fields: vec![InvokeReportField::new(
      "Result",
      format!("{} display(s)", displays.len()),
    )],
    tables: vec![InvokeReportTable::from_rows(&rows, TableOptions::default())],
    wide_tables: vec![InvokeReportTable::from_rows(
      &rows,
      TableOptions::default().wide(true),
    )],
    sections: Vec::new(),
  }
}

#[cfg(test)]
#[path = "display_test.rs"]
mod tests;
