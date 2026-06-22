//! Window accessibility tree snapshots via Microsoft UI Automation (UIA).
//!
//! Mirrors the spirit of the macOS driver's AX tree capture: it walks a single
//! window's accessibility hierarchy and flattens it into an ordered list of
//! nodes carrying role, name, identifier, and screen bounds. macOS reads the
//! `AXUIElement` tree; Windows reads the UIA tree through the COM
//! `IUIAutomation` interface and its control-view tree walker.
//!
//! The snapshot is read-only structure: it captures what the tree looks like,
//! not how to act on it. Acting on a node (invoke/focus/value) is a separate
//! concern delivered through the input and control surfaces.
// TODO(windows-ax-value-write): UIA ValuePattern writes remain deferred until
// an owner-approved consumer needs them. Read-only ValuePattern text,
// path-targeted SetFocus, and result-item selection are exposed for Apple Music.

use auv_driver::error::DriverResult;
use auv_driver::geometry::Rect;
use auv_driver::input::{DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath};

use crate::error::invalid_input;

// NOTICE: traversal bounds keep a pathological or very deep UI tree from
// producing an unbounded snapshot (and guard the recursive walk against deep
// stacks). They are independent limits: depth caps how far down we descend,
// node count caps total breadth-times-depth output.
const MAX_DEPTH: usize = 40;
const MAX_NODES: usize = 2_000;

/// One node in a flattened accessibility tree snapshot.
///
/// `path` is a `/`-joined chain of child indices from the root (e.g. `0/2/1`),
/// so a node's position in the original tree is recoverable from the flat list.
#[derive(Clone, Debug, PartialEq)]
pub struct AxNode {
  pub depth: usize,
  pub path: String,
  pub control_type: String,
  pub name: String,
  /// Current text exposed through UIA ValuePattern, when supported.
  pub value: Option<String>,
  pub automation_id: String,
  pub class_name: String,
  pub focused: bool,
  pub bounds: Rect,
}

/// A flattened, depth-first accessibility tree snapshot for one window.
#[derive(Clone, Debug, PartialEq)]
pub struct AxTreeSnapshot {
  pub window_ref: String,
  pub nodes: Vec<AxNode>,
}

/// Captures the accessibility tree for `window` via UI Automation.
pub fn snapshot_window(
  window: &auv_driver::window::Window,
) -> auv_driver::error::DriverResult<AxTreeSnapshot> {
  native::snapshot_window(window)
}

/// Moves keyboard focus to a node from a fresh [`AxTreeSnapshot`].
///
/// The node path is resolved again against the current UIA control-view tree,
/// then `IUIAutomationElement::SetFocus` performs the action. Callers should
/// refresh the snapshot and retry when the tree changes between observation
/// and action.
pub fn focus_node(
  window: &auv_driver::window::Window,
  node_path: &str,
) -> DriverResult<InputActionResult> {
  let indices = child_indices(node_path)?;
  native::focus_node(window, &indices)?;
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::AxFocus,
    attempts: vec![InputAttempt::success(InputDeliveryPath::AxFocus)],
    fallback_reason: None,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Foreground,
    clipboard_disturbance: DisturbanceLevel::None,
  })
}

/// Selects or invokes a node from a recent [`AxTreeSnapshot`].
///
/// `SelectionItemPattern::Select` is preferred because result containers such
/// as WinUI `GridViewItem` expose semantic selection. `InvokePattern::Invoke`
/// is the typed fallback for actionable nodes that do not expose selection.
pub fn select_node(
  window: &auv_driver::window::Window,
  node_path: &str,
) -> DriverResult<InputActionResult> {
  let indices = child_indices(node_path)?;
  let selected_pattern = native::select_node(window, &indices)?;
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::AxPress,
    attempts: vec![InputAttempt {
      path: InputDeliveryPath::AxPress,
      succeeded: true,
      message: Some(selected_pattern.to_string()),
    }],
    fallback_reason: (selected_pattern == "InvokePattern.Invoke")
      .then_some("SelectionItemPattern was unavailable".to_string()),
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Foreground,
    clipboard_disturbance: DisturbanceLevel::None,
  })
}

fn child_indices(path: &str) -> DriverResult<Vec<usize>> {
  let mut parts = path.split('/');
  if parts.next() != Some("0") {
    return Err(invalid_input(format!(
      "UIA node path {path:?} must start at root 0"
    )));
  }
  parts
    .map(|part| {
      part.parse::<usize>().map_err(|_| {
        invalid_input(format!(
          "UIA node path {path:?} contains invalid child index {part:?}"
        ))
      })
    })
    .collect()
}

/// Builds a screen-space rectangle from UIA bounding-rectangle edges.
///
/// UIA reports bounds as inclusive `left/top` and exclusive `right/bottom`
/// edges in physical screen pixels; this converts them to an origin/size
/// rectangle in the same screen space as window frames.
fn rect_from_edges(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
  Rect::new(
    f64::from(left),
    f64::from(top),
    f64::from(right - left),
    f64::from(bottom - top),
  )
}

#[cfg(target_os = "windows")]
mod native {
  use auv_driver::error::DriverResult;
  use auv_driver::geometry::Rect;
  use auv_driver::window::Window;
  use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
  };
  use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern,
    IUIAutomationSelectionItemPattern, IUIAutomationTreeWalker, IUIAutomationValuePattern,
    UIA_InvokePatternId, UIA_SelectionItemPatternId, UIA_ValuePatternId,
  };
  use windows::core::{BSTR, Result as WindowsResult};

  use super::{AxNode, AxTreeSnapshot, MAX_DEPTH, MAX_NODES, rect_from_edges};
  use crate::error::backend;
  use crate::window::window_handle;

  /// Balances a successful `CoInitializeEx` with `CoUninitialize` on the same
  /// thread. When COM was already initialized in a different apartment model
  /// (`RPC_E_CHANGED_MODE`), `uninit` stays false so we do not tear down an
  /// initialization we did not perform.
  struct ComGuard {
    uninit: bool,
  }

  impl Drop for ComGuard {
    fn drop(&mut self) {
      if self.uninit {
        unsafe { CoUninitialize() };
      }
    }
  }

  fn init_com() -> ComGuard {
    // UIA is happiest in an MTA; if the thread is already STA this returns
    // RPC_E_CHANGED_MODE and we proceed against the existing apartment.
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    ComGuard { uninit: hr.is_ok() }
  }

  pub(super) fn snapshot_window(window: &Window) -> DriverResult<AxTreeSnapshot> {
    let hwnd = window_handle(window)?;
    let _com = init_com();

    let automation: IUIAutomation =
      unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
        .map_err(|error| backend(format!("failed to create UI Automation client: {error}")))?;
    let root = unsafe { automation.ElementFromHandle(hwnd) }
      .map_err(|error| backend(format!("failed to resolve window UI element: {error}")))?;
    let walker = unsafe { automation.ControlViewWalker() }.map_err(|error| {
      backend(format!(
        "failed to get UI Automation control walker: {error}"
      ))
    })?;

    let mut nodes = Vec::new();
    walk(&walker, &root, 0, "0".to_string(), &mut nodes);
    Ok(AxTreeSnapshot {
      window_ref: window.reference.id.clone(),
      nodes,
    })
  }

  pub(super) fn focus_node(window: &Window, child_indices: &[usize]) -> DriverResult<()> {
    let (_com, element) = resolve_element(window, child_indices)?;
    unsafe { element.SetFocus() }
      .map_err(|error| backend(format!("failed to focus UIA node: {error}")))
  }

  pub(super) fn select_node(
    window: &Window,
    child_indices: &[usize],
  ) -> DriverResult<&'static str> {
    let (_com, element) = resolve_element(window, child_indices)?;
    if let Ok(pattern) = unsafe {
      element.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId)
    } {
      unsafe { pattern.Select() }
        .map_err(|error| backend(format!("failed to select UIA node: {error}")))?;
      return Ok("SelectionItemPattern.Select");
    }
    let pattern =
      unsafe { element.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
        .map_err(|error| {
          backend(format!(
            "UIA node supports neither SelectionItemPattern nor InvokePattern: {error}"
          ))
        })?;
    unsafe { pattern.Invoke() }
      .map_err(|error| backend(format!("failed to invoke UIA node: {error}")))?;
    Ok("InvokePattern.Invoke")
  }

  fn resolve_element(
    window: &Window,
    child_indices: &[usize],
  ) -> DriverResult<(ComGuard, IUIAutomationElement)> {
    let hwnd = window_handle(window)?;
    let com = init_com();
    let automation: IUIAutomation =
      unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
        .map_err(|error| backend(format!("failed to create UI Automation client: {error}")))?;
    let mut element = unsafe { automation.ElementFromHandle(hwnd) }
      .map_err(|error| backend(format!("failed to resolve window UI element: {error}")))?;
    let walker = unsafe { automation.ControlViewWalker() }.map_err(|error| {
      backend(format!(
        "failed to get UI Automation control walker: {error}"
      ))
    })?;
    for index in child_indices {
      let mut child = unsafe { walker.GetFirstChildElement(&element) }
        .map_err(|error| backend(format!("failed to resolve UIA child 0: {error}")))?;
      for sibling_index in 0..*index {
        child = unsafe { walker.GetNextSiblingElement(&child) }.map_err(|error| {
          backend(format!(
            "failed to resolve UIA child {}: {error}",
            sibling_index + 1
          ))
        })?;
      }
      element = child;
    }
    Ok((com, element))
  }

  /// Depth-first traversal that appends each visited element, then descends
  /// through its control-view children. Traversal stops widening once the node
  /// budget is exhausted and stops descending past the depth limit.
  fn walk(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    depth: usize,
    path: String,
    nodes: &mut Vec<AxNode>,
  ) {
    if nodes.len() >= MAX_NODES {
      return;
    }
    nodes.push(node_from_element(element, depth, path.clone()));
    if depth >= MAX_DEPTH {
      return;
    }

    let mut next = unsafe { walker.GetFirstChildElement(element) }.ok();
    let mut index = 0usize;
    while let Some(child) = next {
      if nodes.len() >= MAX_NODES {
        break;
      }
      walk(walker, &child, depth + 1, format!("{path}/{index}"), nodes);
      next = unsafe { walker.GetNextSiblingElement(&child) }.ok();
      index += 1;
    }
  }

  fn node_from_element(element: &IUIAutomationElement, depth: usize, path: String) -> AxNode {
    AxNode {
      depth,
      path,
      control_type: bstr_or_default(unsafe { element.CurrentLocalizedControlType() }),
      name: bstr_or_default(unsafe { element.CurrentName() }),
      value: value_or_none(element),
      automation_id: bstr_or_default(unsafe { element.CurrentAutomationId() }),
      class_name: bstr_or_default(unsafe { element.CurrentClassName() }),
      focused: unsafe { element.CurrentHasKeyboardFocus() }
        .map(|value| value.as_bool())
        .unwrap_or(false),
      bounds: bounds_or_default(element),
    }
  }

  fn bstr_or_default(result: WindowsResult<BSTR>) -> String {
    result.map(|value| value.to_string()).unwrap_or_default()
  }

  fn value_or_none(element: &IUIAutomationElement) -> Option<String> {
    let pattern =
      unsafe { element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) }
        .ok()?;
    let value = unsafe { pattern.CurrentValue() }.ok()?.to_string();
    (!value.is_empty()).then_some(value)
  }

  fn bounds_or_default(element: &IUIAutomationElement) -> Rect {
    match unsafe { element.CurrentBoundingRectangle() } {
      Ok(rect) => rect_from_edges(rect.left, rect.top, rect.right, rect.bottom),
      Err(_) => Rect::default(),
    }
  }
}

#[cfg(not(target_os = "windows"))]
mod native {
  use auv_driver::error::{DriverError, DriverResult};
  use auv_driver::window::Window;

  use super::AxTreeSnapshot;

  pub(super) fn snapshot_window(_window: &Window) -> DriverResult<AxTreeSnapshot> {
    Err(DriverError::unsupported("accessibility.snapshot_window"))
  }

  pub(super) fn focus_node(_window: &Window, _child_indices: &[usize]) -> DriverResult<()> {
    Err(DriverError::unsupported("accessibility.focus_node"))
  }

  pub(super) fn select_node(
    _window: &Window,
    _child_indices: &[usize],
  ) -> DriverResult<&'static str> {
    Err(DriverError::unsupported("accessibility.select_node"))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rect_from_edges_converts_inclusive_exclusive_edges_to_origin_size() {
    let rect = rect_from_edges(100, 200, 340, 500);

    assert_eq!(rect, Rect::new(100.0, 200.0, 240.0, 300.0));
  }

  #[test]
  fn rect_from_edges_handles_zero_area() {
    let rect = rect_from_edges(10, 20, 10, 20);

    assert_eq!(rect, Rect::new(10.0, 20.0, 0.0, 0.0));
  }

  #[test]
  fn child_indices_parse_snapshot_path() {
    assert_eq!(child_indices("0/2/0/0/1/3/0").unwrap(), [2, 0, 0, 1, 3, 0]);
  }

  #[test]
  fn child_indices_reject_invalid_root_and_child() {
    assert!(child_indices("1/2").is_err());
    assert!(child_indices("0/search").is_err());
  }

  // Live smoke test: snapshot the first enumerated top-level window and prove
  // the UIA COM walk produces at least the root node with the expected root
  // path. Skips cleanly when no windows are present (headless session).
  #[cfg(target_os = "windows")]
  #[test]
  fn snapshot_window_captures_root_node_for_a_live_window() {
    let windows = crate::window::list_windows().expect("list windows");
    let Some(window) = windows.into_iter().next() else {
      return;
    };

    let snapshot = snapshot_window(&window).expect("snapshot window ax tree");

    assert_eq!(snapshot.window_ref, window.reference.id);
    assert!(!snapshot.nodes.is_empty());
    assert_eq!(snapshot.nodes[0].depth, 0);
    assert_eq!(snapshot.nodes[0].path, "0");
  }
}
