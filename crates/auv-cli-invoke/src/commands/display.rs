use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportTable,
  InvokeReportTableRow, InvokeReportValue, InvokeSignalValue,
  arg::{NO_ARGS, TARGET_ARGS},
  artifact::{ArtifactInstrumentationReceipt, ArtifactPublication},
  invoke_command,
};

pub fn group() -> CommandGroup {
  CommandGroup::new("display", "DISPLAY")
    .command(capture_display_invoke_command())
    .command(list_displays_invoke_command())
    .command(project_screenshot_point_invoke_command())
    .command(identify_point_invoke_command())
}

#[invoke_command(
  id = "display.capture",
  group = "display",
  summary = "Capture one display screenshot with a coordinate contract through xcap. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = TARGET_ARGS,
)]
async fn capture_display(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::new("dry run: display.capture would capture the primary display"));
  }
  capture_primary_display().await.map(|publication| {
    let (result, instrumentation) = publication.into_parts();
    let mut output = InvokeCommandOutput::new(format!(
      "Captured {} through {} ({}x{} pixels).",
      display_label(&result.display),
      result.capture.backend,
      result.capture.image.width(),
      result.capture.image.height()
    ));
    output.backend = Some(result.capture.backend.clone());
    insert_display_signals(&mut output, "display", &result.display);
    output.signals.insert("capture.pixel_width".to_string(), result.capture.image.width().to_string());
    output.signals.insert("capture.pixel_height".to_string(), result.capture.image.height().to_string());
    output.signals.insert("capture.bounds".to_string(), result.capture.bounds.signal_value());
    output.signals.insert("capture.scale_factor".to_string(), format!("{:.3}", result.capture.scale_factor));
    if let Some(reason) = result.capture.fallback_reason {
      output.signals.insert("capture.fallback_reason".to_string(), reason);
    }
    // TODO(invoke-capture-contract-artifacts): this handler records the screenshot
    // and coordinate signals, but not the old standalone capture-contract
    // artifact. Add the contract artifact after its direct-invoke JSON shape is
    // accepted in `2026-06-18-invoke-direct-command-implementations-handoff.md`.
    output.verification = Some("capture-only; no semantic success claim".to_string());
    output
      .known_limits
      .push("display.capture records a screenshot and coordinate signals only; it does not verify UI semantics.".to_string());
    output.artifact_failures = instrumentation.into_failures();
    output
  })
}

pub async fn capture_primary_display() -> Result<ArtifactPublication<auv_driver::DisplayCapture>, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let result = session.display().capture(auv_driver::CaptureOptions::default()).map_err(|error| error.to_string())?;
    let mut instrumentation = ArtifactInstrumentationReceipt::default();
    instrumentation.publish_png("auv.driver.display_capture", &result.capture.image).await;
    Ok(ArtifactPublication::new(result, instrumentation))
  }
  #[cfg(not(target_os = "macos"))]
  {
    Err("display.capture is only available on macOS through auv-driver-macos".to_string())
  }
}

#[invoke_command(
  id = "display.list",
  group = "display",
  summary = "List connected displays using the normalized AUV coordinate contract.",
  args = NO_ARGS,
)]
async fn list_displays(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::new("dry run: display.list would enumerate connected displays"));
  }
  observe_displays().await.map(|displays| display_list_output(&displays.displays))
}

pub async fn observe_displays() -> Result<auv_driver::ObservedDisplays, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    session.display().list().map_err(|error| error.to_string())
  }
  #[cfg(not(target_os = "macos"))]
  {
    Err("display.list is only available on macOS through auv-driver-macos".to_string())
  }
}

#[invoke_command(
  id = "display.projectScreenshotPoint",
  group = "display",
  summary = "Project main-display screenshot pixels back into AUV global logical coordinates.",
  args = NO_ARGS,
)]
async fn project_screenshot_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  project_primary_screenshot_point().await?;
  Ok(InvokeCommandOutput::new("projected screenshot point"))
}

pub async fn project_primary_screenshot_point() -> Result<(), String> {
  // TODO(invoke-display-typed-api): projectScreenshotPoint needs a typed
  // display projection API before this invoke command can replace root-driver
  // routing.
  Err("display.projectScreenshotPoint requires a typed display API for screenshot point projection".to_string())
}

#[invoke_command(
  id = "display.identifyPoint",
  group = "display",
  summary = "Resolve a logical desktop point against the current macOS display layout.",
  args = NO_ARGS,
)]
async fn identify_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  identify_display_point().await?;
  Ok(InvokeCommandOutput::new("identified display point"))
}

pub async fn identify_display_point() -> Result<(), String> {
  // TODO(invoke-display-typed-api): identifyPoint needs a typed display point
  // resolution API before this invoke command can replace root-driver routing.
  Err("display.identifyPoint requires a typed display API for point identification".to_string())
}

fn display_list_output(displays: &[auv_driver::Display]) -> InvokeCommandOutput {
  let primary = displays.iter().find(|display| display.is_primary).or_else(|| displays.first());
  let mut output = InvokeCommandOutput::new(match primary {
    Some(display) => format!(
      "Listed {} display(s); primary display is {} at {:.0}x{:.0} logical.",
      displays.len(),
      display_label(display),
      display.frame.size.width,
      display.frame.size.height
    ),
    None => "Listed 0 display(s).".to_string(),
  });
  output.backend = Some("auv-driver-macos.display".to_string());
  output.report = Some(display_list_report(displays));
  output.signals.insert("display.count".to_string(), displays.len().to_string());
  if let Some(display) = primary {
    insert_display_signals(&mut output, "display.primary", display);
  }
  for (index, display) in displays.iter().take(5).enumerate() {
    insert_display_signals(&mut output, &format!("display.{index}"), display);
  }
  output.verification = Some("read-only; no semantic success claim".to_string());
  output.known_limits.push("display.list records the observed display inventory only.".to_string());
  output
}

fn insert_display_signals(output: &mut InvokeCommandOutput, prefix: &str, display: &auv_driver::Display) {
  output.signals.insert(format!("{prefix}.id"), display.id.clone());
  output.signals.insert(format!("{prefix}.label"), display_label(display));
  output.signals.insert(format!("{prefix}.frame"), display.frame.signal_value());
  output.signals.insert(format!("{prefix}.scale_factor"), format!("{:.3}", display.scale_factor));
  output.signals.insert(format!("{prefix}.is_primary"), display.is_primary.to_string());
  if let Some(is_builtin) = display.is_builtin {
    output.signals.insert(format!("{prefix}.is_builtin"), is_builtin.to_string());
  }
}

fn display_label(display: &auv_driver::Display) -> String {
  display.name.clone().unwrap_or_else(|| format!("display {}", display.id))
}

fn display_list_report(displays: &[auv_driver::Display]) -> InvokeReport {
  InvokeReport {
    fields: vec![InvokeReportField::new(
      "Result",
      format!("{} display(s)", displays.len()),
    )],
    tables: vec![InvokeReportTable::from_columns(
      &["REF", "ROLE", "NAME", "FRAME", "SCALE"],
      displays
        .iter()
        .map(|display| {
          InvokeReportTableRow::from_cells([
            display.id.clone(),
            display_role(display).to_string(),
            display_label(display),
            display.frame.report_value(),
            format!("{:.3}", display.scale_factor),
          ])
        })
        .collect(),
    )],
    wide_tables: vec![InvokeReportTable::from_columns(
      &["REF", "ROLE", "NAME", "FRAME", "SCALE", "KIND"],
      displays
        .iter()
        .map(|display| {
          InvokeReportTableRow::from_cells([
            display.id.clone(),
            display_role(display).to_string(),
            display_label(display),
            display.frame.report_value(),
            format!("{:.3}", display.scale_factor),
            display_kind(display).to_string(),
          ])
        })
        .collect(),
    )],
    sections: Vec::new(),
  }
}

fn display_role(display: &auv_driver::Display) -> &'static str {
  if display.is_primary {
    "primary"
  } else {
    "secondary"
  }
}

fn display_kind(display: &auv_driver::Display) -> &'static str {
  match display.is_builtin {
    Some(true) => "built-in",
    Some(false) => "external",
    None => "unknown",
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use auv_driver::{
    CoordinateSpace, Display,
    geometry::{Point, Rect, Size},
  };

  use super::*;

  fn input(command_id: &str, inputs: &BTreeMap<String, String>) -> InvokeCommandInput {
    InvokeCommandInput {
      command_id: command_id.to_string(),
      target_application_id: None,
      inputs: inputs.clone(),
      dry_run: false,
    }
  }

  #[test]
  fn display_list_report_uses_human_first_table_and_wide_kind_column() {
    let displays = vec![
      Display {
        id: "display_0".to_string(),
        name: Some("Built-in Retina Display".to_string()),
        frame: Rect {
          origin: Point::new(0.0, 0.0),
          size: Size::new(3008.0, 1692.0),
        },
        coordinate_space: CoordinateSpace::Screen,
        scale_factor: 2.0,
        is_primary: true,
        is_builtin: Some(true),
      },
      Display {
        id: "display_1".to_string(),
        name: None,
        frame: Rect {
          origin: Point::new(3008.0, 0.0),
          size: Size::new(1920.0, 1080.0),
        },
        coordinate_space: CoordinateSpace::Screen,
        scale_factor: 1.0,
        is_primary: false,
        is_builtin: Some(false),
      },
    ];

    let output = display_list_output(&displays);
    assert!(
      output.report.is_some(),
      "display.list live path calls this helper after OS enumeration, so this stable helper test verifies report population without requiring live display state"
    );
    let report = output.report.as_ref().expect("report should be set");

    assert_eq!(report.fields[0].value, "2 display(s)");
    assert!(report.sections.is_empty());
    assert_eq!(report.tables[0].columns, ["REF", "ROLE", "NAME", "FRAME", "SCALE"]);
    assert_eq!(
      report.tables[0].rows[0].cells,
      [
        "display_0",
        "primary",
        "Built-in Retina Display",
        "0,0 3008x1692",
        "2.000"
      ]
    );
    assert_eq!(
      report.tables[0].rows[1].cells,
      [
        "display_1",
        "secondary",
        "display display_1",
        "3008,0 1920x1080",
        "1.000"
      ]
    );
    assert_eq!(report.wide_tables[0].columns, ["REF", "ROLE", "NAME", "FRAME", "SCALE", "KIND"]);
    assert_eq!(report.wide_tables[0].rows[0].cells[5], "built-in");
    assert_eq!(report.wide_tables[0].rows[1].cells[5], "external");
  }

  #[test]
  fn commands_without_typed_display_api_report_explicit_gap() {
    let inputs = BTreeMap::new();

    for command in [
      identify_point_invoke_command(),
      project_screenshot_point_invoke_command(),
    ] {
      let command_id = command.id;
      let error =
        futures_executor::block_on(command.invoke(input(command_id, &inputs))).expect_err("command should not route to root driver");

      assert!(error.contains("typed display API"), "{command_id} returned unclear error: {error}");
    }
  }
}
