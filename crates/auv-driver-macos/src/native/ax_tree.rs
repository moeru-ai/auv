// File: src/driver/macos/native/ax_tree.rs
#[cfg(target_os = "macos")]
use super::binding::ffi::{
  NativeAxActionRequest, NativeAxActionResponse, NativeAxFocusRequest, NativeAxFocusResponse, NativeAxNodeInspectionRequest,
  NativeAxNodeInspectionResponse, NativeAxTreeRequest, NativeAxTreeResponse, capture_ax_tree, inspect_ax_node, perform_ax_action,
  set_ax_focused,
};
use super::types::{AuvResult, AxNodeInspection, ObservedAxNode, ObservedAxTreeSnapshot, ObservedRect};

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
pub fn set_ax_focused_path(pid: i32, path: &str, expected_role: &str) -> AuvResult<NativeAxFocus> {
  decode_ax_focus_response(DecodedAxFocusResponse::from(set_ax_focused(NativeAxFocusRequest {
    pid: i64::from(pid),
    path: path.to_string(),
    expected_role: expected_role.to_string(),
  })))
}

#[cfg(not(target_os = "macos"))]
pub fn set_ax_focused_path(_pid: i32, _path: &str, _expected_role: &str) -> AuvResult<NativeAxFocus> {
  Err("macOS native AX focus dispatch is unsupported on this target".to_string())
}

#[cfg(target_os = "macos")]
pub fn inspect_ax_node_path(pid: i32, path: &str, expected_role: &str) -> AuvResult<AxNodeInspection> {
  decode_ax_node_inspection_response(
    path.to_string(),
    DecodedAxNodeInspectionResponse::from(inspect_ax_node(NativeAxNodeInspectionRequest {
      pid: i64::from(pid),
      path: path.to_string(),
      expected_role: expected_role.to_string(),
    })),
  )
}

#[cfg(not(target_os = "macos"))]
pub fn inspect_ax_node_path(_pid: i32, _path: &str, _expected_role: &str) -> AuvResult<AxNodeInspection> {
  Err("macOS native AX node inspection is unsupported on this target".to_string())
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
    subrole: response.subrole,
    title: response.title,
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

pub fn decode_ax_focus_response(response: DecodedAxFocusResponse) -> AuvResult<NativeAxFocus> {
  if response.error_message.is_some() {
    return super::error::native_result("set_ax_focused", None, response.error_message, response.recovery_hint);
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

pub fn render_ax_tree_report(capture: &NativeAxTreeCapture) -> String {
  let snapshot = &capture.snapshot;
  let mut lines = vec![
    format!("observedAt={}", snapshot.observed_at),
    format!("appName={}", snapshot.app_name),
    format!("bundleId={}", snapshot.bundle_id),
    format!("pid={}", capture.pid),
    format!("windowTitle={}", snapshot.window_title),
    format!("rootRole={}", capture.root_role),
  ];
  for node in &snapshot.nodes {
    lines.push(format!(
      "node\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
      node.depth,
      node.path,
      node.role,
      node.subrole,
      node.title,
      node.description,
      node.help,
      node.identifier,
      node.placeholder,
      node.value,
      node.focused,
      node.bounds.x,
      node.bounds.y,
      node.bounds.width,
      node.bounds.height
    ));
  }
  lines.push(format!("nodeCount={}", snapshot.nodes.len()));
  lines.join("\n") + "\n"
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
  pub subrole: String,
  pub title: String,
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
      subrole: value.subrole,
      title: value.title,
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
mod tests {
  use super::*;

  fn base_response() -> DecodedAxTreeResponse {
    DecodedAxTreeResponse {
      observed_at: "2026-05-20T00:00:00Z".to_string(),
      app_name: "Notes".to_string(),
      bundle_id: "com.apple.Notes".to_string(),
      pid: 123,
      window_title: "Todo".to_string(),
      root_role: "AXWindow".to_string(),
      depths: vec![0],
      paths: vec!["0".to_string()],
      roles: vec!["AXStaticText".to_string()],
      subroles: vec!["".to_string()],
      titles: vec!["Title".to_string()],
      descriptions: vec!["Description".to_string()],
      helps: vec!["".to_string()],
      identifiers: vec!["".to_string()],
      placeholders: vec!["".to_string()],
      values: vec!["Value".to_string()],
      focused_values: vec![false],
      x_values: vec![10],
      y_values: vec![20],
      width_values: vec![100],
      height_values: vec![30],
      error_message: None,
      recovery_hint: None,
    }
  }

  #[test]
  fn decode_ax_tree_rejects_mismatched_node_vectors() {
    let mut response = base_response();
    response.paths.clear();

    let error = decode_ax_tree_response(response).unwrap_err();

    assert!(error.contains("mismatched AX node vector lengths"));
  }

  #[test]
  fn decode_ax_tree_preserves_text_priority_fields() {
    let capture = decode_ax_tree_response(base_response()).unwrap();

    assert_eq!(capture.snapshot.nodes[0].value, "Value");
    assert_eq!(capture.snapshot.nodes[0].title, "Title");
    assert_eq!(capture.snapshot.pid, 123);
  }

  #[test]
  fn decode_ax_action_rejects_native_error() {
    let error = decode_ax_action_response(DecodedAxActionResponse {
      performed_action: "".to_string(),
      available_actions: "".to_string(),
      error_message: Some("missing action".to_string()),
      recovery_hint: Some("try another node".to_string()),
    })
    .unwrap_err();

    assert!(error.contains("perform_ax_action failed"));
    assert!(error.contains("missing action"));
  }

  fn base_focus_response() -> DecodedAxFocusResponse {
    DecodedAxFocusResponse {
      set_attribute: "AXFocused".to_string(),
      was_already_focused: false,
      role: "AXTextArea".to_string(),
      subrole: "".to_string(),
      title: "".to_string(),
      description: "Note Body Text View".to_string(),
      identifier: "".to_string(),
      placeholder: "".to_string(),
      x: 10,
      y: 20,
      width: 300,
      height: 200,
      error_message: None,
      recovery_hint: None,
    }
  }

  #[test]
  fn decode_ax_focus_passes_through_successful_set() {
    let focus = decode_ax_focus_response(base_focus_response()).unwrap();

    assert_eq!(focus.set_attribute, "AXFocused");
    assert!(!focus.was_already_focused);
    assert_eq!(focus.role, "AXTextArea");
    assert_eq!(focus.bounds.width, 300);
  }

  #[test]
  fn decode_ax_focus_preserves_already_focused_signal() {
    let mut response = base_focus_response();
    response.was_already_focused = true;
    let focus = decode_ax_focus_response(response).unwrap();

    assert!(focus.was_already_focused);
    assert_eq!(focus.set_attribute, "AXFocused");
  }

  fn base_inspection_response() -> DecodedAxNodeInspectionResponse {
    DecodedAxNodeInspectionResponse {
      role: "AXToolbar".to_string(),
      subrole: "".to_string(),
      title: "".to_string(),
      available_actions: vec![],
      available_attributes: vec!["AXRole".to_string(), "AXChildren".to_string()],
      children_count: 0,
      visible_children_count: 2,
      contents_count: 0,
      navigation_children_count: 0,
      error_message: None,
      recovery_hint: None,
    }
  }

  #[test]
  fn decode_ax_node_inspection_reports_attribute_specific_child_counts() {
    let inspection = decode_ax_node_inspection_response("0.1".to_string(), base_inspection_response()).unwrap();

    assert_eq!(inspection.path, "0.1");
    assert_eq!(inspection.children_count, 0);
    assert_eq!(inspection.visible_children_count, 2);
  }

  #[test]
  fn decode_ax_node_inspection_rejects_native_error() {
    let mut response = base_inspection_response();
    response.error_message = Some("AXUIElementCopyAttributeNames returned -25200".to_string());
    response.recovery_hint = Some("verify the AX path still resolves".to_string());

    let error = decode_ax_node_inspection_response("0.1".to_string(), response).unwrap_err();

    assert!(error.contains("inspect_ax_node failed"));
    assert!(error.contains("AXUIElementCopyAttributeNames returned -25200"));
  }

  #[test]
  fn decode_ax_focus_rejects_native_error() {
    let mut response = base_focus_response();
    response.error_message = Some("AXUIElementSetAttributeValue returned -25204".to_string());
    response.recovery_hint = Some("element may not accept programmatic focus".to_string());

    let error = decode_ax_focus_response(response).unwrap_err();

    assert!(error.contains("set_ax_focused failed"));
    assert!(error.contains("AXUIElementSetAttributeValue returned -25204"));
  }
}
