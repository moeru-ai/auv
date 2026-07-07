//! NOTICE(core-b2): this crate currently owns only narrow dual-backend compare policy helpers.
//! Broader spatial compare abstraction is deferred until more cross-vertical evidence exists.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DualBackendStageStatus {
  Answered,
  Blocked,
  Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DualBackendCompareVerdict {
  Match,
  Divergent,
  ProviderOnly,
  ReferenceOnly,
  NotComparable,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenPoint {
  pub x: f64,
  pub y: f64,
}

pub trait DualBackendAnswer {
  type VisibilityKey: PartialEq;

  fn stage_status(&self) -> DualBackendStageStatus;
  fn visibility_key(&self) -> Option<Self::VisibilityKey>;
  fn screen_point(&self) -> Option<ScreenPoint>;
  fn match_radius_px(&self) -> Option<f64>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DualBackendSelectedSide {
  Provider,
  Reference,
  Neither,
}

pub fn screen_points_match_with_tolerance(
  provider_point: ScreenPoint,
  reference_point: ScreenPoint,
  provider_radius_px: Option<f64>,
  reference_radius_px: Option<f64>,
) -> bool {
  let tolerance = provider_radius_px.unwrap_or(1.0).max(reference_radius_px.unwrap_or(1.0));
  let dx = provider_point.x - reference_point.x;
  let dy = provider_point.y - reference_point.y;
  (dx * dx + dy * dy).sqrt() <= tolerance
}

pub fn compare_dual_backend_verdict<P, R>(provider_answer: Option<&P>, reference_answer: Option<&R>) -> Option<DualBackendCompareVerdict>
where
  P: DualBackendAnswer,
  R: DualBackendAnswer<VisibilityKey = P::VisibilityKey>,
{
  let provider_answered = provider_answer.is_some_and(|answer| answer.stage_status() == DualBackendStageStatus::Answered);
  let reference_answered = reference_answer.is_some_and(|answer| answer.stage_status() == DualBackendStageStatus::Answered);

  match (provider_answered, reference_answered) {
    (true, true) => {
      let provider = provider_answer.expect("provider answered");
      let reference = reference_answer.expect("reference answered");
      Some(if dual_backend_answers_match(provider, reference) {
        DualBackendCompareVerdict::Match
      } else {
        DualBackendCompareVerdict::Divergent
      })
    }
    (true, false) => Some(DualBackendCompareVerdict::ProviderOnly),
    (false, true) => Some(DualBackendCompareVerdict::ReferenceOnly),
    (false, false) => Some(DualBackendCompareVerdict::NotComparable),
  }
}

pub fn select_dual_backend_outcome<A, F>(
  provider_answer: Option<&A>,
  reference_answer: Option<&A>,
  pick_fallback: F,
) -> (DualBackendSelectedSide, A, Option<DualBackendCompareVerdict>)
where
  A: DualBackendAnswer + Clone,
  F: FnOnce(Option<&A>, Option<&A>) -> A,
{
  let provider_answered = provider_answer.is_some_and(|answer| answer.stage_status() == DualBackendStageStatus::Answered);
  let reference_answered = reference_answer.is_some_and(|answer| answer.stage_status() == DualBackendStageStatus::Answered);

  if provider_answered {
    let answer = provider_answer.expect("provider answered implies provider answer present").clone();
    let comparison_verdict = compare_dual_backend_verdict(provider_answer, reference_answer);
    return (DualBackendSelectedSide::Provider, answer, comparison_verdict);
  }

  if reference_answered {
    let answer = reference_answer.expect("reference answered implies reference answer present").clone();
    let comparison_verdict = compare_dual_backend_verdict(provider_answer, reference_answer);
    return (DualBackendSelectedSide::Reference, answer, comparison_verdict);
  }

  let answer = pick_fallback(provider_answer, reference_answer);
  let comparison_verdict = compare_dual_backend_verdict(provider_answer, reference_answer);
  (DualBackendSelectedSide::Neither, answer, comparison_verdict)
}

pub fn pick_blocked_or_failed_preferred<'a, T>(
  candidates: impl IntoIterator<Item = Option<&'a T>>,
  is_blocked: impl Fn(&T) -> bool,
) -> Option<&'a T> {
  let candidates: Vec<&'a T> = candidates.into_iter().flatten().collect();
  candidates.iter().find(|candidate| is_blocked(candidate)).copied().or_else(|| candidates.first().copied())
}

fn dual_backend_answers_match<P, R>(provider: &P, reference: &R) -> bool
where
  P: DualBackendAnswer,
  R: DualBackendAnswer<VisibilityKey = P::VisibilityKey>,
{
  if provider.visibility_key() != reference.visibility_key() {
    return false;
  }
  match (provider.screen_point(), reference.screen_point()) {
    (Some(provider_point), Some(reference_point)) => {
      screen_points_match_with_tolerance(provider_point, reference_point, provider.match_radius_px(), reference.match_radius_px())
    }
    (None, None) => true,
    _ => false,
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  enum TestVisibility {
    Visible,
    Occluded,
  }

  #[derive(Clone, Debug, PartialEq)]
  struct TestAnswer {
    status: DualBackendStageStatus,
    visibility: Option<TestVisibility>,
    screen_point: Option<ScreenPoint>,
    match_radius_px: Option<f64>,
  }

  impl DualBackendAnswer for TestAnswer {
    type VisibilityKey = TestVisibility;

    fn stage_status(&self) -> DualBackendStageStatus {
      self.status
    }

    fn visibility_key(&self) -> Option<Self::VisibilityKey> {
      self.visibility
    }

    fn screen_point(&self) -> Option<ScreenPoint> {
      self.screen_point
    }

    fn match_radius_px(&self) -> Option<f64> {
      self.match_radius_px
    }
  }

  fn answered(visibility: Option<TestVisibility>, screen_point: Option<ScreenPoint>, match_radius_px: Option<f64>) -> TestAnswer {
    TestAnswer {
      status: DualBackendStageStatus::Answered,
      visibility,
      screen_point,
      match_radius_px,
    }
  }

  fn blocked() -> TestAnswer {
    TestAnswer {
      status: DualBackendStageStatus::Blocked,
      visibility: None,
      screen_point: None,
      match_radius_px: None,
    }
  }

  fn failed() -> TestAnswer {
    TestAnswer {
      status: DualBackendStageStatus::Failed,
      visibility: None,
      screen_point: None,
      match_radius_px: None,
    }
  }

  #[test]
  fn compare_dual_backend_verdict_covers_five_label_matrix() {
    let provider = answered(Some(TestVisibility::Visible), Some(ScreenPoint { x: 1.0, y: 2.0 }), None);
    let reference = answered(Some(TestVisibility::Visible), Some(ScreenPoint { x: 1.0, y: 2.0 }), None);
    let provider_only = answered(Some(TestVisibility::Visible), None, None);
    let reference_only = answered(Some(TestVisibility::Occluded), None, None);

    assert_eq!(compare_dual_backend_verdict(Some(&provider), Some(&reference)), Some(DualBackendCompareVerdict::Match));
    assert_eq!(compare_dual_backend_verdict(Some(&provider), Some(&reference_only)), Some(DualBackendCompareVerdict::Divergent));
    assert_eq!(compare_dual_backend_verdict(Some(&provider_only), None::<&TestAnswer>), Some(DualBackendCompareVerdict::ProviderOnly));
    assert_eq!(compare_dual_backend_verdict(None::<&TestAnswer>, Some(&reference_only)), Some(DualBackendCompareVerdict::ReferenceOnly));
    assert_eq!(compare_dual_backend_verdict(Some(&blocked()), Some(&failed())), Some(DualBackendCompareVerdict::NotComparable));
  }

  #[test]
  fn select_dual_backend_outcome_prefers_provider_then_reference() {
    let provider = answered(Some(TestVisibility::Visible), None, None);
    let reference = answered(Some(TestVisibility::Visible), None, None);

    let (side, _, verdict) = select_dual_backend_outcome(Some(&provider), Some(&reference), |_, _| failed());
    assert_eq!(side, DualBackendSelectedSide::Provider);
    assert_eq!(verdict, Some(DualBackendCompareVerdict::Match));

    let (side, _, verdict) = select_dual_backend_outcome(None, Some(&reference), |_, _| failed());
    assert_eq!(side, DualBackendSelectedSide::Reference);
    assert_eq!(verdict, Some(DualBackendCompareVerdict::ReferenceOnly));

    let (side, selected, verdict) = select_dual_backend_outcome(Some(&blocked()), Some(&failed()), |provider, reference| {
      pick_blocked_or_failed_preferred([provider, reference], |answer| answer.stage_status() == DualBackendStageStatus::Blocked)
        .cloned()
        .unwrap_or_else(failed)
    });
    assert_eq!(side, DualBackendSelectedSide::Neither);
    assert_eq!(selected.status, DualBackendStageStatus::Blocked);
    assert_eq!(verdict, Some(DualBackendCompareVerdict::NotComparable));
  }

  #[test]
  fn screen_points_match_with_tolerance_uses_max_radius_floor() {
    let left = ScreenPoint { x: 0.0, y: 0.0 };
    let near = ScreenPoint { x: 1.0, y: 0.0 };
    let far = ScreenPoint { x: 3.0, y: 0.0 };

    assert!(screen_points_match_with_tolerance(left, near, None, None));
    assert!(!screen_points_match_with_tolerance(left, far, None, None));
    assert!(screen_points_match_with_tolerance(left, far, Some(3.0), None));
    assert!(screen_points_match_with_tolerance(left, near, Some(0.5), Some(2.0)));
  }

  #[test]
  fn pick_blocked_or_failed_preferred_chooses_blocked_before_first_candidate() {
    let provider = failed();
    let reference = blocked();

    let picked = pick_blocked_or_failed_preferred([Some(&provider), Some(&reference)], |answer| {
      answer.stage_status() == DualBackendStageStatus::Blocked
    });
    assert_eq!(picked, Some(&reference));

    let picked = pick_blocked_or_failed_preferred([Some(&provider), Some(&reference)], |answer| {
      answer.stage_status() == DualBackendStageStatus::Failed
    });
    assert_eq!(picked, Some(&provider));
  }
}
