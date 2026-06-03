// File: src/driver/macos/control/window.rs
use super::super::support::{
  artifacts::{build_text_artifact, sanitize_file_component},
  call::{
    app_identifier, optional_i64, optional_string, parse_mouse_button, parse_window_selection,
  },
  geometry::{render_rect_compact, resolve_window_point},
  selector::{parse_app_selector, resolve_app_ref, resolve_window_candidate_for_input},
};
use super::super::{DriverCall, DriverResponse};
use super::common::{
  ClickPointCallOptions, build_click_point_call, parse_input_policy, resolve_click_interval_ms,
};
use super::pointer::click_point;
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

  let (logical_x, logical_y, coordinate_summary) = resolve_window_point(call, window)?;
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
