//! Typed MC-17 baseline evidence and verdict derivation.

use auv_stage_status::StageStatus;
use auv_tracing::ArtifactUri;
use serde::Serialize;

use crate::{
  BlockFace, BlockPosition, HoldoutRenderQualityVerdict, MinecraftTargetSemantics, ProjectionVisibility,
  TrainingResultHoldoutPreviewManifest, TrainingResultHoldoutRenderQualityManifest, TrainingResultSpatialQueryManifest,
  TrainingResultSpatialQueryStatus,
};

const PROFILE_ID: &str = "mc17-d2-primary-v1";
const SEMANTIC_MANIFEST: &str = ".tmp/mc10-smoke-review/semantic/minecraft-3dgs-training-result-semantic.json";
const BASIS_CHECKPOINT_SUFFIX: &str = "step-000001.ckpt";

const REFERENCE_GEOMETRY_NOTE: &str =
  "MC-12 projection_reference answers are scene-packet reference geometry only; they are not Gaussian-native inference";
const SCREENSHOT_COPY_NOTE: &str =
  "MC-17 screenshot-copy render probe measures pipeline comparability only; it is not trained-splat usefulness evidence";

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MinecraftInspectedArtifact<T> {
  pub uri: ArtifactUri,
  pub payload: T,
}

impl<T> MinecraftInspectedArtifact<T> {
  pub(crate) fn new(uri: ArtifactUri, payload: T) -> Self {
    Self { uri, payload }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityEvidenceCoverage {
  MissingStage,
  Partial,
  Complete,
}

impl QualityEvidenceCoverage {
  pub(crate) const fn as_str(self) -> &'static str {
    match self {
      Self::MissingStage => "missing_stage",
      Self::Partial => "partial",
      Self::Complete => "complete",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityStage {
  SpatialQuery,
  HoldoutWitness,
  RenderQuality,
}

impl QualityStage {
  pub(crate) const fn as_str(self) -> &'static str {
    match self {
      Self::SpatialQuery => "spatial_query",
      Self::HoldoutWitness => "holdout_witness",
      Self::RenderQuality => "render_quality",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityStageOutcome {
  Pass,
  Partial,
  Fail,
  Blocked,
}

impl QualityStageOutcome {
  pub(crate) const fn as_str(self) -> &'static str {
    match self {
      Self::Pass => "pass",
      Self::Partial => "partial",
      Self::Fail => "fail",
      Self::Blocked => "blocked",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityVerdictOutcome {
  Pass,
  Partial,
  Fail,
  Blocked,
}

impl QualityVerdictOutcome {
  pub(crate) const fn as_str(self) -> &'static str {
    match self {
      Self::Pass => "pass",
      Self::Partial => "partial",
      Self::Fail => "fail",
      Self::Blocked => "blocked",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityRenderEvidenceMode {
  ScreenshotCopyProbe,
  TrainedRender,
}

impl QualityRenderEvidenceMode {
  pub(crate) const fn as_str(self) -> &'static str {
    match self {
      Self::ScreenshotCopyProbe => "screenshot_copy_probe",
      Self::TrainedRender => "trained_render",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct QualityStageCheck {
  pub stage: QualityStage,
  pub outcome: QualityStageOutcome,
  pub reasons: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MinecraftQualityVerdict {
  pub profile_id: &'static str,
  pub render_evidence_mode: QualityRenderEvidenceMode,
  pub evidence_coverage: QualityEvidenceCoverage,
  pub quality_verdict: QualityVerdictOutcome,
  pub stage_checks: Vec<QualityStageCheck>,
  pub trust_notes: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MinecraftQualityVerdicts {
  pub probe: MinecraftQualityVerdict,
  pub trained_render: MinecraftQualityVerdict,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MinecraftQualityBaseline {
  pub profile_id: &'static str,
  pub evidence_coverage: QualityEvidenceCoverage,
  pub spatial_query: Option<MinecraftInspectedArtifact<TrainingResultSpatialQueryManifest>>,
  pub holdout_witness: Option<MinecraftInspectedArtifact<TrainingResultHoldoutPreviewManifest>>,
  pub render_quality: Option<MinecraftInspectedArtifact<TrainingResultHoldoutRenderQualityManifest>>,
  pub mismatched_stages: Vec<QualityStage>,
  pub trust_notes: Vec<String>,
  pub verdicts: MinecraftQualityVerdicts,
}

#[derive(Clone, Copy)]
struct QualityBaselineProfile {
  target_block: BlockPosition,
  target_face: Option<BlockFace>,
  target_semantics: MinecraftTargetSemantics,
  holdout_frame_index: usize,
}

#[derive(Clone, Copy)]
struct SpatialThresholds {
  required_status: TrainingResultSpatialQueryStatus,
  required_visibility: Option<ProjectionVisibility>,
}

#[derive(Clone, Copy)]
struct HoldoutThresholds {
  required_status: StageStatus,
}

#[derive(Clone, Copy)]
struct RenderThresholds {
  required_status: StageStatus,
  required_verdict: HoldoutRenderQualityVerdict,
  require_image_size_match: bool,
  l1_mean_max: Option<f64>,
  mse_max: Option<f64>,
  psnr_min: Option<f64>,
}

#[derive(Clone, Copy)]
struct VerdictThresholds {
  mode: QualityRenderEvidenceMode,
  spatial: SpatialThresholds,
  holdout: HoldoutThresholds,
  render: RenderThresholds,
  trust_notes: &'static [&'static str],
}

pub(crate) fn derive_quality_baseline(
  spatial_queries: &[MinecraftInspectedArtifact<TrainingResultSpatialQueryManifest>],
  holdout_previews: &[MinecraftInspectedArtifact<TrainingResultHoldoutPreviewManifest>],
  render_quality: &[MinecraftInspectedArtifact<TrainingResultHoldoutRenderQualityManifest>],
) -> MinecraftQualityBaseline {
  let profile = QualityBaselineProfile {
    target_block: BlockPosition::new(511, 73, 728),
    target_face: Some(BlockFace::North),
    target_semantics: MinecraftTargetSemantics::HitFaceCenter,
    holdout_frame_index: 6,
  };

  let exact_spatial_query = spatial_queries.iter().find(|artifact| spatial_query_matches_profile(&artifact.payload, profile)).cloned();
  let spatial_query = exact_spatial_query.or_else(|| {
    spatial_queries.iter().find(|artifact| artifact.payload.training_result_semantic_manifest_path == SEMANTIC_MANIFEST).cloned()
  });
  let exact_holdout_witness = holdout_previews.iter().find(|artifact| holdout_matches_profile(&artifact.payload, profile)).cloned();
  let holdout_witness = exact_holdout_witness.or_else(|| {
    holdout_previews.iter().find(|artifact| artifact.payload.training_result_semantic_manifest_path == SEMANTIC_MANIFEST).cloned()
  });
  let exact_render = render_quality.iter().find(|artifact| render_matches_profile(&artifact.payload, profile)).cloned();
  let selected_render_quality = exact_render.or_else(|| {
    render_quality.iter().find(|artifact| artifact.payload.training_result_semantic_manifest_path == SEMANTIC_MANIFEST).cloned()
  });

  let mut mismatched_stages = Vec::new();
  if spatial_query.as_ref().is_some_and(|artifact| !spatial_query_matches_profile(&artifact.payload, profile))
    || (spatial_query.is_none()
      && spatial_queries.iter().any(|artifact| spatial_query_matches_profile_except_semantic_manifest(&artifact.payload, profile)))
  {
    mismatched_stages.push(QualityStage::SpatialQuery);
  }
  if holdout_witness.as_ref().is_some_and(|artifact| !holdout_matches_profile(&artifact.payload, profile))
    || (holdout_witness.is_none()
      && holdout_previews.iter().any(|artifact| holdout_matches_profile_except_semantic_manifest(&artifact.payload, profile)))
  {
    mismatched_stages.push(QualityStage::HoldoutWitness);
  }
  if selected_render_quality.as_ref().is_some_and(|artifact| !render_matches_profile(&artifact.payload, profile))
    || (selected_render_quality.is_none()
      && render_quality.iter().any(|artifact| render_matches_profile_except_semantic_manifest(&artifact.payload, profile)))
  {
    mismatched_stages.push(QualityStage::RenderQuality);
  }

  let stage_count =
    usize::from(spatial_query.is_some()) + usize::from(holdout_witness.is_some()) + usize::from(selected_render_quality.is_some());
  let evidence_coverage = if stage_count == 0 && mismatched_stages.is_empty() {
    QualityEvidenceCoverage::MissingStage
  } else if stage_count == 3 && mismatched_stages.is_empty() {
    QualityEvidenceCoverage::Complete
  } else {
    QualityEvidenceCoverage::Partial
  };
  let trust_notes = build_trust_notes(selected_render_quality.as_ref().map(|artifact| &artifact.payload));

  let mut baseline = MinecraftQualityBaseline {
    profile_id: PROFILE_ID,
    evidence_coverage,
    spatial_query,
    holdout_witness,
    render_quality: selected_render_quality,
    mismatched_stages,
    trust_notes,
    verdicts: MinecraftQualityVerdicts {
      probe: empty_verdict(QualityRenderEvidenceMode::ScreenshotCopyProbe, evidence_coverage),
      trained_render: empty_verdict(QualityRenderEvidenceMode::TrainedRender, evidence_coverage),
    },
  };
  baseline.verdicts = MinecraftQualityVerdicts {
    probe: derive_quality_verdict(&baseline, probe_thresholds()),
    trained_render: derive_quality_verdict(&baseline, trained_render_thresholds()),
  };
  baseline
}

fn spatial_query_matches_profile(manifest: &TrainingResultSpatialQueryManifest, profile: QualityBaselineProfile) -> bool {
  manifest.training_result_semantic_manifest_path == SEMANTIC_MANIFEST
    && spatial_query_matches_profile_except_semantic_manifest(manifest, profile)
}

fn spatial_query_matches_profile_except_semantic_manifest(
  manifest: &TrainingResultSpatialQueryManifest,
  profile: QualityBaselineProfile,
) -> bool {
  manifest.target_block == profile.target_block
    && manifest.target_face == profile.target_face
    && manifest.target_semantics == profile.target_semantics
}

fn holdout_matches_profile(manifest: &TrainingResultHoldoutPreviewManifest, profile: QualityBaselineProfile) -> bool {
  manifest.training_result_semantic_manifest_path == SEMANTIC_MANIFEST && holdout_matches_profile_except_semantic_manifest(manifest, profile)
}

fn holdout_matches_profile_except_semantic_manifest(
  manifest: &TrainingResultHoldoutPreviewManifest,
  profile: QualityBaselineProfile,
) -> bool {
  manifest.holdout_frame_index == profile.holdout_frame_index
    && manifest.basis_checkpoint_path.as_deref().is_some_and(|path| path.ends_with(BASIS_CHECKPOINT_SUFFIX))
}

fn render_matches_profile(manifest: &TrainingResultHoldoutRenderQualityManifest, profile: QualityBaselineProfile) -> bool {
  manifest.training_result_semantic_manifest_path == SEMANTIC_MANIFEST && render_matches_profile_except_semantic_manifest(manifest, profile)
}

fn render_matches_profile_except_semantic_manifest(
  manifest: &TrainingResultHoldoutRenderQualityManifest,
  profile: QualityBaselineProfile,
) -> bool {
  manifest.holdout_frame_index == profile.holdout_frame_index
    && manifest.basis_checkpoint_path.as_deref().is_some_and(|path| path.ends_with(BASIS_CHECKPOINT_SUFFIX))
}

fn build_trust_notes(render_quality: Option<&TrainingResultHoldoutRenderQualityManifest>) -> Vec<String> {
  let mut notes = vec![
    REFERENCE_GEOMETRY_NOTE.to_string(),
    SCREENSHOT_COPY_NOTE.to_string(),
  ];
  if let Some(render_quality) = render_quality {
    for limit in &render_quality.known_limits {
      if !notes.contains(limit) {
        notes.push(limit.clone());
      }
    }
  }
  notes
}

fn empty_verdict(mode: QualityRenderEvidenceMode, evidence_coverage: QualityEvidenceCoverage) -> MinecraftQualityVerdict {
  MinecraftQualityVerdict {
    profile_id: PROFILE_ID,
    render_evidence_mode: mode,
    evidence_coverage,
    quality_verdict: QualityVerdictOutcome::Blocked,
    stage_checks: Vec::new(),
    trust_notes: Vec::new(),
  }
}

fn probe_thresholds() -> VerdictThresholds {
  VerdictThresholds {
    mode: QualityRenderEvidenceMode::ScreenshotCopyProbe,
    spatial: SpatialThresholds {
      required_status: TrainingResultSpatialQueryStatus::Answered,
      required_visibility: Some(ProjectionVisibility::Visible),
    },
    holdout: HoldoutThresholds {
      required_status: StageStatus::Ready,
    },
    render: RenderThresholds {
      required_status: StageStatus::Ready,
      required_verdict: HoldoutRenderQualityVerdict::MeasuredOnly,
      require_image_size_match: true,
      l1_mean_max: Some(0.001),
      mse_max: Some(0.001),
      psnr_min: None,
    },
    trust_notes: &["screenshot_copy_probe thresholds judge pipeline wiring only, not splat usefulness"],
  }
}

fn trained_render_thresholds() -> VerdictThresholds {
  VerdictThresholds {
    mode: QualityRenderEvidenceMode::TrainedRender,
    spatial: SpatialThresholds {
      required_status: TrainingResultSpatialQueryStatus::Answered,
      required_visibility: Some(ProjectionVisibility::Visible),
    },
    holdout: HoldoutThresholds {
      required_status: StageStatus::Ready,
    },
    render: RenderThresholds {
      required_status: StageStatus::Ready,
      required_verdict: HoldoutRenderQualityVerdict::MeasuredOnly,
      require_image_size_match: true,
      l1_mean_max: Some(0.05),
      mse_max: Some(0.01),
      psnr_min: Some(20.0),
    },
    trust_notes: &[
      "trained_render thresholds are provisional v1 photometric gates; tune only via fixture revision plus new closure, not inline code",
      "pass under trained_render profile means metrics met pre-committed bounds, not downstream action eligibility",
    ],
  }
}

fn derive_quality_verdict(baseline: &MinecraftQualityBaseline, thresholds: VerdictThresholds) -> MinecraftQualityVerdict {
  let stage_checks = vec![
    check_spatial_stage(baseline.spatial_query.as_ref().map(|artifact| &artifact.payload), thresholds.spatial),
    check_holdout_stage(baseline.holdout_witness.as_ref().map(|artifact| &artifact.payload), thresholds.holdout),
    check_render_stage(baseline.render_quality.as_ref().map(|artifact| &artifact.payload), thresholds.render),
  ];
  let quality_verdict = aggregate_quality_verdict(baseline, &stage_checks);
  let mut trust_notes = baseline.trust_notes.clone();
  for note in thresholds.trust_notes {
    if !trust_notes.iter().any(|existing| existing == note) {
      trust_notes.push((*note).to_string());
    }
  }
  MinecraftQualityVerdict {
    profile_id: PROFILE_ID,
    render_evidence_mode: thresholds.mode,
    evidence_coverage: baseline.evidence_coverage,
    quality_verdict,
    stage_checks,
    trust_notes,
  }
}

fn check_spatial_stage(evidence: Option<&TrainingResultSpatialQueryManifest>, thresholds: SpatialThresholds) -> QualityStageCheck {
  let Some(evidence) = evidence else {
    return blocked_check(QualityStage::SpatialQuery, "spatial query evidence missing");
  };
  if evidence.status == TrainingResultSpatialQueryStatus::Blocked {
    return blocked_check(QualityStage::SpatialQuery, "status=blocked blocks threshold evaluation");
  }
  let mut reasons = Vec::new();
  if evidence.status != thresholds.required_status {
    reasons.push(format!("status={} expected required_status={}", evidence.status.as_str(), thresholds.required_status.as_str()));
  }
  if let Some(required_visibility) = thresholds.required_visibility {
    match evidence.visibility {
      Some(visibility) if visibility == required_visibility => {}
      Some(visibility) => reasons.push(format!(
        "visibility={} expected required_visibility={}",
        visibility_label(visibility),
        visibility_label(required_visibility)
      )),
      None => reasons.push(format!("visibility missing expected required_visibility={}", visibility_label(required_visibility))),
    }
  }
  completed_check(QualityStage::SpatialQuery, reasons)
}

fn check_holdout_stage(evidence: Option<&TrainingResultHoldoutPreviewManifest>, thresholds: HoldoutThresholds) -> QualityStageCheck {
  let Some(evidence) = evidence else {
    return blocked_check(QualityStage::HoldoutWitness, "holdout witness evidence missing");
  };
  if evidence.status == StageStatus::Blocked {
    return blocked_check(QualityStage::HoldoutWitness, "status=blocked blocks threshold evaluation");
  }
  let reasons = if evidence.status == thresholds.required_status {
    Vec::new()
  } else {
    vec![format!(
      "status={} expected required_status={}",
      evidence.status.as_str(),
      thresholds.required_status.as_str()
    )]
  };
  completed_check(QualityStage::HoldoutWitness, reasons)
}

fn check_render_stage(evidence: Option<&TrainingResultHoldoutRenderQualityManifest>, thresholds: RenderThresholds) -> QualityStageCheck {
  let Some(evidence) = evidence else {
    return blocked_check(QualityStage::RenderQuality, "render quality evidence missing");
  };
  if evidence.status == StageStatus::Blocked {
    return blocked_check(QualityStage::RenderQuality, "status=blocked blocks threshold evaluation");
  }
  if evidence.verdict == HoldoutRenderQualityVerdict::MetricPartial {
    return QualityStageCheck {
      stage: QualityStage::RenderQuality,
      outcome: QualityStageOutcome::Partial,
      reasons: vec!["verdict=metric_partial records incomplete photometric evidence".to_string()],
    };
  }

  let mut reasons = Vec::new();
  if evidence.status != thresholds.required_status {
    reasons.push(format!("status={} expected required_status={}", evidence.status.as_str(), thresholds.required_status.as_str()));
  }
  if evidence.verdict != thresholds.required_verdict {
    reasons.push(format!("verdict={} expected required_verdict={}", evidence.verdict.as_str(), thresholds.required_verdict.as_str()));
  }
  if thresholds.require_image_size_match && !evidence.image_size_match {
    reasons.push("image_size_match=false expected true".to_string());
  }
  let metrics = evidence.metrics.as_ref();
  check_max_metric(&mut reasons, "l1_mean", metrics.and_then(|value| value.l1_mean), thresholds.l1_mean_max);
  check_max_metric(&mut reasons, "mse", metrics.and_then(|value| value.mse), thresholds.mse_max);
  check_min_metric(&mut reasons, "psnr", metrics.and_then(|value| value.psnr), thresholds.psnr_min);
  completed_check(QualityStage::RenderQuality, reasons)
}

fn check_max_metric(reasons: &mut Vec<String>, name: &str, value: Option<f64>, maximum: Option<f64>) {
  let Some(maximum) = maximum else {
    return;
  };
  match value {
    Some(value) if value <= maximum => {}
    Some(value) => reasons.push(format!("{name}={value} exceeds {name}_max={maximum}")),
    None => reasons.push(format!("{name} missing for threshold evaluation")),
  }
}

fn check_min_metric(reasons: &mut Vec<String>, name: &str, value: Option<f64>, minimum: Option<f64>) {
  let Some(minimum) = minimum else {
    return;
  };
  match value {
    Some(value) if value >= minimum => {}
    Some(value) => reasons.push(format!("{name}={value} below {name}_min={minimum}")),
    None => reasons.push(format!("{name} missing for threshold evaluation")),
  }
}

fn blocked_check(stage: QualityStage, reason: &str) -> QualityStageCheck {
  QualityStageCheck {
    stage,
    outcome: QualityStageOutcome::Blocked,
    reasons: vec![reason.to_string()],
  }
}

fn completed_check(stage: QualityStage, reasons: Vec<String>) -> QualityStageCheck {
  QualityStageCheck {
    stage,
    outcome: if reasons.is_empty() {
      QualityStageOutcome::Pass
    } else {
      QualityStageOutcome::Fail
    },
    reasons,
  }
}

fn aggregate_quality_verdict(baseline: &MinecraftQualityBaseline, stage_checks: &[QualityStageCheck]) -> QualityVerdictOutcome {
  if baseline.evidence_coverage != QualityEvidenceCoverage::Complete || !baseline.mismatched_stages.is_empty() {
    return QualityVerdictOutcome::Blocked;
  }
  if stage_checks.iter().any(|check| check.outcome == QualityStageOutcome::Blocked) {
    return QualityVerdictOutcome::Blocked;
  }
  if stage_checks.iter().all(|check| check.outcome == QualityStageOutcome::Pass) {
    return QualityVerdictOutcome::Pass;
  }
  if stage_checks.iter().any(|check| {
    check.outcome == QualityStageOutcome::Fail
      && check
        .reasons
        .iter()
        .any(|reason| reason.contains("exceeds l1_mean_max=") || reason.contains("exceeds mse_max=") || reason.contains("below psnr_min="))
  }) {
    return QualityVerdictOutcome::Fail;
  }
  if stage_checks.iter().any(|check| matches!(check.outcome, QualityStageOutcome::Partial | QualityStageOutcome::Fail)) {
    return QualityVerdictOutcome::Partial;
  }
  QualityVerdictOutcome::Blocked
}

fn visibility_label(visibility: ProjectionVisibility) -> &'static str {
  match visibility {
    ProjectionVisibility::Visible => "visible",
    ProjectionVisibility::BehindCamera => "behind_camera",
    ProjectionVisibility::OutOfFrustum => "out_of_frustum",
    ProjectionVisibility::OutsideWindow => "outside_window",
  }
}

#[cfg(test)]
mod tests {
  use auv_tracing::{ArtifactId, ArtifactUri, RunId};
  use serde::de::DeserializeOwned;
  use serde_json::json;

  use super::*;

  #[test]
  fn spatial_profile_mismatch_is_retained_and_blocks_verdicts() {
    let mut spatial = sample_spatial_query();
    spatial.target_block = BlockPosition::new(512, 73, 728);

    let baseline = derive_quality_baseline(&[inspected(spatial)], &[inspected(sample_holdout())], &[inspected(sample_render_quality())]);

    assert!(baseline.spatial_query.is_some());
    assert_eq!(baseline.mismatched_stages, [QualityStage::SpatialQuery]);
    assert_eq!(baseline.evidence_coverage, QualityEvidenceCoverage::Partial);
    assert_eq!(baseline.verdicts.probe.quality_verdict, QualityVerdictOutcome::Blocked);
    assert_eq!(baseline.verdicts.trained_render.quality_verdict, QualityVerdictOutcome::Blocked);
  }

  #[test]
  fn holdout_profile_mismatch_is_retained_and_blocks_verdicts() {
    let mut holdout = sample_holdout();
    holdout.holdout_frame_index = 7;

    let baseline =
      derive_quality_baseline(&[inspected(sample_spatial_query())], &[inspected(holdout)], &[inspected(sample_render_quality())]);

    assert!(baseline.holdout_witness.is_some());
    assert_eq!(baseline.mismatched_stages, [QualityStage::HoldoutWitness]);
    assert_eq!(baseline.evidence_coverage, QualityEvidenceCoverage::Partial);
    assert_eq!(baseline.verdicts.probe.quality_verdict, QualityVerdictOutcome::Blocked);
  }

  // ROOT CAUSE:
  //
  // Semantic-path-only mismatches were discarded by the path-pinned fallback selectors before mismatch diagnostics ran.
  //
  // Before the fix, those artifacts appeared missing. The fix diagnoses near-profile artifacts without selecting unrelated lineage as evidence.
  #[test]
  fn spatial_semantic_path_mismatch_is_diagnosed_without_selecting_artifact() {
    let mut spatial = sample_spatial_query();
    spatial.training_result_semantic_manifest_path = "other-semantic.json".to_string();

    let baseline = derive_quality_baseline(&[inspected(spatial)], &[inspected(sample_holdout())], &[inspected(sample_render_quality())]);

    assert!(baseline.spatial_query.is_none());
    assert_eq!(baseline.mismatched_stages, [QualityStage::SpatialQuery]);
    assert_eq!(baseline.evidence_coverage, QualityEvidenceCoverage::Partial);
    assert_eq!(baseline.verdicts.probe.quality_verdict, QualityVerdictOutcome::Blocked);
    assert_eq!(baseline.verdicts.trained_render.quality_verdict, QualityVerdictOutcome::Blocked);
  }

  #[test]
  fn holdout_semantic_path_mismatch_is_diagnosed_without_selecting_artifact() {
    let mut holdout = sample_holdout();
    holdout.training_result_semantic_manifest_path = "other-semantic.json".to_string();

    let baseline =
      derive_quality_baseline(&[inspected(sample_spatial_query())], &[inspected(holdout)], &[inspected(sample_render_quality())]);

    assert!(baseline.holdout_witness.is_none());
    assert_eq!(baseline.mismatched_stages, [QualityStage::HoldoutWitness]);
    assert_eq!(baseline.evidence_coverage, QualityEvidenceCoverage::Partial);
    assert_eq!(baseline.verdicts.probe.quality_verdict, QualityVerdictOutcome::Blocked);
    assert_eq!(baseline.verdicts.trained_render.quality_verdict, QualityVerdictOutcome::Blocked);
  }

  #[test]
  fn render_semantic_path_mismatch_is_diagnosed_without_selecting_artifact() {
    let mut render = sample_render_quality();
    render.training_result_semantic_manifest_path = "other-semantic.json".to_string();

    let baseline = derive_quality_baseline(&[inspected(sample_spatial_query())], &[inspected(sample_holdout())], &[inspected(render)]);

    assert!(baseline.render_quality.is_none());
    assert_eq!(baseline.mismatched_stages, [QualityStage::RenderQuality]);
    assert_eq!(baseline.evidence_coverage, QualityEvidenceCoverage::Partial);
    assert_eq!(baseline.verdicts.probe.quality_verdict, QualityVerdictOutcome::Blocked);
    assert_eq!(baseline.verdicts.trained_render.quality_verdict, QualityVerdictOutcome::Blocked);
  }

  #[test]
  fn all_semantic_path_mismatches_are_partial_without_selecting_unrelated_lineage() {
    let mut spatial = sample_spatial_query();
    spatial.training_result_semantic_manifest_path = "other-spatial-semantic.json".to_string();
    let mut holdout = sample_holdout();
    holdout.training_result_semantic_manifest_path = "other-holdout-semantic.json".to_string();
    let mut render = sample_render_quality();
    render.training_result_semantic_manifest_path = "other-render-semantic.json".to_string();

    let baseline = derive_quality_baseline(&[inspected(spatial)], &[inspected(holdout)], &[inspected(render)]);

    assert!(baseline.spatial_query.is_none());
    assert!(baseline.holdout_witness.is_none());
    assert!(baseline.render_quality.is_none());
    assert_eq!(
      baseline.mismatched_stages,
      [
        QualityStage::SpatialQuery,
        QualityStage::HoldoutWitness,
        QualityStage::RenderQuality
      ]
    );
    assert_eq!(baseline.evidence_coverage, QualityEvidenceCoverage::Partial);
    assert_eq!(baseline.verdicts.probe.quality_verdict, QualityVerdictOutcome::Blocked);
    assert_eq!(baseline.verdicts.trained_render.quality_verdict, QualityVerdictOutcome::Blocked);
  }

  #[test]
  fn blocked_stage_produces_blocked_quality_verdict() {
    let mut spatial = sample_spatial_query();
    spatial.status = TrainingResultSpatialQueryStatus::Blocked;

    let baseline = derive_quality_baseline(&[inspected(spatial)], &[inspected(sample_holdout())], &[inspected(sample_render_quality())]);

    assert_eq!(baseline.evidence_coverage, QualityEvidenceCoverage::Complete);
    assert_eq!(baseline.verdicts.probe.stage_checks[0].outcome, QualityStageOutcome::Blocked);
    assert_eq!(baseline.verdicts.probe.quality_verdict, QualityVerdictOutcome::Blocked);
  }

  #[test]
  fn partial_render_evidence_produces_partial_quality_verdict() {
    let mut render = sample_render_quality();
    render.verdict = HoldoutRenderQualityVerdict::MetricPartial;

    let baseline = derive_quality_baseline(&[inspected(sample_spatial_query())], &[inspected(sample_holdout())], &[inspected(render)]);

    assert_eq!(baseline.evidence_coverage, QualityEvidenceCoverage::Complete);
    assert_eq!(baseline.verdicts.probe.stage_checks[2].outcome, QualityStageOutcome::Partial);
    assert_eq!(baseline.verdicts.probe.quality_verdict, QualityVerdictOutcome::Partial);
  }

  #[test]
  fn metric_threshold_failures_produce_failed_quality_verdicts() {
    let mut render = sample_render_quality();
    let metrics = render.metrics.as_mut().expect("render metrics");
    metrics.l1_mean = Some(0.1);
    metrics.mse = Some(0.1);
    metrics.psnr = Some(10.0);

    let baseline = derive_quality_baseline(&[inspected(sample_spatial_query())], &[inspected(sample_holdout())], &[inspected(render)]);

    assert_eq!(baseline.verdicts.probe.quality_verdict, QualityVerdictOutcome::Fail);
    assert_eq!(baseline.verdicts.trained_render.quality_verdict, QualityVerdictOutcome::Fail);
    assert!(baseline.verdicts.trained_render.stage_checks[2].reasons.iter().any(|reason| reason.contains("below psnr_min=20")));
  }

  fn inspected<T>(payload: T) -> MinecraftInspectedArtifact<T> {
    MinecraftInspectedArtifact::new(ArtifactUri::from_ids(RunId::new(), ArtifactId::new()), payload)
  }

  fn sample_spatial_query() -> TrainingResultSpatialQueryManifest {
    decode(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "training_result_semantic_manifest_path": SEMANTIC_MANIFEST,
      "source_training_result_artifact_manifest_path": "result-artifacts.json",
      "source_training_result_manifest_path": "result.json",
      "source_training_job_manifest_path": "job.json",
      "source_training_launch_plan_path": "launch.json",
      "source_training_package_manifest_path": "package.json",
      "source_scene_packet_manifest_path": "scene.json",
      "source_bundle_manifest_paths": ["bundle.json"],
      "source_run_ids": ["run-source"],
      "trainer_backend": "nerfstudio",
      "job_backend": "fixture",
      "normalized_result_dir": "normalized",
      "query_kind": "block_projection",
      "target_block": {"x": 511, "y": 73, "z": 728},
      "target_face": "north",
      "target_semantics": "hit_face_center",
      "selected_backend": "projection_reference",
      "status": "answered",
      "visibility": "visible",
      "screen_point": {"x": 12.0, "y": 34.0},
      "match_radius_px": 8.0,
      "confidence": 1.0,
      "basis_frame_id": "frame-20",
      "comparison_verdict": "match",
      "known_limits": ["fixture"]
    }))
  }

  fn sample_holdout() -> TrainingResultHoldoutPreviewManifest {
    decode(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "training_result_semantic_manifest_path": SEMANTIC_MANIFEST,
      "source_training_result_artifact_manifest_path": "result-artifacts.json",
      "source_training_result_manifest_path": "result.json",
      "source_training_job_manifest_path": "job.json",
      "source_training_launch_plan_path": "launch.json",
      "source_training_package_manifest_path": "package.json",
      "source_scene_packet_manifest_path": "scene.json",
      "source_bundle_manifest_paths": ["bundle.json"],
      "source_run_ids": ["run-source"],
      "trainer_backend": "nerfstudio",
      "job_backend": "fixture",
      "normalized_result_dir": "normalized",
      "holdout_frame_index": 6,
      "basis_checkpoint_path": "checkpoints/step-000001.ckpt",
      "status": "ready",
      "known_limits": ["fixture"]
    }))
  }

  fn sample_render_quality() -> TrainingResultHoldoutRenderQualityManifest {
    decode(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "training_result_semantic_manifest_path": SEMANTIC_MANIFEST,
      "holdout_preview_manifest_path": "preview.json",
      "source_training_result_artifact_manifest_path": "result-artifacts.json",
      "source_training_result_manifest_path": "result.json",
      "source_training_job_manifest_path": "job.json",
      "source_training_launch_plan_path": "launch.json",
      "source_training_package_manifest_path": "package.json",
      "source_scene_packet_manifest_path": "scene.json",
      "source_bundle_manifest_paths": ["bundle.json"],
      "source_run_ids": ["run-source"],
      "trainer_backend": "nerfstudio",
      "job_backend": "fixture",
      "normalized_result_dir": "normalized",
      "holdout_frame_index": 6,
      "basis_checkpoint_path": "checkpoints/step-000001.ckpt",
      "render_backend": "external_command",
      "image_size_match": true,
      "metrics": {"l1_mean": 0.0, "mse": 0.0, "psnr": 30.0},
      "status": "ready",
      "verdict": "measured_only",
      "known_limits": ["fixture"]
    }))
  }

  fn decode<T: DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).expect("typed quality fixture")
  }
}
