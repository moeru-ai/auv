use std::collections::BTreeMap;

use image::RgbaImage;

use super::*;
use crate::InvokeCancellation;

fn inputs(values: [(&str, &str); 4]) -> BTreeMap<String, String> {
  values.into_iter().map(|(name, value)| (name.to_string(), value.to_string())).collect()
}

#[test]
fn capture_region_validates_the_same_region_before_dry_and_live_branches() {
  let valid_dry_run = InvokeCommandInput {
    command_id: "screen.captureRegion".to_string(),
    target: None,
    inputs: inputs([("x", "1"), ("y", "2"), ("width", "3"), ("height", "4")]),
    typed_args: None,
    dry_run: true,
    cancellation: InvokeCancellation::new(),
  };
  assert!(futures_executor::block_on(capture_region_invoke_command().invoke(valid_dry_run)).is_ok());

  let invalid_live = InvokeCommandInput {
    command_id: "screen.captureRegion".to_string(),
    target: None,
    inputs: inputs([("x", "1"), ("y", "2"), ("width", "0"), ("height", "4")]),
    typed_args: None,
    dry_run: false,
    cancellation: InvokeCancellation::new(),
  };
  let error = futures_executor::block_on(capture_region_invoke_command().invoke(invalid_live))
    .expect_err("invalid live region must fail before capture");
  assert!(error.message.contains("greater than zero"));
}

#[test]
fn screen_text_output_returns_typed_ocr_matches() {
  let matches = auv_driver::OcrMatches {
    matches: vec![auv_driver::OcrMatch {
      text: "Pause".to_string(),
      confidence: 0.95,
      bounds: auv_driver::Rect::new(10.0, 20.0, 80.0, 24.0),
    }],
  };

  let output = screen_text_matches_output(&matches).expect("OCR result should serialize");

  assert_eq!(output.result(), Some(&serde_json::to_value(&matches).expect("fixture should serialize")));
}

#[test]
fn region_capture_result_keeps_pixels_out_of_json() {
  let capture = auv_driver::RegionCapture {
    display: auv_driver::Display {
      id: "display_1".to_string(),
      name: None,
      frame: auv_driver::Rect::new(0.0, 0.0, 1920.0, 1080.0),
      coordinate_space: auv_driver::CoordinateSpace::Screen,
      scale_factor: 1.0,
      is_primary: false,
      is_builtin: Some(false),
    },
    capture: auv_driver::Capture {
      origin: None,
      image: RgbaImage::new(320, 180),
      bounds: auv_driver::Rect::new(100.0, 120.0, 320.0, 180.0),
      scale_factor: 1.0,
      backend: "fixture-region".to_string(),
      fallback_reason: Some("fixture fallback".to_string()),
    },
  };

  let output =
    region_capture_output(&capture.display, super::super::capture_result(&capture.capture), None).expect("region result should serialize");
  let result = output.result().expect("capture should have a result");

  assert_eq!(result["display"]["id"], "display_1");
  assert_eq!(result["capture"]["bounds"]["origin"]["x"], 100.0);
  assert_eq!(result["capture"]["pixel_dimensions"]["width"], 320);
  assert_eq!(result["capture"]["backend"], "fixture-region");
  assert_eq!(result["capture"]["fallback_reason"], "fixture fallback");
  assert!(result.get("image").is_none());
}

#[test]
fn screen_text_click_result_keeps_resolution_and_delivery_together() {
  let click = ScreenTextClick {
    matches: auv_driver::OcrMatches {
      matches: vec![auv_driver::OcrMatch {
        text: "Pause".to_string(),
        confidence: 0.97,
        bounds: auv_driver::Rect::new(40.0, 50.0, 70.0, 20.0),
      }],
    },
    point: auv_driver::Point::new(75.0, 60.0),
    action: auv_driver::InputActionResult::single_success(auv_driver::InputDeliveryPath::ForegroundSystemEvents),
  };

  let output = screen_text_click_output(&click).expect("screen click result should serialize");
  let result = output.result().expect("click should have a result");

  assert_eq!(result["matches"]["matches"][0]["text"], "Pause");
  assert_eq!(result["point"]["x"], 75.0);
  assert_eq!(result["action"]["selected_path"], "foreground_system_events");
  let report = output.report.as_ref().expect("screen click report");
  assert_eq!(report_field(report, "Delivery"), "delivered");
  assert_eq!(report_field(report, "Verification"), "delivery_only");
  assert_eq!(report_field(report, "Path"), "foreground_system_events");
}

// ROOT CAUSE:
//
// If `screen.findText` or `screen.waitForText` ran on Windows, each answered "only available on
// macOS and Linux" before reaching the driver, although the Windows driver already captures the
// primary display and reads its text with the system OCR.
//
// Before the fix, both handlers were compiled out on Windows. The fix keeps them compiled in, so a
// dry run reaches the same validation every other platform reaches. A dry run proves the gate
// only; what the commands do on a desktop is recorded in the Windows window commands reference.
#[cfg(target_os = "windows")]
#[test]
fn screen_text_commands_are_not_gated_out_on_windows() {
  let commands = [
    (find_screen_text_invoke_command(), dry_run_text_input("screen.findText", None)),
    (wait_for_screen_text_invoke_command(), dry_run_text_input("screen.waitForText", None)),
  ];

  for (command, input) in commands {
    let id = input.command_id.clone();
    futures_executor::block_on(command.invoke(input)).unwrap_or_else(|error| panic!("{id} was refused on Windows: {}", error.message));
  }
}

// ROOT CAUSE:
//
// If `screen.clickText` ran on a platform without a click path, the refusal
// reached the caller as `command_failed`,
// because the handler returned a plain `String` and `InvokeFailure::from(String)` assigns that code.
//
// Before the fix, a caller could not tell a platform without a path from a click that failed. The
// fix keeps the refusal typed as `unsupported`, and the command still refuses instead of clicking.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[test]
fn screen_click_text_refuses_as_unsupported_without_a_platform_path() {
  let input = dry_run_text_input("screen.clickText", None);

  let error = futures_executor::block_on(click_screen_text_invoke_command().invoke(input)).expect_err("screen.clickText has no path here");

  assert_eq!(error.code, crate::FailureCode::Unsupported, "unexpected refusal: {}", error.message);
  assert!(error.message.contains("only available on macOS and Windows"), "unexpected message: {}", error.message);
}

// ROOT CAUSE:
//
// Windows has primary-display OCR and foreground clicks, but the local invoke
// handler compiled both out. Validate the same command without opening a driver
// session; the owned Windows receiver separately verifies actual click effects.
#[cfg(target_os = "windows")]
#[test]
fn screen_click_text_validates_on_windows() {
  let output = futures_executor::block_on(click_screen_text_invoke_command().invoke(dry_run_text_input("screen.clickText", None)))
    .expect("Windows must reach screen click validation instead of Unsupported");
  assert_eq!(output.result(), super::super::input::validation_only_output().result());
}

#[cfg(target_os = "windows")]
#[test]
fn screen_click_text_rejects_target_activation_before_windows_input() {
  let error =
    futures_executor::block_on(click_screen_text_invoke_command().invoke(dry_run_text_input("screen.clickText", Some("unrelated.exe"))))
      .expect_err("screen OCR must not silently activate a target");
  assert_eq!(error.code, crate::FailureCode::InvalidTarget);
}

#[cfg(target_os = "windows")]
#[test]
fn cancelled_screen_click_never_opens_the_desktop() {
  let cancellation = InvokeCancellation::new();
  cancellation.cancel();
  let error = futures_executor::block_on(click_recognized_screen_text_with_cancellation("Absent".into(), &cancellation))
    .expect_err("already cancelled before capture");
  assert!(error.message.contains("cancelled"), "{error:?}");
}

#[cfg(not(target_os = "macos"))]
fn dry_run_text_input(command_id: &str, target: Option<&str>) -> InvokeCommandInput {
  InvokeCommandInput {
    command_id: command_id.to_string(),
    target: target.map(|id| crate::ExecutionTarget::Application { id: id.to_string() }),
    inputs: [("query".to_string(), "Settings".to_string())].into_iter().collect(),
    typed_args: None,
    dry_run: true,
    cancellation: InvokeCancellation::new(),
  }
}

fn report_field<'a>(report: &'a InvokeReport, label: &str) -> &'a str {
  report.fields.iter().find(|field| field.label == label).map(|field| field.value.as_str()).expect("report field")
}

fn wait_input(pairs: &[(&str, &str)]) -> InvokeCommandInput {
  InvokeCommandInput {
    command_id: "screen.waitForText".into(),
    target: None,
    inputs: pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect(),
    typed_args: None,
    dry_run: false,
    cancellation: Default::default(),
  }
}

// ROOT CAUSE:
//
// If a page took longer than 5 s to show the text, screen.waitForText
// failed: the wait budget was hard-coded. OSWorld agents passed `timeout`
// keys that did not exist (49 calls, silently ignored) and saw the text
// appear 10-85 s after failed waits.
//
// The fix accepts `timeout-ms`/`interval-ms` and defaults to 10 s.
#[test]
fn screen_wait_decodes_its_budget_from_protocol_inputs() {
  let (query, options) = WaitForScreenTextArgs::decode(&wait_input(&[("query", "Ready")])).unwrap();
  assert_eq!(query, "Ready");
  assert_eq!(options.timeout, std::time::Duration::from_secs(10));
  assert_eq!(options.poll_interval, std::time::Duration::from_millis(250));

  let (_, options) = WaitForScreenTextArgs::decode(&wait_input(&[("query", "Ready"), ("timeout-ms", "30000")])).unwrap();
  assert_eq!(options.timeout, std::time::Duration::from_secs(30));
  // Protocol inputs use the advertised key only; `InvokeCommand::validate`
  // rejects the `timeout_ms` id spelling before decoding (#363).
}

#[test]
fn screen_wait_rejects_budgets_outside_the_supported_range() {
  let error = WaitForScreenTextArgs::decode(&wait_input(&[("query", "Ready"), ("timeout-ms", "999999")])).unwrap_err();
  assert!(error.contains("timeout-ms must be within 100..=120000"), "{error}");
}
