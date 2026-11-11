//! Display topology descriptors and the existing selector/containment rules.
use auv_driver_common::error::not_found;
use auv_driver_common::{Display, DriverResult, Rect};

/// A display paired with its index in one native enumeration snapshot.
///
/// The index addresses that snapshot only. Backends with indexed pixel acquisition
/// must use the same monitor enumeration to acquire pixels after resolving.
#[derive(Clone, Debug)]
pub struct DisplayTarget {
  pub index: usize,
  pub display: Display,
}

pub fn resolve_target(targets: &[DisplayTarget], selector: Option<&str>) -> DriverResult<DisplayTarget> {
  if let Some(selector) = selector {
    let selector = selector.trim();
    return targets
      .iter()
      .find(|target| target.display.id == selector || target.display.name.as_deref().is_some_and(|display_ref| display_ref == selector))
      .cloned()
      .ok_or_else(|| not_found(format!("display {selector:?}")));
  }

  targets.iter().find(|target| target.display.is_primary).or_else(|| targets.first()).cloned().ok_or_else(|| not_found("primary display"))
}

pub fn resolve_for_region(targets: &[DisplayTarget], selector: Option<&str>, region: Rect) -> DriverResult<DisplayTarget> {
  let selected = if selector.is_some() {
    vec![resolve_target(targets, selector)?]
  } else {
    targets.to_vec()
  };
  selected.into_iter().find(|target| rect_contains_rect(target.display.frame, region)).ok_or_else(|| not_found("display containing region"))
}

fn rect_contains_rect(container: Rect, candidate: Rect) -> bool {
  candidate.origin.x >= container.origin.x
    && candidate.origin.y >= container.origin.y
    && candidate.origin.x + candidate.size.width <= container.origin.x + container.size.width
    && candidate.origin.y + candidate.size.height <= container.origin.y + container.size.height
}

#[cfg(test)]
#[path = "display_test.rs"]
mod tests;
