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
