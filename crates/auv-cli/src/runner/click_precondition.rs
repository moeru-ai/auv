//! Capture-bound click admission. All checks precede the input capability call.

use auv_api_proto::auv::api::driver::v1::{self as proto, ClickPreconditionFailureReason as Reason};
use auv_driver::{Capture, CaptureOptions, CaptureResolution, CoordinateSpace, Display, Position, ScreenPoint, Window};
use tonic::Status;

use super::capture_store::CaptureStore;

/// Resolved resources at capture time, rather than selectors for future captures.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum CaptureTarget {
  Window(Window),
  Display(Display),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct CaptureState {
  target: CaptureTarget,
  resolution: CaptureResolution,
  before: Vec<Window>,
  after: Vec<Window>,
}

impl CaptureState {
  /// Heap retained by this reference's provenance, independent of shared pixels.
  pub(super) fn retained_bytes(&self) -> usize {
    let space_bytes = |space: &CoordinateSpace| match space {
      CoordinateSpace::Screen => 0,
      CoordinateSpace::Window(id) | CoordinateSpace::Display(id) => id.capacity(),
    };
    let window_bytes = |window: &Window| {
      window.reference.id.capacity()
        + [&window.title, &window.app_name, &window.application_id].into_iter().flatten().map(String::capacity).sum::<usize>()
        + space_bytes(&window.coordinate_space)
    };
    let target_bytes = match &self.target {
      CaptureTarget::Window(window) => window_bytes(window),
      CaptureTarget::Display(display) => {
        display.id.capacity() + display.name.as_ref().map_or(0, String::capacity) + space_bytes(&display.coordinate_space)
      }
    };
    target_bytes
      + (self.before.capacity() + self.after.capacity()) * std::mem::size_of::<Window>()
      + self.before.iter().chain(&self.after).map(window_bytes).sum::<usize>()
  }

  /// Preserve both snapshots so a display click can select its relevant
  /// windows later. Capture remains readable even if admission is unavailable.
  pub(super) fn new(
    target: CaptureTarget,
    resolution: CaptureResolution,
    before: Result<Vec<Window>, String>,
    after: Result<Vec<Window>, String>,
  ) -> Result<Self, String> {
    let visible = |windows: Vec<Window>| windows.into_iter().filter(|window| window.is_visible).collect::<Vec<_>>();
    let before = visible(before.map_err(|cause| format!("window list before capture: {cause}"))?);
    let after = visible(after.map_err(|cause| format!("window list after capture: {cause}"))?);
    if let CaptureTarget::Window(window) = &target
      && (!before.contains(window) || !after.contains(window))
    {
      return Err("target window changed during capture".into());
    }
    Ok(Self {
      target,
      resolution,
      before,
      after,
    })
  }

  /// Window captures include intersecting windows; display captures include
  /// windows at the click point. Preserve their backend order, without
  /// claiming that enumeration exposes a portable z-order guarantee.
  fn windows_at(&self, point: auv_driver::Point) -> Result<Vec<&Window>, String> {
    let relevant = |window: &&Window| match &self.target {
      CaptureTarget::Window(target) => window.frame.intersect(target.frame).is_some(),
      CaptureTarget::Display(_) => window.frame.contains_point(point),
    };
    let before: Vec<_> = self.before.iter().filter(relevant).collect();
    let after: Vec<_> = self.after.iter().filter(relevant).collect();
    if before != after {
      return Err(format!("windows intersecting the target changed during capture: {}", window_difference(&before, &after)));
    }
    Ok(before)
  }

  pub(super) fn recapture(
    &self,
    session: &auv_driver::LocalDriverSession,
    recipient: Option<&Window>,
  ) -> Result<(Capture, Result<Self, String>), Status> {
    let before = session.window().list().map_err(super::local_driver::driver_status)?;
    if recipient.is_some_and(|recipient| !before.contains(recipient)) {
      return Err(Status::failed_precondition("click recipient changed during coordinate resolution"));
    }
    let options = CaptureOptions {
      resolution: self.resolution,
      freshness: auv_driver::CaptureFreshness::RequireNewFrame,
      ..Default::default()
    };
    let (target, capture) = match &self.target {
      CaptureTarget::Window(expected) => {
        let window = auv_driver::find_window(before.clone(), &expected.reference.id).map_err(super::local_driver::driver_status)?;
        let capture = session.window().capture_with(&window, options).map_err(super::local_driver::driver_status)?;
        (CaptureTarget::Window(window), capture)
      }
      CaptureTarget::Display(expected) => {
        let captured = session
          .display()
          .capture(CaptureOptions {
            display: Some(expected.id.clone()),
            ..options
          })
          .map_err(super::local_driver::driver_status)?;
        (CaptureTarget::Display(captured.display), captured.capture)
      }
    };
    let after = session.window().list().map_err(|error| error.to_string());
    Ok((capture, Self::new(target, self.resolution, Ok(before), after)))
  }
}

/// Describe an already rejected comparison. Match identities without sorting
/// the snapshots: backend order remains part of admission, not a z-order claim.
fn window_difference(before: &[&Window], after: &[&Window]) -> String {
  use std::collections::HashMap;

  let before_by_ref: HashMap<_, _> = before.iter().map(|window| (&window.reference, *window)).collect();
  let after_by_ref: HashMap<_, _> = after.iter().map(|window| (&window.reference, *window)).collect();
  let (mut added, mut removed, mut modified) = (0, 0, 0);
  let mut first = None;
  for &window in before {
    match after_by_ref.get(&window.reference) {
      None => removed += 1,
      Some(current) if *current != window => modified += 1,
      _ => continue,
    }
    first.get_or_insert(window);
  }
  for &window in after {
    if !before_by_ref.contains_key(&window.reference) {
      added += 1;
      first.get_or_insert(window);
    }
  }
  // Compare the relative order of shared identities. Insertions and removals
  // alone do not mean the remaining windows changed order.
  let order_changed = before
    .iter()
    .map(|window| &window.reference)
    .filter(|reference| after_by_ref.contains_key(reference))
    .ne(after.iter().map(|window| &window.reference).filter(|reference| before_by_ref.contains_key(reference)));
  let mut summary = format!("added={added}, removed={removed}, modified={modified}, order_changed={order_changed}");
  let first = first.or_else(|| before.iter().zip(after).find_map(|(old, new)| (old.reference != new.reference).then_some(*old)));
  if let Some(window) = first {
    // Bound each escaped label before formatting, so a long backend ID cannot
    // hide the app name. Do not include document titles in this diagnostic.
    let label = |value: &str| {
      let mut characters = value.chars().flat_map(char::escape_debug);
      let mut text: String = characters.by_ref().take(80).collect();
      if characters.next().is_some() {
        text.push('…');
      }
      text
    };
    let id = label(&window.reference.id);
    let app = label(window.app_name.as_deref().or(window.application_id.as_deref()).unwrap_or("unknown"));
    summary.push_str(&format!("; first difference: id=\"{id}\", app=\"{app}\""));
  }
  summary
}

/// Owns the admission decision and invokes delivery only after every check.
/// The closures are external boundaries: live capture and native input.
pub(super) fn deliver<T>(
  captures: &CaptureStore,
  reference: Option<&proto::CaptureRef>,
  position: &Position,
  window: Option<&Window>,
  screen_point: ScreenPoint,
  recapture: impl FnOnce(&CaptureState) -> Result<(Capture, Result<CaptureState, String>), Status>,
  input: impl FnOnce() -> Result<T, Status>,
) -> Result<T, Status> {
  let Some(reference) = reference else {
    return input();
  };
  // Wait for other AUV pointer operations before checking evidence. Platform
  // delivery reuses this thread's reservation, so it cannot queue after the
  // check. Recapture also rejects a recipient moved since coordinate resolution.
  let _desktop = auv_driver::mouse_input::reserve_desktop_input().map_err(super::local_driver::driver_status)?;
  let id = &reference.capture_id;
  let expected =
    captures.get(id).ok_or_else(|| refusal(id, Reason::CaptureUnavailable, "capture expired, was evicted, or belongs to another Runner"))?;
  // TODO: OCR, region and streamed captures have no click baseline yet. Add
  // provenance at their producers only in an owner-approved follow-up slice.
  let state = captures.click_state(id).map_err(|cause| refusal(id, Reason::CheckUnavailable, &cause))?;
  // NOTICE: AT-SPI can report (0,0) for moved GTK4 windows on Wayland.
  // This crop cannot prove window location. Re-enable after the driver
  // exposes reliable screen placement; see PR #429, discussion_r4382693034.
  if expected.backend.starts_with("atspi.extents+") {
    return Err(refusal(id, Reason::CheckUnavailable, "AT-SPI window crops cannot establish reliable screen placement"));
  }
  match &state.target {
    CaptureTarget::Window(target) if window.is_none_or(|window| window.reference != target.reference) => {
      return Err(refusal(id, Reason::TargetMismatch, "click does not name the captured window"));
    }
    CaptureTarget::Window(target) if window != Some(target) => {
      return Err(refusal(id, Reason::StateChanged, "captured window metadata changed"));
    }
    CaptureTarget::Display(target) => {
      if let CoordinateSpace::Display(id) = &position.coordinate_space
        && id != &target.id
      {
        return Err(refusal(&reference.capture_id, Reason::TargetMismatch, "click names a different display"));
      }
      if matches!(position.coordinate_space, CoordinateSpace::Display(_))
        && screen_point.point() != auv_driver::Point::new(target.frame.origin.x + position.point.x, target.frame.origin.y + position.point.y)
      {
        return Err(refusal(id, Reason::StateChanged, "display coordinate mapping changed"));
      }
    }
    _ => {}
  }
  let point = screen_point.point();
  let bounds = expected.bounds;
  if !point.x.is_finite()
    || !point.y.is_finite()
    || point.x < bounds.origin.x
    || point.y < bounds.origin.y
    || point.x >= bounds.origin.x + bounds.size.width
    || point.y >= bounds.origin.y + bounds.size.height
  {
    return Err(refusal(id, Reason::TargetMismatch, "click lies outside the captured bounds"));
  }
  let windows = state.windows_at(point).map_err(|cause| refusal(id, Reason::CheckUnavailable, &cause))?;
  let (current, current_state) = recapture(&state).map_err(|error| refusal(id, Reason::CheckUnavailable, error.message()))?;
  let current_state = current_state.map_err(|cause| refusal(id, Reason::CheckUnavailable, &cause))?;
  let current_windows = current_state.windows_at(point).map_err(|cause| refusal(id, Reason::CheckUnavailable, &cause))?;
  if state.target != current_state.target || state.resolution != current_state.resolution {
    return Err(refusal(id, Reason::StateChanged, "target or intersecting window metadata changed"));
  }
  if windows != current_windows {
    let cause = format!("target or intersecting window metadata changed: {}", window_difference(&windows, &current_windows));
    return Err(refusal(id, Reason::StateChanged, &cause));
  }
  if *expected != current {
    return Err(refusal(id, Reason::StateChanged, "captured pixels or capture metadata changed (exact comparison)"));
  }
  if expected.image.width() == 0 || expected.image.height() == 0 {
    return Err(refusal(id, Reason::CheckUnavailable, "capture has no pixels"));
  }
  // NOTICE: This is not an OS transaction. The UI can still change during
  // native activation, fallback or event submission. See
  // `proto/auv/api/driver/v1/docs/input.click.md`.
  // TODO: Semantic matching and automatic replanning require a separate
  // owner-approved policy; this slice compares exact evidence only.
  if auv_driver::input_cancellation::current_input_cancellation().is_some_and(|signal| signal.is_cancelled()) {
    return Err(Status::cancelled("click cancelled during capture admission"));
  }
  input()
}

pub(super) fn refusal(capture_id: &str, reason: Reason, cause: &str) -> Status {
  // Bound backend diagnostics in both the status and the recorded event.
  let cause: String = cause.chars().take(512).collect();
  auv_tracing::emit_event!(Refused {
    capture_id,
    reason: reason.as_str_name(),
    cause: &cause,
    recovery: "new_capture",
    input_delivered: false,
  });
  auv_api_proto::click_precondition::status(capture_id, reason, &cause)
}

#[derive(serde::Serialize)]
struct Refused<'a> {
  capture_id: &'a str,
  reason: &'static str,
  cause: &'a str,
  recovery: &'static str,
  input_delivered: bool,
}

impl auv_tracing::EventPayload for Refused<'_> {
  const NAME: &'static str = "auv.input.click_refused";
  const VERSION: u32 = 1;
}

#[cfg(test)]
#[path = "click_precondition_test.rs"]
mod tests;
