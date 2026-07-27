// File: src/driver/macos/native/tree.rs
#[cfg(target_os = "macos")]
use super::binding::ffi::{
  NativeAxActionRequest, NativeAxActionResponse, NativeAxFocusRequest, NativeAxFocusResponse, NativeAxNodeInspectionRequest,
  NativeAxNodeInspectionResponse, NativeAxTreeRequest, NativeAxTreeResponse, capture_ax_tree, inspect_ax_node, perform_ax_action,
  set_ax_focused,
};
use auv_driver_common::error::{DriverError, DriverResult};

use super::types::{AuvResult, ObservedAxNode, ObservedAxTreeSnapshot, ObservedRect};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeAxTreeCapture {
  pub snapshot: ObservedAxTreeSnapshot,
  pub pid: i64,
  pub root_role: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeAxAction {
  pub performed_action: String,
  pub available_actions: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeAxFocus {
  pub set_attribute: String,
  pub was_already_focused: bool,
  pub role: String,
  pub subrole: String,
  pub title: String,
  pub description: String,
  pub identifier: String,
  pub placeholder: String,
  pub bounds: ObservedRect,
}

/// Temporary native diagnostic payload for the Apple Music AX probe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AxNodeInspection {
  pub path: String,
  pub role: String,
  pub available_actions: Vec<String>,
  pub available_attributes: Vec<String>,
  pub children_count: usize,
  pub visible_children_count: usize,
  pub contents_count: usize,
  pub navigation_children_count: usize,
}

#[cfg(target_os = "macos")]
pub fn capture_ax_tree_snapshot(app: &str, max_depth: i64, max_children: i64) -> AuvResult<NativeAxTreeCapture> {
  decode_ax_tree_response(DecodedAxTreeResponse::from(capture_ax_tree(NativeAxTreeRequest {
    app: app.to_string(),
    max_depth,
    max_children,
  })))
}

#[cfg(not(target_os = "macos"))]
pub fn capture_ax_tree_snapshot(_app: &str, _max_depth: i64, _max_children: i64) -> AuvResult<NativeAxTreeCapture> {
  Err("macOS native AX tree capture is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn perform_ax_path_action(pid: i32, path: &str, expected_role: &str, action_name: &str) -> AuvResult<NativeAxAction> {
  decode_ax_action_response(DecodedAxActionResponse::from(perform_ax_action(NativeAxActionRequest {
    pid: i64::from(pid),
    path: path.to_string(),
    expected_role: expected_role.to_string(),
    action_name: action_name.to_string(),
  })))
}

#[cfg(not(target_os = "macos"))]
pub fn perform_ax_path_action(_pid: i32, _path: &str, _expected_role: &str, _action_name: &str) -> AuvResult<NativeAxAction> {
  Err("macOS native AX action dispatch is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn set_ax_focused_path(pid: i32, path: &str, expected_role: &str) -> DriverResult<NativeAxFocus> {
  decode_ax_focus_response(DecodedAxFocusResponse::from(set_ax_focused(NativeAxFocusRequest {
    pid: i64::from(pid),
    path: path.to_string(),
    expected_role: expected_role.to_string(),
  })))
}

#[cfg(not(target_os = "macos"))]
pub fn set_ax_focused_path(_pid: i32, _path: &str, _expected_role: &str) -> DriverResult<NativeAxFocus> {
  Err(DriverError::unsupported("macos.ax.set_focused_path"))
}

// NOTICE: unlike the sibling `native` AX helpers (which return `AuvResult` and
// are wrapped into `DriverError` by `accessibility.rs` before leaving the
// crate), this one is consumed directly by the Apple Music probe. It converts
// the native String failure to `DriverError` here so no `Result<_, String>`
// crosses the crate boundary. AGENTS.md: "Internal FFI decode may use strings;
// convert to DriverError ... before leaving the crate."
#[cfg(target_os = "macos")]
pub fn inspect_ax_node_path(pid: i32, path: &str, expected_role: &str) -> DriverResult<AxNodeInspection> {
  decode_ax_node_inspection_response(
    path.to_string(),
    DecodedAxNodeInspectionResponse::from(inspect_ax_node(NativeAxNodeInspectionRequest {
      pid: i64::from(pid),
      path: path.to_string(),
      expected_role: expected_role.to_string(),
    })),
  )
  .map_err(|message| DriverError::Backend { message })
}

#[cfg(not(target_os = "macos"))]
pub fn inspect_ax_node_path(_pid: i32, _path: &str, _expected_role: &str) -> DriverResult<AxNodeInspection> {
  Err(DriverError::unsupported("macos.ax.inspect_node_path"))
}

pub fn decode_ax_tree_response(response: DecodedAxTreeResponse) -> AuvResult<NativeAxTreeCapture> {
  if response.error_message.is_some() {
    return super::error::native_result("capture_ax_tree", None, response.error_message, response.recovery_hint);
  }

  let count = response.depths.len();
  let lengths = [
    response.paths.len(),
    response.roles.len(),
    response.subroles.len(),
    response.titles.len(),
    response.descriptions.len(),
    response.helps.len(),
    response.identifiers.len(),
    response.placeholders.len(),
    response.values.len(),
    response.focused_values.len(),
    response.x_values.len(),
    response.y_values.len(),
    response.width_values.len(),
    response.height_values.len(),
  ];
  if lengths.iter().any(|length| *length != count) {
    return Err("native AX tree response had mismatched AX node vector lengths".to_string());
  }

  let nodes = (0..count)
    .map(|index| {
      let depth = usize::try_from(response.depths[index])
        .map_err(|error| format!("native AX tree response had invalid depth {}: {error}", response.depths[index]))?;
      Ok(ObservedAxNode {
        depth,
        path: response.paths[index].clone(),
        role: response.roles[index].clone(),
        subrole: response.subroles[index].clone(),
        title: response.titles[index].clone(),
        description: response.descriptions[index].clone(),
        help: response.helps[index].clone(),
        identifier: response.identifiers[index].clone(),
        placeholder: response.placeholders[index].clone(),
        value: response.values[index].clone(),
        focused: response.focused_values[index],
        bounds: ObservedRect {
          x: response.x_values[index],
          y: response.y_values[index],
          width: response.width_values[index],
          height: response.height_values[index],
        },
      })
    })
    .collect::<AuvResult<Vec<_>>>()?;

  if nodes.is_empty() {
    return Err("AX tree report contained no nodes".to_string());
  }

  let pid = i32::try_from(response.pid).map_err(|error| format!("native AX tree response had invalid pid {}: {error}", response.pid))?;

  Ok(NativeAxTreeCapture {
    snapshot: ObservedAxTreeSnapshot {
      observed_at: response.observed_at,
      app_name: response.app_name,
      bundle_id: response.bundle_id,
      pid,
      window_title: response.window_title,
      nodes,
    },
    pid: response.pid,
    root_role: response.root_role,
  })
}

// TODO(ax-typed-error-fanout): this decode still flattens to `AuvResult`
// (String) via `native_result`, unlike `decode_ax_focus_response` which now
// classifies into typed `DriverError` variants (see `classify_ax_native_error`).
// `perform_ax_path_action` currently has no caller, so it is left un-migrated in
// this slice; migrate it (and `decode_ax_node_inspection_response`) the same way
// once a consumer needs classified action errors. See
// `docs/ai/references/driver/2026-07-19-error-chain-inventory.md`.
pub fn decode_ax_action_response(response: DecodedAxActionResponse) -> AuvResult<NativeAxAction> {
  if response.error_message.is_some() {
    return super::error::native_result("perform_ax_action", None, response.error_message, response.recovery_hint);
  }

  Ok(NativeAxAction {
    performed_action: response.performed_action,
    available_actions: response.available_actions,
  })
}

pub fn decode_ax_node_inspection_response(path: String, response: DecodedAxNodeInspectionResponse) -> AuvResult<AxNodeInspection> {
  if response.error_message.is_some() {
    return super::error::native_result("inspect_ax_node", None, response.error_message, response.recovery_hint);
  }

  Ok(AxNodeInspection {
    path,
    role: response.role,
    available_actions: response.available_actions,
    available_attributes: response.available_attributes,
    children_count: non_negative_count(response.children_count),
    visible_children_count: non_negative_count(response.visible_children_count),
    contents_count: non_negative_count(response.contents_count),
    navigation_children_count: non_negative_count(response.navigation_children_count),
  })
}

fn non_negative_count(value: i64) -> usize {
  usize::try_from(value).unwrap_or(0)
}

// Classifies a native AX failure message into a typed `DriverError`.
//
// This is the one place allowed to inspect the raw Swift error text: it is the
// decode boundary that converts the unstructured native response into a
// structured variant. Callers above this layer match on the variant, never on
// the message string (AGENTS.md: no `contains("stale")` control flow in the
// operation/CLI layers).
//
// NOTICE(ax-native-error-kind): the ideal is for Swift to emit a structured
// error kind over FFI so this layer maps a code, not a substring. That is
// deferred — it requires a Swift/FFI/bridge change; this Rust-side slice proves
// the classification pattern first. Unlock when a second native domain (OCR /
// window) needs the same typing and the Swift-side kind pays for itself.
//
// Pure Rust (no AX calls) so it compiles on every target — `decode_ax_focus_response`
// is not target-gated and its tests run on the CI Linux host too.
fn classify_ax_native_error(message: Option<String>, recovery: Option<String>) -> DriverError {
  let message = message.unwrap_or_else(|| "unknown native AX error".to_string());
  let lowered = message.to_lowercase();

  // Caller supplied a malformed observed path — not a live-tree problem.
  if lowered.contains("must begin with 0") || lowered.contains("is not a non-negative integer") {
    return DriverError::InvalidInput {
      message: join_recovery(message, recovery),
    };
  }
  // Accessibility permission gate.
  if lowered.contains("permission") || lowered.contains("accessibility") {
    return DriverError::PermissionDenied {
      permission: "accessibility",
      message: Some(message),
      recovery,
    };
  }
  // A node resolved but its role differs from the expected role.
  if lowered.contains("expected role") {
    return DriverError::RoleMismatch { message, recovery };
  }
  // The recorded path/tree no longer resolves against the live UI.
  if lowered.contains("is out of range") || lowered.contains("tree likely shifted") || lowered.contains("could not resolve target") {
    return DriverError::StaleObservation { message, recovery };
  }
  DriverError::Backend {
    message: join_recovery(message, recovery),
  }
}

// Folds a recovery hint into the message for variants that carry no dedicated
// recovery field (`InvalidInput` / `Backend`), so the hint is never dropped.
fn join_recovery(message: String, recovery: Option<String>) -> String {
  match recovery {
    Some(recovery) => format!("{message}; recovery: {recovery}"),
    None => message,
  }
}

pub fn decode_ax_focus_response(response: DecodedAxFocusResponse) -> DriverResult<NativeAxFocus> {
  if response.error_message.is_some() {
    return Err(classify_ax_native_error(response.error_message, response.recovery_hint));
  }

  Ok(NativeAxFocus {
    set_attribute: response.set_attribute,
    was_already_focused: response.was_already_focused,
    role: response.role,
    subrole: response.subrole,
    title: response.title,
    description: response.description,
    identifier: response.identifier,
    placeholder: response.placeholder,
    bounds: ObservedRect {
      x: response.x,
      y: response.y,
      width: response.width,
      height: response.height,
    },
  })
}

#[derive(Clone, Debug)]
pub struct DecodedAxTreeResponse {
  pub observed_at: String,
  pub app_name: String,
  pub bundle_id: String,
  pub pid: i64,
  pub window_title: String,
  pub root_role: String,
  pub depths: Vec<i64>,
  pub paths: Vec<String>,
  pub roles: Vec<String>,
  pub subroles: Vec<String>,
  pub titles: Vec<String>,
  pub descriptions: Vec<String>,
  pub helps: Vec<String>,
  pub identifiers: Vec<String>,
  pub placeholders: Vec<String>,
  pub values: Vec<String>,
  pub focused_values: Vec<bool>,
  pub x_values: Vec<i64>,
  pub y_values: Vec<i64>,
  pub width_values: Vec<i64>,
  pub height_values: Vec<i64>,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DecodedAxActionResponse {
  pub performed_action: String,
  pub available_actions: String,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DecodedAxNodeInspectionResponse {
  pub role: String,
  pub available_actions: Vec<String>,
  pub available_attributes: Vec<String>,
  pub children_count: i64,
  pub visible_children_count: i64,
  pub contents_count: i64,
  pub navigation_children_count: i64,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DecodedAxFocusResponse {
  pub set_attribute: String,
  pub was_already_focused: bool,
  pub role: String,
  pub subrole: String,
  pub title: String,
  pub description: String,
  pub identifier: String,
  pub placeholder: String,
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
  pub error_message: Option<String>,
  pub recovery_hint: Option<String>,
}

#[cfg(target_os = "macos")]
impl From<NativeAxTreeResponse> for DecodedAxTreeResponse {
  fn from(value: NativeAxTreeResponse) -> Self {
    Self {
      observed_at: value.observed_at,
      app_name: value.app_name,
      bundle_id: value.bundle_id,
      pid: value.pid,
      window_title: value.window_title,
      root_role: value.root_role,
      depths: value.depths,
      paths: value.paths,
      roles: value.roles,
      subroles: value.subroles,
      titles: value.titles,
      descriptions: value.descriptions,
      helps: value.helps,
      identifiers: value.identifiers,
      placeholders: value.placeholders,
      values: value.values,
      focused_values: value.focused_values,
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
impl From<NativeAxActionResponse> for DecodedAxActionResponse {
  fn from(value: NativeAxActionResponse) -> Self {
    Self {
      performed_action: value.performed_action,
      available_actions: value.available_actions,
      error_message: value.error_message,
      recovery_hint: value.recovery_hint,
    }
  }
}

#[cfg(target_os = "macos")]
impl From<NativeAxNodeInspectionResponse> for DecodedAxNodeInspectionResponse {
  fn from(value: NativeAxNodeInspectionResponse) -> Self {
    Self {
      role: value.role,
      available_actions: value.available_actions,
      available_attributes: value.available_attributes,
      children_count: value.children_count,
      visible_children_count: value.visible_children_count,
      contents_count: value.contents_count,
      navigation_children_count: value.navigation_children_count,
      error_message: value.error_message,
      recovery_hint: value.recovery_hint,
    }
  }
}

#[cfg(target_os = "macos")]
impl From<NativeAxFocusResponse> for DecodedAxFocusResponse {
  fn from(value: NativeAxFocusResponse) -> Self {
    Self {
      set_attribute: value.set_attribute,
      was_already_focused: value.was_already_focused,
      role: value.role,
      subrole: value.subrole,
      title: value.title,
      description: value.description,
      identifier: value.identifier,
      placeholder: value.placeholder,
      x: value.x,
      y: value.y,
      width: value.width,
      height: value.height,
      error_message: value.error_message,
      recovery_hint: value.recovery_hint,
    }
  }
}

#[cfg(test)]
#[path = "tree_test.rs"]
mod tests;
