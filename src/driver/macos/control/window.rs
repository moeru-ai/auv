// File: src/driver/macos/control/window.rs
use super::super::support::{
  artifacts::{build_text_artifact, sanitize_file_component},
  call::{
    app_identifier, optional_i64, optional_non_empty_string, optional_string, parse_mouse_button,
    parse_window_selection,
  },
  geometry::{render_rect_compact, resolve_window_point},
  selector::{parse_app_selector, resolve_app_ref, resolve_window_candidate_for_input},
};
use super::super::{DriverCall, DriverResponse};
use super::common::{
  ClickPointCallOptions, build_click_point_call, parse_input_policy, resolve_click_interval_ms,
};
use super::pointer::click_point;
use crate::contract::{Candidate, TargetGrounding};
use crate::model::AuvResult;
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WindowClickDeliveryPath {
  WindowTargetedMouse,
  ForegroundGlobalHid,
}

pub(crate) fn click_window_point(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| {
      "operation requires --target <application-id> or --app <application-id>".to_string()
    })?;
  let selector = parse_app_selector(&app)?;
  let selection = parse_window_selection(call)?;

  let displays = super::super::capture::xcap_backend::list_displays().unwrap_or_default();
  let snapshot = super::super::observe::list_windows_snapshot(
    auv_driver_macos::native::window::ListWindowsOptions::app(128, &app),
  )?;
  let resolved_app = resolve_app_ref(&snapshot, &selector)?;
  let selected =
    resolve_window_candidate_for_input(&snapshot, &resolved_app, &displays, &selection)?;
  let window = &selected.window_ref;

  let consumed_candidate_local_id = optional_non_empty_window_click_candidate(call)?;
  let (logical_x, logical_y, coordinate_summary) =
    resolve_window_point_with_candidate(call, window, consumed_candidate_local_id.as_deref())?;
  let window_x = logical_x - window.bounds.x as f64;
  let window_y = logical_y - window.bounds.y as f64;
  let button_label = optional_string(call, "button").unwrap_or_else(|| "left".to_string());
  let (parsed_button_name, _) = parse_mouse_button(call)?;
  if parsed_button_name != "left" {
    return Err("typed window click currently supports only --button left".to_string());
  }
  let click_count = optional_i64(call, "click_count")?.unwrap_or(1);
  let click_interval_ms = resolve_click_interval_ms(call)?;
  let input_policy = parse_input_policy(call)?;
  let delivery_path = parse_window_click_delivery_path(call)?;
  let window_click_strategy = parse_window_click_strategy(call)?;
  let click_outcome = match delivery_path {
    WindowClickDeliveryPath::WindowTargetedMouse => {
      let click = parse_window_click(call, click_interval_ms)?;
      let typed_window = typed_window_from_ref(&selected);
      ClickWindowPointOutcome::from_typed(
        crate::driver::macos::typed::session::click_window_point_bridge(
          typed_window,
          window_x,
          window_y,
          input_policy,
          click,
          window_click_strategy,
        )?,
      )
    }
    WindowClickDeliveryPath::ForegroundGlobalHid => {
      if input_policy != auv_driver::InputPolicy::ForegroundPreferred {
        return Err(
          "--delivery_path foreground_global_hid requires --input_policy foreground_preferred"
            .to_string(),
        );
      }
      // NOTICE: This path intentionally uses the global HID click primitive.
      // Some app-rendered or canvas-backed affordances need the mouse-move +
      // settle behavior in `click_point`; use this as a mitigation when a
      // window-targeted click reports success but the UI does not react.
      let nested_call = build_click_point_call(
        &call.target,
        call.working_directory.as_path(),
        call.run_context.clone(),
        ClickPointCallOptions {
          x: logical_x,
          y: logical_y,
          button: &button_label,
          click_count,
          click_interval_ms: Some(click_interval_ms),
          settle_ms: None,
          app: Some(&app),
        },
      );
      let _ = click_point(&nested_call)?;
      ClickWindowPointOutcome {
        input_policy: "foreground_preferred",
        input_bridge: "legacy-quartz",
        selected_path: "foreground_global_hid",
        fallback_reason: Some("explicit_delivery_path".to_string()),
      }
    }
  };

  let artifact = build_text_artifact(
    "click-window-point",
    "txt",
    &format!("click-window-point-{}", sanitize_file_component(&app)),
    [
      format!("app={app}"),
      format!("appSelector={}", resolved_app.selector.raw),
      format!("matchStrategy={}", resolved_app.match_strategy),
      format!(
        "resolvedAppBundleId={}",
        resolved_app
          .resolved_bundle_id
          .clone()
          .unwrap_or_else(|| "n/a".to_string())
      ),
      format!("resolvedAppName={}", resolved_app.resolved_app_name),
      format!("windowRef={}", window.window_number),
      format!("windowTitle={}", window.title),
      format!("windowBounds={}", render_rect_compact(&window.bounds)),
      format!("ownerBundleId={}", window.owner_bundle_id),
      format!("ownerPid={}", window.owner_pid),
      format!("candidateIndex={}", selected.candidate_index),
      format!("selectionReason={}", selected.selection_reason),
      format!(
        "isFullyContainedInDisplay={}",
        selected.is_fully_contained_in_display
      ),
      format!("resolvedLogicalPoint={logical_x:.3},{logical_y:.3}"),
      format!("windowPoint={window_x:.3},{window_y:.3}"),
      coordinate_summary.clone(),
      format!("button={button_label}"),
      format!("clickCount={click_count}"),
      format!("clickIntervalMs={click_interval_ms}"),
      format!(
        "consumedCandidateLocalId={}",
        consumed_candidate_local_id.as_deref().unwrap_or("-")
      ),
      format!("inputPolicy={}", click_outcome.input_policy),
      format!("deliveryPath={}", delivery_path.name()),
      format!(
        "windowClickStrategy={}",
        window_click_strategy_name(window_click_strategy)
      ),
      format!("inputBridge={}", click_outcome.input_bridge),
      format!("selectedPath={}", click_outcome.selected_path),
      format!(
        "fallbackReason={}",
        click_outcome.fallback_reason.as_deref().unwrap_or("-")
      ),
    ]
    .join("\n"),
    "Clicked a point relative to a resolved macOS app window.",
  )?;
  let mut notes = vec![
    format!("app={app}"),
    format!("appSelector={}", resolved_app.selector.raw),
    format!("matchStrategy={}", resolved_app.match_strategy),
    format!(
      "resolvedAppBundleId={}",
      resolved_app
        .resolved_bundle_id
        .clone()
        .unwrap_or_else(|| "n/a".to_string())
    ),
    format!("windowRef={}", window.window_number),
    format!("windowBounds={}", render_rect_compact(&window.bounds)),
    format!("candidateIndex={}", selected.candidate_index),
    format!("selectionReason={}", selected.selection_reason),
    format!(
      "isFullyContainedInDisplay={}",
      selected.is_fully_contained_in_display
    ),
    format!("logicalPoint={logical_x:.3},{logical_y:.3}"),
    format!("windowPoint={window_x:.3},{window_y:.3}"),
    coordinate_summary,
    format!("clickIntervalMs={click_interval_ms}"),
    format!(
      "consumedCandidateLocalId={}",
      consumed_candidate_local_id.as_deref().unwrap_or("-")
    ),
    format!("inputPolicy={}", click_outcome.input_policy),
    format!("deliveryPath={}", delivery_path.name()),
    format!(
      "windowClickStrategy={}",
      window_click_strategy_name(window_click_strategy)
    ),
    format!("inputBridge={}", click_outcome.input_bridge),
    format!("selectedPath={}", click_outcome.selected_path),
  ];
  if let Some(reason) = click_outcome.fallback_reason {
    notes.push(format!("fallbackReason={reason}"));
  }
  if !window.owner_bundle_id.is_empty() {
    notes.push(format!("ownerBundleId={}", window.owner_bundle_id));
  }
  if !window.title.is_empty() {
    notes.push(format!("windowTitle={}", window.title));
  }

  Ok(DriverResponse {
    summary: format!(
      "Clicked {} window-relative point in {} at window point ({window_x:.3}, {window_y:.3}) via {}.",
      button_label, app, click_outcome.selected_path
    ),
    backend: Some("macos.typed.input.click-window-point".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes,
    artifacts: vec![artifact],
  })
}

fn optional_non_empty_window_click_candidate(call: &DriverCall) -> AuvResult<Option<String>> {
  match optional_non_empty_string(call, "candidate") {
    Some(raw_candidate) => {
      let candidate = parse_window_click_candidate(&raw_candidate)?;
      Ok(Some(candidate.candidate_local_id))
    }
    None => Ok(None),
  }
}

fn resolve_window_point_with_candidate(
  call: &DriverCall,
  window: &super::super::WindowRef,
  consumed_candidate_local_id: Option<&str>,
) -> AuvResult<(f64, f64, String)> {
  let Some(raw_candidate) = optional_non_empty_string(call, "candidate") else {
    return resolve_window_point(call, window);
  };

  let candidate = parse_window_click_candidate(&raw_candidate)?;
  ensure_window_click_candidate_matches_window(&candidate, window)?;
  let observation = candidate.evidence.observation.as_object().ok_or_else(|| {
    "candidate observation is missing coordinate detail for click_window_point".to_string()
  })?;
  let relative_point = observation
    .get("relative_point")
    .and_then(|value| value.as_object())
    .ok_or_else(|| {
      "candidate observation is missing relative_point detail for click_window_point".to_string()
    })?;
  let relative_x = relative_point
    .get("x")
    .and_then(|value| value.as_f64())
    .ok_or_else(|| "candidate observation relative_point.x must be a number".to_string())?;
  let relative_y = relative_point
    .get("y")
    .and_then(|value| value.as_f64())
    .ok_or_else(|| "candidate observation relative_point.y must be a number".to_string())?;
  let logical_x = window.bounds.x as f64 + (window.bounds.width as f64 * relative_x);
  let logical_y = window.bounds.y as f64 + (window.bounds.height as f64 * relative_y);
  Ok((
    logical_x,
    logical_y,
    format!(
      "windowCandidate={} windowRelative={relative_x:.3},{relative_y:.3}",
      consumed_candidate_local_id.unwrap_or(candidate.candidate_local_id.as_str())
    ),
  ))
}

fn parse_window_click_candidate(raw_candidate: &str) -> AuvResult<Candidate> {
  let candidate: Candidate = serde_json::from_str(raw_candidate)
    .map_err(|error| format!("invalid --candidate JSON: {error}"))?;
  if candidate.target_spec.grounding != TargetGrounding::Coordinate {
    return Err(format!(
      "click_window_point only accepts Coordinate candidates; got {:?}",
      candidate.target_spec.grounding
    ));
  }
  let observation = candidate.evidence.observation.as_object().ok_or_else(|| {
    "candidate observation is missing coordinate detail for click_window_point".to_string()
  })?;
  let relative_point = observation
    .get("relative_point")
    .and_then(|value| value.as_object())
    .ok_or_else(|| {
      "candidate observation is missing relative_point detail for click_window_point".to_string()
    })?;
  let relative_x = relative_point
    .get("x")
    .and_then(|value| value.as_f64())
    .ok_or_else(|| "candidate observation relative_point.x must be a number".to_string())?;
  let relative_y = relative_point
    .get("y")
    .and_then(|value| value.as_f64())
    .ok_or_else(|| "candidate observation relative_point.y must be a number".to_string())?;
  if !(0.0..=1.0).contains(&relative_x) || !(0.0..=1.0).contains(&relative_y) {
    return Err(format!(
      "candidate relative_point must stay within 0.0..=1.0, got x={relative_x:.6} y={relative_y:.6}"
    ));
  }
  Ok(candidate)
}

fn ensure_window_click_candidate_matches_window(
  candidate: &Candidate,
  window: &crate::driver::macos::WindowRef,
) -> AuvResult<()> {
  let Some(expected_window) = candidate.liveness.preconditions.window_ref.as_ref() else {
    return Ok(());
  };

  if !expected_window.app_bundle_id.trim().is_empty()
    && !window.owner_bundle_id.trim().is_empty()
    && !window
      .owner_bundle_id
      .eq_ignore_ascii_case(expected_window.app_bundle_id.as_str())
  {
    return Err(format!(
      "click_window_point candidate {} expected app bundle {} but resolved window belonged to {}",
      candidate.candidate_local_id, expected_window.app_bundle_id, window.owner_bundle_id
    ));
  }

  if let Some(expected_title) = expected_window.window_title_substring.as_deref() {
    let expected_title = expected_title.trim();
    if !expected_title.is_empty() && !window.title.contains(expected_title) {
      return Err(format!(
        "click_window_point candidate {} expected window title containing {:?} but resolved window title was {:?}",
        candidate.candidate_local_id, expected_title, window.title
      ));
    }
  }

  if let Some(expected_window_number) = expected_window.window_number
    && window.window_number != expected_window_number
  {
    return Err(format!(
      "click_window_point candidate {} expected window number {} but resolved {}",
      candidate.candidate_local_id, expected_window_number, window.window_number
    ));
  }

  Ok(())
}

struct ClickWindowPointOutcome {
  input_policy: &'static str,
  input_bridge: &'static str,
  selected_path: &'static str,
  fallback_reason: Option<String>,
}

impl ClickWindowPointOutcome {
  fn from_typed(outcome: crate::driver::macos::typed::session::InputActionBridgeOutcome) -> Self {
    Self {
      input_policy: outcome.input_policy,
      input_bridge: outcome.input_bridge,
      selected_path: outcome.selected_path,
      fallback_reason: outcome.fallback_reason,
    }
  }
}

impl WindowClickDeliveryPath {
  fn name(self) -> &'static str {
    match self {
      Self::WindowTargetedMouse => "window_targeted_mouse",
      Self::ForegroundGlobalHid => "foreground_global_hid",
    }
  }
}

pub(crate) fn parse_window_click_delivery_path(
  call: &DriverCall,
) -> AuvResult<WindowClickDeliveryPath> {
  match optional_string(call, "delivery_path")
    .unwrap_or_else(|| "window_targeted_mouse".to_string())
    .trim()
    .to_ascii_lowercase()
    .as_str()
  {
    "window_targeted_mouse" | "window-targeted-mouse" => {
      Ok(WindowClickDeliveryPath::WindowTargetedMouse)
    }
    "foreground_global_hid" | "foreground-global-hid" => {
      Ok(WindowClickDeliveryPath::ForegroundGlobalHid)
    }
    other => Err(format!(
      "invalid --delivery_path value {other:?}; expected window_targeted_mouse or foreground_global_hid"
    )),
  }
}

pub(crate) fn parse_window_click_strategy(
  call: &DriverCall,
) -> AuvResult<auv_driver::WindowClickStrategy> {
  match optional_string(call, "window_click_strategy")
    .unwrap_or_else(|| "chromium_compatible".to_string())
    .trim()
    .to_ascii_lowercase()
    .as_str()
  {
    "chromium_compatible" | "chromium-compatible" => {
      Ok(auv_driver::WindowClickStrategy::ChromiumCompatible)
    }
    "pid_targeted" | "pid-targeted" => Ok(auv_driver::WindowClickStrategy::PidTargeted),
    other => Err(format!(
      "invalid --window-click-strategy value {other:?}; expected chromium_compatible or pid_targeted"
    )),
  }
}

pub(crate) fn parse_window_click(
  call: &DriverCall,
  click_interval_ms: u64,
) -> AuvResult<auv_driver::Click> {
  match optional_i64(call, "click_count")?.unwrap_or(1) {
    1 => Ok(auv_driver::Click::Single),
    2 => Ok(auv_driver::Click::Double {
      interval: Duration::from_millis(click_interval_ms),
    }),
    _ => Err("typed window click supports only click_count 1 or 2".to_string()),
  }
}

fn window_click_strategy_name(strategy: auv_driver::WindowClickStrategy) -> &'static str {
  match strategy {
    auv_driver::WindowClickStrategy::ChromiumCompatible => "chromium_compatible",
    auv_driver::WindowClickStrategy::PidTargeted => "pid_targeted",
  }
}

fn typed_window_from_ref(candidate: &super::super::WindowCandidate) -> auv_driver::Window {
  let window = &candidate.window_ref;
  auv_driver::Window {
    reference: auv_driver::WindowRef {
      id: window.window_number.to_string(),
    },
    title: (!window.title.is_empty()).then(|| window.title.clone()),
    app_name: (!window.app_name.is_empty()).then(|| window.app_name.clone()),
    app_bundle_id: (!window.owner_bundle_id.is_empty()).then(|| window.owner_bundle_id.clone()),
    process_id: u32::try_from(window.owner_pid).ok(),
    frame: auv_driver::Rect::new(
      window.bounds.x as f64,
      window.bounds.y as f64,
      window.bounds.width as f64,
      window.bounds.height as f64,
    ),
    coordinate_space: auv_driver::CoordinateSpace::Screen,
    is_main: candidate.is_main_candidate,
    is_visible: window.bounds.width > 0 && window.bounds.height > 0,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::contract::{
    ArtifactRef, CandidateEvidence, CandidateLiveness, ControlRequirements, LivenessPreconditions,
    TargetSpec, WindowRefPrecondition,
  };
  use crate::trace::{ArtifactId, RunId, SpanId};
  use serde_json::json;
  use std::collections::BTreeMap;
  use std::path::PathBuf;

  fn build_call<const N: usize>(entries: [(&str, &str); N]) -> DriverCall {
    let mut inputs = BTreeMap::new();
    for (key, value) in entries {
      inputs.insert(key.to_string(), value.to_string());
    }

    DriverCall {
      operation: "test".to_string(),
      target: crate::model::ExecutionTarget::default(),
      inputs,
      working_directory: PathBuf::from("."),
      run_context: crate::model::DriverRunContext::default(),
    }
  }

  fn sample_window_ref() -> crate::driver::macos::WindowRef {
    crate::driver::macos::WindowRef {
      window_number: 7,
      owner_pid: 1,
      owner_bundle_id: "com.example.editor".to_string(),
      app_name: "ExampleEditor".to_string(),
      title: "Untitled".to_string(),
      bounds: crate::driver::macos::ObservedRect {
        x: 100,
        y: 200,
        width: 640,
        height: 480,
      },
      layer: 0,
    }
  }

  fn sample_window_candidate_json(grounding: TargetGrounding) -> String {
    serde_json::to_string(&Candidate {
      candidate_local_id: "window-primary-region".to_string(),
      kind: "window_action".to_string(),
      label: Some("Example".to_string()),
      target_spec: TargetSpec {
        grounding,
        anchor_text: None,
        region_hint: None,
        row_index: None,
      },
      evidence: CandidateEvidence {
        artifact_ref: ArtifactRef {
          run_id: RunId::new("run_probe"),
          span_id: SpanId::new("span_probe"),
          artifact_id: ArtifactId::new("artifact_0002"),
          captured_event_id: None,
        },
        observation: json!({
          "source": "window",
          "surface_candidate_id": "window-primary-region",
          "relative_point": {
            "x": 0.5,
            "y": 0.25
          }
        }),
      },
      liveness: CandidateLiveness {
        preconditions: LivenessPreconditions {
          window_ref: Some(WindowRefPrecondition {
            app_bundle_id: "com.example.editor".to_string(),
            window_title_substring: Some("Untitled".to_string()),
            window_number: None,
          }),
          anchor_recheck: None,
        },
        ttl_hint_ms: Some(5000),
      },
      control: ControlRequirements {
        requires_app_frontmost: true,
        requires_window_focus: true,
      },
      known_limits: Vec::new(),
    })
    .expect("sample candidate should serialize")
  }

  #[test]
  fn optional_non_empty_window_click_candidate_accepts_coordinate_candidate() {
    let candidate_json = sample_window_candidate_json(TargetGrounding::Coordinate);
    let call = build_call([("candidate", candidate_json.as_str())]);

    let candidate_local_id =
      optional_non_empty_window_click_candidate(&call).expect("coordinate candidate should parse");

    assert_eq!(candidate_local_id.as_deref(), Some("window-primary-region"));
  }

  #[test]
  fn optional_non_empty_window_click_candidate_rejects_non_coordinate_candidate() {
    let candidate_json = sample_window_candidate_json(TargetGrounding::AxNode);
    let call = build_call([("candidate", candidate_json.as_str())]);

    let error = optional_non_empty_window_click_candidate(&call)
      .expect_err("non-coordinate candidate should fail");

    assert!(error.contains("only accepts Coordinate candidates"));
  }

  #[test]
  fn resolve_window_point_with_candidate_uses_relative_point_from_candidate() {
    let candidate_json = sample_window_candidate_json(TargetGrounding::Coordinate);
    let call = build_call([("candidate", candidate_json.as_str())]);
    let window = sample_window_ref();

    let (x, y, summary) =
      resolve_window_point_with_candidate(&call, &window, Some("window-primary-region"))
        .expect("candidate point should resolve");

    assert_eq!(x, 420.0);
    assert_eq!(y, 320.0);
    assert_eq!(
      summary,
      "windowCandidate=window-primary-region windowRelative=0.500,0.250"
    );
  }

  #[test]
  fn resolve_window_point_with_candidate_rejects_mismatched_window_bundle() {
    let candidate_json = sample_window_candidate_json_with_window_ref(
      TargetGrounding::Coordinate,
      "com.other.App",
      Some("Untitled"),
      None,
    );
    let call = build_call([("candidate", candidate_json.as_str())]);
    let window = sample_window_ref();

    let error = resolve_window_point_with_candidate(&call, &window, Some("window-primary-region"))
      .expect_err("candidate with mismatched bundle should fail");

    assert!(error.contains("expected app bundle"));
  }

  #[test]
  fn resolve_window_point_with_candidate_rejects_mismatched_window_title() {
    let candidate_json = sample_window_candidate_json_with_window_ref(
      TargetGrounding::Coordinate,
      "com.example.editor",
      Some("Other Window"),
      None,
    );
    let call = build_call([("candidate", candidate_json.as_str())]);
    let window = sample_window_ref();

    let error = resolve_window_point_with_candidate(&call, &window, Some("window-primary-region"))
      .expect_err("candidate with mismatched title should fail");

    assert!(error.contains("expected window title containing"));
  }

  #[test]
  fn resolve_window_point_with_candidate_rejects_mismatched_window_number() {
    let candidate_json = sample_window_candidate_json_with_window_ref(
      TargetGrounding::Coordinate,
      "com.example.editor",
      Some("Untitled"),
      Some(99),
    );
    let call = build_call([("candidate", candidate_json.as_str())]);
    let window = sample_window_ref();

    let error = resolve_window_point_with_candidate(&call, &window, Some("window-primary-region"))
      .expect_err("candidate with mismatched window number should fail");

    assert!(error.contains("expected window number 99"));
  }

  fn sample_window_candidate_json_with_window_ref(
    grounding: TargetGrounding,
    app_bundle_id: &str,
    window_title_substring: Option<&str>,
    window_number: Option<i64>,
  ) -> String {
    serde_json::to_string(&Candidate {
      candidate_local_id: "window-primary-region".to_string(),
      kind: "window_action".to_string(),
      label: Some("Example".to_string()),
      target_spec: TargetSpec {
        grounding,
        anchor_text: None,
        region_hint: None,
        row_index: None,
      },
      evidence: CandidateEvidence {
        artifact_ref: ArtifactRef {
          run_id: RunId::new("run_probe"),
          span_id: SpanId::new("span_probe"),
          artifact_id: ArtifactId::new("artifact_0002"),
          captured_event_id: None,
        },
        observation: json!({
          "source": "window",
          "surface_candidate_id": "window-primary-region",
          "relative_point": {
            "x": 0.5,
            "y": 0.25
          }
        }),
      },
      liveness: CandidateLiveness {
        preconditions: LivenessPreconditions {
          window_ref: Some(WindowRefPrecondition {
            app_bundle_id: app_bundle_id.to_string(),
            window_title_substring: window_title_substring.map(str::to_string),
            window_number,
          }),
          anchor_recheck: None,
        },
        ttl_hint_ms: Some(5000),
      },
      control: ControlRequirements {
        requires_app_frontmost: true,
        requires_window_focus: true,
      },
      known_limits: Vec::new(),
    })
    .expect("sample candidate should serialize")
  }
}
