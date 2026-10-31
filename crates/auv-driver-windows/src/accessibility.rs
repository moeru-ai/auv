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

use auv_driver_common::accessibility::{AxFocusResult, AxTextSelector, FocusTextOptions};
use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::Rect;
use auv_driver_common::input::{DisturbanceLevel, InputActionResult, InputAttempt, InputDeliveryPath};
use auv_driver_common::window::Window;

use crate::error::{invalid_input, not_found};

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
  /// Provider-localized control type, retained for existing tree readers.
  pub control_type: String,
  /// Programmatic UIA control type, such as `Edit`, independent of locale.
  pub role: String,
  pub name: String,
  /// Current text exposed through UIA ValuePattern, when supported.
  pub value: Option<String>,
  pub automation_id: String,
  pub class_name: String,
  pub focused: bool,
  pub enabled: bool,
  pub keyboard_focusable: bool,
  pub offscreen: bool,
  pub bounds: Rect,
}

/// A flattened, depth-first accessibility tree snapshot for one window.
#[derive(Clone, Debug, PartialEq)]
pub struct AxTreeSnapshot {
  pub window_ref: String,
  pub nodes: Vec<AxNode>,
  /// A traversal limit or provider error prevented a complete control tree.
  pub truncated: bool,
}

/// Captures the accessibility tree for `window` via UI Automation.
pub fn snapshot_window(window: &Window) -> DriverResult<AxTreeSnapshot> {
  native::snapshot_window(window)
}

/// Resolves a text query, rechecks its live identity, and reads back focus.
/// This operation never falls back to pointer input or text writes.
pub fn focus_text(options: FocusTextOptions) -> DriverResult<AxFocusResult> {
  options.validate()?;
  // TODO(windows-focus-reference): raw child paths cannot identify the same
  // control across calls. Enable path delivery only with a typed identity
  // receipt; see 2026-10-28-windows-core-convergence.md.
  if matches!(options.selector, AxTextSelector::Path(_)) {
    return Err(auv_driver_common::DriverError::unsupported("accessibility.focus_text with an unbound UIA path"));
  }
  let window = crate::window::resolve_window(
    &auv_driver_common::selector::Window::main_visible().owned_by(auv_driver_common::selector::App::id(options.app.trim())),
  )?;
  native::focus_text(&window, &options)
}

/// Moves keyboard focus to a node from a fresh [`AxTreeSnapshot`].
///
/// The node path is resolved again against the current UIA control-view tree,
/// then `IUIAutomationElement::SetFocus` performs the action. Callers should
/// refresh the snapshot and retry when the tree changes between observation
/// and action.
pub fn focus_node(window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
  let indices = child_indices(node_path)?;
  native::focus_node(window, &indices)?;
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::AxFocus,
    attempts: vec![InputAttempt::success(InputDeliveryPath::AxFocus)],
    verified: false,
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
pub fn select_node(window: &Window, node_path: &str) -> DriverResult<InputActionResult> {
  let indices = child_indices(node_path)?;
  let selected_pattern = native::select_node(window, &indices)?;
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::AxPress,
    attempts: if selected_pattern == "InvokePattern.Invoke" {
      vec![
        InputAttempt::failure(InputDeliveryPath::AxPress, "SelectionItemPattern was unavailable"),
        InputAttempt {
          path: InputDeliveryPath::AxPress,
          succeeded: true,
          message: Some(selected_pattern.to_string()),
        },
      ]
    } else {
      vec![InputAttempt {
        path: InputDeliveryPath::AxPress,
        succeeded: true,
        message: Some(selected_pattern.to_string()),
      }]
    },
    verified: false,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Foreground,
    clipboard_disturbance: DisturbanceLevel::None,
  })
}

fn child_indices(path: &str) -> DriverResult<Vec<usize>> {
  let mut parts = path.split('/');
  if parts.next() != Some("0") {
    return Err(invalid_input(format!("UIA node path {path:?} must start at root 0")));
  }
  parts
    .map(|part| part.parse::<usize>().map_err(|_| invalid_input(format!("UIA node path {path:?} contains invalid child index {part:?}"))))
    .collect()
}

/// Scores one node against a lowercased query, mirroring
/// `score_ax_node_match` on macOS. The visible name is the strongest text
/// carrier, the automation id identifies a control a caller saw before, and
/// the ValuePattern value is current text; `class_name` is deliberately
/// unmatched because it is a toolkit label, not visible text.
fn score_uia_node_match(node: &AxNode, query: &str) -> Option<i64> {
  if query.is_empty() {
    return None;
  }

  let mut score = 0i64;
  for (weight, raw) in [(80, node.name.as_str()), (40, node.automation_id.as_str())] {
    let value = raw.trim().to_lowercase();
    if value.is_empty() || !value.contains(query) {
      continue;
    }
    score += weight;
    if value == query {
      score += 20;
    }
  }
  if let Some(raw) = &node.value {
    let value = raw.trim().to_lowercase();
    if !value.is_empty() && value.contains(query) {
      score += 24;
      if value == query {
        score += 20;
      }
    }
  }

  if score == 0 {
    return None;
  }

  if node.role.eq_ignore_ascii_case("Edit") || node.role.eq_ignore_ascii_case("Document") {
    score += 24;
  }
  if node.role.eq_ignore_ascii_case("Button") || node.role.eq_ignore_ascii_case("Hyperlink") {
    score += 18;
  }

  Some(score - node.depth as i64)
}

fn usable_focus_target(node: &AxNode) -> bool {
  // NOTICE: focus does not write a value, so it does not require editability.
  // Add IsReadOnly only with the owner-approved ValuePattern write capability.
  node.enabled
    && node.keyboard_focusable
    && !node.offscreen
    && [
      node.bounds.origin.x,
      node.bounds.origin.y,
      node.bounds.size.width,
      node.bounds.size.height,
    ]
    .into_iter()
    .all(f64::is_finite)
    && node.bounds.size.width > 0.0
    && node.bounds.size.height > 0.0
}

/// Selects one usable node without input. Equal best scores refuse selection.
/// Path selection describes this snapshot only; it is not a durable identity.
/// Read-only text is allowed because focusing does not write a value.
pub fn resolve_focus_target<'a>(snapshot: &'a AxTreeSnapshot, options: &FocusTextOptions) -> DriverResult<&'a AxNode> {
  options.validate()?;
  if snapshot.truncated {
    return Err(auv_driver_common::DriverError::StaleUiReference {
      message: "an incomplete UIA tree cannot prove a unique focus target".into(),
      recovery: Some("capture a complete tree before selecting the target".into()),
    });
  }
  let role_matches = |node: &&AxNode| options.expected_role.as_deref().is_none_or(|role| node.role.eq_ignore_ascii_case(role.trim()));
  match &options.selector {
    AxTextSelector::Query(query) => {
      let query = query.trim();
      if query.is_empty() {
        return Err(invalid_input("resolving a focus target requires a non-empty query"));
      }
      let query = query.to_lowercase();
      let mut ranked = snapshot
        .nodes
        .iter()
        .filter(|node| usable_focus_target(node))
        .filter(role_matches)
        .filter_map(|node| score_uia_node_match(node, &query).map(|score| (score, node)))
        .collect::<Vec<_>>();
      ranked.sort_by_key(|(score, _)| std::cmp::Reverse(*score));
      let (score, node) = ranked.first().ok_or_else(|| not_found(format!("UIA text node matching query {query}")))?;
      if ranked.get(1).is_some_and(|next| next.0 == *score) {
        return Err(invalid_input("UIA focus query is ambiguous; use a more specific query or expected_role"));
      }
      Ok(node)
    }
    AxTextSelector::Path(path) => {
      let path = path.trim();
      if path.is_empty() {
        return Err(invalid_input("resolving a focus target requires a non-empty candidate path"));
      }
      child_indices(path)?;
      let node = snapshot.nodes.iter().find(|node| node.path == path).ok_or_else(|| not_found(format!("UIA candidate path {path}")))?;
      if !role_matches(&node) {
        return Err(auv_driver_common::DriverError::RoleMismatch {
          message: format!("UIA candidate {path} has role {}, expected {}", node.role, options.expected_role.as_deref().unwrap_or_default()),
          recovery: Some("read a new UIA tree and select the target again".into()),
        });
      }
      if !usable_focus_target(node) {
        return Err(not_found(format!("usable UIA candidate path {path}")));
      }
      Ok(node)
    }
  }
}

/// Keeps admission, delivery, and focus readback ordered at the UIA boundary.
/// The reader also checks native element identity; matching text alone cannot
/// distinguish a replacement control at the same child path.
#[cfg(any(target_os = "windows", test))]
fn focus_selected(
  selected: &AxNode,
  mut read_current: impl FnMut() -> DriverResult<AxNode>,
  focus: impl FnOnce() -> DriverResult<()>,
) -> DriverResult<InputActionResult> {
  let current = read_current()?;
  let mut expected = selected.clone();
  expected.focused = current.focused;
  if current != expected || !usable_focus_target(&current) {
    return Err(auv_driver_common::DriverError::StaleUiReference {
      message: "UIA focus target changed before delivery".into(),
      recovery: Some("read a new UIA tree and select the target again".into()),
    });
  }
  focus()?;
  // TODO(windows-focus-progress): Phase 2 owns structured post-delivery
  // failure metadata. Keep existing error categories here and never retry;
  // see `2026-10-28-windows-core-convergence.md`.
  // The native read closure has already compared the retained control's
  // identity with the live element and (when focused) GetFocusedElement.
  // Focus can legitimately change bounds, value, name, or other UIA facts.
  // Requiring the whole pre-action snapshot to remain equal is not evidence
  // of focus and can turn a successful SetFocus into a false failure.
  let current = read_current()?;
  if !current.focused {
    return Err(crate::error::backend(
      "UIA SetFocus returned, but focus readback did not confirm the selected control; inspect state before retrying",
    ));
  }
  Ok(InputActionResult {
    selected_path: InputDeliveryPath::AxFocus,
    attempts: vec![InputAttempt::success(InputDeliveryPath::AxFocus)],
    verified: true,
    mouse_disturbance: DisturbanceLevel::None,
    focus_disturbance: DisturbanceLevel::Foreground,
    clipboard_disturbance: DisturbanceLevel::None,
  })
}

/// Builds a screen-space rectangle from UIA bounding-rectangle edges.
///
/// UIA reports bounds as inclusive `left/top` and exclusive `right/bottom`
/// edges in physical screen pixels; this converts them to an origin/size
/// rectangle in the same screen space as window frames.
fn rect_from_edges(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
  Rect::new(f64::from(left), f64::from(top), f64::from(right - left), f64::from(bottom - top))
}

#[cfg(target_os = "windows")]
mod native {
  use auv_driver_common::error::DriverResult;
  use auv_driver_common::geometry::Rect;
  use auv_driver_common::window::Window;
  use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize};
  use windows::Win32::UI::Accessibility as uia;
  use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationInvokePattern, IUIAutomationSelectionItemPattern,
    IUIAutomationTreeWalker, IUIAutomationValuePattern, UIA_InvokePatternId, UIA_SelectionItemPatternId, UIA_ValuePatternId,
  };
  use windows::core::{BSTR, Interface, Result as WindowsResult};

  use super::{
    AxFocusResult, AxNode, AxTextSelector, AxTreeSnapshot, FocusTextOptions, MAX_DEPTH, MAX_NODES, child_indices, focus_selected,
    rect_from_edges, resolve_focus_target,
  };
  use crate::error::backend;
  use crate::window::window_handle;

  /// Bounded tree facts and the provider interfaces that produced them.
  /// Retained interfaces let focus admission distinguish replacement nodes.
  #[derive(Default)]
  struct TreeCapture {
    nodes: Vec<AxNode>,
    elements: Vec<IUIAutomationElement>,
    truncated: bool,
    failure: Option<auv_driver_common::DriverError>,
  }

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

    let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
      .map_err(|error| backend(format!("failed to create UI Automation client: {error}")))?;
    let root =
      unsafe { automation.ElementFromHandle(hwnd) }.map_err(|error| backend(format!("failed to resolve window UI element: {error}")))?;
    let walker =
      unsafe { automation.ControlViewWalker() }.map_err(|error| backend(format!("failed to get UI Automation control walker: {error}")))?;

    let mut capture = TreeCapture::default();
    walk(&walker, &root, 0, "0".to_string(), &mut capture);
    Ok(AxTreeSnapshot {
      window_ref: window.reference.id.clone(),
      nodes: capture.nodes,
      truncated: capture.truncated,
    })
  }

  pub(super) fn focus_text(window: &Window, options: &FocusTextOptions) -> DriverResult<AxFocusResult> {
    // TODO(windows-uia-cancellation): synchronous provider calls cannot be
    // interrupted here. Add bounded provider execution in the Phase 2
    // input-lifetime slice; callers must not retry an uncertain delivery.
    // Serialize with this process's other AUV input. This does not lock out the user
    // or make the final identity check and SetFocus atomic.
    let hwnd = window_handle(window)?;
    let initial = crate::window::probe_window(hwnd.0 as isize);
    let check_window = || {
      let current = crate::window::probe_window(hwnd.0 as isize);
      // SAFETY: a read-only query of the opaque HWND; Windows retains it.
      let visible = unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd) }.as_bool();
      if !current.exists
        || !visible
        || Some(current.process_id) != window.process_id
        || current.thread_id != initial.thread_id
        || current.class_name != initial.class_name
        || current.minimized
        || current.cloaked
      {
        return Err(auv_driver_common::DriverError::StaleUiReference {
          message: "UIA focus window disappeared, changed identity, or became unavailable".into(),
          recovery: Some("resolve the application window again".into()),
        });
      }
      Ok(())
    };
    check_window()?;
    let pid = i32::try_from(initial.process_id).map_err(|_| backend("UIA window process ID exceeds the focus result range"))?;
    let _com = init_com();
    // SAFETY: all UIA interfaces remain on this COM-initialized thread and
    // are released before the apartment guard. Windows owns the HWND.
    let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
      .map_err(|error| focus_error("create UI Automation client", error))?;
    let root = unsafe { automation.ElementFromHandle(hwnd) }.map_err(|error| focus_error("resolve focus window", error))?;
    let walker = unsafe { automation.ControlViewWalker() }.map_err(|error| focus_error("get control walker", error))?;
    let mut capture = TreeCapture::default();
    walk(&walker, &root, 0, "0".into(), &mut capture);
    if let Some(error) = capture.failure.take() {
      return Err(error);
    }
    let snapshot = AxTreeSnapshot {
      window_ref: window.reference.id.clone(),
      nodes: capture.nodes,
      truncated: capture.truncated,
    };
    let selected = resolve_focus_target(&snapshot, options)?;
    let original = snapshot
      .nodes
      .iter()
      .zip(&capture.elements)
      .find(|(node, _)| node.path == selected.path)
      .map(|(_, element)| element)
      .ok_or_else(|| backend("UIA snapshot lost the selected native element"))?;
    let indices = child_indices(&selected.path)?;
    // The read-only scan must not reserve desktop input. The admission
    // covers the final live identity check, SetFocus, and focus readback.
    let _input = auv_driver_common::mouse_input::reserve_desktop_input()?;
    let action = focus_selected(
      selected,
      || {
        check_window()?;
        // SAFETY: both retained interfaces and the newly resolved interfaces
        // belong to this apartment. CompareElements checks provider identity.
        let current_root = unsafe { automation.ElementFromHandle(hwnd) }.map_err(|error| focus_error("recheck focus window", error))?;
        let current = element_at_path(&walker, current_root.clone(), &indices)?;
        let same_root =
          unsafe { automation.CompareElements(&root, &current_root) }.map_err(|error| focus_error("compare window identity", error))?;
        let same_node =
          unsafe { automation.CompareElements(original, &current) }.map_err(|error| focus_error("compare control identity", error))?;
        if !same_root.as_bool() || !same_node.as_bool() {
          return Err(auv_driver_common::DriverError::StaleUiReference {
            message: "UIA focus path now identifies a different control".into(),
            recovery: Some("read a new UIA tree and select the target again".into()),
          });
        }
        let (mut node, failure) = node_from_element(&current, selected.depth, selected.path.clone());
        if let Some(error) = failure {
          return Err(error);
        }
        if node.focused {
          // SAFETY: GetFocusedElement returns an owned COM interface in the
          // same apartment. Confirmation checks the actual focused element.
          let focused = unsafe { automation.GetFocusedElement() }.map_err(|error| focus_error("read focused control", error))?;
          node.focused = unsafe { automation.CompareElements(original, &focused) }
            .map_err(|error| focus_error("confirm focused control", error))?
            .as_bool();
        }
        Ok(node)
      },
      || {
        // SAFETY: the retained element belongs to this apartment, and the
        // immediately preceding read checked its window and provider identity.
        unsafe { original.SetFocus() }.map_err(|error| focus_error("set control focus", error))
      },
    )?;
    let query = match &options.selector {
      AxTextSelector::Query(query) => query.trim(),
      AxTextSelector::Path(_) => "",
    };
    Ok(AxFocusResult {
      app: options.app.trim().into(),
      pid,
      path: selected.path.clone(),
      role: selected.role.clone(),
      title: selected.name.clone(),
      value: selected.value.clone().unwrap_or_default(),
      query: query.into(),
      input_action_result: action,
    })
  }

  fn focus_error(operation: &str, error: windows::core::Error) -> auv_driver_common::DriverError {
    use auv_driver_common::DriverError;
    if error.code() == windows::Win32::Foundation::E_ACCESSDENIED {
      return DriverError::PermissionDenied {
        permission: "UI Automation",
        message: Some(format!("{operation}: {error}")),
        recovery: None,
      };
    }
    if error.code().0 as u32 == uia::UIA_E_ELEMENTNOTAVAILABLE {
      return DriverError::StaleUiReference {
        message: format!("{operation}: {error}"),
        recovery: Some("read a new UIA tree".into()),
      };
    }
    if error.code().0 as u32 == uia::UIA_E_NOTSUPPORTED {
      return DriverError::unsupported("accessibility.focus_text");
    }
    backend(format!("{operation}: {error}"))
  }

  pub(super) fn focus_node(window: &Window, child_indices: &[usize]) -> DriverResult<()> {
    let (_com, element) = resolve_element(window, child_indices)?;
    unsafe { element.SetFocus() }.map_err(|error| backend(format!("failed to focus UIA node: {error}")))
  }

  pub(super) fn select_node(window: &Window, child_indices: &[usize]) -> DriverResult<&'static str> {
    let (_com, element) = resolve_element(window, child_indices)?;
    if let Ok(pattern) = unsafe { element.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) } {
      unsafe { pattern.Select() }.map_err(|error| backend(format!("failed to select UIA node: {error}")))?;
      return Ok("SelectionItemPattern.Select");
    }
    let pattern = unsafe { element.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) }
      .map_err(|error| backend(format!("UIA node supports neither SelectionItemPattern nor InvokePattern: {error}")))?;
    unsafe { pattern.Invoke() }.map_err(|error| backend(format!("failed to invoke UIA node: {error}")))?;
    Ok("InvokePattern.Invoke")
  }

  fn resolve_element(window: &Window, child_indices: &[usize]) -> DriverResult<(ComGuard, IUIAutomationElement)> {
    let hwnd = window_handle(window)?;
    let com = init_com();
    let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
      .map_err(|error| backend(format!("failed to create UI Automation client: {error}")))?;
    let element =
      unsafe { automation.ElementFromHandle(hwnd) }.map_err(|error| backend(format!("failed to resolve window UI element: {error}")))?;
    let walker =
      unsafe { automation.ControlViewWalker() }.map_err(|error| backend(format!("failed to get UI Automation control walker: {error}")))?;
    Ok((com, element_at_path(&walker, element, child_indices)?))
  }

  /// Re-resolves a control-view path without substituting another candidate.
  fn element_at_path(
    walker: &IUIAutomationTreeWalker,
    mut element: IUIAutomationElement,
    child_indices: &[usize],
  ) -> DriverResult<IUIAutomationElement> {
    for index in child_indices {
      let mut child = tree_step(walker, &element, true)
        .map_err(|error| focus_error("resolve UIA child 0", error))?
        .ok_or_else(stale_focus_path)?;
      for sibling_index in 0..*index {
        child = tree_step(walker, &child, false)
          .map_err(|error| focus_error(&format!("resolve UIA child {}", sibling_index + 1), error))?
          .ok_or_else(stale_focus_path)?;
      }
      element = child;
    }
    Ok(element)
  }

  fn stale_focus_path() -> auv_driver_common::DriverError {
    auv_driver_common::DriverError::StaleUiReference {
      message: "UIA focus path no longer identifies a control".into(),
      recovery: Some("read a new UIA tree before focusing".into()),
    }
  }

  /// Depth-first traversal that appends each visited element, then descends
  /// through its control-view children. Traversal stops widening once the node
  /// budget is exhausted and stops descending past the depth limit.
  fn walk(walker: &IUIAutomationTreeWalker, element: &IUIAutomationElement, depth: usize, path: String, capture: &mut TreeCapture) {
    if capture.nodes.len() >= MAX_NODES {
      capture.truncated = true;
      return;
    }
    let (node, failure) = node_from_element(element, depth, path.clone());
    if let Some(error) = failure {
      capture.truncated = true;
      capture.failure.get_or_insert(error);
    }
    capture.nodes.push(node);
    // Retain the provider identity beside each bounded snapshot node. A later
    // path lookup must compare elements, not only role/name/geometry.
    capture.elements.push(element.clone());
    let mut next = traversal_element(walker, element, true, capture);
    if depth >= MAX_DEPTH {
      capture.truncated |= next.is_some();
      return;
    }

    let mut index = 0usize;
    while let Some(child) = next {
      if capture.nodes.len() >= MAX_NODES {
        capture.truncated = true;
        break;
      }
      walk(walker, &child, depth + 1, format!("{path}/{index}"), capture);
      next = traversal_element(walker, &child, false, capture);
      index += 1;
    }
  }

  /// Distinguishes a successful end-of-list from a failed provider call.
  /// Reads the raw tree-walker HRESULT to distinguish S_OK + null (no child)
  /// from a genuine provider error, including E_POINTER. The generated
  /// windows 0.58 wrapper conflates these cases.
  fn tree_step(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    first_child: bool,
  ) -> WindowsResult<Option<IUIAutomationElement>> {
    let mut raw = std::ptr::null_mut();
    // SAFETY: both COM interfaces are alive in this apartment, and raw is a
    // writable out-pointer. Successful non-null results transfer ownership.
    let status = unsafe {
      let table = walker.vtable();
      let step = if first_child {
        table.GetFirstChildElement
      } else {
        table.GetNextSiblingElement
      };
      step(walker.as_raw(), element.as_raw(), &mut raw)
    };
    tree_step_result(status, raw)
  }

  fn tree_step_result(
    status: windows::core::HRESULT,
    raw: *mut std::ffi::c_void,
  ) -> WindowsResult<Option<IUIAutomationElement>> {
    if status.is_err() {
      return Err(windows::core::Error::from_hresult(status));
    }
    if raw.is_null() {
      return Ok(None);
    }
    // SAFETY: this is an owned element pointer returned by a successful
    // IUIAutomationTreeWalker vtable call; from_raw consumes the reference.
    Ok(Some(unsafe { IUIAutomationElement::from_raw(raw) }))
  }

  fn traversal_element(
    walker: &IUIAutomationTreeWalker,
    element: &IUIAutomationElement,
    first_child: bool,
    capture: &mut TreeCapture,
  ) -> Option<IUIAutomationElement> {
    match tree_step(walker, element, first_child) {
      Ok(next) => next,
      Err(error) => {
        capture.truncated = true;
        capture.failure.get_or_insert_with(|| focus_error("walk UIA control tree", error));
        None
      }
    }
  }

  fn node_from_element(element: &IUIAutomationElement, depth: usize, path: String) -> (AxNode, Option<auv_driver_common::DriverError>) {
    let mut failure = None;
    let node = AxNode {
      depth,
      path,
      // SAFETY: property reads use the live interface on its COM thread.
      control_type: read_property(
        unsafe { element.CurrentLocalizedControlType() }.map(|value| value.to_string()),
        "read localized control type",
        &mut failure,
        String::new(),
      ),
      role: read_property(unsafe { element.CurrentControlType() }.map(control_type_name), "read control type", &mut failure, String::new()),
      name: read_property(unsafe { element.CurrentName() }.map(|value| value.to_string()), "read control name", &mut failure, String::new()),
      value: value_or_none(element, &mut failure),
      automation_id: read_property(
        unsafe { element.CurrentAutomationId() }.map(|value| value.to_string()),
        "read automation ID",
        &mut failure,
        String::new(),
      ),
      class_name: read_property(
        unsafe { element.CurrentClassName() }.map(|value| value.to_string()),
        "read control class",
        &mut failure,
        String::new(),
      ),
      focused: read_property(
        unsafe { element.CurrentHasKeyboardFocus() }.map(|value| value.as_bool()),
        "read keyboard focus",
        &mut failure,
        false,
      ),
      enabled: read_property(unsafe { element.CurrentIsEnabled() }.map(|value| value.as_bool()), "read enabled state", &mut failure, false),
      keyboard_focusable: read_property(
        unsafe { element.CurrentIsKeyboardFocusable() }.map(|value| value.as_bool()),
        "read focusable state",
        &mut failure,
        false,
      ),
      offscreen: read_property(
        unsafe { element.CurrentIsOffscreen() }.map(|value| value.as_bool()),
        "read offscreen state",
        &mut failure,
        true,
      ),
      bounds: read_property(
        unsafe { element.CurrentBoundingRectangle() }.map(|rect| rect_from_edges(rect.left, rect.top, rect.right, rect.bottom)),
        "read control bounds",
        &mut failure,
        Rect::default(),
      ),
    };
    (node, failure)
  }

  // Stable UIA names come from the SDK's CONTROLTYPEID constants, not from
  // CurrentLocalizedControlType, which changes with the provider language.
  fn control_type_name(control_type: uia::UIA_CONTROLTYPE_ID) -> String {
    let name = match control_type {
      uia::UIA_ButtonControlTypeId => "Button",
      uia::UIA_CalendarControlTypeId => "Calendar",
      uia::UIA_CheckBoxControlTypeId => "CheckBox",
      uia::UIA_ComboBoxControlTypeId => "ComboBox",
      uia::UIA_EditControlTypeId => "Edit",
      uia::UIA_HyperlinkControlTypeId => "Hyperlink",
      uia::UIA_ImageControlTypeId => "Image",
      uia::UIA_ListItemControlTypeId => "ListItem",
      uia::UIA_ListControlTypeId => "List",
      uia::UIA_MenuControlTypeId => "Menu",
      uia::UIA_MenuBarControlTypeId => "MenuBar",
      uia::UIA_MenuItemControlTypeId => "MenuItem",
      uia::UIA_ProgressBarControlTypeId => "ProgressBar",
      uia::UIA_RadioButtonControlTypeId => "RadioButton",
      uia::UIA_ScrollBarControlTypeId => "ScrollBar",
      uia::UIA_SliderControlTypeId => "Slider",
      uia::UIA_SpinnerControlTypeId => "Spinner",
      uia::UIA_StatusBarControlTypeId => "StatusBar",
      uia::UIA_TabControlTypeId => "Tab",
      uia::UIA_TabItemControlTypeId => "TabItem",
      uia::UIA_TextControlTypeId => "Text",
      uia::UIA_ToolBarControlTypeId => "ToolBar",
      uia::UIA_ToolTipControlTypeId => "ToolTip",
      uia::UIA_TreeControlTypeId => "Tree",
      uia::UIA_TreeItemControlTypeId => "TreeItem",
      uia::UIA_CustomControlTypeId => "Custom",
      uia::UIA_GroupControlTypeId => "Group",
      uia::UIA_ThumbControlTypeId => "Thumb",
      uia::UIA_DataGridControlTypeId => "DataGrid",
      uia::UIA_DataItemControlTypeId => "DataItem",
      uia::UIA_DocumentControlTypeId => "Document",
      uia::UIA_SplitButtonControlTypeId => "SplitButton",
      uia::UIA_WindowControlTypeId => "Window",
      uia::UIA_PaneControlTypeId => "Pane",
      uia::UIA_HeaderControlTypeId => "Header",
      uia::UIA_HeaderItemControlTypeId => "HeaderItem",
      uia::UIA_TableControlTypeId => "Table",
      uia::UIA_TitleBarControlTypeId => "TitleBar",
      uia::UIA_SeparatorControlTypeId => "Separator",
      uia::UIA_SemanticZoomControlTypeId => "SemanticZoom",
      uia::UIA_AppBarControlTypeId => "AppBar",
      _ => return format!("ControlType({})", control_type.0),
    };
    name.into()
  }

  /// Raw snapshots retain partial facts, but focus must preserve the first
  /// property error and refuse selection instead of treating it as empty data.
  fn read_property<T>(result: WindowsResult<T>, property: &str, failure: &mut Option<auv_driver_common::DriverError>, fallback: T) -> T {
    match result {
      Ok(value) => value,
      Err(error) => {
        failure.get_or_insert_with(|| focus_error(property, error));
        fallback
      }
    }
  }

  fn value_or_none(element: &IUIAutomationElement, failure: &mut Option<auv_driver_common::DriverError>) -> Option<String> {
    let mut raw = std::ptr::null_mut();
    // NOTICE: as in traversal_element, preserve the difference between a
    // successful null pattern (unsupported) and a provider E_POINTER failure.
    // SAFETY: the interface is apartment-local; IID names the exact requested
    // pattern, and the initialized out-pointer lives through this call.
    let status =
      unsafe { (element.vtable().GetCurrentPatternAs)(element.as_raw(), UIA_ValuePatternId, &IUIAutomationValuePattern::IID, &mut raw) };
    if status.is_err() {
      if status != windows::Win32::Foundation::E_NOINTERFACE && status.0 as u32 != uia::UIA_E_NOTSUPPORTED {
        failure.get_or_insert_with(|| focus_error("read ValuePattern", windows::core::Error::from_hresult(status)));
      }
      return None;
    }
    if raw.is_null() {
      return None;
    }
    // SAFETY: the successful COM out-pointer owns one reference to the exact
    // requested IUIAutomationValuePattern interface. from_raw takes ownership.
    let pattern = unsafe { IUIAutomationValuePattern::from_raw(raw) };
    // SAFETY: pattern remains on its COM apartment thread.
    let value = read_property(unsafe { pattern.CurrentValue() }, "read control value", failure, BSTR::default()).to_string();
    (!value.is_empty()).then_some(value)
  }

  #[cfg(test)]
  mod property_tests {
    use super::*;

    #[test]
    fn empty_tree_step_is_not_a_provider_pointer_error() {
      let empty = tree_step_result(windows::core::HRESULT(0), std::ptr::null_mut()).expect("S_OK + null is end-of-list");
      assert!(empty.is_none());
      let error = tree_step_result(windows::Win32::Foundation::E_POINTER, std::ptr::null_mut()).err().expect("real E_POINTER must fail");
      assert_eq!(error.code(), windows::Win32::Foundation::E_POINTER);
    }

    #[test]
    fn failed_property_reads_preserve_the_first_native_failure() {
      let mut failure = None;
      let value = read_property::<bool>(
        Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_ACCESSDENIED)),
        "read focusable state",
        &mut failure,
        false,
      );
      assert!(!value);
      assert!(matches!(failure, Some(auv_driver_common::DriverError::PermissionDenied { .. })));
      let _ = read_property::<String>(
        Err(windows::core::Error::from_hresult(windows::Win32::Foundation::E_FAIL)),
        "read name",
        &mut failure,
        String::new(),
      );
      assert!(matches!(failure, Some(auv_driver_common::DriverError::PermissionDenied { .. })));
    }

    #[test]
    fn a_legitimate_empty_property_is_not_a_provider_failure() {
      let mut failure = None;
      assert_eq!(read_property(Ok(String::new()), "read name", &mut failure, "fallback".into()), "");
      assert!(failure.is_none());
    }
  }
}

#[cfg(not(target_os = "windows"))]
mod native {
  use auv_driver_common::error::{DriverError, DriverResult};
  use auv_driver_common::window::Window;

  use super::{AxFocusResult, AxTreeSnapshot, FocusTextOptions};

  pub(super) fn focus_text(_window: &Window, _options: &FocusTextOptions) -> DriverResult<AxFocusResult> {
    Err(DriverError::unsupported("accessibility.focus_text"))
  }

  pub(super) fn snapshot_window(_window: &Window) -> DriverResult<AxTreeSnapshot> {
    Err(DriverError::unsupported("accessibility.snapshot_window"))
  }

  pub(super) fn focus_node(_window: &Window, _child_indices: &[usize]) -> DriverResult<()> {
    Err(DriverError::unsupported("accessibility.focus_node"))
  }

  pub(super) fn select_node(_window: &Window, _child_indices: &[usize]) -> DriverResult<&'static str> {
    Err(DriverError::unsupported("accessibility.select_node"))
  }
}

#[cfg(test)]
#[path = "accessibility_test.rs"]
mod tests;
