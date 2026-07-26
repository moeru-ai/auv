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
#[path = "lib_test.rs"]
mod tests;
