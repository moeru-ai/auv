// File: src/driver/macos/native/window.rs
#[cfg(target_os = "macos")]
use super::ffi::ffi::{
  NativeBundleIdsByPidRequest, NativeBundleIdsByPidResponse, NativeDisplayListResponse,
  NativeWindowListRequest, NativeWindowListResponse, bundle_ids_by_pid as native_bundle_ids_by_pid,
  list_displays, list_windows,
};
use crate::driver::macos::{
  ObservedDisplay, ObservedDisplaySnapshot, ObservedRect, ObservedWindow, ObservedWindowSnapshot,
  compute_combined_bounds,
};
use crate::model::AuvResult;
use std::collections::{HashMap, HashSet};

#[cfg(target_os = "macos")]
pub(crate) fn enumerate_displays() -> AuvResult<ObservedDisplaySnapshot> {
  decode_display_response(DecodedDisplayListResponse::from(list_displays()))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn enumerate_displays() -> AuvResult<ObservedDisplaySnapshot> {
  Err("macOS native display enumeration is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub(crate) fn observe_windows_snapshot(
  limit: i64,
  app_filter: &str,
) -> AuvResult<ObservedWindowSnapshot> {
  let response = list_windows(NativeWindowListRequest {
    limit,
    app_filter: app_filter.to_string(),
  });
  decode_window_response(DecodedWindowListResponse::from(response))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn observe_windows_snapshot(
  _limit: i64,
  _app_filter: &str,
) -> AuvResult<ObservedWindowSnapshot> {
  Err("macOS native window listing is unsupported on this target".to_string())
}

pub(crate) fn decode_display_response(
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

pub(crate) fn decode_window_response(
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
pub(crate) fn bundle_ids_by_pid(pids: &HashSet<u32>) -> AuvResult<HashMap<u32, String>> {
  let mut sorted_pids = pids.iter().copied().collect::<Vec<_>>();
  sorted_pids.sort_unstable();
  let response = native_bundle_ids_by_pid(NativeBundleIdsByPidRequest {
    pids: sorted_pids.into_iter().map(i64::from).collect(),
  });
  decode_bundle_ids_by_pid_response(DecodedBundleIdsByPidResponse::from(response))
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn bundle_ids_by_pid(_pids: &HashSet<u32>) -> AuvResult<HashMap<u32, String>> {
  Err("macOS native bundle id lookup is unsupported on this target".to_string())
}

pub(crate) fn decode_bundle_ids_by_pid_response(
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

#[derive(Clone, Debug)]
pub(crate) struct DecodedDisplayListResponse {
  pub(crate) captured_at: String,
  pub(crate) ids: Vec<i64>,
  pub(crate) main_flags: Vec<bool>,
  pub(crate) built_in_flags: Vec<bool>,
  pub(crate) bounds_x_values: Vec<i64>,
  pub(crate) bounds_y_values: Vec<i64>,
  pub(crate) bounds_width_values: Vec<i64>,
  pub(crate) bounds_height_values: Vec<i64>,
  pub(crate) visible_x_values: Vec<i64>,
  pub(crate) visible_y_values: Vec<i64>,
  pub(crate) visible_width_values: Vec<i64>,
  pub(crate) visible_height_values: Vec<i64>,
  pub(crate) scale_factors: Vec<f64>,
  pub(crate) pixel_width_values: Vec<i64>,
  pub(crate) pixel_height_values: Vec<i64>,
  pub(crate) error_message: Option<String>,
  pub(crate) recovery_hint: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodedWindowListResponse {
  pub(crate) observed_at: String,
  pub(crate) frontmost_app_name: String,
  pub(crate) frontmost_app_bundle_id: String,
  pub(crate) frontmost_window_title: String,
  pub(crate) app_names: Vec<String>,
  pub(crate) owner_pids: Vec<i64>,
  pub(crate) owner_bundle_ids: Vec<String>,
  pub(crate) window_numbers: Vec<i64>,
  pub(crate) layers: Vec<i64>,
  pub(crate) titles: Vec<String>,
  pub(crate) x_values: Vec<i64>,
  pub(crate) y_values: Vec<i64>,
  pub(crate) width_values: Vec<i64>,
  pub(crate) height_values: Vec<i64>,
  pub(crate) error_message: Option<String>,
  pub(crate) recovery_hint: Option<String>,
}

#[derive(Clone, Debug)]
pub(crate) struct DecodedBundleIdsByPidResponse {
  pub(crate) pids: Vec<i64>,
  pub(crate) bundle_ids: Vec<String>,
  pub(crate) error_message: Option<String>,
  pub(crate) recovery_hint: Option<String>,
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

#[cfg(test)]
mod tests {
  use super::*;

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
}
