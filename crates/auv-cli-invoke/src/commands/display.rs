use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, InvokeReport, InvokeReportField, InvokeReportTable,
  InvokeReportTableRow, InvokeReportValue,
  arg::{NO_ARGS, TARGET_ARGS},
  artifact::emit_png,
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
  description = "Capture one display screenshot with a coordinate contract through xcap. If activate_target_before_capture is true, the target app is foregrounded first.",
  args = TARGET_ARGS,
)]
async fn capture_display(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let result = capture_primary_display().await?;
  Ok(
    InvokeCommandOutput::from_result(&super::display_capture_result(&result.display, &result.capture))?
      .with_report(display_capture_report(&result)),
  )
}

pub async fn capture_primary_display() -> Result<auv_driver::DisplayCapture, String> {
  #[cfg(target_os = "macos")]
  {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    let result = session.display().capture(auv_driver::CaptureOptions::default()).map_err(|error| error.to_string())?;
    emit_png("auv.driver.display_capture", &result.capture.image);
    Ok(result)
  }
  #[cfg(not(target_os = "macos"))]
  {
    Err("display.capture is only available on macOS through auv-driver-macos".to_string())
  }
}

#[invoke_command(
  id = "display.list",
  group = "display",
  description = "List connected displays using the normalized AUV coordinate contract.",
  args = NO_ARGS,
)]
async fn list_displays(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    return Ok(InvokeCommandOutput::completed());
  }
  let displays = observe_displays().await?;
  Ok(InvokeCommandOutput::from_result(&displays)?.with_report(display_list_report(&displays.displays)))
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
  description = "Project main-display screenshot pixels back into AUV global logical coordinates.",
  args = NO_ARGS,
)]
async fn project_screenshot_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-display-point): add screenshot x/y arguments and expose the
  // projection through DisplayApi before implementing this command.
  unimplemented!("display.projectScreenshotPoint")
}

#[invoke_command(
  id = "display.identifyPoint",
  group = "display",
  description = "Resolve a logical desktop point against the current macOS display layout.",
  args = NO_ARGS,
)]
async fn identify_point(_input: InvokeCommandInput) -> InvokeCommandResult {
  // TODO(invoke-display-point): add logical x/y arguments and expose display
  // point resolution through DisplayApi before implementing this command.
  unimplemented!("display.identifyPoint")
}

fn display_capture_report(result: &auv_driver::DisplayCapture) -> InvokeReport {
  let mut fields = vec![
    InvokeReportField::new("Display", display_label(&result.display)),
    InvokeReportField::new("Display ID", result.display.id.clone()),
    InvokeReportField::new("Display frame", result.display.frame.report_value()),
    InvokeReportField::new("Capture bounds", result.capture.bounds.report_value()),
    InvokeReportField::new("Pixel size", format!("{}x{}", result.capture.image.width(), result.capture.image.height())),
    InvokeReportField::new("Scale factor", format!("{:.3}", result.capture.scale_factor)),
  ];
  if let Some(reason) = result.capture.fallback_reason.as_deref() {
    fields.push(InvokeReportField::new("Fallback reason", reason));
  }
  InvokeReport::new(fields, Vec::new())
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
    tables: vec![InvokeReportTable::new(
      &["REF", "ROLE", "NAME", "FRAME", "SCALE"],
      displays
        .iter()
        .map(|display| {
          InvokeReportTableRow::new([
            display.id.clone(),
            if display.is_primary {
              "primary"
            } else {
              "secondary"
            }
            .to_string(),
            display_label(display),
            display.frame.report_value(),
            format!("{:.3}", display.scale_factor),
          ])
        })
        .collect(),
    )],
    wide_tables: vec![InvokeReportTable::new(
      &["REF", "ROLE", "NAME", "FRAME", "SCALE", "KIND"],
      displays
        .iter()
        .map(|display| {
          InvokeReportTableRow::new([
            display.id.clone(),
            if display.is_primary {
              "primary"
            } else {
              "secondary"
            }
            .to_string(),
            display_label(display),
            display.frame.report_value(),
            format!("{:.3}", display.scale_factor),
            match display.is_builtin {
              Some(true) => "built-in",
              Some(false) => "external",
              None => "unknown",
            }
            .to_string(),
          ])
        })
        .collect(),
    )],
    sections: Vec::new(),
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::{
    Capture, CoordinateSpace, Display, DisplayCapture,
    geometry::{Point, Rect, Size},
  };
  use image::RgbaImage;

  use super::*;

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

    let observed = auv_driver::ObservedDisplays { displays };
    let output = InvokeCommandOutput::from_result(&observed)
      .expect("display result should serialize")
      .with_report(display_list_report(&observed.displays));
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
    assert_eq!(output.result(), Some(&serde_json::to_value(&observed).expect("fixture should serialize")));
  }

  #[test]
  fn display_capture_result_keeps_pixels_out_of_json() {
    let capture = DisplayCapture {
      display: Display {
        id: "display_0".to_string(),
        name: Some("Fixture Display".to_string()),
        frame: Rect::new(0.0, 0.0, 1440.0, 900.0),
        coordinate_space: CoordinateSpace::Screen,
        scale_factor: 2.0,
        is_primary: true,
        is_builtin: Some(true),
      },
      capture: Capture {
        image: RgbaImage::new(2880, 1800),
        bounds: Rect::new(0.0, 0.0, 1440.0, 900.0),
        scale_factor: 2.0,
        backend: "fixture-capture".to_string(),
        fallback_reason: None,
      },
    };

    let output = InvokeCommandOutput::from_result(&super::super::display_capture_result(&capture.display, &capture.capture))
      .expect("capture result should serialize")
      .with_report(display_capture_report(&capture));
    let result = output.result().expect("capture should have a result");

    assert_eq!(result["display"]["id"], "display_0");
    assert_eq!(result["capture"]["pixel_dimensions"]["width"], 2880);
    assert_eq!(result["capture"]["pixel_dimensions"]["height"], 1800);
    assert_eq!(result["capture"]["backend"], "fixture-capture");
    assert!(result.get("image").is_none());
  }
}
