// File: src/driver/macos/native/window.rs
#[cfg(target_os = "macos")]
use super::binding::ffi::{
  NativeBundleIdsByPidRequest, NativeBundleIdsByPidResponse, NativeDisplayListResponse,
  NativeWindowListRequest, NativeWindowListResponse, NativeWindowMutationKind,
  NativeWindowMutationRequest, NativeWindowMutationResponse,
  bundle_ids_by_pid as native_bundle_ids_by_pid, list_displays,
  list_windows as native_list_windows, mutate_window as native_mutate_window,
};
use super::types::{
  AuvResult, ObservedDisplay, ObservedDisplaySnapshot, ObservedRect, ObservedWindow,
  ObservedWindowSnapshot, compute_combined_bounds,
};
use std::collections::{HashMap, HashSet};

#[cfg(target_os = "macos")]
pub fn enumerate_displays() -> AuvResult<ObservedDisplaySnapshot> {
  decode_display_response(DecodedDisplayListResponse::from(list_displays()))
}

#[cfg(not(target_os = "macos"))]
pub fn enumerate_displays() -> AuvResult<ObservedDisplaySnapshot> {
  Err("macOS native display enumeration is unsupported on this target".to_string())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListWindowsOptions {
  pub limit: i64,
  pub scope: WindowListScope,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowListScope {
  /// Best-effort global enumeration. On some macOS/app combinations this is
  /// not guaranteed to be a superset of app-scoped queries.
  AllVisible,
  /// App-scoped query for resolving or inspecting a known target application.
  App(String),
}

impl ListWindowsOptions {
  pub fn all_visible(limit: i64) -> Self {
    Self {
      limit,
      scope: WindowListScope::AllVisible,
    }
  }

  pub fn app(limit: i64, app: impl Into<String>) -> Self {
    Self {
      limit,
      scope: WindowListScope::App(app.into()),
    }
  }
}

#[cfg(target_os = "macos")]
pub fn list_windows(options: ListWindowsOptions) -> AuvResult<ObservedWindowSnapshot> {
  let response = native_list_windows(NativeWindowListRequest {
    limit: options.limit,
    app_filter: options.scope.app_filter(),
  });
  decode_window_response(DecodedWindowListResponse::from(response))
}

#[cfg(not(target_os = "macos"))]
pub fn list_windows(_options: ListWindowsOptions) -> AuvResult<ObservedWindowSnapshot> {
  Err("macOS native window listing is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn mutate_window(
  request: DecodedWindowMutationRequest,
) -> AuvResult<DecodedWindowMutationResponse> {
  // TODO(window-management-api-task3): pointer or foreground fallback is deferred
  // because Task 2 owns only the native AX bridge; add it when WindowApi dispatch
  // chooses fallback policy.
  let response = native_mutate_window(NativeWindowMutationRequest {
    pid: request.pid,
    window_number: request.window_number,
    title: request.title,
    kind: NativeWindowMutationKind::from(request.kind),
    x: request.x,
    y: request.y,
    width: request.width,
    height: request.height,
  });
  decode_window_mutation_response(DecodedWindowMutationResponse::from(response))
}

#[cfg(not(target_os = "macos"))]
pub fn mutate_window(
  _request: DecodedWindowMutationRequest,
) -> AuvResult<DecodedWindowMutationResponse> {
  Err("macOS native window mutation is unsupported on this target".to_string())
}

impl WindowListScope {
  fn app_filter(&self) -> String {
    match self {
      Self::AllVisible => String::new(),
      Self::App(app) => app.clone(),
    }
  }
}

pub fn decode_display_response(
  response: DecodedDisplayListResponse,
) -> AuvResult<ObservedDisplaySnapshot> {
  if response.error_message.is_some() {
    return super::error::native_result(
      "list_displays",
      None,
      response.error_message,
      response.recovery_hint,
    );
  }

  let count = response.ids.len();
  let lengths = [
    response.main_flags.len(),
    response.built_in_flags.len(),
    response.bounds_x_values.len(),
    response.bounds_y_values.len(),
    response.bounds_width_values.len(),
    response.bounds_height_values.len(),
    response.visible_x_values.len(),
    response.visible_y_values.len(),
    response.visible_width_values.len(),
    response.visible_height_values.len(),
    response.scale_factors.len(),
    response.pixel_width_values.len(),
    response.pixel_height_values.len(),
  ];
  if lengths.iter().any(|length| *length != count) {
    return Err("native display response had mismatched vector lengths".to_string());
  }

  let displays = (0..count)
    .map(|index| {
      let display_id = u32::try_from(response.ids[index]).map_err(|error| {
        format!(
          "native display response had invalid display id {}: {error}",
          response.ids[index]
        )
      })?;
      Ok(ObservedDisplay {
        display_id,
        is_main: response.main_flags[index],
        is_built_in: response.built_in_flags[index],
        bounds: ObservedRect {
          x: response.bounds_x_values[index],
          y: response.bounds_y_values[index],
          width: response.bounds_width_values[index],
          height: response.bounds_height_values[index],
        },
        visible_bounds: ObservedRect {
          x: response.visible_x_values[index],
          y: response.visible_y_values[index],
          width: response.visible_width_values[index],
          height: response.visible_height_values[index],
        },
        scale_factor: response.scale_factors[index],
        pixel_width: response.pixel_width_values[index],
        pixel_height: response.pixel_height_values[index],
      })
    })
    .collect::<AuvResult<Vec<_>>>()?;

  if displays.is_empty() {
    return Err("display probe returned no connected displays".to_string());
  }

  Ok(ObservedDisplaySnapshot {
    combined_bounds: compute_combined_bounds(&displays),
    displays,
    captured_at: response.captured_at,
  })
}

pub fn decode_window_response(
  response: DecodedWindowListResponse,
) -> AuvResult<ObservedWindowSnapshot> {
  if response.error_message.is_some() {
    return super::error::native_result(
      "list_windows",
      None,
      response.error_message,
      response.recovery_hint,
    );
  }

  let count = response.app_names.len();
  let lengths = [
    response.owner_pids.len(),
    response.owner_bundle_ids.len(),
    response.window_numbers.len(),
    response.layers.len(),
    response.titles.len(),
    response.x_values.len(),
    response.y_values.len(),
    response.width_values.len(),
    response.height_values.len(),
  ];
  if lengths.iter().any(|length| *length != count) {
    return Err("native window response had mismatched vector lengths".to_string());
  }

  let windows = (0..count)
    .map(|index| ObservedWindow {
      app_name: response.app_names[index].clone(),
      owner_pid: response.owner_pids[index],
      owner_bundle_id: response.owner_bundle_ids[index].clone(),
      window_number: response.window_numbers[index],
      layer: response.layers[index],
      title: response.titles[index].clone(),
      bounds: ObservedRect {
        x: response.x_values[index],
        y: response.y_values[index],
        width: response.width_values[index],
        height: response.height_values[index],
      },
    })
    .collect();

  Ok(ObservedWindowSnapshot {
    frontmost_app_name: response.frontmost_app_name,
    frontmost_app_bundle_id: response.frontmost_app_bundle_id,
    frontmost_window_title: response.frontmost_window_title,
    observed_at: response.observed_at,
    windows,
  })
}

#[cfg(target_os = "macos")]
pub fn bundle_ids_by_pid(pids: &HashSet<u32>) -> AuvResult<HashMap<u32, String>> {
  let mut sorted_pids = pids.iter().copied().collect::<Vec<_>>();
  sorted_pids.sort_unstable();
  let response = native_bundle_ids_by_pid(NativeBundleIdsByPidRequest {
    pids: sorted_pids.into_iter().map(i64::from).collect(),
  });
  decode_bundle_ids_by_pid_response(DecodedBundleIdsByPidResponse::from(response))
}

#[cfg(not(target_os = "macos"))]
pub fn bundle_ids_by_pid(_pids: &HashSet<u32>) -> AuvResult<HashMap<u32, String>> {
  Err("macOS native bundle id lookup is unsupported on this target".to_string())
}

pub fn decode_bundle_ids_by_pid_response(
  response: DecodedBundleIdsByPidResponse,
) -> AuvResult<HashMap<u32, String>> {
  if response.error_message.is_some() {
    return super::error::native_result(
      "bundle_ids_by_pid",
      None,
      response.error_message,
      response.recovery_hint,
    );
  }
  if response.pids.len() != response.bundle_ids.len() {
    return Err("native bundle id response had mismatched vector lengths".to_string());
  }

  let mut bundle_ids = HashMap::new();
  for (pid, bundle_id) in response.pids.into_iter().zip(response.bundle_ids) {
    let pid = u32::try_from(pid)
      .map_err(|error| format!("native bundle id response had invalid pid: {error}"))?;
    if !bundle_id.trim().is_empty() {
      bundle_ids.insert(pid, bundle_id);
    }
  }
  Ok(bundle_ids)
}

pub fn decode_window_mutation_response(
  response: DecodedWindowMutationResponse,
) -> AuvResult<DecodedWindowMutationResponse> {
  if response.error_message.is_some() {
    return super::error::native_result(
      "mutate_window",
      None,
      response.error_message,
      response.recovery_hint,
    );
  }
  Ok(response)
}

#[derive(Clone, Debug)]
pub struct DecodedDisplayListResponse {
  pub captured_at: String,
  pub ids: Vec<i64>,
  pub main_flags: Vec<bool>,
  pub built_in_flags: Vec<bool>,
  pub bounds_x_values: Vec<i64>,
  pub bounds_y_values: Vec<i64>,
  pub bounds_width_values: Vec<i64>,
  pub bounds_height_values: Vec<i64>,
  pub visible_x_values: Vec<i64>,
  pub visible_y_values: Vec<i64>,
  pub visible_width_values: Vec<i64>,
  pub visible_height_values: Vec<i64>,
  pub scale_factors: Vec<f64>,
  pub pixel_width_values: Vec<i64>,
  pub pixel_height_values: Vec<i64>,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DecodedWindowListResponse {
  pub observed_at: String,
  pub frontmost_app_name: String,
  pub frontmost_app_bundle_id: String,
  pub frontmost_window_title: String,
  pub app_names: Vec<String>,
  pub owner_pids: Vec<i64>,
  pub owner_bundle_ids: Vec<String>,
  pub window_numbers: Vec<i64>,
  pub layers: Vec<i64>,
  pub titles: Vec<String>,
  pub x_values: Vec<i64>,
  pub y_values: Vec<i64>,
  pub width_values: Vec<i64>,
  pub height_values: Vec<i64>,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DecodedBundleIdsByPidResponse {
  pub pids: Vec<i64>,
  pub bundle_ids: Vec<String>,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodedWindowMutationKind {
  MoveTo,
  Resize,
  SetFrame,
  Minimize,
  Restore,
  Zoom,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedWindowMutationRequest {
  pub pid: i64,
  pub window_number: i64,
  pub title: String,
  pub kind: DecodedWindowMutationKind,
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedWindowMutationResponse {
  pub performed_action: String,
  pub path: String,
  pub before_x: i64,
  pub before_y: i64,
  pub before_width: i64,
  pub before_height: i64,
  pub after_x: i64,
  pub after_y: i64,
  pub after_width: i64,
  pub after_height: i64,
  pub was_minimized: bool,
  pub is_minimized: bool,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

#[cfg(target_os = "macos")]
impl From<DecodedWindowMutationKind> for NativeWindowMutationKind {
  fn from(value: DecodedWindowMutationKind) -> Self {
    match value {
      DecodedWindowMutationKind::MoveTo => Self::MoveTo,
      DecodedWindowMutationKind::Resize => Self::Resize,
      DecodedWindowMutationKind::SetFrame => Self::SetFrame,
      DecodedWindowMutationKind::Minimize => Self::Minimize,
      DecodedWindowMutationKind::Restore => Self::Restore,
      DecodedWindowMutationKind::Zoom => Self::Zoom,
    }
  }
}

#[cfg(target_os = "macos")]
impl From<NativeDisplayListResponse> for DecodedDisplayListResponse {
  fn from(value: NativeDisplayListResponse) -> Self {
    Self {
      captured_at: value.captured_at,
      ids: value.ids,
      main_flags: value.main_flags,
      built_in_flags: value.built_in_flags,
      bounds_x_values: value.bounds_x_values,
      bounds_y_values: value.bounds_y_values,
      bounds_width_values: value.bounds_width_values,
      bounds_height_values: value.bounds_height_values,
      visible_x_values: value.visible_x_values,
      visible_y_values: value.visible_y_values,
      visible_width_values: value.visible_width_values,
      visible_height_values: value.visible_height_values,
      scale_factors: value.scale_factors,
      pixel_width_values: value.pixel_width_values,
      pixel_height_values: value.pixel_height_values,
      error_message: value.error_message,
      recovery_hint: value.recovery_hint,
    }
  }
}

#[cfg(target_os = "macos")]
impl From<NativeWindowListResponse> for DecodedWindowListResponse {
  fn from(value: NativeWindowListResponse) -> Self {
    Self {
      observed_at: value.observed_at,
      frontmost_app_name: value.frontmost_app_name,
      frontmost_app_bundle_id: value.frontmost_app_bundle_id,
      frontmost_window_title: value.frontmost_window_title,
      app_names: value.app_names,
      owner_pids: value.owner_pids,
      owner_bundle_ids: value.owner_bundle_ids,
      window_numbers: value.window_numbers,
      layers: value.layers,
      titles: value.titles,
      x_values: value.x_values,
      y_values: value.y_values,
      width_values: value.width_values,
      height_values: value.height_values,
      error_message: value.error_message,
      recovery_hint: value.recovery_hint,
    }
  }
}

#[cfg(target_os = "macos")]
impl From<NativeBundleIdsByPidResponse> for DecodedBundleIdsByPidResponse {
  fn from(value: NativeBundleIdsByPidResponse) -> Self {
    Self {
      pids: value.pids,
      bundle_ids: value.bundle_ids,
      error_message: value.error_message,
      recovery_hint: value.recovery_hint,
    }
  }
}

#[cfg(target_os = "macos")]
impl From<NativeWindowMutationResponse> for DecodedWindowMutationResponse {
  fn from(value: NativeWindowMutationResponse) -> Self {
    Self {
      performed_action: value.performed_action,
      path: value.path,
      before_x: value.before_x,
      before_y: value.before_y,
      before_width: value.before_width,
      before_height: value.before_height,
      after_x: value.after_x,
      after_y: value.after_y,
      after_width: value.after_width,
      after_height: value.after_height,
      was_minimized: value.was_minimized,
      is_minimized: value.is_minimized,
      error_message: value.error_message,
      recovery_hint: value.recovery_hint,
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn base_mutation_response() -> DecodedWindowMutationResponse {
    DecodedWindowMutationResponse {
      performed_action: "move_to".to_string(),
      path: "pid=42 window_number=7".to_string(),
      before_x: 10,
      before_y: 20,
      before_width: 800,
      before_height: 600,
      after_x: 30,
      after_y: 40,
      after_width: 800,
      after_height: 600,
      was_minimized: false,
      is_minimized: false,
      error_message: None,
      recovery_hint: None,
    }
  }

  #[test]
  fn list_window_scope_maps_to_native_app_filter_explicitly() {
    assert_eq!(WindowListScope::AllVisible.app_filter(), "");
    assert_eq!(
      WindowListScope::App("com.example.App".to_string()).app_filter(),
      "com.example.App"
    );
  }

  #[test]
  fn decode_display_response_rejects_mismatched_vectors() {
    let error = decode_display_response(DecodedDisplayListResponse {
      captured_at: "2026-05-20T00:00:00Z".to_string(),
      ids: vec![1],
      main_flags: vec![],
      built_in_flags: vec![true],
      bounds_x_values: vec![0],
      bounds_y_values: vec![0],
      bounds_width_values: vec![100],
      bounds_height_values: vec![100],
      visible_x_values: vec![0],
      visible_y_values: vec![0],
      visible_width_values: vec![100],
      visible_height_values: vec![100],
      scale_factors: vec![2.0],
      pixel_width_values: vec![200],
      pixel_height_values: vec![200],
      error_message: None,
      recovery_hint: None,
    })
    .unwrap_err();

    assert!(error.contains("mismatched vector lengths"));
  }

  #[test]
  fn decode_window_response_rejects_mismatched_vectors() {
    let error = decode_window_response(DecodedWindowListResponse {
      observed_at: "2026-05-20T00:00:00Z".to_string(),
      frontmost_app_name: "Notes".to_string(),
      frontmost_app_bundle_id: "com.apple.Notes".to_string(),
      frontmost_window_title: "Todo".to_string(),
      app_names: vec!["Notes".to_string()],
      owner_pids: vec![],
      owner_bundle_ids: vec!["com.apple.Notes".to_string()],
      window_numbers: vec![42],
      layers: vec![0],
      titles: vec!["Todo".to_string()],
      x_values: vec![0],
      y_values: vec![0],
      width_values: vec![640],
      height_values: vec![480],
      error_message: None,
      recovery_hint: None,
    })
    .unwrap_err();

    assert!(error.contains("mismatched vector lengths"));
  }

  #[test]
  fn decode_bundle_ids_by_pid_rejects_mismatched_vectors() {
    let error = decode_bundle_ids_by_pid_response(DecodedBundleIdsByPidResponse {
      pids: vec![1],
      bundle_ids: vec![],
      error_message: None,
      recovery_hint: None,
    })
    .unwrap_err();

    assert!(error.contains("mismatched vector lengths"));
  }

  #[test]
  fn decode_window_mutation_response_preserves_bridge_fields() {
    let response = decode_window_mutation_response(base_mutation_response()).unwrap();

    assert_eq!(response.performed_action, "move_to");
    assert_eq!(response.path, "pid=42 window_number=7");
    assert_eq!(response.before_x, 10);
    assert_eq!(response.before_y, 20);
    assert_eq!(response.before_width, 800);
    assert_eq!(response.before_height, 600);
    assert_eq!(response.after_x, 30);
    assert_eq!(response.after_y, 40);
    assert_eq!(response.after_width, 800);
    assert_eq!(response.after_height, 600);
    assert!(!response.was_minimized);
    assert!(!response.is_minimized);
  }

  #[test]
  fn decode_window_mutation_response_maps_native_error() {
    let mut response = base_mutation_response();
    response.error_message = Some("target AX window was not found".to_string());
    response.recovery_hint = Some("refresh the window list and retry".to_string());

    let error = decode_window_mutation_response(response).unwrap_err();

    assert_eq!(
      error,
      "macos native mutate_window failed: target AX window was not found; recovery=refresh the window list and retry"
    );
  }
}
