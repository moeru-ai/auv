use crate::*;

pub(crate) fn sidebar_ax_scrollbar_boundary(
  nodes: &[ObservedAxNode],
  window: &auv_driver::Window,
  sidebar_bounds: ViewBounds,
) -> Option<SidebarScrollbarBoundary> {
  let sidebar_screen_bounds = ViewBounds::new(
    window.frame.origin.x + sidebar_bounds.x,
    window.frame.origin.y + sidebar_bounds.y,
    sidebar_bounds.width,
    sidebar_bounds.height,
  );
  let sidebar_right = sidebar_screen_bounds.x + sidebar_screen_bounds.width;
  let sidebar_bottom = sidebar_screen_bounds.y + sidebar_screen_bounds.height;

  let scrollbar = nodes
    .iter()
    .filter(|node| node.role == "AXScrollBar" && node.bounds.width > 0 && node.bounds.height > 0 && node.bounds.height > node.bounds.width)
    .filter(|node| {
      let node_top = node.bounds.y as f64;
      let node_bottom = node_top + node.bounds.height as f64;
      let vertical_overlap = (node_bottom.min(sidebar_bottom) - node_top.max(sidebar_screen_bounds.y)).max(0.0);
      let overlap_ratio = vertical_overlap / node.bounds.height as f64;
      let center_x = node.bounds.x as f64 + (node.bounds.width as f64 / 2.0);
      overlap_ratio >= 0.5 && center_x >= sidebar_screen_bounds.x && center_x <= sidebar_right + 20.0
    })
    .max_by(|left, right| {
      let left_overlap = scrollbar_overlap_score(left, sidebar_screen_bounds);
      let right_overlap = scrollbar_overlap_score(right, sidebar_screen_bounds);
      left_overlap.total_cmp(&right_overlap)
    })?;

  vertical_scrollbar_boundary_from_nodes(nodes, scrollbar)
}

#[cfg(target_os = "macos")]
pub(crate) fn scrollbar_overlap_score(node: &ObservedAxNode, sidebar_screen_bounds: ViewBounds) -> f64 {
  let sidebar_right = sidebar_screen_bounds.x + sidebar_screen_bounds.width;
  let sidebar_bottom = sidebar_screen_bounds.y + sidebar_screen_bounds.height;
  let node_top = node.bounds.y as f64;
  let node_bottom = node_top + node.bounds.height as f64;
  let vertical_overlap = (node_bottom.min(sidebar_bottom) - node_top.max(sidebar_screen_bounds.y)).max(0.0);
  let overlap_ratio = vertical_overlap / node.bounds.height as f64;
  let node_right = node.bounds.x as f64 + node.bounds.width as f64;
  let right_edge_distance = (sidebar_right - node_right).abs();
  overlap_ratio * 1000.0 - right_edge_distance
}

#[cfg(target_os = "macos")]
pub(crate) fn vertical_scrollbar_boundary_from_nodes(
  nodes: &[ObservedAxNode],
  scrollbar: &ObservedAxNode,
) -> Option<SidebarScrollbarBoundary> {
  let path_prefix = format!("{}.", scrollbar.path);
  let mut increment_page_height = None;
  let mut decrement_page_height = None;

  for node in nodes.iter().filter(|node| node.path.starts_with(path_prefix.as_str())) {
    match node.subrole.as_str() {
      "AXIncrementPage" => increment_page_height = Some(node.bounds.height),
      "AXDecrementPage" => decrement_page_height = Some(node.bounds.height),
      _ => {}
    }
  }

  match (increment_page_height, decrement_page_height) {
    (Some(height), _) if height <= 1 => Some(SidebarScrollbarBoundary::Bottom),
    (_, Some(height)) if height <= 1 => Some(SidebarScrollbarBoundary::Top),
    (Some(_), Some(_)) => Some(SidebarScrollbarBoundary::Interior),
    _ => None,
  }
}
