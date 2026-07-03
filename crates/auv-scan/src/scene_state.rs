//! Scene state product (L2 read-model): evidence-first draft answers to S0 five questions.
//!
//! NOTICE(scan-s5a): `motion` is supporting evidence only — must not drive draft-answer conclusions.

use thiserror::Error;

use crate::association::{AssociationResult, FrameObservation, associate_adjacent_frames};
use crate::coverage::{CoverageView, build_coverage_view};
use crate::lifecycle::{LifecycleError, LifecycleEvent, LifecycleVerdict, evaluate_lifecycle};
use crate::motion::{MotionResult, MotionUnknown, estimate_viewport_motion};
use crate::reader::ScanFrameBundle;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneStateInput {
  pub bundle: ScanFrameBundle,
  pub observations_by_frame: Vec<Vec<FrameObservation>>,
  pub lifecycle_events: Option<Vec<LifecycleEvent>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IdentityAssessment {
  Linked,
  NewTrack,
  Ambiguous,
  Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VisibilityAssessment {
  Visible,
  StaleCandidate,
  Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrackSceneSummary {
  pub track_id: String,
  pub last_seen_frame_id: Option<String>,
  pub latest_observation_present: bool,
  pub identity_assessment: IdentityAssessment,
  pub visibility_assessment: VisibilityAssessment,
  pub lifecycle_verdict: Option<LifecycleVerdict>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActionReadiness {
  pub ready: bool,
  pub reason: String,
  pub blocking_codes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationRequest {
  pub code: String,
  pub rationale: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneDiagnostic {
  pub code: String,
  pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneDraftAnswers {
  pub as_of_frame_id: String,
  pub track_summaries: Vec<TrackSceneSummary>,
  pub action_readiness: ActionReadiness,
  pub recommended_observations: Vec<ObservationRequest>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneStateProduct {
  pub as_of_frame_id: String,
  /// Supporting evidence only — does not drive S5a draft-answer conclusions.
  pub motion: MotionResult,
  pub coverage: CoverageView,
  pub lifecycle: Option<LifecycleVerdict>,
  pub tracks: Vec<TrackSceneSummary>,
  pub action_readiness: ActionReadiness,
  pub recommended_observations: Vec<ObservationRequest>,
  pub diagnostics: Vec<SceneDiagnostic>,
  pub draft_answers: SceneDraftAnswers,
}

#[derive(Debug, Error)]
pub enum SceneStateError {
  #[error("scene state requires a non-empty frame bundle")]
  EmptyBundle,
}

fn track_id_for_label(label: &str) -> String {
  format!("track-{label}")
}

fn label_for_track_id(track_id: &str) -> Option<&str> {
  track_id.strip_prefix("track-")
}

pub(crate) fn observations_match_bundle(
  bundle: &ScanFrameBundle,
  observations: &[Vec<FrameObservation>],
) -> bool {
  !bundle.frames.is_empty() && !observations.is_empty() && observations.len() == bundle.frames.len()
}

fn collect_labels(observations_by_frame: &[Vec<FrameObservation>]) -> Vec<String> {
  let mut labels = Vec::new();
  for frame_obs in observations_by_frame {
    for obs in frame_obs {
      if !labels.contains(&obs.label) {
        labels.push(obs.label.clone());
      }
    }
  }
  labels.sort();
  labels
}

fn last_seen_frame_id(
  bundle: &ScanFrameBundle,
  observations_by_frame: &[Vec<FrameObservation>],
  label: &str,
) -> Option<String> {
  for (index, frame_obs) in observations_by_frame.iter().enumerate().rev() {
    if frame_obs.iter().any(|obs| obs.label == label) {
      return bundle.frames.get(index).map(|frame| frame.frame_id.clone());
    }
  }
  None
}

fn associations_for_bundle(
  bundle: &ScanFrameBundle,
  observations_by_frame: &[Vec<FrameObservation>],
) -> Vec<AssociationResult> {
  if bundle.frames.len() < 2 {
    return Vec::new();
  }
  let last = bundle.frames.len() - 1;
  associate_adjacent_frames(
    &observations_by_frame[last - 1],
    &observations_by_frame[last],
  )
}

fn identity_for_track(track_id: &str, associations: &[AssociationResult]) -> IdentityAssessment {
  let label = label_for_track_id(track_id).unwrap_or("");
  for association in associations {
    match association {
      AssociationResult::Linked { track_id: tid, .. } if tid == track_id => {
        return IdentityAssessment::Linked;
      }
      AssociationResult::NewTrack { track_id: tid, .. } if tid == track_id => {
        return IdentityAssessment::NewTrack;
      }
      AssociationResult::AmbiguousAssociation {
        label: assoc_label, ..
      } if track_id_for_label(assoc_label) == track_id || assoc_label == label => {
        return IdentityAssessment::Ambiguous;
      }
      _ => {}
    }
  }
  IdentityAssessment::Unknown
}

fn visibility_for_track(
  latest_observation_present: bool,
  identity: &IdentityAssessment,
  coverage: &CoverageView,
  last_seen_frame_id: &Option<String>,
) -> VisibilityAssessment {
  if matches!(identity, IdentityAssessment::Ambiguous) {
    return VisibilityAssessment::Unknown;
  }
  if latest_observation_present {
    return VisibilityAssessment::Visible;
  }
  let stale_candidate = coverage
    .negative_evidence
    .iter()
    .any(|entry| entry.code == "no_new_observation")
    && last_seen_frame_id.is_some();
  if stale_candidate {
    return VisibilityAssessment::StaleCandidate;
  }
  VisibilityAssessment::Unknown
}

fn lifecycle_verdict_for_track(
  track_id: &str,
  lifecycle: &Option<LifecycleVerdict>,
) -> Option<LifecycleVerdict> {
  match lifecycle {
    Some(LifecycleVerdict::Reacquired { track_id: tid }) if tid == track_id => lifecycle.clone(),
    Some(LifecycleVerdict::Lost { track_id: tid }) if tid == track_id => lifecycle.clone(),
    Some(LifecycleVerdict::AmbiguousReacquire { track_id: tid }) if tid == track_id => {
      lifecycle.clone()
    }
    Some(LifecycleVerdict::ObservationFailed { .. }) => lifecycle.clone(),
    _ => None,
  }
}

fn evaluate_lifecycle_optional(
  events: &Option<Vec<LifecycleEvent>>,
) -> (Option<LifecycleVerdict>, Option<String>) {
  let Some(stream) = events else {
    return (None, None);
  };
  if stream.is_empty() {
    return (None, Some("lifecycle_incomplete".into()));
  }
  match evaluate_lifecycle(stream) {
    Ok(verdict) => (Some(verdict), None),
    Err(LifecycleError::MissingEvidence { .. }) => {
      (None, Some("lifecycle_missing_evidence".into()))
    }
    Err(LifecycleError::EmptyEvents) => (None, Some("lifecycle_incomplete".into())),
  }
}

fn push_unique(codes: &mut Vec<String>, code: &str) {
  if !codes.iter().any(|existing| existing == code) {
    codes.push(code.to_string());
  }
}

fn collect_blocking_codes(
  observations_valid: bool,
  coverage: &CoverageView,
  lifecycle: &Option<LifecycleVerdict>,
  lifecycle_input_error: &Option<String>,
) -> Vec<String> {
  let mut codes = Vec::new();
  if !observations_valid {
    push_unique(&mut codes, "missing_observations");
    return codes;
  }
  for uncertainty in &coverage.open_uncertainty_codes {
    if uncertainty == "ambiguous_association" {
      push_unique(&mut codes, "ambiguous_association");
    }
  }
  for negative in &coverage.negative_evidence {
    if negative.code == "no_new_observation" {
      push_unique(&mut codes, "no_new_observation");
    }
  }
  if let Some(code) = lifecycle_input_error {
    push_unique(&mut codes, code);
  }
  if let Some(verdict) = lifecycle {
    match verdict {
      LifecycleVerdict::Lost { .. } => push_unique(&mut codes, "lifecycle_lost"),
      LifecycleVerdict::ObservationFailed { .. } => {
        push_unique(&mut codes, "lifecycle_observation_failed")
      }
      LifecycleVerdict::Incomplete => push_unique(&mut codes, "lifecycle_incomplete"),
      LifecycleVerdict::Reacquired { .. } | LifecycleVerdict::AmbiguousReacquire { .. } => {}
    }
  }
  codes
}

fn recommended_observations_for_codes(blocking_codes: &[String]) -> Vec<ObservationRequest> {
  let mut requests = Vec::new();
  for code in blocking_codes {
    let request = match code.as_str() {
      "ambiguous_association" => Some(ObservationRequest {
        code: "disambiguate_label".into(),
        rationale: "collect distinguishing observation for duplicate label".into(),
      }),
      "no_new_observation" => Some(ObservationRequest {
        code: "rescan_after_motion".into(),
        rationale: "capture frame after viewport motion".into(),
      }),
      "missing_observations" => Some(ObservationRequest {
        code: "supply_observations".into(),
        rationale: "provide per-frame observations matching bundle length".into(),
      }),
      "lifecycle_missing_evidence" => Some(ObservationRequest {
        code: "fix_lifecycle_evidence".into(),
        rationale: "repair lifecycle event transition evidence".into(),
      }),
      "lifecycle_lost" => Some(ObservationRequest {
        code: "stop_or_reacquire".into(),
        rationale: "target lost at domain layer; reacquire not in S5a".into(),
      }),
      "lifecycle_observation_failed" => Some(ObservationRequest {
        code: "retry_capture".into(),
        rationale: "infra observation failure".into(),
      }),
      "lifecycle_incomplete" => Some(ObservationRequest {
        code: "complete_lifecycle_chain".into(),
        rationale: "lifecycle stream lacks terminal verdict".into(),
      }),
      _ => None,
    };
    if let Some(entry) = request {
      if !requests
        .iter()
        .any(|existing: &ObservationRequest| existing.code == entry.code)
      {
        requests.push(entry);
      }
    }
  }
  requests
}

fn build_track_summaries(
  bundle: &ScanFrameBundle,
  observations_by_frame: &[Vec<FrameObservation>],
  as_of_frame_id: &str,
  associations: &[AssociationResult],
  coverage: &CoverageView,
  lifecycle: &Option<LifecycleVerdict>,
) -> Vec<TrackSceneSummary> {
  let labels = collect_labels(observations_by_frame);
  labels
    .into_iter()
    .map(|label| {
      let track_id = track_id_for_label(&label);
      let last_seen = last_seen_frame_id(bundle, observations_by_frame, &label);
      let latest_observation_present = last_seen.as_deref() == Some(as_of_frame_id);
      let identity_assessment = identity_for_track(&track_id, associations);
      let visibility_assessment = visibility_for_track(
        latest_observation_present,
        &identity_assessment,
        coverage,
        &last_seen,
      );
      let lifecycle_verdict = lifecycle_verdict_for_track(&track_id, lifecycle);
      TrackSceneSummary {
        track_id,
        last_seen_frame_id: last_seen,
        latest_observation_present,
        identity_assessment,
        visibility_assessment,
        lifecycle_verdict,
      }
    })
    .collect()
}

/// Build an in-memory scene state product from frames, observations, and optional lifecycle events.
pub fn build_scene_state_product(
  input: &SceneStateInput,
) -> Result<SceneStateProduct, SceneStateError> {
  if input.bundle.frames.is_empty() {
    return Err(SceneStateError::EmptyBundle);
  }

  let as_of_frame_id = input
    .bundle
    .frames
    .last()
    .expect("non-empty bundle")
    .frame_id
    .clone();

  // NOTICE(scan-s5a): motion exposed for context; not used in readiness / visibility / presence.
  let motion = estimate_viewport_motion(&input.bundle).unwrap_or_else(|error| match error {
    crate::motion::MotionError::InsufficientFrames { found } => {
      MotionResult::Unknown(MotionUnknown {
        code: "motion_unknown".into(),
        message: format!(
          "scene state accepted with supporting-evidence-only motion; found {found} frame(s)"
        ),
      })
    }
  });

  let observations_valid = observations_match_bundle(&input.bundle, &input.observations_by_frame);
  let associations = if observations_valid {
    associations_for_bundle(&input.bundle, &input.observations_by_frame)
  } else {
    Vec::new()
  };
  let coverage = build_coverage_view(&input.bundle, &associations);
  let (lifecycle, lifecycle_input_error) = evaluate_lifecycle_optional(&input.lifecycle_events);

  let blocking_codes = collect_blocking_codes(
    observations_valid,
    &coverage,
    &lifecycle,
    &lifecycle_input_error,
  );
  let ready = blocking_codes.is_empty();
  let reason = if ready {
    "no blocking codes".into()
  } else {
    format!("blocking: {}", blocking_codes.join(", "))
  };
  let action_readiness = ActionReadiness {
    ready,
    reason,
    blocking_codes: blocking_codes.clone(),
  };
  let recommended_observations = recommended_observations_for_codes(&blocking_codes);

  let tracks = if observations_valid {
    build_track_summaries(
      &input.bundle,
      &input.observations_by_frame,
      &as_of_frame_id,
      &associations,
      &coverage,
      &lifecycle,
    )
  } else {
    Vec::new()
  };

  let draft_answers = SceneDraftAnswers {
    as_of_frame_id: as_of_frame_id.clone(),
    track_summaries: tracks.clone(),
    action_readiness: action_readiness.clone(),
    recommended_observations: recommended_observations.clone(),
  };

  let diagnostics = blocking_codes
    .iter()
    .map(|code| SceneDiagnostic {
      code: code.clone(),
      message: format!("blocking code active: {code}"),
    })
    .collect();

  Ok(SceneStateProduct {
    as_of_frame_id,
    motion,
    coverage,
    lifecycle,
    tracks,
    action_readiness,
    recommended_observations,
    diagnostics,
    draft_answers,
  })
}

/// Metadata-only L2 summary (no IO). Full consumption surface uses [`crate::format_scene_state_inspect_text`].
pub fn summarize_scene_state_text(product: &SceneStateProduct) -> String {
  let recommended = product
    .recommended_observations
    .iter()
    .map(|req| req.code.as_str())
    .collect::<Vec<_>>()
    .join(",");
  let mut lines = vec![
    format!("as_of_frame_id={}", product.as_of_frame_id),
    format!(
      "action_ready={} blocking={:?}",
      product.action_readiness.ready, product.action_readiness.blocking_codes
    ),
    format!("tracks={}", product.tracks.len()),
    format!("recommended=[{recommended}]"),
  ];
  for track in &product.tracks {
    lines.push(format!(
      "track_id={} last_seen={:?} latest_present={} identity={:?} visibility={:?}",
      track.track_id,
      track.last_seen_frame_id,
      track.latest_observation_present,
      track.identity_assessment,
      track.visibility_assessment,
    ));
  }
  lines.join("\n")
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::producer::{produce_frame_from_fixture_dir, produce_frames_from_fixture_dir};
  use crate::reader::load_scan_frames_from_dir;
  use crate::scene_fixture_support::{SceneFixture, load_scene_fixture, scene_input_from_fixture};
  use std::path::PathBuf;

  fn build_from_scene_fixture(scenario_dir: &str) -> SceneStateProduct {
    let input = scene_input_from_fixture(scenario_dir);
    build_scene_state_product(&input).expect("build scene state")
  }

  fn parse_identity(value: &str) -> IdentityAssessment {
    match value {
      "linked" => IdentityAssessment::Linked,
      "new_track" => IdentityAssessment::NewTrack,
      "ambiguous" => IdentityAssessment::Ambiguous,
      "unknown" => IdentityAssessment::Unknown,
      other => panic!("unknown identity expect: {other}"),
    }
  }

  fn parse_visibility(value: &str) -> VisibilityAssessment {
    match value {
      "visible" => VisibilityAssessment::Visible,
      "stale_candidate" => VisibilityAssessment::StaleCandidate,
      "unknown" => VisibilityAssessment::Unknown,
      other => panic!("unknown visibility expect: {other}"),
    }
  }

  fn assert_scene_expect(fixture: &SceneFixture, product: &SceneStateProduct) {
    let expect = &fixture.expect;
    if let Some(as_of) = &expect.as_of_frame_id {
      assert_eq!(product.as_of_frame_id, *as_of);
    }
    if let Some(ready) = expect.action_ready {
      assert_eq!(product.action_readiness.ready, ready);
    }
    if let Some(codes) = &expect.blocking_codes {
      assert_eq!(product.action_readiness.blocking_codes, *codes);
    }
    if let Some(code) = &expect.lifecycle_blocking {
      assert!(
        product
          .action_readiness
          .blocking_codes
          .iter()
          .any(|c| c == code)
      );
    }
    if expect.identity.is_some()
      || expect.last_seen_frame_id.is_some()
      || expect.latest_observation_present.is_some()
      || expect.visibility.is_some()
    {
      let track = product
        .tracks
        .first()
        .unwrap_or_else(|| panic!("expected track for {}", fixture.scenario));
      if let Some(identity) = &expect.identity {
        assert_eq!(track.identity_assessment, parse_identity(identity));
      }
      if let Some(last_seen) = &expect.last_seen_frame_id {
        assert_eq!(
          track.last_seen_frame_id.as_deref(),
          Some(last_seen.as_str())
        );
      }
      if let Some(present) = expect.latest_observation_present {
        assert_eq!(track.latest_observation_present, present);
      }
      if let Some(visibility) = &expect.visibility {
        assert_eq!(track.visibility_assessment, parse_visibility(visibility));
      }
    }
  }

  #[test]
  fn scene_stable_fixture() {
    let fixture = load_scene_fixture("scene_stable_v0");
    let product = build_from_scene_fixture("scene_stable_v0");
    assert_scene_expect(&fixture, &product);
    assert!(product.recommended_observations.is_empty());
    assert!(summarize_scene_state_text(&product).contains("as_of_frame_id=frame-0002"));
  }

  #[test]
  fn scene_stale_fixture() {
    let fixture = load_scene_fixture("scene_stale_v0");
    let product = build_from_scene_fixture("scene_stale_v0");
    assert_scene_expect(&fixture, &product);
    assert!(
      product
        .recommended_observations
        .iter()
        .any(|req| req.code == "rescan_after_motion")
    );
  }

  #[test]
  fn scene_ambiguous_fixture() {
    let fixture = load_scene_fixture("scene_ambiguous_v0");
    let product = build_from_scene_fixture("scene_ambiguous_v0");
    assert_scene_expect(&fixture, &product);
    assert!(
      product
        .recommended_observations
        .iter()
        .any(|req| req.code == "disambiguate_label")
    );
  }

  #[test]
  fn scene_lost_fixture() {
    let fixture = load_scene_fixture("scene_lost_v0");
    let product = build_from_scene_fixture("scene_lost_v0");
    assert_scene_expect(&fixture, &product);
    assert!(matches!(
      product.lifecycle,
      Some(LifecycleVerdict::Lost { .. })
    ));
  }

  #[test]
  fn scene_missing_observations_fixture() {
    let fixture = load_scene_fixture("scene_missing_observations_v0");
    let product = build_from_scene_fixture("scene_missing_observations_v0");
    assert_scene_expect(&fixture, &product);
    assert!(product.tracks.is_empty());
  }

  #[test]
  fn scene_lifecycle_bad_evidence_fixture() {
    let fixture = load_scene_fixture("scene_lifecycle_bad_evidence_v0");
    let product = build_from_scene_fixture("scene_lifecycle_bad_evidence_v0");
    assert_scene_expect(&fixture, &product);
    assert!(product.lifecycle.is_none());
  }

  #[test]
  fn scene_single_frame_degrades_motion_to_unknown() {
    let fixture_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("tests/fixtures/scan/temporal/single_frame_v0");
    let out_dir = std::env::temp_dir().join(format!(
      "auv-scan-scene-single-frame-{}",
      std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out_dir);
    produce_frame_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    assert_eq!(bundle.frames.len(), 1);
    let input = SceneStateInput {
      bundle,
      observations_by_frame: vec![vec![FrameObservation {
        observation_id: "o0".into(),
        label: "widget".into(),
      }]],
      lifecycle_events: None,
    };
    let product = build_scene_state_product(&input).expect("single frame scene");
    assert!(matches!(
      product.motion,
      MotionResult::Unknown(MotionUnknown { ref code, .. }) if code == "motion_unknown"
    ));
    let _ = std::fs::remove_dir_all(&out_dir);
  }

  #[test]
  fn scene_track_summary_carries_matching_lifecycle_verdict() {
    let product = build_from_scene_fixture("scene_lost_v0");
    let track = product.tracks.first().expect("track");
    assert!(matches!(
      track.lifecycle_verdict,
      Some(LifecycleVerdict::Lost { ref track_id }) if track_id == "track-widget"
    ));
  }

  #[test]
  fn scene_empty_lifecycle_stream_blocks_as_incomplete() {
    let fixture_dir =
      PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scan/temporal/two_frame_v0");
    let out_dir = std::env::temp_dir().join(format!(
      "auv-scan-scene-empty-lifecycle-{}",
      std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&out_dir);
    produce_frames_from_fixture_dir(&fixture_dir, &out_dir).expect("produce");
    let bundle = load_scan_frames_from_dir(&out_dir).expect("load");
    let input = SceneStateInput {
      bundle,
      observations_by_frame: vec![
        vec![FrameObservation {
          observation_id: "o0".into(),
          label: "widget".into(),
        }],
        vec![FrameObservation {
          observation_id: "o1".into(),
          label: "widget".into(),
        }],
      ],
      lifecycle_events: Some(Vec::new()),
    };
    let product = build_scene_state_product(&input).expect("scene state");
    assert!(
      product
        .action_readiness
        .blocking_codes
        .iter()
        .any(|code| code == "lifecycle_incomplete")
    );
    assert!(
      product
        .recommended_observations
        .iter()
        .any(|req| req.code == "complete_lifecycle_chain")
    );
    let _ = std::fs::remove_dir_all(&out_dir);
  }
}
