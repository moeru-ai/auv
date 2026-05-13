use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use crate::model::{
  AuvResult,
  DriverCall,
  DriverDescriptor,
  DriverResponse,
  ProducedArtifact,
  now_millis,
};

const PROBE_ACCESSIBILITY_SCRIPT: &str = r#"
import ApplicationServices
print(AXIsProcessTrusted() ? "granted" : "missing")
"#;

const PROBE_SCREEN_RECORDING_SCRIPT: &str = r#"
import CoreGraphics
import Foundation
print(CGPreflightScreenCaptureAccess() ? "granted" : "missing")
"#;

const OBSERVE_WINDOWS_SCRIPT_TEMPLATE: &str = r#"
import AppKit
import CoreGraphics
import Foundation

func boundsDict(_ value: NSDictionary?) -> [String: Int]? {
  guard let value else { return nil }
  var rect = CGRect.zero
  guard CGRectMakeWithDictionaryRepresentation(value, &rect) else { return nil }
  return [
    "x": Int(rect.origin.x.rounded()),
    "y": Int(rect.origin.y.rounded()),
    "width": Int(rect.size.width.rounded()),
    "height": Int(rect.size.height.rounded())
  ]
}

let limit = __LIMIT__
let appFilter = __APP_FILTER__.lowercased().trimmingCharacters(in: .whitespacesAndNewlines)
let frontmostAppName = NSWorkspace.shared.frontmostApplication?.localizedName ?? ""

let options: CGWindowListOption = [.optionOnScreenOnly, .excludeDesktopElements]
let rawWindowInfo = CGWindowListCopyWindowInfo(options, kCGNullWindowID) as? [[String: Any]] ?? []
var windows: [[String: Any]] = []
for window in rawWindowInfo {
  let ownerName = (window[kCGWindowOwnerName as String] as? String) ?? "Unknown"
  if !appFilter.isEmpty && !ownerName.lowercased().contains(appFilter) {
    continue
  }

  let alpha = window[kCGWindowAlpha as String] as? Double ?? 1.0
  let layer = window[kCGWindowLayer as String] as? Int ?? 0
  let bounds = boundsDict(window[kCGWindowBounds as String] as? NSDictionary)
  let title = (window[kCGWindowName as String] as? String)?.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
  let ownerPid = window[kCGWindowOwnerPID as String] as? Int ?? 0

  if alpha <= 0 || (bounds?["width"] ?? 0) <= 1 || (bounds?["height"] ?? 0) <= 1 {
    continue
  }

  windows.append([
    "appName": ownerName,
    "title": title,
    "ownerPid": ownerPid,
    "layer": layer,
    "x": bounds?["x"] ?? 0,
    "y": bounds?["y"] ?? 0,
    "width": bounds?["width"] ?? 0,
    "height": bounds?["height"] ?? 0
  ])

  if windows.count >= limit {
    break
  }
}

let frontmostWindowTitle = windows.first(where: { ($0["appName"] as? String) == frontmostAppName })?["title"] as? String ?? ""

print("frontmostAppName=\(frontmostAppName)")
print("frontmostWindowTitle=\(frontmostWindowTitle)")
print("observedAt=\(ISO8601DateFormatter().string(from: Date()))")
print("windowCount=\(windows.count)")
for window in windows {
  let appName = window["appName"] as? String ?? "Unknown"
  let title = window["title"] as? String ?? ""
  let ownerPid = window["ownerPid"] as? Int ?? 0
  let layer = window["layer"] as? Int ?? 0
  let x = window["x"] as? Int ?? 0
  let y = window["y"] as? Int ?? 0
  let width = window["width"] as? Int ?? 0
  let height = window["height"] as? Int ?? 0
  print("window\t\(appName)\t\(ownerPid)\t\(layer)\t\(title)\t\(x)\t\(y)\t\(width)\t\(height)")
}
"#;

const CLICK_SCRIPT_TEMPLATE: &str = r#"
import CoreGraphics
import Foundation

func mouseButton(_ value: Int) -> CGMouseButton {
  switch value {
  case 1: return .right
  case 2: return .center
  default: return .left
  }
}

func mouseDownType(_ button: CGMouseButton) -> CGEventType {
  switch button {
  case .right: return .rightMouseDown
  case .center: return .otherMouseDown
  default: return .leftMouseDown
  }
}

func mouseUpType(_ button: CGMouseButton) -> CGEventType {
  switch button {
  case .right: return .rightMouseUp
  case .center: return .otherMouseUp
  default: return .leftMouseUp
  }
}

let original = CGEvent(source: nil)?.location ?? CGPoint.zero
defer {
  CGWarpMouseCursorPosition(original)
}

let x = __X__
let y = __Y__
let clickCount = __CLICK_COUNT__
let button = mouseButton(__BUTTON__)
let location = CGPoint(x: x, y: y)

if let moveEvent = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: location, mouseButton: .left) {
  moveEvent.post(tap: .cghidEventTap)
}

for _ in 0..<max(clickCount, 1) {
  if let down = CGEvent(mouseEventSource: nil, mouseType: mouseDownType(button), mouseCursorPosition: location, mouseButton: button),
     let up = CGEvent(mouseEventSource: nil, mouseType: mouseUpType(button), mouseCursorPosition: location, mouseButton: button) {
    down.setIntegerValueField(.mouseEventClickState, value: Int64(clickCount))
    up.setIntegerValueField(.mouseEventClickState, value: Int64(clickCount))
    down.post(tap: .cghidEventTap)
    up.post(tap: .cghidEventTap)
  }
}

print("clicked")
"#;

const TYPE_TEXT_SCRIPT_TEMPLATE: &str = r#"
import CoreGraphics
import Foundation

let text = __TEXT__
let pressEnter = __PRESS_ENTER__
let characterDelayMicros: useconds_t = 12_000
let settleDelayMicros: useconds_t = 80_000

func postText(_ chunk: String) {
  let chars = Array(chunk.utf16)
  let length = chars.count
  guard length > 0 else { return }
  chars.withUnsafeBufferPointer { buffer in
    if let keyDown = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: true),
       let keyUp = CGEvent(keyboardEventSource: nil, virtualKey: 0, keyDown: false) {
      keyDown.keyboardSetUnicodeString(stringLength: length, unicodeString: buffer.baseAddress!)
      keyUp.keyboardSetUnicodeString(stringLength: length, unicodeString: buffer.baseAddress!)
      keyDown.post(tap: .cghidEventTap)
      keyUp.post(tap: .cghidEventTap)
    }
  }

  usleep(characterDelayMicros)
}

for character in text {
  postText(String(character))
}

if !text.isEmpty {
  usleep(settleDelayMicros)
}

if pressEnter {
  if let down = CGEvent(keyboardEventSource: nil, virtualKey: 36, keyDown: true),
     let up = CGEvent(keyboardEventSource: nil, virtualKey: 36, keyDown: false) {
    down.post(tap: .cghidEventTap)
    up.post(tap: .cghidEventTap)
  }
}

print("typed")
"#;

const PRESS_KEYS_SCRIPT_TEMPLATE: &str = r#"
import CoreGraphics
import Foundation

let keyCode: CGKeyCode = __KEY_CODE__
let modifierFlags: CGEventFlags = __MODIFIER_FLAGS__

if let keyDown = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: true),
   let keyUp = CGEvent(keyboardEventSource: nil, virtualKey: keyCode, keyDown: false) {
  keyDown.flags = modifierFlags
  keyUp.flags = modifierFlags
  keyDown.post(tap: .cghidEventTap)
  keyUp.post(tap: .cghidEventTap)
}

print("pressed")
"#;

const SCROLL_SCRIPT_TEMPLATE: &str = r#"
import CoreGraphics
import Foundation

let original = CGEvent(source: nil)?.location ?? CGPoint.zero
defer {
  CGWarpMouseCursorPosition(original)
}

let hasPoint = __HAS_POINT__
let x = __X__
let y = __Y__
let deltaX = Int32(__DELTA_X__)
let deltaY = Int32(__DELTA_Y__)

if hasPoint {
  let location = CGPoint(x: x, y: y)
  if let moveEvent = CGEvent(mouseEventSource: nil, mouseType: .mouseMoved, mouseCursorPosition: location, mouseButton: .left) {
    moveEvent.post(tap: .cghidEventTap)
  }
}

if let scrollEvent = CGEvent(scrollWheelEvent2Source: nil, units: .pixel, wheelCount: 2, wheel1: deltaY, wheel2: deltaX, wheel3: 0) {
  scrollEvent.post(tap: .cghidEventTap)
}

print("scrolled")
"#;

const XCRUN_BINARY: &str = "/usr/bin/xcrun";
const OSASCRIPT_BINARY: &str = "/usr/bin/osascript";
const OPEN_BINARY: &str = "/usr/bin/open";
const SCREEN_CAPTURE_BINARY: &str = "/usr/sbin/screencapture";

pub trait Driver {
  fn descriptor(&self) -> DriverDescriptor;
  fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse>;
}

pub struct DriverRegistry {
  drivers: HashMap<String, Box<dyn Driver>>,
}

impl DriverRegistry {
  pub fn new(drivers: Vec<Box<dyn Driver>>) -> Self {
    let mut registry = HashMap::new();
    for driver in drivers {
      let descriptor = driver.descriptor();
      registry.insert(descriptor.id.to_string(), driver);
    }
    Self { drivers: registry }
  }

  pub fn get(&self, driver_id: &str) -> Option<&dyn Driver> {
    self.drivers.get(driver_id).map(Box::as_ref)
  }

  pub fn descriptors(&self) -> Vec<DriverDescriptor> {
    let mut descriptors = self
      .drivers
      .values()
      .map(|driver| driver.descriptor())
      .collect::<Vec<_>>();
    descriptors.sort_by(|left, right| left.id.cmp(right.id));
    descriptors
  }
}

pub fn default_driver_registry() -> DriverRegistry {
  DriverRegistry::new(vec![
    Box::new(FixtureObserveDriver),
    Box::new(MacOsDesktopDriver),
  ])
}

struct FixtureObserveDriver;

impl Driver for FixtureObserveDriver {
  fn descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: "fixture.observe",
      summary: "Non-UI fixture driver that proves invoke -> run -> inspect without platform side effects.",
      capabilities: &["observe.fixture"],
      donor_boundary: "AUV-native fixture driver; useful for validating the shared execution substrate before real app drivers land.",
    }
  }

  fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
    if call.operation != "observe_fixture_scene" {
      return Err(format!(
        "driver fixture.observe does not support operation {}",
        call.operation
      ));
    }

    let target = call
      .target
      .application_id
      .clone()
      .unwrap_or_else(|| "fixture://default".to_string());
    let label = call
      .inputs
      .get("label")
      .cloned()
      .unwrap_or_else(|| "fixture-observation".to_string());

    Ok(DriverResponse {
      summary: format!(
        "Observed deterministic fixture scene for target {} with label {}.",
        target, label
      ),
      backend: Some("fixture.static".to_string()),
      notes: vec![
        "This command does not touch the real desktop.".to_string(),
        "Use it to verify that implicit run creation and inspect output stay stable.".to_string(),
      ],
      artifacts: Vec::new(),
    })
  }
}

struct MacOsDesktopDriver;

impl Driver for MacOsDesktopDriver {
  fn descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: "macos.desktop",
      summary: "Desktop donor primitives extracted into the shared AUV driver protocol.",
      capabilities: &[
        "observe.screenshot",
        "observe.windows",
        "observe.permissions",
        "control.open-app",
        "control.focus-app",
        "control.click",
        "control.type-text",
        "control.press-keys",
        "control.scroll",
        "control.wait",
      ],
      donor_boundary: "Borrow Swift/Quartz desktop primitives from AIRI, but keep MCP tools, action executors, approval queues, and workflow shells out of AUV core.",
    }
  }

  fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
    require_macos()?;

    match call.operation.as_str() {
      "capture_screen" => capture_screen(call),
      "observe_windows" => observe_windows(call),
      "probe_permissions" => probe_permissions(call),
      "open_app" => open_app(call),
      "focus_app" => focus_app(call),
      "click" => click(call),
      "type_text" => type_text(call),
      "press_keys" => press_keys(call),
      "scroll" => scroll(call),
      "wait" => wait(call),
      other => Err(format!(
        "driver macos.desktop does not support operation {}",
        other
      )),
    }
  }
}

fn capture_screen(call: &DriverCall) -> AuvResult<DriverResponse> {
  let label = optional_string(call, "label").unwrap_or_else(|| "desktop".to_string());
  let temporary_path = screenshot_temp_path(&label);
  let args = vec![
    "-x".to_string(),
    temporary_path.display().to_string(),
  ];
  run_command(SCREEN_CAPTURE_BINARY, &args)?;

  if !temporary_path.exists() {
    return Err(format!(
      "screencapture reported success but no image was created at {}",
      temporary_path.display()
    ));
  }

  Ok(DriverResponse {
    summary: "Captured one desktop screenshot through the shared AUV runtime.".to_string(),
    backend: Some("macos.screencapture".to_string()),
    notes: vec![
      format!(
        "Temporary screenshot created at {} before artifact ingestion.",
        temporary_path.display()
      ),
      "This remains a driver-level primitive instead of an AIRI-style desktop tool wrapper.".to_string(),
    ],
    artifacts: vec![ProducedArtifact {
      kind: "screenshot".to_string(),
      source_path: temporary_path,
      preferred_name: format!("{}.png", sanitize_file_component(&label)),
      note: Some("Phase-1 screenshot artifact captured through the macOS desktop driver.".to_string()),
    }],
  })
}

fn observe_windows(call: &DriverCall) -> AuvResult<DriverResponse> {
  let limit = optional_i64(call, "limit")?.unwrap_or(12).max(1);
  let app_filter = app_identifier(call).unwrap_or_default();
  let report = run_swift_script(&build_observe_windows_script(limit, &app_filter))?;
  let window_count = report_value(&report, "windowCount=")
    .unwrap_or("0")
    .parse::<usize>()
    .unwrap_or(0);
  let frontmost_app = report_value(&report, "frontmostAppName=").unwrap_or("").to_string();
  let frontmost_window = report_value(&report, "frontmostWindowTitle=").unwrap_or("").to_string();
  let observed_at = report_value(&report, "observedAt=").unwrap_or("").to_string();
  let artifact = build_text_artifact(
    "observe-windows",
    "txt",
    &format!("observe-windows-{}", sanitize_file_component(&frontmost_app)),
    report.clone(),
    "Captured window observation report from the macOS desktop driver.",
  )?;
  let mut notes = vec![format!("observedAt={observed_at}")];
  for line in report.lines().filter(|line| line.starts_with("window\t")).take(5) {
    notes.push(line.to_string());
  }

  let summary = if frontmost_app.is_empty() {
    format!("Observed {} visible macOS window(s).", window_count)
  }
  else if frontmost_window.is_empty() {
    format!(
      "Observed {} visible macOS window(s); frontmost app is {}.",
      window_count, frontmost_app
    )
  }
  else {
    format!(
      "Observed {} visible macOS window(s); frontmost app is {} ({})",
      window_count, frontmost_app, frontmost_window
    )
  };

  Ok(DriverResponse {
    summary,
    backend: Some("macos.swift.cgwindowlist".to_string()),
    notes,
    artifacts: vec![artifact],
  })
}

fn probe_permissions(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let screen_recording = run_swift_script(PROBE_SCREEN_RECORDING_SCRIPT)?.trim().to_string();
  let accessibility = run_swift_script(PROBE_ACCESSIBILITY_SCRIPT)?.trim().to_string();
  let automation = probe_automation_to_system_events();
  let launch_host = launch_host_process();

  let report = [
    format!("screenRecording={screen_recording}"),
    format!("accessibility={accessibility}"),
    format!("automationToSystemEvents={automation}"),
    format!("launchHostProcess={launch_host}"),
  ]
  .join("\n")
    + "\n";

  let artifact = build_text_artifact(
    "probe-permissions",
    "txt",
    "permission-report",
    report.clone(),
    "Captured macOS permission probe report from the desktop driver.",
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Permission probe: screenRecording={}, accessibility={}, automationToSystemEvents={}.",
      screen_recording, accessibility, automation
    ),
    backend: Some("macos.swift-and-osascript".to_string()),
    notes: report.lines().map(str::to_string).collect(),
    artifacts: vec![artifact],
  })
}

fn open_app(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = required_app(call)?;
  let resolved = resolve_installed_app_name(&app);
  let args = vec!["-a".to_string(), resolved.clone()];
  run_command(OPEN_BINARY, &args)?;

  Ok(DriverResponse {
    summary: format!("Opened app {}.", resolved),
    backend: Some("macos.open".to_string()),
    notes: vec![format!("requestedApp={app}")],
    artifacts: Vec::new(),
  })
}

fn focus_app(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = required_app(call)?;
  let resolved = resolve_installed_app_name(&app);
  let open_args = vec!["-a".to_string(), resolved.clone()];
  run_command(OPEN_BINARY, &open_args)?;
  let activate_args = vec![
    "-e".to_string(),
    format!("tell application {} to activate", apple_script_string(&resolved)),
  ];
  run_command(OSASCRIPT_BINARY, &activate_args)?;

  Ok(DriverResponse {
    summary: format!("Focused app {}.", resolved),
    backend: Some("macos.open-and-osascript".to_string()),
    notes: vec![format!("requestedApp={app}")],
    artifacts: Vec::new(),
  })
}

fn click(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = required_f64(call, "x")?;
  let y = required_f64(call, "y")?;
  let button_name = optional_string(call, "button").unwrap_or_else(|| "left".to_string());
  let button = button_code(&button_name)?;
  let click_count = optional_i64(call, "clickCount")?.unwrap_or(1).max(1);
  run_swift_script(
    &CLICK_SCRIPT_TEMPLATE
      .replace("__X__", &format!("{x}"))
      .replace("__Y__", &format!("{y}"))
      .replace("__CLICK_COUNT__", &click_count.to_string())
      .replace("__BUTTON__", &button.to_string()),
  )?;

  Ok(DriverResponse {
    summary: format!("Clicked at ({x}, {y}) on the local macOS desktop."),
    backend: Some("macos.swift.quartz-click".to_string()),
    notes: vec![
      format!("button={button_name}"),
      format!("clickCount={click_count}"),
    ],
    artifacts: Vec::new(),
  })
}

fn type_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let text = required_string(call, "text")?;
  let press_enter = optional_bool(call, "pressEnter").unwrap_or(false);
  run_swift_script(
    &TYPE_TEXT_SCRIPT_TEMPLATE
      .replace("__TEXT__", &swift_string_literal(&text))
      .replace("__PRESS_ENTER__", if press_enter { "true" } else { "false" }),
  )?;

  Ok(DriverResponse {
    summary: format!(
      "Typed {} character(s) on the local macOS desktop.",
      text.chars().count()
    ),
    backend: Some("macos.swift.quartz-type".to_string()),
    notes: vec![format!("pressEnter={press_enter}")],
    artifacts: Vec::new(),
  })
}

fn press_keys(call: &DriverCall) -> AuvResult<DriverResponse> {
  let keys = required_string(call, "keys")?;
  let normalized = split_keys(&keys);
  if normalized.is_empty() {
    return Err("press_keys requires at least one key".to_string());
  }

  let main_key = normalized.last().cloned().unwrap_or_default();
  let key_code = key_code(&main_key)
    .ok_or_else(|| format!("unsupported macOS key for press_keys: {}", main_key))?;
  let modifier_expression = if normalized.len() > 1 {
    normalized[..normalized.len() - 1]
      .iter()
      .map(|modifier| {
        modifier_flag(modifier)
          .ok_or_else(|| format!("unsupported modifier key: {}", modifier))
      })
      .collect::<AuvResult<Vec<_>>>()?
      .join(" | ")
  }
  else {
    "[]".to_string()
  };

  run_swift_script(
    &PRESS_KEYS_SCRIPT_TEMPLATE
      .replace("__KEY_CODE__", &key_code.to_string())
      .replace("__MODIFIER_FLAGS__", &modifier_expression),
  )?;

  Ok(DriverResponse {
    summary: format!("Pressed keys {}.", normalized.join("+")),
    backend: Some("macos.swift.quartz-keys".to_string()),
    notes: vec![format!("rawKeys={keys}")],
    artifacts: Vec::new(),
  })
}

fn scroll(call: &DriverCall) -> AuvResult<DriverResponse> {
  let x = optional_f64(call, "x")?.unwrap_or(0.0);
  let y = optional_f64(call, "y")?.unwrap_or(0.0);
  let has_point = call.inputs.contains_key("x") && call.inputs.contains_key("y");
  let delta_x = optional_i64(call, "deltaX")?.unwrap_or(0);
  let delta_y = optional_i64(call, "deltaY")?.unwrap_or(0);
  run_swift_script(
    &SCROLL_SCRIPT_TEMPLATE
      .replace("__HAS_POINT__", if has_point { "true" } else { "false" })
      .replace("__X__", &format!("{x}"))
      .replace("__Y__", &format!("{y}"))
      .replace("__DELTA_X__", &delta_x.to_string())
      .replace("__DELTA_Y__", &delta_y.to_string()),
  )?;

  Ok(DriverResponse {
    summary: format!("Scrolled on the local macOS desktop with deltaX={}, deltaY={}.", delta_x, delta_y),
    backend: Some("macos.swift.quartz-scroll".to_string()),
    notes: vec![format!("hasPoint={has_point}")],
    artifacts: Vec::new(),
  })
}

fn wait(call: &DriverCall) -> AuvResult<DriverResponse> {
  let duration_ms = optional_i64(call, "durationMs")?.unwrap_or(0).max(0) as u64;
  thread::sleep(Duration::from_millis(duration_ms));

  Ok(DriverResponse {
    summary: format!("Waited {}ms inside the shared runtime path.", duration_ms),
    backend: Some("std.thread.sleep".to_string()),
    notes: Vec::new(),
    artifacts: Vec::new(),
  })
}

fn build_observe_windows_script(limit: i64, app_filter: &str) -> String {
  OBSERVE_WINDOWS_SCRIPT_TEMPLATE
    .replace("__LIMIT__", &limit.to_string())
    .replace("__APP_FILTER__", &swift_string_literal(app_filter))
}

fn probe_automation_to_system_events() -> String {
  let args = vec![
    "-e".to_string(),
    "tell application \"System Events\"".to_string(),
    "-e".to_string(),
    "return name of first application process whose frontmost is true".to_string(),
    "-e".to_string(),
    "end tell".to_string(),
  ];

  match run_command(OSASCRIPT_BINARY, &args) {
    Ok(_) => "granted".to_string(),
    Err(_) => "missing".to_string(),
  }
}

fn run_swift_script(source: &str) -> AuvResult<String> {
  let script_path = temp_file_path("swift-script", "swift");
  fs::write(&script_path, source)
    .map_err(|error| format!("failed to write Swift script {}: {error}", script_path.display()))?;

  let result = run_swift_script_with_fallback(&script_path);
  let _ = fs::remove_file(&script_path);
  result
}

fn run_swift_script_with_fallback(script_path: &PathBuf) -> AuvResult<String> {
  let xcrun_args = vec![
    "swift".to_string(),
    script_path.display().to_string(),
  ];

  match run_command(XCRUN_BINARY, &xcrun_args) {
    Ok(output) => Ok(output.stdout),
    Err(error) if error.contains("failed to spawn xcrun") => {
      let swift_args = vec![script_path.display().to_string()];
      Ok(run_command("swift", &swift_args)?.stdout)
    }
    Err(error) => Err(error),
  }
}

fn run_command(binary: &str, args: &[String]) -> AuvResult<CommandOutput> {
  let output = Command::new(binary)
    .args(args)
    .output()
    .map_err(|error| match error.kind() {
      ErrorKind::NotFound => format!("failed to spawn {}: command not found", binary),
      _ => format!("failed to spawn {}: {}", binary, error),
    })?;

  let stdout = String::from_utf8_lossy(&output.stdout).to_string();
  let stderr = String::from_utf8_lossy(&output.stderr).to_string();

  if !output.status.success() {
    let trimmed_stderr = stderr.trim();
    return Err(format!(
      "{} exited with status {}: {}",
      binary,
      output.status,
      if trimmed_stderr.is_empty() {
        "no stderr output"
      }
      else {
        trimmed_stderr
      }
    ));
  }

  Ok(CommandOutput { stdout })
}

fn temp_file_path(label: &str, extension: &str) -> PathBuf {
  env::temp_dir().join(format!(
    "auv-{}-{}-{}.{}",
    sanitize_file_component(label),
    now_millis(),
    std::process::id(),
    extension
  ))
}

fn build_text_artifact(
  kind: &str,
  extension: &str,
  label: &str,
  content: String,
  note: &str,
) -> AuvResult<ProducedArtifact> {
  let source_path = temp_file_path(label, extension);
  fs::write(&source_path, content)
    .map_err(|error| format!("failed to write artifact source {}: {error}", source_path.display()))?;

  Ok(ProducedArtifact {
    kind: kind.to_string(),
    source_path,
    preferred_name: format!("{}.{}", sanitize_file_component(label), extension),
    note: Some(note.to_string()),
  })
}

fn screenshot_temp_path(label: &str) -> PathBuf {
  temp_file_path(label, "png")
}

fn require_macos() -> AuvResult<()> {
  if env::consts::OS != "macos" {
    return Err("macos.desktop is only available on macOS".to_string());
  }

  Ok(())
}

fn required_app(call: &DriverCall) -> AuvResult<String> {
  app_identifier(call)
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| "operation requires --app <Application Name> or --target <Application Name>".to_string())
}

fn app_identifier(call: &DriverCall) -> Option<String> {
  optional_string(call, "app").or_else(|| {
    call.target.application_id.clone().filter(|value| !value.trim().is_empty())
  })
}

fn required_string(call: &DriverCall, key: &str) -> AuvResult<String> {
  optional_string(call, key)
    .filter(|value| !value.trim().is_empty())
    .ok_or_else(|| format!("operation requires --{} <value>", key))
}

fn optional_string(call: &DriverCall, key: &str) -> Option<String> {
  call.inputs.get(key).cloned()
}

fn required_f64(call: &DriverCall, key: &str) -> AuvResult<f64> {
  optional_f64(call, key)?
    .ok_or_else(|| format!("operation requires --{} <number>", key))
}

fn optional_f64(call: &DriverCall, key: &str) -> AuvResult<Option<f64>> {
  match call.inputs.get(key) {
    Some(value) => value
      .parse::<f64>()
      .map(Some)
      .map_err(|error| format!("invalid --{} value {}: {}", key, value, error)),
    None => Ok(None),
  }
}

fn optional_i64(call: &DriverCall, key: &str) -> AuvResult<Option<i64>> {
  match call.inputs.get(key) {
    Some(value) => value
      .parse::<i64>()
      .map(Some)
      .map_err(|error| format!("invalid --{} value {}: {}", key, value, error)),
    None => Ok(None),
  }
}

fn optional_bool(call: &DriverCall, key: &str) -> Option<bool> {
  call.inputs.get(key).map(|value| {
    matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
  })
}

fn report_value<'a>(report: &'a str, prefix: &str) -> Option<&'a str> {
  report
    .lines()
    .find_map(|line| line.strip_prefix(prefix))
    .map(str::trim)
}

fn split_keys(raw: &str) -> Vec<String> {
  raw
    .split(|character| character == '+' || character == ',')
    .map(|key| key.trim().to_ascii_lowercase())
    .filter(|key| !key.is_empty())
    .collect()
}

fn button_code(button: &str) -> AuvResult<i64> {
  match button.trim().to_ascii_lowercase().as_str() {
    "left" => Ok(0),
    "right" => Ok(1),
    "middle" => Ok(2),
    other => Err(format!("unsupported mouse button: {}", other)),
  }
}

fn key_code(key: &str) -> Option<i64> {
  match key {
    "a" => Some(0),
    "b" => Some(11),
    "c" => Some(8),
    "d" => Some(2),
    "e" => Some(14),
    "f" => Some(3),
    "g" => Some(5),
    "h" => Some(4),
    "i" => Some(34),
    "j" => Some(38),
    "k" => Some(40),
    "l" => Some(37),
    "m" => Some(46),
    "n" => Some(45),
    "o" => Some(31),
    "p" => Some(35),
    "q" => Some(12),
    "r" => Some(15),
    "s" => Some(1),
    "t" => Some(17),
    "u" => Some(32),
    "v" => Some(9),
    "w" => Some(13),
    "x" => Some(7),
    "y" => Some(16),
    "z" => Some(6),
    "0" => Some(29),
    "1" => Some(18),
    "2" => Some(19),
    "3" => Some(20),
    "4" => Some(21),
    "5" => Some(23),
    "6" => Some(22),
    "7" => Some(26),
    "8" => Some(28),
    "9" => Some(25),
    "enter" | "return" => Some(36),
    "tab" => Some(48),
    "space" => Some(49),
    "escape" | "esc" => Some(53),
    "delete" | "backspace" => Some(51),
    "up" => Some(126),
    "down" => Some(125),
    "left" => Some(123),
    "right" => Some(124),
    _ => None,
  }
}

fn modifier_flag(key: &str) -> Option<String> {
  match key {
    "command" | "cmd" => Some(".maskCommand".to_string()),
    "shift" => Some(".maskShift".to_string()),
    "control" | "ctrl" => Some(".maskControl".to_string()),
    "option" | "alt" => Some(".maskAlternate".to_string()),
    _ => None,
  }
}

fn resolve_installed_app_name(app: &str) -> String {
  let mut roots = vec![PathBuf::from("/Applications")];
  if let Some(home) = env::var_os("HOME") {
    roots.push(PathBuf::from(home).join("Applications"));
  }

  for root in roots {
    let entries = match fs::read_dir(&root) {
      Ok(entries) => entries,
      Err(_) => continue,
    };

    for entry in entries.flatten() {
      let file_name = entry.file_name();
      let bundle_name = file_name.to_string_lossy();
      if !bundle_name.ends_with(".app") {
        continue;
      }

      let trimmed = bundle_name.trim_end_matches(".app");
      if trimmed.eq_ignore_ascii_case(app) {
        return trimmed.to_string();
      }
    }
  }

  app.to_string()
}

fn launch_host_process() -> String {
  env::args()
    .next()
    .map(PathBuf::from)
    .as_ref()
    .and_then(|value| value.file_name())
    .and_then(|value| value.to_str())
    .unwrap_or("auv-cli")
    .to_string()
}

fn apple_script_string(raw: &str) -> String {
  format!("\"{}\"", raw.replace('\\', "\\\\").replace('"', "\\\""))
}

fn swift_string_literal(raw: &str) -> String {
  let mut escaped = String::from("\"");
  for character in raw.chars() {
    match character {
      '\\' => escaped.push_str("\\\\"),
      '"' => escaped.push_str("\\\""),
      '\n' => escaped.push_str("\\n"),
      '\r' => escaped.push_str("\\r"),
      '\t' => escaped.push_str("\\t"),
      _ => escaped.push(character),
    }
  }
  escaped.push('"');
  escaped
}

fn sanitize_file_component(raw: &str) -> String {
  let sanitized = raw
    .chars()
    .map(|character| match character {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
      _ => '-',
    })
    .collect::<String>()
    .trim_matches('-')
    .to_string();

  if sanitized.is_empty() {
    "artifact".to_string()
  }
  else {
    sanitized
  }
}

pub fn copy_file(source: &PathBuf, destination: &PathBuf) -> AuvResult<()> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent)
      .map_err(|error| format!("failed to create artifact directory {}: {error}", parent.display()))?;
  }

  fs::copy(source, destination).map_err(|error| {
    format!(
      "failed to copy artifact from {} to {}: {error}",
      source.display(),
      destination.display()
    )
  })?;

  Ok(())
}

pub fn sanitized_artifact_name(raw: &str) -> String {
  sanitize_file_component(raw)
}

struct CommandOutput {
  stdout: String,
}
