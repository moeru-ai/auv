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
  fn windows_at(&self, point: auv_driver::Point) -> Result<Vec<&Window>, &'static str> {
    let relevant = |window: &&Window| match &self.target {
      CaptureTarget::Window(target) => window.frame.intersect(target.frame).is_some(),
      CaptureTarget::Display(_) => window.frame.contains_point(point),
    };
    let before: Vec<_> = self.before.iter().filter(relevant).collect();
    let after: Vec<_> = self.after.iter().filter(relevant).collect();
    if before != after {
      return Err("windows intersecting the target changed during capture");
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
      ..Default::default()
    };
    let (target, capture) = match &self.target {
      CaptureTarget::Window(expected) => {
        let window = auv_driver::find_window(before.clone(), &expected.reference.id).map_err(super::local_driver::driver_status)?;
        let capture = session.window().capture_with(&window, options).map_err(super::local_driver::driver_status)?;
        (CaptureTarget::Window(window), capture)
      }
      CaptureTarget::Display(expected) => {
        // TODO(click-frame-freshness): Linux can return a cached PipeWire frame.
        // Freshness admission is deferred until the planned Linux validation;
        // see the click-capture-precondition contract's review limits.
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
  let windows = state.windows_at(point).map_err(|cause| refusal(id, Reason::CheckUnavailable, cause))?;
  let (current, current_state) = recapture(&state).map_err(|error| refusal(id, Reason::CheckUnavailable, error.message()))?;
  let current_state = current_state.map_err(|cause| refusal(id, Reason::CheckUnavailable, &cause))?;
  let current_windows = current_state.windows_at(point).map_err(|cause| refusal(id, Reason::CheckUnavailable, cause))?;
  if state.target != current_state.target || state.resolution != current_state.resolution || windows != current_windows {
    return Err(refusal(id, Reason::StateChanged, "target or intersecting window metadata changed"));
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
