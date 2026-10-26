//! Capture-bound click admission. All checks precede the input capability call.

use auv_api_proto::auv::api::driver::v1::{self as proto, ClickPreconditionFailureReason as Reason};
use auv_driver::{Capture, CaptureOptions, CaptureResolution, CoordinateSpace, Display, Position, ScreenPoint, Window};
use prost::Message as _;
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
  windows: [u8; 32],
}

impl CaptureState {
  /// A changing or unreadable window list cannot establish a baseline. Keep
  /// the capture usable for reading, but refuse clicks that depend on it.
  pub(super) fn new(
    target: CaptureTarget,
    resolution: CaptureResolution,
    before: Option<Vec<Window>>,
    after: Option<Vec<Window>>,
  ) -> Option<Self> {
    let visible = |windows: Vec<Window>| windows.into_iter().filter(|window| window.is_visible).collect::<Vec<_>>();
    let before = visible(before?);
    let after = visible(after?);
    if before != after {
      return None;
    }
    if let CaptureTarget::Window(window) = &target
      && !before.contains(window)
    {
      return None;
    }
    // Preserve backend order too: an ordering change can represent a dialog
    // or occluder moving above the target. No platform-independent z-order is
    // inferred from this list.
    let windows = *blake3::hash(&serde_json::to_vec(&before).ok()?).as_bytes();
    Some(Self {
      target,
      resolution,
      windows,
    })
  }

  pub(super) fn recapture(
    &self,
    session: &auv_driver::LocalDriverSession,
    recipient: Option<&Window>,
  ) -> Result<(Capture, Option<Self>), Status> {
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
    let after = session.window().list().ok();
    Ok((capture, Self::new(target, self.resolution, Some(before), after)))
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
  recapture: impl FnOnce(&CaptureState) -> Result<(Capture, Option<CaptureState>), Status>,
  input: impl FnOnce() -> Result<T, Status>,
) -> Result<T, Status> {
  let Some(reference) = reference else {
    return input();
  };
  let id = &reference.capture_id;
  let expected = captures.get(id).ok_or_else(|| refusal(id, Reason::CaptureUnavailable))?;
  // TODO: OCR, region and streamed captures have no click baseline yet. Add
  // provenance at their producers only in an owner-approved follow-up slice.
  let state = captures.click_state(id).ok_or_else(|| refusal(id, Reason::CheckUnavailable))?;
  match &state.target {
    CaptureTarget::Window(target) if window.is_none_or(|window| window.reference != target.reference) => {
      return Err(refusal(id, Reason::TargetMismatch));
    }
    CaptureTarget::Window(target) if window != Some(target) => {
      return Err(refusal(id, Reason::StateChanged));
    }
    CaptureTarget::Display(target) => {
      if let CoordinateSpace::Display(id) = &position.coordinate_space
        && id != &target.id
      {
        return Err(refusal(&reference.capture_id, Reason::TargetMismatch));
      }
      if matches!(position.coordinate_space, CoordinateSpace::Display(_))
        && screen_point.point() != auv_driver::Point::new(target.frame.origin.x + position.point.x, target.frame.origin.y + position.point.y)
      {
        return Err(refusal(id, Reason::StateChanged));
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
    return Err(refusal(id, Reason::TargetMismatch));
  }
  let (current, current_state) = recapture(&state).map_err(|_| refusal(id, Reason::CheckUnavailable))?;
  let current_state = current_state.ok_or_else(|| refusal(id, Reason::CheckUnavailable))?;
  if state != current_state || *expected != current {
    return Err(refusal(id, Reason::StateChanged));
  }
  if expected.image.width() == 0 || expected.image.height() == 0 {
    return Err(refusal(id, Reason::CheckUnavailable));
  }
  // NOTICE: This is not an OS transaction. The UI can still change during
  // native activation, fallback or event submission. See
  // `proto/auv/api/driver/v1/docs/input.click.md`.
  // TODO: Semantic matching and automatic replanning require a separate
  // owner-approved policy; this slice compares exact evidence only.
  input()
}

pub(super) fn refusal(capture_id: &str, reason: Reason) -> Status {
  let message = format!("click capture precondition failed ({}); take a new capture and select the target again", reason.as_str_name());
  auv_tracing::emit_event!(Refused {
    capture_id,
    reason: reason.as_str_name(),
    recovery: "new_capture",
    input_delivered: false,
  });
  let details = proto::ClickPreconditionFailure {
    capture_id: capture_id.to_string(),
    reason: reason as i32,
  };
  Status::with_details(tonic::Code::FailedPrecondition, message, details.encode_to_vec().into())
}

#[derive(serde::Serialize)]
struct Refused<'a> {
  capture_id: &'a str,
  reason: &'static str,
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
