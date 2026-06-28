// File: src/inspect.rs
//! Human-readable run inspection helpers.
//!
//! This module renders stored run snapshots (`CanonicalRun`) into a simple text
//! form (useful for CLI/debug output). It does not provide a live viewer or any
//! runtime execution logic; see `inspect_server` for the HTTP/WebSocket UI.

use crate::contract::{
  FailureLayer, ObservationSnapshot, ObservationSource, VerificationMethod, VerificationResult,
};
use crate::model::AuvResult;
use crate::run_read::{
  BalatroCardDetectionEvalWitnessInspectReportLineage,
  BalatroCardDetectionEvalWitnessManifestLineage, BalatroCardDetectionQualityInspectReportLineage,
  BalatroCardDetectionQualityManifestLineage, BalatroCardDetectionSemanticInspectReportLineage,
  BalatroCardDetectionSemanticManifestLineage,
  BalatroCardDetectionSpatialQueryInspectReportLineage,
  BalatroCardDetectionSpatialQueryManifestLineage, CandidateActionDecisionLineage,
  CandidateActionDecisionLineageStatus, CandidateActionExecutionClosureState,
  CandidateActionExecutionLineage, CandidateActionExecutionLineageStatus,
  CandidatePromotionLineage, CandidatePromotionLineageStatus, DetectorRecognitionLineage,
  MinecraftHoldoutRenderQualityInspectReportLineage, MinecraftHoldoutRenderQualityManifestLineage,
  MinecraftQueryWiredLiveActionSummary, MinecraftSpatialBundleManifestLineage,
  MinecraftTelemetrySampleArtifactLineage, MinecraftTrainingJobInspectReportLineage,
  MinecraftTrainingJobManifestLineage, MinecraftTrainingLaunchInspectReportLineage,
  MinecraftTrainingLaunchManifestLineage, MinecraftTrainingPackageInspectReportLineage,
  MinecraftTrainingPackageManifestLineage,
  MinecraftTrainingResultArtifactFetchInspectReportLineage,
  MinecraftTrainingResultArtifactFetchManifestLineage,
  MinecraftTrainingResultHoldoutPreviewInspectReportLineage,
  MinecraftTrainingResultHoldoutPreviewManifestLineage,
  MinecraftTrainingResultInspectReportLineage, MinecraftTrainingResultManifestLineage,
  MinecraftTrainingResultQualityBaselineReportSummary,
  MinecraftTrainingResultQualityVerdictSummary,
  MinecraftTrainingResultSemanticInspectReportLineage,
  MinecraftTrainingResultSemanticManifestLineage,
  MinecraftTrainingResultSpatialQueryInspectReportLineage,
  MinecraftTrainingResultSpatialQueryManifestLineage, OsuDetectionEvalQualityInspectReportLineage,
  OsuDetectionEvalQualityManifestLineage, OsuDetectionEvalWitnessInspectReportLineage,
  OsuDetectionEvalWitnessManifestLineage, OsuQueryWiredLiveActionSummary,
  OsuVisualTruthSemanticInspectReportLineage, OsuVisualTruthSemanticManifestLineage,
  OsuVisualTruthSpatialQueryInspectReportLineage, OsuVisualTruthSpatialQueryManifestLineage,
  collect_quality_baseline_evidence_for_run,
  derive_minecraft_training_result_quality_baseline_report,
  derive_minecraft_training_result_quality_verdict,
  derive_minecraft_training_result_spatial_query_action_readiness,
  derive_osu_detection_eval_quality_verdict_summary,
  derive_osu_visual_truth_spatial_query_action_readiness,
  list_balatro_card_detection_eval_witness_inspect_reports,
  list_balatro_card_detection_eval_witness_manifests,
  list_balatro_card_detection_quality_inspect_reports,
  list_balatro_card_detection_quality_manifests,
  list_balatro_card_detection_semantic_inspect_reports,
  list_balatro_card_detection_semantic_manifests,
  list_balatro_card_detection_spatial_query_inspect_reports,
  list_balatro_card_detection_spatial_query_manifests,
  list_minecraft_holdout_render_quality_inspect_reports,
  list_minecraft_holdout_render_quality_manifests, list_minecraft_projection_artifacts,
  list_minecraft_query_wired_live_action_summaries, list_minecraft_spatial_bundle_manifests,
  list_minecraft_telemetry_sample_artifacts, list_minecraft_training_job_inspect_reports,
  list_minecraft_training_job_manifests, list_minecraft_training_launch_inspect_reports,
  list_minecraft_training_launch_manifests, list_minecraft_training_package_inspect_reports,
  list_minecraft_training_package_manifests,
  list_minecraft_training_result_artifact_fetch_inspect_reports,
  list_minecraft_training_result_artifact_fetch_manifests,
  list_minecraft_training_result_holdout_preview_inspect_reports,
  list_minecraft_training_result_holdout_preview_manifests,
  list_minecraft_training_result_inspect_reports, list_minecraft_training_result_manifests,
  list_minecraft_training_result_semantic_inspect_reports,
  list_minecraft_training_result_semantic_manifests,
  list_minecraft_training_result_spatial_query_inspect_reports,
  list_minecraft_training_result_spatial_query_manifests,
  list_osu_detection_eval_quality_inspect_reports, list_osu_detection_eval_quality_manifests,
  list_osu_detection_eval_witness_inspect_reports, list_osu_detection_eval_witness_manifests,
  list_osu_query_wired_live_action_summaries, list_osu_visual_truth_semantic_inspect_reports,
  list_osu_visual_truth_semantic_manifests, list_osu_visual_truth_spatial_query_inspect_reports,
  list_osu_visual_truth_spatial_query_manifests, quality_baseline_profile_v1,
  quality_baseline_verdict_thresholds_probe_v1,
  quality_baseline_verdict_thresholds_trained_render_v1,
};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use std::collections::BTreeSet;

fn holdout_preview_manifest_matches_report(
  manifest: &crate::run_read::MinecraftTrainingResultHoldoutPreviewManifestSummary,
  report: &crate::run_read::MinecraftTrainingResultHoldoutPreviewInspectReportSummary,
) -> bool {
  report.training_result_semantic_manifest_path == manifest.training_result_semantic_manifest_path
    && report.source_training_result_artifact_manifest_path
      == manifest.source_training_result_artifact_manifest_path
    && report.source_training_result_manifest_path == manifest.source_training_result_manifest_path
    && report.source_training_job_manifest_path == manifest.source_training_job_manifest_path
    && report.source_training_launch_plan_path == manifest.source_training_launch_plan_path
    && report.source_scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
    && report.source_run_ids == manifest.source_run_ids
}

fn holdout_render_quality_manifest_matches_report(
  manifest: &crate::run_read::MinecraftHoldoutRenderQualityManifestSummary,
  report: &crate::run_read::MinecraftHoldoutRenderQualityInspectReportSummary,
) -> bool {
  report.training_result_semantic_manifest_path == manifest.training_result_semantic_manifest_path
    && report.holdout_preview_manifest_path == manifest.holdout_preview_manifest_path
    && report.source_training_result_artifact_manifest_path
      == manifest.source_training_result_artifact_manifest_path
    && report.source_training_result_manifest_path == manifest.source_training_result_manifest_path
    && report.source_training_job_manifest_path == manifest.source_training_job_manifest_path
    && report.source_scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
    && report.source_run_ids == manifest.source_run_ids
}

fn spatial_query_manifest_matches_report(
  manifest: &crate::run_read::MinecraftTrainingResultSpatialQueryManifestSummary,
  report: &crate::run_read::MinecraftTrainingResultSpatialQueryInspectReportSummary,
) -> bool {
  report.training_result_semantic_manifest_path == manifest.training_result_semantic_manifest_path
    && report.source_training_result_artifact_manifest_path
      == manifest.source_training_result_artifact_manifest_path
    && report.source_training_result_manifest_path == manifest.source_training_result_manifest_path
    && report.source_training_job_manifest_path == manifest.source_training_job_manifest_path
    && report.source_training_launch_plan_path == manifest.source_training_launch_plan_path
    && report.source_scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
    && report.source_run_ids == manifest.source_run_ids
    && report.query_kind == manifest.query_kind
    && report.target_block == manifest.target_block
    && report.target_face == manifest.target_face
    && report.target_semantics == manifest.target_semantics
}

fn unique_matching_report<'a, T>(
  reports: &'a [T],
  mut matches: impl FnMut(&'a T) -> bool,
) -> Option<&'a T> {
  let mut iter = reports.iter().filter(|report| matches(report));
  let first = iter.next()?;
  if iter.next().is_some() {
    None
  } else {
    Some(first)
  }
}

pub fn read_run(store: &LocalStore, run_id: &str) -> AuvResult<CanonicalRun> {
  crate::run_read::read_run(store, run_id)
}

pub fn list_verifications(store: &LocalStore, run_id: &str) -> AuvResult<Vec<VerificationResult>> {
  crate::run_read::list_verifications(store, run_id)
}

pub fn list_observation_snapshots(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<ObservationSnapshot>> {
  crate::run_read::list_observation_snapshots(store, run_id)
}

pub fn list_detector_recognition_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<DetectorRecognitionLineage>> {
  crate::run_read::list_detector_recognition_lineage(store, run_id)
}

pub fn list_candidate_promotion_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<CandidatePromotionLineage>> {
  crate::run_read::list_candidate_promotion_lineage(store, run_id)
}

pub fn list_candidate_action_decision_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<CandidateActionDecisionLineage>> {
  crate::run_read::list_candidate_action_decision_lineage(store, run_id)
}

pub fn list_candidate_action_execution_lineage(
  store: &LocalStore,
  run_id: &str,
) -> AuvResult<Vec<CandidateActionExecutionLineage>> {
  crate::run_read::list_candidate_action_execution_lineage(store, run_id)
}

pub fn inspect_run(store: &LocalStore, run_id: &str) -> AuvResult<String> {
  let canonical = read_run(store, run_id)?;
  let verifications = list_verifications(store, run_id)?;
  let observation_snapshots = list_observation_snapshots(store, run_id)?;
  let detector_recognition_lineage = list_detector_recognition_lineage(store, run_id)?;
  let candidate_promotion_lineage = list_candidate_promotion_lineage(store, run_id)?;
  let candidate_action_decision_lineage = list_candidate_action_decision_lineage(store, run_id)?;
  let candidate_action_execution_lineage = list_candidate_action_execution_lineage(store, run_id)?;
  let minecraft_projection_artifacts = list_minecraft_projection_artifacts(store, run_id)?;
  let minecraft_telemetry_sample_artifacts =
    list_minecraft_telemetry_sample_artifacts(store, run_id)?;
  let minecraft_spatial_bundle_manifests = list_minecraft_spatial_bundle_manifests(store, run_id)?;
  let minecraft_training_package_manifests =
    list_minecraft_training_package_manifests(store, run_id)?;
  let minecraft_training_package_inspect_reports =
    list_minecraft_training_package_inspect_reports(store, run_id)?;
  let minecraft_training_launch_manifests =
    list_minecraft_training_launch_manifests(store, run_id)?;
  let minecraft_training_launch_inspect_reports =
    list_minecraft_training_launch_inspect_reports(store, run_id)?;
  let minecraft_training_job_manifests = list_minecraft_training_job_manifests(store, run_id)?;
  let minecraft_training_job_inspect_reports =
    list_minecraft_training_job_inspect_reports(store, run_id)?;
  let minecraft_training_result_manifests =
    list_minecraft_training_result_manifests(store, run_id)?;
  let minecraft_training_result_inspect_reports =
    list_minecraft_training_result_inspect_reports(store, run_id)?;
  let minecraft_training_result_artifact_fetch_manifests =
    list_minecraft_training_result_artifact_fetch_manifests(store, run_id)?;
  let minecraft_training_result_artifact_fetch_inspect_reports =
    list_minecraft_training_result_artifact_fetch_inspect_reports(store, run_id)?;
  let minecraft_training_result_semantic_manifests =
    list_minecraft_training_result_semantic_manifests(store, run_id)?;
  let minecraft_training_result_semantic_inspect_reports =
    list_minecraft_training_result_semantic_inspect_reports(store, run_id)?;
  let minecraft_training_result_spatial_query_manifests =
    list_minecraft_training_result_spatial_query_manifests(store, run_id)?;
  let minecraft_training_result_holdout_preview_manifests =
    list_minecraft_training_result_holdout_preview_manifests(store, run_id)?;
  let minecraft_training_result_holdout_preview_inspect_reports =
    list_minecraft_training_result_holdout_preview_inspect_reports(store, run_id)?;
  let minecraft_holdout_render_quality_manifests =
    list_minecraft_holdout_render_quality_manifests(store, run_id)?;
  let minecraft_holdout_render_quality_inspect_reports =
    list_minecraft_holdout_render_quality_inspect_reports(store, run_id)?;
  let minecraft_training_result_spatial_query_inspect_reports =
    list_minecraft_training_result_spatial_query_inspect_reports(store, run_id)?;
  let osu_visual_truth_semantic_manifests =
    list_osu_visual_truth_semantic_manifests(store, run_id)?;
  let osu_visual_truth_semantic_inspect_reports =
    list_osu_visual_truth_semantic_inspect_reports(store, run_id)?;
  let osu_visual_truth_spatial_query_manifests =
    list_osu_visual_truth_spatial_query_manifests(store, run_id)?;
  let osu_visual_truth_spatial_query_inspect_reports =
    list_osu_visual_truth_spatial_query_inspect_reports(store, run_id)?;
  let minecraft_query_wired_live_action_summaries =
    list_minecraft_query_wired_live_action_summaries(store, run_id)?;
  let osu_query_wired_live_action_summaries =
    list_osu_query_wired_live_action_summaries(store, run_id)?;
  let osu_detection_eval_witness_manifests =
    list_osu_detection_eval_witness_manifests(store, run_id)?;
  let osu_detection_eval_witness_inspect_reports =
    list_osu_detection_eval_witness_inspect_reports(store, run_id)?;
  let osu_detection_eval_quality_manifests =
    list_osu_detection_eval_quality_manifests(store, run_id)?;
  let balatro_card_detection_semantic_manifests =
    list_balatro_card_detection_semantic_manifests(store, run_id)?;
  let balatro_card_detection_semantic_inspect_reports =
    list_balatro_card_detection_semantic_inspect_reports(store, run_id)?;
  let balatro_card_detection_spatial_query_manifests =
    list_balatro_card_detection_spatial_query_manifests(store, run_id)?;
  let balatro_card_detection_spatial_query_inspect_reports =
    list_balatro_card_detection_spatial_query_inspect_reports(store, run_id)?;
  let balatro_card_detection_eval_witness_manifests =
    list_balatro_card_detection_eval_witness_manifests(store, run_id)?;
  let balatro_card_detection_eval_witness_inspect_reports =
    list_balatro_card_detection_eval_witness_inspect_reports(store, run_id)?;
  let balatro_card_detection_quality_manifests =
    list_balatro_card_detection_quality_manifests(store, run_id)?;
  let balatro_card_detection_quality_inspect_reports =
    list_balatro_card_detection_quality_inspect_reports(store, run_id)?;
  let osu_detection_eval_quality_inspect_reports =
    list_osu_detection_eval_quality_inspect_reports(store, run_id)?;
  let quality_baseline_report = quality_baseline_profile_v1().ok().and_then(|profile| {
    collect_quality_baseline_evidence_for_run(store, run_id, &profile)
      .ok()
      .map(|bundle| {
        derive_minecraft_training_result_quality_baseline_report(
          &profile,
          bundle.spatial_query.as_ref(),
          bundle.holdout_preview.as_ref(),
          bundle.render_quality.as_ref(),
          &bundle.collection_issues,
        )
      })
  });
  let (quality_verdict_probe, quality_verdict_trained_render) = quality_baseline_report
    .as_ref()
    .map_or((None, None), |report| {
      let probe = quality_baseline_verdict_thresholds_probe_v1()
        .ok()
        .map(|thresholds| derive_minecraft_training_result_quality_verdict(report, &thresholds));
      let trained_render = quality_baseline_verdict_thresholds_trained_render_v1()
        .ok()
        .map(|thresholds| derive_minecraft_training_result_quality_verdict(report, &thresholds));
      (probe, trained_render)
    });
  Ok(render_run_text(
    &canonical,
    &verifications,
    &observation_snapshots,
    &detector_recognition_lineage,
    &candidate_promotion_lineage,
    &candidate_action_decision_lineage,
    &candidate_action_execution_lineage,
    &minecraft_projection_artifacts,
    &minecraft_telemetry_sample_artifacts,
    &minecraft_spatial_bundle_manifests,
    &minecraft_training_package_manifests,
    &minecraft_training_package_inspect_reports,
    &minecraft_training_launch_manifests,
    &minecraft_training_launch_inspect_reports,
    &minecraft_training_job_manifests,
    &minecraft_training_job_inspect_reports,
    &minecraft_training_result_manifests,
    &minecraft_training_result_inspect_reports,
    &minecraft_training_result_artifact_fetch_manifests,
    &minecraft_training_result_artifact_fetch_inspect_reports,
    &minecraft_training_result_semantic_manifests,
    &minecraft_training_result_semantic_inspect_reports,
    &minecraft_training_result_holdout_preview_manifests,
    &minecraft_training_result_holdout_preview_inspect_reports,
    &minecraft_holdout_render_quality_manifests,
    &minecraft_holdout_render_quality_inspect_reports,
    &minecraft_training_result_spatial_query_manifests,
    &minecraft_training_result_spatial_query_inspect_reports,
    &minecraft_query_wired_live_action_summaries,
    &osu_visual_truth_semantic_manifests,
    &osu_visual_truth_semantic_inspect_reports,
    &osu_visual_truth_spatial_query_manifests,
    &osu_visual_truth_spatial_query_inspect_reports,
    &osu_query_wired_live_action_summaries,
    &osu_detection_eval_witness_manifests,
    &osu_detection_eval_witness_inspect_reports,
    &osu_detection_eval_quality_manifests,
    &osu_detection_eval_quality_inspect_reports,
    &balatro_card_detection_semantic_manifests,
    &balatro_card_detection_semantic_inspect_reports,
    &balatro_card_detection_spatial_query_manifests,
    &balatro_card_detection_spatial_query_inspect_reports,
    &balatro_card_detection_eval_witness_manifests,
    &balatro_card_detection_eval_witness_inspect_reports,
    &balatro_card_detection_quality_manifests,
    &balatro_card_detection_quality_inspect_reports,
    quality_baseline_report.as_ref(),
    quality_verdict_probe.as_ref(),
    quality_verdict_trained_render.as_ref(),
  ))
}

fn format_quality_verdict_stage_summary(
  verdict: &MinecraftTrainingResultQualityVerdictSummary,
) -> String {
  verdict
    .stage_checks
    .iter()
    .map(|check| {
      let reason = check
        .reasons
        .first()
        .map(|value| format!(" reason={value}"))
        .unwrap_or_default();
      format!("{}={}{}", check.stage, check.outcome, reason)
    })
    .collect::<Vec<_>>()
    .join(" ")
}

fn format_quality_verdict_line(verdict: &MinecraftTrainingResultQualityVerdictSummary) -> String {
  let mut line = format!(
    "- profile_id={} render_evidence_mode={} evidence_coverage={} quality_verdict={} {} issue={}
",
    verdict.profile_id,
    verdict.render_evidence_mode,
    verdict.evidence_coverage,
    verdict.quality_verdict,
    format_quality_verdict_stage_summary(verdict),
    verdict.issue.as_deref().unwrap_or("n/a"),
  );
  if !verdict.trust_notes.is_empty() {
    line.push_str(&format!(
      "  trust_notes={}
",
      verdict.trust_notes.join(" | ")
    ));
  }
  line
}

pub fn render_run_text(
  run: &CanonicalRun,
  verifications: &[VerificationResult],
  observation_snapshots: &[ObservationSnapshot],
  detector_recognition_lineage: &[DetectorRecognitionLineage],
  candidate_promotion_lineage: &[CandidatePromotionLineage],
  candidate_action_decision_lineage: &[CandidateActionDecisionLineage],
  candidate_action_execution_lineage: &[CandidateActionExecutionLineage],
  minecraft_projection_artifacts: &[auv_game_minecraft::artifact::MinecraftProjectionArtifact],
  minecraft_telemetry_sample_artifacts: &[MinecraftTelemetrySampleArtifactLineage],
  minecraft_spatial_bundle_manifests: &[MinecraftSpatialBundleManifestLineage],
  minecraft_training_package_manifests: &[MinecraftTrainingPackageManifestLineage],
  minecraft_training_package_inspect_reports: &[MinecraftTrainingPackageInspectReportLineage],
  minecraft_training_launch_manifests: &[MinecraftTrainingLaunchManifestLineage],
  minecraft_training_launch_inspect_reports: &[MinecraftTrainingLaunchInspectReportLineage],
  minecraft_training_job_manifests: &[MinecraftTrainingJobManifestLineage],
  minecraft_training_job_inspect_reports: &[MinecraftTrainingJobInspectReportLineage],
  minecraft_training_result_manifests: &[MinecraftTrainingResultManifestLineage],
  minecraft_training_result_inspect_reports: &[MinecraftTrainingResultInspectReportLineage],
  minecraft_training_result_artifact_fetch_manifests: &[MinecraftTrainingResultArtifactFetchManifestLineage],
  minecraft_training_result_artifact_fetch_inspect_reports: &[MinecraftTrainingResultArtifactFetchInspectReportLineage],
  minecraft_training_result_semantic_manifests: &[MinecraftTrainingResultSemanticManifestLineage],
  minecraft_training_result_semantic_inspect_reports: &[MinecraftTrainingResultSemanticInspectReportLineage],
  minecraft_training_result_holdout_preview_manifests: &[MinecraftTrainingResultHoldoutPreviewManifestLineage],
  minecraft_training_result_holdout_preview_inspect_reports: &[MinecraftTrainingResultHoldoutPreviewInspectReportLineage],
  minecraft_holdout_render_quality_manifests: &[MinecraftHoldoutRenderQualityManifestLineage],
  minecraft_holdout_render_quality_inspect_reports: &[MinecraftHoldoutRenderQualityInspectReportLineage],
  minecraft_training_result_spatial_query_manifests: &[MinecraftTrainingResultSpatialQueryManifestLineage],
  minecraft_training_result_spatial_query_inspect_reports: &[MinecraftTrainingResultSpatialQueryInspectReportLineage],
  minecraft_query_wired_live_action_summaries: &[MinecraftQueryWiredLiveActionSummary],
  osu_visual_truth_semantic_manifests: &[OsuVisualTruthSemanticManifestLineage],
  osu_visual_truth_semantic_inspect_reports: &[OsuVisualTruthSemanticInspectReportLineage],
  osu_visual_truth_spatial_query_manifests: &[OsuVisualTruthSpatialQueryManifestLineage],
  osu_visual_truth_spatial_query_inspect_reports: &[OsuVisualTruthSpatialQueryInspectReportLineage],
  osu_query_wired_live_action_summaries: &[OsuQueryWiredLiveActionSummary],
  osu_detection_eval_witness_manifests: &[OsuDetectionEvalWitnessManifestLineage],
  osu_detection_eval_witness_inspect_reports: &[OsuDetectionEvalWitnessInspectReportLineage],
  osu_detection_eval_quality_manifests: &[OsuDetectionEvalQualityManifestLineage],
  osu_detection_eval_quality_inspect_reports: &[OsuDetectionEvalQualityInspectReportLineage],
  balatro_card_detection_semantic_manifests: &[BalatroCardDetectionSemanticManifestLineage],
  balatro_card_detection_semantic_inspect_reports: &[BalatroCardDetectionSemanticInspectReportLineage],
  balatro_card_detection_spatial_query_manifests: &[BalatroCardDetectionSpatialQueryManifestLineage],
  balatro_card_detection_spatial_query_inspect_reports: &[BalatroCardDetectionSpatialQueryInspectReportLineage],
  balatro_card_detection_eval_witness_manifests: &[BalatroCardDetectionEvalWitnessManifestLineage],
  balatro_card_detection_eval_witness_inspect_reports: &[BalatroCardDetectionEvalWitnessInspectReportLineage],
  balatro_card_detection_quality_manifests: &[BalatroCardDetectionQualityManifestLineage],
  balatro_card_detection_quality_inspect_reports: &[BalatroCardDetectionQualityInspectReportLineage],
  quality_baseline_report: Option<&MinecraftTrainingResultQualityBaselineReportSummary>,
  quality_verdict_probe: Option<&MinecraftTrainingResultQualityVerdictSummary>,
  quality_verdict_trained_render: Option<&MinecraftTrainingResultQualityVerdictSummary>,
) -> String {
  let mut output = format!(
    "Run {}\nType: {}\nStatus: {}\nState: {}\n",
    run.run.run_id,
    run.run.run_type.as_str(),
    run.run.status_code.as_str(),
    run.run.state.as_str()
  );
  if let Some(summary) = &run.run.summary {
    output.push_str(&format!("Summary: {summary}\n"));
  }
  if let Some(failure) = &run.run.failure {
    output.push_str(&format!("Failure: {}\n", failure.message));
  }

  output.push_str(&format!("\nSpans: {}\n", run.spans.len()));
  for span in run.spans.iter().take(20) {
    output.push_str(&format!(
      "- {} name={} parent={} status={}\n",
      span.span_id,
      span.name,
      span
        .parent_span_id
        .as_ref()
        .map(|span_id| span_id.as_str())
        .unwrap_or("n/a"),
      span.status_code.as_str()
    ));
  }
  if run.spans.len() > 20 {
    output.push_str(&format!("- … {} more\n", run.spans.len() - 20));
  }

  output.push_str(&format!("\nEvents: {}\n", run.events.len()));
  for event in run.events.iter().take(20) {
    let message = event.message.as_deref().unwrap_or("");
    output.push_str(&format!(
      "- {} span={} name={} {}\n",
      event.event_id, event.span_id, event.name, message
    ));
  }
  if run.events.len() > 20 {
    output.push_str(&format!("- … {} more\n", run.events.len() - 20));
  }

  output.push_str(&format!("\nArtifacts: {}\n", run.artifacts.len()));
  for artifact in run.artifacts.iter().take(20) {
    output.push_str(&format!(
      "- {} span={} role={} path={}\n",
      artifact.artifact_id, artifact.span_id, artifact.role, artifact.path
    ));
  }
  if run.artifacts.len() > 20 {
    output.push_str(&format!("- … {} more\n", run.artifacts.len() - 20));
  }

  let command_boundary_claims = run
    .events
    .iter()
    .filter(|event| event.name == "command.verification")
    .collect::<Vec<_>>();
  let command_known_limits = run
    .events
    .iter()
    .filter(|event| event.name == "command.known_limit")
    .collect::<Vec<_>>();
  output.push_str("\nCommand Boundary Claims:\n");
  if command_boundary_claims.is_empty() && command_known_limits.is_empty() {
    output.push_str("- none\n");
  } else {
    for event in command_boundary_claims {
      output.push_str(&format!(
        "- verification={} span={}\n",
        event.message.as_deref().unwrap_or("n/a"),
        event.span_id
      ));
    }
    for event in command_known_limits {
      output.push_str(&format!(
        "- known_limit={} span={}\n",
        event.message.as_deref().unwrap_or("n/a"),
        event.span_id
      ));
    }
  }

  output.push_str("\nVerifications:\n");
  if verifications.is_empty() {
    output.push_str("- none\n");
  } else {
    for verification in verifications {
      output.push_str(&format!(
        "- method={} executed={} state_changed={} semantic_matched={} failure_layer={} evidence={} observed_label={}\n",
        render_verification_method(&verification.method),
        verification.executed,
        verification.state_changed,
        render_optional_bool(verification.semantic_matched),
        render_failure_layer(verification.failure_layer),
        verification.evidence.len(),
        verification.observed_label.as_deref().unwrap_or("n/a")
      ));
    }
  }

  output.push_str("\nObservations:\n");
  if observation_snapshots.is_empty() {
    output.push_str("- none\n");
  } else {
    for snapshot in observation_snapshots {
      output.push_str(&format!(
        "- {} span={} source={} nodes={} evidence={} limits={}\n",
        snapshot.snapshot_id,
        snapshot.span_id,
        render_observation_source(snapshot.source),
        snapshot.nodes.len(),
        snapshot.evidence.len(),
        snapshot.known_limits.len()
      ));
    }
  }

  output.push_str("\nDetector Recognition Lineage:\n");
  if detector_recognition_lineage.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in detector_recognition_lineage {
      output.push_str(&format!(
        "- artifact={} status={} source={} model={} backend={} items={}/{} best={} projection={} capture={} limits={}\n",
        lineage.artifact.artifact_id,
        render_detector_status(&lineage.status),
        lineage
          .source
          .map(render_recognition_source)
          .unwrap_or("n/a"),
        lineage.model_id.as_deref().unwrap_or("n/a"),
        lineage.backend.as_deref().unwrap_or("n/a"),
        lineage.filtered_count.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
        lineage.all_count.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
        lineage.best_item_id.as_deref().unwrap_or("n/a"),
        lineage.runtime_projection_kind.as_deref().unwrap_or("n/a"),
        lineage
          .capture_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage.known_limits.len()
      ));
      output.push_str(&format!(
        "  evidence={} class_label_source={} provider={} issue={}\n",
        lineage.evidence_artifacts.len(),
        lineage.class_label_source_kind.as_deref().unwrap_or("n/a"),
        lineage.execution_provider.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a")
      ));
      if !lineage.known_limits.is_empty() {
        output.push_str(&format!(
          "  known_limits={}\n",
          lineage.known_limits.join(" | ")
        ));
      }
    }
  }

  output.push_str("\nCandidate Promotion Lineage:\n");
  if candidate_promotion_lineage.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in candidate_promotion_lineage {
      output.push_str(&format!(
        "- artifact={} status={} promotion_id={} decision={} stability={} projection={} source_recognition={} capture={} promoted={} refusals={}\n",
        lineage.artifact.artifact_id,
        render_candidate_promotion_status(&lineage.status),
        lineage.promotion_id.as_deref().unwrap_or("n/a"),
        lineage.decision_kind.as_deref().unwrap_or("n/a"),
        lineage.stability_kind.as_deref().unwrap_or("n/a"),
        lineage.projection_kind.as_deref().unwrap_or("n/a"),
        lineage
          .source_recognition_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage
          .capture_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        if lineage.promoted_candidate_local_ids.is_empty() {
          "none".to_string()
        } else {
          lineage.promoted_candidate_local_ids.join(",")
        },
        if lineage.refusal_reasons.is_empty() {
          "none".to_string()
        } else {
          lineage.refusal_reasons.join(" | ")
        }
      ));
      output.push_str(&format!(
        "  recognition={} observed_frames={} freshness_present={} freshness_source={} permission_granted={} consent_id={} consent_provenance={} consent_grade={} consent_scope={} permission_by={} issue={}\n",
        lineage
          .promotion_input_recognition_id
          .as_deref()
          .unwrap_or("n/a"),
        lineage
          .stability_observed_frames
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        lineage
          .freshness_present
          .map(|value| if value { "true" } else { "false" })
          .unwrap_or("n/a"),
        lineage
          .freshness_source_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage
          .permission_granted
          .map(|value| if value { "true" } else { "false" })
          .unwrap_or("n/a"),
        lineage.consent_id.as_deref().unwrap_or("n/a"),
        lineage.consent_provenance.as_deref().unwrap_or("n/a"),
        lineage.consent_grade.as_deref().unwrap_or("n/a"),
        lineage.consent_scope.as_deref().unwrap_or("n/a"),
        lineage.permission_granted_by.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a")
      ));
      if let Some(stability_reason) = &lineage.stability_reason {
        output.push_str(&format!("  stability_reason={stability_reason}\n"));
      }
      if !lineage.known_limits.is_empty() {
        output.push_str(&format!(
          "  known_limits={}\n",
          lineage.known_limits.join(" | ")
        ));
      }
    }
  }

  output.push_str("\nCandidate Action Decision Lineage:\n");
  if candidate_action_decision_lineage.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in candidate_action_decision_lineage {
      output.push_str(&format!(
        "- artifact={} status={} decision_id={} source_promotion={} candidate={} resolver={} selected={} side_effect={} input_delivery={} operation_result={} verification_result={}\n",
        lineage.artifact.artifact_id,
        render_candidate_action_decision_status(&lineage.status),
        lineage.decision_id.as_deref().unwrap_or("n/a"),
        lineage
          .source_candidate_promotion_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage.candidate_local_id.as_deref().unwrap_or("n/a"),
        lineage.resolver_operation.as_deref().unwrap_or("n/a"),
        lineage.selected_method.as_deref().unwrap_or("n/a"),
        lineage.side_effect.as_deref().unwrap_or("n/a"),
        lineage.input_delivery.as_deref().unwrap_or("n/a"),
        lineage.operation_result.as_deref().unwrap_or("n/a"),
        lineage.verification_result.as_deref().unwrap_or("n/a"),
      ));
      output.push_str(&format!(
        "  primary={} fallback_allowed={} fallback_used={} fallback_reason={} policy={} cursor={} press={} issue={}\n",
        lineage.primary_method.as_deref().unwrap_or("n/a"),
        lineage
          .fallback_allowed
          .map(|value| if value { "true" } else { "false" })
          .unwrap_or("n/a"),
        lineage
          .fallback_used
          .map(|value| if value { "true" } else { "false" })
          .unwrap_or("n/a"),
        lineage.fallback_reason.as_deref().unwrap_or("none"),
        lineage.policy.as_deref().unwrap_or("n/a"),
        lineage.cursor_disturbance.as_deref().unwrap_or("n/a"),
        lineage.press_mechanism.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a"),
      ));
      if !lineage.known_limits.is_empty() {
        output.push_str(&format!(
          "  known_limits={}\n",
          lineage.known_limits.join(" | ")
        ));
      }
    }
  }

  output.push_str("\nMC-1 Telemetry Samples:\n");
  if minecraft_telemetry_sample_artifacts.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in minecraft_telemetry_sample_artifacts {
      output.push_str(&format!(
        "- artifact={} line_count={} bytes={} path={} issue={}\n",
        lineage.artifact.artifact_id,
        lineage
          .line_count
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        lineage
          .byte_size
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        lineage.artifact.path.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a"),
      ));
    }
  }

  output.push_str("\nMC-2 Projection Artifacts:\n");
  if minecraft_projection_artifacts.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in minecraft_projection_artifacts {
      output.push_str(&format!(
        "- frame={} tick={} timestamp_ms={} screenshot_artifact_ref={} capture_skew_ms={} viewport={}x{}@{},{} visibility={} raycast={} screen_state={} refusal_reason={} verification_reference={} projected_point={}\n",
        artifact.spatial_frame_id,
        artifact.world_tick,
        artifact.monotonic_timestamp_ms,
        artifact
          .screenshot_artifact_ref
          .as_deref()
          .unwrap_or("n/a"),
        artifact
          .mc_capture_skew_ms
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        artifact.viewport_bounds.width,
        artifact.viewport_bounds.height,
        artifact.viewport_bounds.x,
        artifact.viewport_bounds.y,
        render_projection_visibility(&artifact.visibility),
        artifact.raycast_block_id.as_deref().unwrap_or("n/a"),
        artifact.screen_state.as_deref().unwrap_or("n/a"),
        artifact
          .mismatch_refusal_reason
          .map(|reason| format!("{reason:?}"))
          .unwrap_or_else(|| "n/a".to_string()),
        artifact.verification_reference.as_deref().unwrap_or("n/a"),
        render_minecraft_projected_point(artifact.projected_point.as_ref()),
      ));
    }
  }

  output.push_str("\nMC-6 Spatial Bundles:\n");
  if minecraft_spatial_bundle_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in minecraft_spatial_bundle_manifests {
      if let Some(manifest) = &lineage.manifest {
        output.push_str(&format!(
          "- artifact={} source_run={} schema={} screenshots={} spatial_frames={} actions={} verification={} overlays={} skipped={} issue={}\n",
          lineage.artifact.artifact_id,
          manifest.source_run.source_run_id,
          manifest.schema_version,
          manifest.counts.screenshots,
          manifest.counts.spatial_frames,
          manifest.counts.actions,
          manifest.counts.verification,
          manifest.counts.overlays,
          manifest.counts.skipped,
          lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      } else {
        output.push_str(&format!(
          "- artifact={} source_run=n/a schema=n/a screenshots=n/a spatial_frames=n/a actions=n/a verification=n/a overlays=n/a skipped=n/a issue={}\n",
          lineage.artifact.artifact_id,
          lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-7 Training Packages:\n");
  if minecraft_training_package_manifests.is_empty()
    && minecraft_training_package_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = std::collections::BTreeSet::new();
    for manifest_lineage in minecraft_training_package_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_package_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| {
            report.scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
              && report.source_run_ids == manifest.source_run_ids
              && report.source_bundle_manifest_paths == manifest.source_bundle_manifest_paths
          })
        })
      });
      if let Some(report_lineage) = paired_report {
        rendered_report_artifacts.insert(report_lineage.artifact.artifact_id.to_string());
      }
      if let Some(manifest) = &manifest_lineage.manifest {
        let primary_view = manifest.compatibility_views.first();
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema={} source_scene_packet={} source_runs={} frames={} images={} compatibility_view={} compatibility_status={} exported={} skipped={} transforms={} paired_report_artifact={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.schema_version,
          manifest.source_scene_packet_manifest_path,
          manifest.source_run_ids.len(),
          manifest.counts.frames,
          manifest.counts.images,
          primary_view.map(|view| view.view_name.as_str()).unwrap_or("n/a"),
          primary_view
            .map(|view| render_training_compatibility_status(&view.status))
            .unwrap_or("n/a"),
          primary_view
            .map(|view| view.exported_frame_count.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          primary_view
            .map(|view| view.skipped_frame_count.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          primary_view
            .and_then(|view| view.transforms_path.as_deref())
            .map(|_| "present")
            .unwrap_or("none"),
          paired_report
            .map(|report| report.artifact.artifact_id.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            manifest.known_limits.join(" | ")
          ));
        }
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema=n/a source_scene_packet=n/a source_runs=n/a frames=n/a images=n/a compatibility_view=n/a compatibility_status=n/a exported=n/a skipped=n/a transforms=n/a paired_report_artifact=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }

    for report_lineage in minecraft_training_package_inspect_reports {
      if rendered_report_artifacts.contains(&report_lineage.artifact.artifact_id.to_string()) {
        continue;
      }

      if let Some(report) = &report_lineage.report {
        let primary_view = report.compatibility_views.first();
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} manifest_path={} schema={} scene_packet={} source_runs={} frames={} images={} compatibility_view={} compatibility_status={} exported={} skipped={} transforms={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report.training_package_manifest_path,
          report.schema_version,
          report.scene_packet_manifest_path,
          report.source_run_ids.len(),
          report.counts.frames,
          report.counts.images,
          primary_view.map(|view| view.view_name.as_str()).unwrap_or("n/a"),
          primary_view
            .map(|view| render_training_compatibility_status(&view.status))
            .unwrap_or("n/a"),
          primary_view
            .map(|view| view.exported_frame_count.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          primary_view
            .map(|view| view.skipped_frame_count.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          primary_view
            .and_then(|view| view.transforms_path.as_deref())
            .map(|_| "present")
            .unwrap_or("none"),
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !report.warnings.is_empty() {
          output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
        }
        if !report.known_limits.is_empty() {
          output.push_str(&format!(
            "  inspect_known_limits={}\n",
            report.known_limits.join(" | ")
          ));
        }
      } else {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} manifest_path=n/a schema=n/a scene_packet=n/a source_runs=n/a frames=n/a images=n/a compatibility_view=n/a compatibility_status=n/a exported=n/a skipped=n/a transforms=n/a warnings=n/a issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-7 Training Launches:\n");
  if minecraft_training_launch_manifests.is_empty()
    && minecraft_training_launch_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_launch_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_launch_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| {
            report.source_training_package_manifest_path
              == manifest.source_training_package_manifest_path
              && report.source_scene_packet_manifest_path
                == manifest.source_scene_packet_manifest_path
              && report.source_run_ids == manifest.source_run_ids
              && report.source_bundle_manifest_paths == manifest.source_bundle_manifest_paths
          })
        })
      });
      if let Some(report_lineage) = paired_report {
        rendered_report_artifacts.insert(report_lineage.artifact.artifact_id.to_string());
      }
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema={} source_training_package={} source_scene_packet={} source_runs={} frames={} images={} trainer_backend={} compatibility_view={} exported={} skipped={} transforms={} launch_command={} paired_report_artifact={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.schema_version,
          manifest.source_training_package_manifest_path,
          manifest.source_scene_packet_manifest_path,
          manifest.source_run_ids.len(),
          manifest.counts.frames,
          manifest.counts.images,
          manifest.trainer_backend,
          manifest.compatibility_view_name,
          manifest.counts.compatibility_exported_frames,
          manifest.counts.compatibility_skipped_frames,
          manifest.transforms_path.as_deref().map(|_| "present").unwrap_or("none"),
          manifest.launch_command,
          paired_report.map(|report| report.artifact.artifact_id.to_string()).unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            manifest.known_limits.join(" | ")
          ));
        }
        if let Some(report) = paired_report.and_then(|lineage| lineage.report.as_ref()) {
          output.push_str(&format!(
            "  paired_report schema={} compatibility_status={} trainer_readiness={} readiness_blocker={} exported={} skipped={} transforms={} probe_command={} probe_succeeded={} warnings={} issue={}\n",
            report.schema_version,
            report.compatibility_status,
            report.trainer_readiness,
            report.readiness_blocker.as_deref().unwrap_or("n/a"),
            report.exported_frame_count,
            report.skipped_frame_count,
            if report.transforms_present { "present" } else { "none" },
            report.probe_command,
            report.probe_succeeded,
            report.warnings.len(),
            paired_report.and_then(|lineage| lineage.issue.as_deref()).unwrap_or("n/a"),
          ));
          if !report.warnings.is_empty() {
            output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
          }
          if !report.known_limits.is_empty() {
            output.push_str(&format!(
              "  known_limits={}\n",
              report.known_limits.join(" | ")
            ));
          }
        }
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema=n/a source_training_package=n/a source_scene_packet=n/a source_runs=n/a frames=n/a images=n/a trainer_backend=n/a compatibility_view=n/a exported=n/a skipped=n/a transforms=n/a launch_command=n/a paired_report_artifact=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in minecraft_training_launch_inspect_reports {
      if rendered_report_artifacts.contains(&report_lineage.artifact.artifact_id.to_string()) {
        continue;
      }
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} manifest_path={} schema={} source_training_package={} source_scene_packet={} source_runs={} compatibility_status={} trainer_readiness={} readiness_blocker={} exported={} skipped={} transforms={} probe_command={} probe_succeeded={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report.training_launch_manifest_path,
          report.schema_version,
          report.source_training_package_manifest_path,
          report.source_scene_packet_manifest_path,
          report.source_run_ids.len(),
          report.compatibility_status,
          report.trainer_readiness,
          report.readiness_blocker.as_deref().unwrap_or("n/a"),
          report.exported_frame_count,
          report.skipped_frame_count,
          if report.transforms_present { "present" } else { "none" },
          report.probe_command,
          report.probe_succeeded,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !report.warnings.is_empty() {
          output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
        }
        if !report.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            report.known_limits.join(" | ")
          ));
        }
      } else {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} manifest_path=n/a schema=n/a source_training_package=n/a source_scene_packet=n/a source_runs=n/a compatibility_status=n/a trainer_readiness=n/a readiness_blocker=n/a exported=n/a skipped=n/a transforms=n/a probe_command=n/a probe_succeeded=n/a warnings=n/a issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-7 Training Jobs:\n");
  if minecraft_training_job_manifests.is_empty()
    && minecraft_training_job_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_job_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_job_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| {
            report.source_training_launch_plan_path == manifest.source_training_launch_plan_path
              && report.source_training_package_manifest_path
                == manifest.source_training_package_manifest_path
              && report.source_scene_packet_manifest_path
                == manifest.source_scene_packet_manifest_path
              && report.source_run_ids == manifest.source_run_ids
              && report.job_backend == manifest.job_backend
              && report.job_submission_endpoint == manifest.job_submission_endpoint
              && report.job_submission_command == manifest.job_submission_command
          })
        })
      });
      if let Some(report_lineage) = paired_report {
        rendered_report_artifacts.insert(report_lineage.artifact.artifact_id.to_string());
      }
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema={} source_training_launch_plan={} source_runs={} frames={} images={} provider_backend={} trainer_backend={} job_backend={} status={} accepted_by_provider={} submission_recorded_at_millis={} job_id={} job_url={} readiness_blocker={} job_submission_endpoint={} job_submission_command={} exported={} skipped={} transforms={} paired_report_artifact={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.schema_version,
          manifest.source_training_launch_plan_path,
          manifest.source_run_ids.len(),
          manifest.counts.frames,
          manifest.counts.images,
          manifest.provider_backend,
          manifest.trainer_backend,
          manifest.job_backend,
          manifest.status,
          manifest.accepted_by_provider,
          manifest.submission_recorded_at_millis.map(|value| value.to_string()).as_deref().unwrap_or("n/a"),
          manifest.job_id.as_deref().unwrap_or("n/a"),
          manifest.job_url.as_deref().unwrap_or("n/a"),
          manifest.readiness_blocker.as_deref().unwrap_or("n/a"),
          manifest.job_submission_endpoint,
          manifest.job_submission_command,
          manifest.counts.compatibility_exported_frames,
          manifest.counts.compatibility_skipped_frames,
          manifest.transforms_path.as_deref().map(|_| "present").unwrap_or("none"),
          paired_report.map(|report| report.artifact.artifact_id.to_string()).unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            manifest.known_limits.join(" | ")
          ));
        }
        if let Some(report) = paired_report.and_then(|lineage| lineage.report.as_ref()) {
          output.push_str(&format!(
            "  paired_report schema={} provider_backend={} trainer_backend={} job_backend={} status={} accepted_by_provider={} submission_recorded_at_millis={} job_id={} job_url={} readiness_blocker={} job_submission_endpoint={} job_submission_command={} exported={} skipped={} transforms={} probe_command={} probe_succeeded={} warnings={} issue={}\n",
            report.schema_version,
            report.provider_backend,
            report.trainer_backend,
            report.job_backend,
            report.status,
            report.accepted_by_provider,
            report.submission_recorded_at_millis.map(|value| value.to_string()).as_deref().unwrap_or("n/a"),
            report.job_id.as_deref().unwrap_or("n/a"),
            report.job_url.as_deref().unwrap_or("n/a"),
            report.readiness_blocker.as_deref().unwrap_or("n/a"),
            report.job_submission_endpoint,
            report.job_submission_command,
            report.exported_frame_count,
            report.skipped_frame_count,
            if report.transforms_present { "present" } else { "none" },
            report.probe_command,
            report.probe_succeeded,
            report.warnings.len(),
            paired_report.and_then(|lineage| lineage.issue.as_deref()).unwrap_or("n/a"),
          ));
          if !report.warnings.is_empty() {
            output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
          }
          if !report.known_limits.is_empty() {
            output.push_str(&format!(
              "  known_limits={}\n",
              report.known_limits.join(" | ")
            ));
          }
        }
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema=n/a source_training_launch_plan=n/a source_runs=n/a frames=n/a images=n/a provider_backend=n/a trainer_backend=n/a job_backend=n/a status=n/a job_id=n/a job_url=n/a readiness_blocker=n/a job_submission_endpoint=n/a job_submission_command=n/a exported=n/a skipped=n/a transforms=n/a paired_report_artifact=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in minecraft_training_job_inspect_reports {
      if rendered_report_artifacts.contains(&report_lineage.artifact.artifact_id.to_string()) {
        continue;
      }
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} manifest_path={} schema={} source_training_launch_plan={} source_runs={} provider_backend={} trainer_backend={} job_backend={} status={} accepted_by_provider={} submission_recorded_at_millis={} job_id={} job_url={} readiness_blocker={} job_submission_endpoint={} job_submission_command={} exported={} skipped={} transforms={} probe_command={} probe_succeeded={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report.training_launch_manifest_path,
          report.schema_version,
          report.source_training_launch_plan_path,
          report.source_run_ids.len(),
          report.provider_backend,
          report.trainer_backend,
          report.job_backend,
          report.status,
          report.accepted_by_provider,
          report.submission_recorded_at_millis.map(|value| value.to_string()).as_deref().unwrap_or("n/a"),
          report.job_id.as_deref().unwrap_or("n/a"),
          report.job_url.as_deref().unwrap_or("n/a"),
          report.readiness_blocker.as_deref().unwrap_or("n/a"),
          report.job_submission_endpoint,
          report.job_submission_command,
          report.exported_frame_count,
          report.skipped_frame_count,
          if report.transforms_present { "present" } else { "none" },
          report.probe_command,
          report.probe_succeeded,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !report.warnings.is_empty() {
          output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
        }
        if !report.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            report.known_limits.join(" | ")
          ));
        }
      } else {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} manifest_path=n/a schema=n/a source_training_launch_plan=n/a source_runs=n/a provider_backend=n/a trainer_backend=n/a job_backend=n/a status=n/a job_id=n/a job_url=n/a readiness_blocker=n/a job_submission_endpoint=n/a job_submission_command=n/a exported=n/a skipped=n/a transforms=n/a probe_command=n/a probe_succeeded=n/a warnings=n/a issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-7 Training Results:\n");
  if minecraft_training_result_manifests.is_empty()
    && minecraft_training_result_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_result_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_result_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| {
            report.source_training_job_manifest_path == manifest.source_training_job_manifest_path
              && report.source_training_launch_plan_path
                == manifest.source_training_launch_plan_path
              && report.source_scene_packet_manifest_path
                == manifest.source_scene_packet_manifest_path
              && report.source_run_ids == manifest.source_run_ids
          })
        })
      });
      if let Some(report_lineage) = paired_report {
        rendered_report_artifacts.insert(report_lineage.artifact.artifact_id.to_string());
      }
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema={} source_training_job_manifest={} source_training_launch_plan={} source_runs={} trainer_backend={} job_backend={} source_job_status={} provider_status={} status_message={} job_id={} job_url={} result_dir={} result_artifacts={} exported={} skipped={} paired_report_artifact={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.schema_version,
          manifest.source_training_job_manifest_path,
          manifest.source_training_launch_plan_path,
          manifest.source_run_ids.len(),
          manifest.trainer_backend,
          manifest.job_backend,
          manifest.source_job_status,
          manifest.status,
          manifest.status_message.as_deref().unwrap_or("n/a"),
          manifest.job_id,
          manifest.job_url.as_deref().unwrap_or("n/a"),
          manifest.result_dir,
          manifest.result_artifacts.len(),
          manifest.exported_frame_count,
          manifest.skipped_frame_count,
          paired_report.map(|report| report.artifact.artifact_id.to_string()).unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            manifest.known_limits.join(" | ")
          ));
        }
        if let Some(report) = paired_report.and_then(|lineage| lineage.report.as_ref()) {
          output.push_str(&format!(
            "  paired_report schema={} trainer_backend={} job_backend={} source_job_status={} provider_status={} status_message={} status_reason={} job_id={} job_url={} result_dir={} local_result_observation result_dir_exists={} key_result_artifacts_present={} result_artifact_count={} warnings={} issue={}\n",
            report.schema_version,
            report.trainer_backend,
            report.job_backend,
            report.source_job_status,
            report.status,
            report.status_message.as_deref().unwrap_or("n/a"),
            report.status_reason.as_deref().unwrap_or("n/a"),
            report.job_id,
            report.job_url.as_deref().unwrap_or("n/a"),
            report.result_dir,
            report.result_dir_exists,
            report.key_result_artifacts_present,
            report.result_artifact_count,
            report.warnings.len(),
            paired_report.and_then(|lineage| lineage.issue.as_deref()).unwrap_or("n/a"),
          ));
          if !report.warnings.is_empty() {
            output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
          }
          if !report.known_limits.is_empty() {
            output.push_str(&format!(
              "  known_limits={}\n",
              report.known_limits.join(" | ")
            ));
          }
        }
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema=n/a source_training_job_manifest=n/a source_training_launch_plan=n/a source_runs=n/a trainer_backend=n/a job_backend=n/a source_job_status=n/a status=n/a job_id=n/a job_url=n/a result_dir=n/a result_artifacts=n/a exported=n/a skipped=n/a paired_report_artifact=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in minecraft_training_result_inspect_reports {
      if rendered_report_artifacts.contains(&report_lineage.artifact.artifact_id.to_string()) {
        continue;
      }
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} manifest_path={} schema={} source_training_job_manifest={} source_training_launch_plan={} source_runs={} trainer_backend={} job_backend={} source_job_status={} provider_status={} status_message={} status_reason={} job_id={} job_url={} result_dir={} local_result_observation result_dir_exists={} key_result_artifacts_present={} result_artifact_count={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report.training_result_manifest_path,
          report.schema_version,
          report.source_training_job_manifest_path,
          report.source_training_launch_plan_path,
          report.source_run_ids.len(),
          report.trainer_backend,
          report.job_backend,
          report.source_job_status,
          report.status,
          report.status_message.as_deref().unwrap_or("n/a"),
          report.status_reason.as_deref().unwrap_or("n/a"),
          report.job_id,
          report.job_url.as_deref().unwrap_or("n/a"),
          report.result_dir,
          report.result_dir_exists,
          report.key_result_artifacts_present,
          report.result_artifact_count,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !report.warnings.is_empty() {
          output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
        }
        if !report.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            report.known_limits.join(" | ")
          ));
        }
      } else {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} manifest_path=n/a schema=n/a source_training_job_manifest=n/a source_training_launch_plan=n/a source_runs=n/a trainer_backend=n/a job_backend=n/a source_job_status=n/a status=n/a status_reason=n/a job_id=n/a job_url=n/a result_dir=n/a result_dir_exists=n/a key_result_artifacts_present=n/a result_artifact_count=n/a warnings=n/a issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-7 Training Result Artifacts:\n");
  if minecraft_training_result_artifact_fetch_manifests.is_empty()
    && minecraft_training_result_artifact_fetch_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_result_artifact_fetch_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(
          minecraft_training_result_artifact_fetch_inspect_reports,
          |lineage| {
            lineage.report.as_ref().is_some_and(|report| {
              report.source_training_result_manifest_path
                == manifest.source_training_result_manifest_path
                && report.source_training_job_manifest_path
                  == manifest.source_training_job_manifest_path
                && report.source_run_ids == manifest.source_run_ids
            })
          },
        )
      });
      if let Some(report_lineage) = paired_report {
        rendered_report_artifacts.insert(report_lineage.artifact.artifact_id.to_string());
      }
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema={} source_training_result_manifest={} source_training_job_manifest={} source_runs={} trainer_backend={} job_backend={} source_job_status={} source_result_status={} source_result_status_reason={} source_result_dir={} normalized_result_dir={} normalized_artifacts={} paired_report_artifact={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.schema_version,
          manifest.source_training_result_manifest_path,
          manifest.source_training_job_manifest_path,
          manifest.source_run_ids.len(),
          manifest.trainer_backend,
          manifest.job_backend,
          manifest.source_job_status,
          manifest.source_result_status,
          manifest
            .source_result_status_reason
            .as_deref()
            .unwrap_or("n/a"),
          manifest.source_result_dir,
          manifest.normalized_result_dir,
          manifest.normalized_artifacts.len(),
          paired_report
            .map(|report| report.artifact.artifact_id.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !manifest.normalized_artifacts.is_empty() {
          for artifact in &manifest.normalized_artifacts {
            output.push_str(&format!(
              "  normalized_artifact kind={} relative_path={} readable={} byte_size={} absolute_path={}\n",
              artifact.kind,
              artifact.relative_path,
              artifact.readable,
              artifact
                .byte_size
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
              artifact.absolute_path,
            ));
          }
        }
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            manifest.known_limits.join(" | ")
          ));
        }
        if let Some(report) = paired_report.and_then(|lineage| lineage.report.as_ref()) {
          output.push_str(&format!(
            "  paired_report schema={} trainer_backend={} job_backend={} source_job_status={} source_result_status={} fetch_status={} fetch_reason={} source_result_dir={} normalized_result_dir={} source_result_dir_exists={} required_artifacts_present={} normalized_artifact_count={} warnings={} issue={}\n",
            report.schema_version,
            report.trainer_backend,
            report.job_backend,
            report.source_job_status,
            report.source_result_status,
            report.fetch_status,
            report.fetch_reason.as_deref().unwrap_or("n/a"),
            report.source_result_dir,
            report.normalized_result_dir,
            report.source_result_dir_exists,
            report.required_artifacts_present,
            report.normalized_artifact_count,
            report.warnings.len(),
            paired_report
              .and_then(|lineage| lineage.issue.as_deref())
              .unwrap_or("n/a"),
          ));
          if !report.warnings.is_empty() {
            output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
          }
          if !report.known_limits.is_empty() {
            output.push_str(&format!(
              "  known_limits={}\n",
              report.known_limits.join(" | ")
            ));
          }
        }
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema=n/a source_training_result_manifest=n/a source_training_job_manifest=n/a source_runs=n/a trainer_backend=n/a job_backend=n/a source_job_status=n/a source_result_status=n/a source_result_status_reason=n/a source_result_dir=n/a normalized_result_dir=n/a normalized_artifacts=n/a paired_report_artifact=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in minecraft_training_result_artifact_fetch_inspect_reports {
      if rendered_report_artifacts.contains(&report_lineage.artifact.artifact_id.to_string()) {
        continue;
      }
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} manifest_path={} schema={} source_training_result_manifest={} source_training_job_manifest={} source_runs={} trainer_backend={} job_backend={} source_job_status={} source_result_status={} fetch_status={} fetch_reason={} source_result_dir={} normalized_result_dir={} source_result_dir_exists={} required_artifacts_present={} normalized_artifact_count={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report.training_result_artifact_fetch_manifest_path,
          report.schema_version,
          report.source_training_result_manifest_path,
          report.source_training_job_manifest_path,
          report.source_run_ids.len(),
          report.trainer_backend,
          report.job_backend,
          report.source_job_status,
          report.source_result_status,
          report.fetch_status,
          report.fetch_reason.as_deref().unwrap_or("n/a"),
          report.source_result_dir,
          report.normalized_result_dir,
          report.source_result_dir_exists,
          report.required_artifacts_present,
          report.normalized_artifact_count,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !report.warnings.is_empty() {
          output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
        }
        if !report.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            report.known_limits.join(" | ")
          ));
        }
      } else {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} manifest_path=n/a schema=n/a source_training_result_manifest=n/a source_training_job_manifest=n/a source_runs=n/a trainer_backend=n/a job_backend=n/a source_job_status=n/a source_result_status=n/a fetch_status=n/a fetch_reason=n/a source_result_dir=n/a normalized_result_dir=n/a source_result_dir_exists=n/a required_artifacts_present=n/a normalized_artifact_count=n/a warnings=n/a issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-10 Training Result Semantics:\n");
  if minecraft_training_result_semantic_manifests.is_empty()
    && minecraft_training_result_semantic_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_result_semantic_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(
          minecraft_training_result_semantic_inspect_reports,
          |lineage| {
            lineage.report.as_ref().is_some_and(|report| {
              report.source_training_result_artifact_manifest_path
                == manifest.source_training_result_artifact_manifest_path
                && report.source_training_result_manifest_path
                  == manifest.source_training_result_manifest_path
                && report.source_training_job_manifest_path
                  == manifest.source_training_job_manifest_path
                && report.source_training_launch_plan_path
                  == manifest.source_training_launch_plan_path
                && report.source_scene_packet_manifest_path
                  == manifest.source_scene_packet_manifest_path
                && report.source_run_ids == manifest.source_run_ids
            })
          },
        )
      });
      if let Some(report_lineage) = paired_report {
        rendered_report_artifacts.insert(report_lineage.artifact.artifact_id.to_string());
      }
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema={} source_training_result_artifact_manifest={} source_training_result_manifest={} source_runs={} trainer_backend={} job_backend={} source_result_status={} semantic_status={} semantic_reason={} normalized_result_dir={} config_path={} models_dir_path={} status_snapshot_path={} config_trainer={} checkpoint_count={} paired_report_artifact={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.schema_version,
          manifest.source_training_result_artifact_manifest_path,
          manifest.source_training_result_manifest_path,
          manifest.source_run_ids.len(),
          manifest.trainer_backend,
          manifest.job_backend,
          manifest.source_result_status,
          manifest.semantic_status,
          manifest.semantic_reason.as_deref().unwrap_or("n/a"),
          manifest.normalized_result_dir,
          manifest.config_path,
          manifest.models_dir_path,
          manifest.status_snapshot_path.as_deref().unwrap_or("n/a"),
          manifest.config_trainer.as_deref().unwrap_or("n/a"),
          manifest.checkpoint_count,
          paired_report
            .map(|report| report.artifact.artifact_id.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !manifest.checkpoint_files.is_empty() {
          output.push_str(&format!(
            "  checkpoints={}\n",
            manifest
              .checkpoint_files
              .iter()
              .map(|checkpoint| format!(
                "relative_path={} byte_size={}",
                checkpoint.relative_path, checkpoint.byte_size
              ))
              .collect::<Vec<_>>()
              .join(" | ")
          ));
        }
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            manifest.known_limits.join(" | ")
          ));
        }
        if let Some(report) = paired_report.and_then(|lineage| lineage.report.as_ref()) {
          output.push_str(&format!(
            "  paired_report schema={} trainer_backend={} job_backend={} source_result_status={} semantic_status={} semantic_reason={} config_yaml_parsed={} config_trainer={} config_backend_matches={} models_dir_readable={} status_snapshot_present={} checkpoint_count={} warnings={} issue={}\n",
            report.schema_version,
            report.trainer_backend,
            report.job_backend,
            report.source_result_status,
            report.semantic_status,
            report.semantic_reason.as_deref().unwrap_or("n/a"),
            report.config_yaml_parsed,
            report.config_trainer.as_deref().unwrap_or("n/a"),
            report.config_backend_matches,
            report.models_dir_readable,
            report.status_snapshot_present,
            report.checkpoint_count,
            report.warnings.len(),
            paired_report
              .and_then(|lineage| lineage.issue.as_deref())
              .unwrap_or("n/a"),
          ));
          if !report.warnings.is_empty() {
            output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
          }
          if !report.known_limits.is_empty() {
            output.push_str(&format!(
              "  known_limits={}\n",
              report.known_limits.join(" | ")
            ));
          }
        }
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema=n/a source_training_result_artifact_manifest=n/a source_training_result_manifest=n/a source_runs=n/a trainer_backend=n/a job_backend=n/a source_result_status=n/a semantic_status=n/a semantic_reason=n/a normalized_result_dir=n/a config_path=n/a models_dir_path=n/a status_snapshot_path=n/a config_trainer=n/a checkpoint_count=n/a paired_report_artifact=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in minecraft_training_result_semantic_inspect_reports {
      if rendered_report_artifacts.contains(&report_lineage.artifact.artifact_id.to_string()) {
        continue;
      }
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} schema={} training_result_semantic_manifest_path={} source_training_result_artifact_manifest={} source_runs={} trainer_backend={} job_backend={} source_result_status={} semantic_status={} semantic_reason={} config_yaml_parsed={} config_trainer={} config_backend_matches={} models_dir_readable={} status_snapshot_present={} checkpoint_count={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report.schema_version,
          report.training_result_semantic_manifest_path,
          report.source_training_result_artifact_manifest_path,
          report.source_run_ids.len(),
          report.trainer_backend,
          report.job_backend,
          report.source_result_status,
          report.semantic_status,
          report.semantic_reason.as_deref().unwrap_or("n/a"),
          report.config_yaml_parsed,
          report.config_trainer.as_deref().unwrap_or("n/a"),
          report.config_backend_matches,
          report.models_dir_readable,
          report.status_snapshot_present,
          report.checkpoint_count,
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !report.warnings.is_empty() {
          output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
        }
        if !report.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            report.known_limits.join(" | ")
          ));
        }
      } else {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} schema=n/a training_result_semantic_manifest_path=n/a source_training_result_artifact_manifest=n/a source_runs=n/a trainer_backend=n/a job_backend=n/a source_result_status=n/a semantic_status=n/a semantic_reason=n/a config_yaml_parsed=n/a config_trainer=n/a config_backend_matches=n/a models_dir_readable=n/a status_snapshot_present=n/a checkpoint_count=n/a issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-16 Training Result Holdout Preview:\n");
  if minecraft_training_result_holdout_preview_manifests.is_empty()
    && minecraft_training_result_holdout_preview_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_result_holdout_preview_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(
          minecraft_training_result_holdout_preview_inspect_reports,
          |lineage| {
            lineage
              .report
              .as_ref()
              .is_some_and(|report| holdout_preview_manifest_matches_report(manifest, report))
          },
        )
      });
      if let Some(report_lineage) = paired_report {
        rendered_report_artifacts.insert(report_lineage.artifact.artifact_id.to_string());
      }
      if let Some(manifest) = &manifest_lineage.manifest {
        let spatial_frame_id = manifest
          .holdout_frame
          .as_ref()
          .map(|witness| witness.spatial_frame_id.as_str())
          .unwrap_or("n/a");
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema={} training_result_semantic_manifest={} source_training_result_artifact_manifest={} source_runs={} holdout_frame_index={} spatial_frame_id={} status={} reason={} basis_checkpoint_path={} holdout_screenshot_path={} reference_overlay_path={} paired_report_artifact={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.schema_version,
          manifest.training_result_semantic_manifest_path,
          manifest.source_training_result_artifact_manifest_path,
          manifest.source_run_ids.len(),
          manifest.holdout_frame_index,
          spatial_frame_id,
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          manifest.basis_checkpoint_path.as_deref().unwrap_or("n/a"),
          manifest.holdout_screenshot_path.as_deref().unwrap_or("n/a"),
          manifest.reference_overlay_path.as_deref().unwrap_or("n/a"),
          paired_report
            .map(|report| report.artifact.artifact_id.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            manifest.known_limits.join(" | ")
          ));
        }
        if let Some(report) = paired_report.and_then(|lineage| lineage.report.as_ref()) {
          output.push_str(&format!(
            "  paired_report schema={} holdout_frame_selection={} checkpoint_count={} scene_packet_frame_count={} warnings={} issue={}\n",
            report.schema_version,
            report.holdout_frame_selection,
            report.checkpoint_count,
            report.scene_packet_frame_count,
            report.warnings.len(),
            paired_report
              .and_then(|lineage| lineage.issue.as_deref())
              .unwrap_or("n/a"),
          ));
          if !report.warnings.is_empty() {
            output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
          }
          if !report.known_limits.is_empty() {
            output.push_str(&format!(
              "  known_limits={}\n",
              report.known_limits.join(" | ")
            ));
          }
        }
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema=n/a training_result_semantic_manifest=n/a source_training_result_artifact_manifest=n/a source_runs=n/a holdout_frame_index=n/a spatial_frame_id=n/a status=n/a reason=n/a basis_checkpoint_path=n/a holdout_screenshot_path=n/a reference_overlay_path=n/a paired_report_artifact=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in minecraft_training_result_holdout_preview_inspect_reports {
      if rendered_report_artifacts.contains(&report_lineage.artifact.artifact_id.to_string()) {
        continue;
      }
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} schema={} training_result_holdout_preview_manifest_path={} training_result_semantic_manifest={} holdout_frame_index={} status={} reason={} holdout_frame_selection={} checkpoint_count={} scene_packet_frame_count={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report.schema_version,
          report.training_result_holdout_preview_manifest_path,
          report.training_result_semantic_manifest_path,
          report.holdout_frame_index,
          report.status,
          report.reason.as_deref().unwrap_or("n/a"),
          report.holdout_frame_selection,
          report.checkpoint_count,
          report.scene_packet_frame_count,
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !report.warnings.is_empty() {
          output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
        }
        if !report.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            report.known_limits.join(" | ")
          ));
        }
      } else {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} schema=n/a training_result_holdout_preview_manifest_path=n/a training_result_semantic_manifest=n/a holdout_frame_index=n/a status=n/a reason=n/a holdout_frame_selection=n/a checkpoint_count=n/a scene_packet_frame_count=n/a issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-17 Holdout Render Quality:\n");
  if minecraft_holdout_render_quality_manifests.is_empty()
    && minecraft_holdout_render_quality_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_holdout_render_quality_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(
          minecraft_holdout_render_quality_inspect_reports,
          |lineage| {
            lineage.report.as_ref().is_some_and(|report| {
              holdout_render_quality_manifest_matches_report(manifest, report)
            })
          },
        )
      });
      if let Some(report_lineage) = paired_report {
        rendered_report_artifacts.insert(report_lineage.artifact.artifact_id.to_string());
      }
      if let Some(manifest) = &manifest_lineage.manifest {
        let (l1_mean, mse, psnr) = manifest
          .metrics
          .as_ref()
          .map(|metrics| {
            (
              metrics
                .l1_mean
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
              metrics
                .mse
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
              metrics
                .psnr
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            )
          })
          .unwrap_or_else(|| ("n/a".to_string(), "n/a".to_string(), "n/a".to_string()));
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema={} training_result_semantic_manifest={} holdout_preview_manifest={} source_training_result_artifact_manifest={} source_runs={} holdout_frame_index={} status={} reason={} verdict={} image_size_match={} basis_checkpoint_path={} rendered_image_path={} l1_mean={} mse={} psnr={} paired_report_artifact={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.schema_version,
          manifest.training_result_semantic_manifest_path,
          manifest.holdout_preview_manifest_path,
          manifest.source_training_result_artifact_manifest_path,
          manifest.source_run_ids.len(),
          manifest.holdout_frame_index,
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          manifest.verdict,
          manifest.image_size_match,
          manifest.basis_checkpoint_path.as_deref().unwrap_or("n/a"),
          manifest.rendered_image_path.as_deref().unwrap_or("n/a"),
          l1_mean,
          mse,
          psnr,
          paired_report
            .map(|report| report.artifact.artifact_id.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            manifest.known_limits.join(" | ")
          ));
        }
        if let Some(report) = paired_report.and_then(|lineage| lineage.report.as_ref()) {
          output.push_str(&format!(
            "  paired_report schema={} image_size_match={} verdict={} warnings={} issue={}\n",
            report.schema_version,
            report.image_size_match,
            report.verdict,
            report.warnings.len(),
            paired_report
              .and_then(|lineage| lineage.issue.as_deref())
              .unwrap_or("n/a"),
          ));
          if !report.warnings.is_empty() {
            output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
          }
          if !report.known_limits.is_empty() {
            output.push_str(&format!(
              "  known_limits={}\n",
              report.known_limits.join(" | ")
            ));
          }
        }
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema=n/a training_result_semantic_manifest=n/a holdout_preview_manifest=n/a source_training_result_artifact_manifest=n/a source_runs=n/a holdout_frame_index=n/a status=n/a reason=n/a verdict=n/a image_size_match=n/a basis_checkpoint_path=n/a rendered_image_path=n/a l1_mean=n/a mse=n/a psnr=n/a paired_report_artifact=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in minecraft_holdout_render_quality_inspect_reports {
      if rendered_report_artifacts.contains(&report_lineage.artifact.artifact_id.to_string()) {
        continue;
      }
      if let Some(report) = &report_lineage.report {
        let (l1_mean, mse, psnr) = report
          .metrics
          .as_ref()
          .map(|metrics| {
            (
              metrics
                .l1_mean
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
              metrics
                .mse
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
              metrics
                .psnr
                .map(|value| value.to_string())
                .unwrap_or_else(|| "n/a".to_string()),
            )
          })
          .unwrap_or_else(|| ("n/a".to_string(), "n/a".to_string(), "n/a".to_string()));
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} schema={} training_result_holdout_render_quality_manifest_path={} training_result_semantic_manifest={} holdout_preview_manifest={} holdout_frame_index={} status={} reason={} verdict={} image_size_match={} l1_mean={} mse={} psnr={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report.schema_version,
          report.training_result_holdout_render_quality_manifest_path,
          report.training_result_semantic_manifest_path,
          report.holdout_preview_manifest_path,
          report.holdout_frame_index,
          report.status,
          report.reason.as_deref().unwrap_or("n/a"),
          report.verdict,
          report.image_size_match,
          l1_mean,
          mse,
          psnr,
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !report.warnings.is_empty() {
          output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
        }
        if !report.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            report.known_limits.join(" | ")
          ));
        }
      } else {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} schema=n/a training_result_holdout_render_quality_manifest_path=n/a training_result_semantic_manifest=n/a holdout_preview_manifest=n/a holdout_frame_index=n/a status=n/a reason=n/a verdict=n/a image_size_match=n/a l1_mean=n/a mse=n/a psnr=n/a issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nBalatro Card Detection Semantic:\n");
  if balatro_card_detection_semantic_manifests.is_empty()
    && balatro_card_detection_semantic_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in balatro_card_detection_semantic_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- manifest_artifact={} semantic_status={} semantic_reason={} ui_detection_count={} entities_detection_count={} frame_source={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.semantic_status,
          manifest.semantic_reason.as_deref().unwrap_or("n/a"),
          manifest.ui_detection_count,
          manifest.entities_detection_count,
          manifest.frame_source,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in balatro_card_detection_semantic_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} semantic_status={} detection_bundle_readable={} detection_sets_non_empty={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.semantic_status,
          report.detection_bundle_readable,
          report.detection_sets_non_empty,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nBalatro Card Detection Spatial Query:\n");
  if balatro_card_detection_spatial_query_manifests.is_empty()
    && balatro_card_detection_spatial_query_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in balatro_card_detection_spatial_query_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- query_artifact={} target_slot={}:{} status={} reason={} pixel_point={} query_backend={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.target_zone,
          manifest.target_index,
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          match (manifest.pixel_x, manifest.pixel_y) {
            (Some(x), Some(y)) => format!("{x},{y}"),
            _ => "n/a".to_string(),
          },
          manifest.query_backend,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in balatro_card_detection_spatial_query_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} status={} semantic_status={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.status,
          report.semantic_status,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nBalatro Card Detection Eval Witness:\n");
  if balatro_card_detection_eval_witness_manifests.is_empty()
    && balatro_card_detection_eval_witness_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in balatro_card_detection_eval_witness_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- witness_artifact={} status={} reason={} expected_slot_count={} scored_slot_count={} unscored_slot_count={} below_confidence_slot_count={} quality_backend={} semantic_manifest={} spatial_query_manifest={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          manifest.expected_slot_count,
          manifest.scored_slot_count,
          manifest.unscored_slot_count,
          manifest.below_confidence_slot_count,
          manifest.quality_backend,
          manifest.card_detection_semantic_manifest_path,
          manifest.card_detection_spatial_query_manifest_path,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in balatro_card_detection_eval_witness_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- witness_inspect_artifact={} status={} slot_score_count={} semantic_readable={} spatial_query_readable={} expected_slots_readable={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.status,
          report.slot_score_count,
          report.semantic_manifest_readable,
          report.spatial_query_manifest_readable,
          report.expected_slots_readable,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nBalatro Card Detection Quality:\n");
  if balatro_card_detection_quality_manifests.is_empty()
    && balatro_card_detection_quality_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in balatro_card_detection_quality_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- quality_artifact={} witness_status={} status={} verdict={} quality_backend={} expected_slot_count={} scored_slot_count={} unscored_slot_count={} slot_coverage_ratio={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.witness_status,
          manifest.status,
          manifest.verdict,
          manifest.quality_backend.as_deref().unwrap_or("n/a"),
          manifest
            .expected_slot_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest
            .scored_slot_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest
            .unscored_slot_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest
            .slot_coverage_ratio
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in balatro_card_detection_quality_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- quality_inspect_artifact={} verdict={} quality_backend={} slot_coverage_ratio_available={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.verdict,
          report.quality_backend.as_deref().unwrap_or("n/a"),
          report.slot_coverage_ratio_available,
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-17 Quality Baseline Report:\n");
  if let Some(report) = quality_baseline_report {
    let spatial_query_status = report
      .spatial_query
      .as_ref()
      .map(|evidence| evidence.status.as_str())
      .unwrap_or("n/a");
    let spatial_visibility = report
      .spatial_query
      .as_ref()
      .and_then(|evidence| evidence.visibility.as_deref())
      .unwrap_or("n/a");
    let spatial_screen_point = report
      .spatial_query
      .as_ref()
      .and_then(|evidence| evidence.screen_point.as_deref())
      .unwrap_or("n/a");
    let holdout_status = report
      .holdout_witness
      .as_ref()
      .map(|evidence| evidence.status.as_str())
      .unwrap_or("n/a");
    let holdout_frame_index = report
      .holdout_witness
      .as_ref()
      .map(|evidence| evidence.holdout_frame_index.to_string())
      .unwrap_or_else(|| "n/a".to_string());
    let basis_checkpoint_path = report
      .holdout_witness
      .as_ref()
      .and_then(|evidence| evidence.basis_checkpoint_path.as_deref())
      .unwrap_or("n/a");
    let render_quality_status = report
      .render_quality
      .as_ref()
      .map(|evidence| evidence.status.as_str())
      .unwrap_or("n/a");
    let render_verdict = report
      .render_quality
      .as_ref()
      .map(|evidence| evidence.verdict.as_str())
      .unwrap_or("n/a");
    let l1_mean = report
      .render_quality
      .as_ref()
      .and_then(|evidence| evidence.l1_mean)
      .map(|value| value.to_string())
      .unwrap_or_else(|| "n/a".to_string());
    let mse = report
      .render_quality
      .as_ref()
      .and_then(|evidence| evidence.mse)
      .map(|value| value.to_string())
      .unwrap_or_else(|| "n/a".to_string());
    let psnr = report
      .render_quality
      .as_ref()
      .and_then(|evidence| evidence.psnr)
      .map(|value| value.to_string())
      .unwrap_or_else(|| "n/a".to_string());
    output.push_str(&format!(
      "- profile_id={} evidence_coverage={} training_result_semantic_manifest={} spatial_query_status={} visibility={} screen_point={} holdout_status={} holdout_frame_index={} basis_checkpoint_path={} render_quality_status={} verdict={} image_size_match={} l1_mean={} mse={} psnr={} issue={}\n",
      report.profile_id,
      report.evidence_coverage,
      report.training_result_semantic_manifest_path,
      spatial_query_status,
      spatial_visibility,
      spatial_screen_point,
      holdout_status,
      holdout_frame_index,
      basis_checkpoint_path,
      render_quality_status,
      render_verdict,
      report
        .render_quality
        .as_ref()
        .map(|evidence| evidence.image_size_match.to_string())
        .unwrap_or_else(|| "n/a".to_string()),
      l1_mean,
      mse,
      psnr,
      report.issue.as_deref().unwrap_or("n/a"),
    ));
    if !report.trust_notes.is_empty() {
      output.push_str(&format!(
        "  trust_notes={}\n",
        report.trust_notes.join(" | ")
      ));
    }
  } else {
    output.push_str("- profile_unavailable\n");
  }

  output.push_str("\nMC-17 Quality Verdict:\n");
  if let Some(verdict) = quality_verdict_probe {
    output.push_str(&format_quality_verdict_line(verdict));
  } else {
    output.push_str("- probe_profile_unavailable\n");
  }
  if let Some(verdict) = quality_verdict_trained_render {
    output.push_str(&format_quality_verdict_line(verdict));
  } else {
    output.push_str("- trained_render_profile_unavailable\n");
  }

  output.push_str("\nMC-12 Training Result Spatial Query:\n");
  if minecraft_training_result_spatial_query_manifests.is_empty()
    && minecraft_training_result_spatial_query_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_result_spatial_query_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(
          minecraft_training_result_spatial_query_inspect_reports,
          |lineage| {
            lineage
              .report
              .as_ref()
              .is_some_and(|report| spatial_query_manifest_matches_report(manifest, report))
          },
        )
      });
      if let Some(report_lineage) = paired_report {
        rendered_report_artifacts.insert(report_lineage.artifact.artifact_id.to_string());
      }
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema={} training_result_semantic_manifest={} source_training_result_artifact_manifest={} source_runs={} target_block={} target_face={} target_semantics={} query_kind={} selected_backend={} status={} reason={} visibility={} screen_point={} basis_frame_id={} comparison_verdict={} paired_report_artifact={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.schema_version,
          manifest.training_result_semantic_manifest_path,
          manifest.source_training_result_artifact_manifest_path,
          manifest.source_run_ids.len(),
          manifest.target_block,
          manifest.target_face.as_deref().unwrap_or("n/a"),
          manifest.target_semantics,
          manifest.query_kind,
          manifest.selected_backend.as_deref().unwrap_or("n/a"),
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          manifest.visibility.as_deref().unwrap_or("n/a"),
          manifest.screen_point.as_deref().unwrap_or("n/a"),
          manifest.basis_frame_id.as_deref().unwrap_or("n/a"),
          manifest.comparison_verdict.as_deref().unwrap_or("n/a"),
          paired_report
            .map(|report| report.artifact.artifact_id.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            manifest.known_limits.join(" | ")
          ));
        }
        if let Some(report) = paired_report.and_then(|lineage| lineage.report.as_ref()) {
          output.push_str(&format!(
            "  paired_report schema={} provider_status={} reference_status={} comparison_verdict={} visibility={} scene_packet_frame_count={} issue={}\n",
            report.schema_version,
            report.provider_status,
            report.reference_status,
            report.comparison_verdict.as_deref().unwrap_or("n/a"),
            report.visibility.as_deref().unwrap_or("n/a"),
            report.scene_packet_frame_count,
            paired_report
              .and_then(|lineage| lineage.issue.as_deref())
              .unwrap_or("n/a"),
          ));
          if !report.warnings.is_empty() {
            output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
          }
          if !report.known_limits.is_empty() {
            output.push_str(&format!(
              "  known_limits={}\n",
              report.known_limits.join(" | ")
            ));
          }
        }
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} schema=n/a training_result_semantic_manifest=n/a source_training_result_artifact_manifest=n/a source_runs=n/a target_block=n/a target_face=n/a target_semantics=n/a query_kind=n/a selected_backend=n/a status=n/a reason=n/a visibility=n/a screen_point=n/a basis_frame_id=n/a comparison_verdict=n/a paired_report_artifact=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in minecraft_training_result_spatial_query_inspect_reports {
      if rendered_report_artifacts.contains(&report_lineage.artifact.artifact_id.to_string()) {
        continue;
      }
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} schema={} training_result_spatial_query_manifest_path={} provider_status={} reference_status={} comparison_verdict={} visibility={} scene_packet_frame_count={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report.schema_version,
          report.training_result_spatial_query_manifest_path,
          report.provider_status,
          report.reference_status,
          report.comparison_verdict.as_deref().unwrap_or("n/a"),
          report.visibility.as_deref().unwrap_or("n/a"),
          report.scene_packet_frame_count,
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
        if !report.warnings.is_empty() {
          output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
        }
        if !report.known_limits.is_empty() {
          output.push_str(&format!(
            "  known_limits={}\n",
            report.known_limits.join(" | ")
          ));
        }
      } else {
        output.push_str(&format!(
          "- inspect_artifact={} role={} path={} schema=n/a training_result_spatial_query_manifest_path=n/a provider_status=n/a reference_status=n/a comparison_verdict=n/a visibility=n/a scene_packet_frame_count=n/a issue={}\n",
          report_lineage.artifact.artifact_id,
          report_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          report_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-14 Training Result Spatial Query Action Readiness:\n");
  if minecraft_training_result_spatial_query_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in minecraft_training_result_spatial_query_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(
          minecraft_training_result_spatial_query_inspect_reports,
          |lineage| {
            lineage
              .report
              .as_ref()
              .is_some_and(|report| spatial_query_manifest_matches_report(manifest, report))
          },
        )
      });
      let readiness =
        derive_minecraft_training_result_spatial_query_action_readiness(manifest_lineage);
      let manifest = manifest_lineage.manifest.as_ref();
      output.push_str(&format!(
        "- query_artifact={} target_block={} status={} visibility={} selected_backend={} action_eligibility={} readiness_class={} window_point={} refusal_reason={} paired_inspect_artifact={} issue={}\n",
        manifest_lineage.artifact.artifact_id,
        manifest.map(|value| value.target_block.as_str()).unwrap_or("n/a"),
        manifest.as_ref().map(|value| value.status.as_str()).unwrap_or("n/a"),
        manifest
          .and_then(|value| value.visibility.as_deref())
          .unwrap_or("n/a"),
        manifest
          .and_then(|value| value.selected_backend.as_deref())
          .unwrap_or("n/a"),
        readiness.action_eligibility,
        readiness.readiness_class.as_deref().unwrap_or("n/a"),
        readiness.window_point.as_deref().unwrap_or("n/a"),
        readiness.refusal_reason.as_deref().unwrap_or("n/a"),
        paired_report
          .map(|report| report.artifact.artifact_id.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        readiness.issue.as_deref().unwrap_or("n/a"),
      ));
    }
  }

  output.push_str("\nOsu Visual Truth Semantic:\n");
  if osu_visual_truth_semantic_manifests.is_empty()
    && osu_visual_truth_semantic_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_visual_truth_semantic_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} semantic_status={} semantic_reason={} frame_count={} beatmap_path={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest.semantic_status,
          manifest.semantic_reason.as_deref().unwrap_or("n/a"),
          manifest.frame_count,
          manifest.beatmap_path,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      } else {
        output.push_str(&format!(
          "- manifest_artifact={} role={} path={} semantic_status=n/a issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest_lineage.artifact.role.as_deref().unwrap_or("n/a"),
          manifest_lineage.artifact.path.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in osu_visual_truth_semantic_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} semantic_status={} projection_eval_ready={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.semantic_status,
          report.projection_eval_ready,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nOsu Visual Truth Spatial Query:\n");
  if osu_visual_truth_spatial_query_manifests.is_empty()
    && osu_visual_truth_spatial_query_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_visual_truth_spatial_query_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- query_artifact={} object_index={} capture_phase={} status={} reason={} pixel_visibility={} pixel_point={} query_backend={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.object_index,
          manifest.capture_phase,
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          manifest.pixel_visibility.as_deref().unwrap_or("n/a"),
          match (manifest.pixel_x, manifest.pixel_y) {
            (Some(x), Some(y)) => format!("{x},{y}"),
            _ => "n/a".to_string(),
          },
          manifest.query_backend,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in osu_visual_truth_spatial_query_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- inspect_artifact={} status={} semantic_status={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.status,
          report.semantic_status,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nOsu Visual Truth Spatial Query Action Readiness:\n");
  if osu_visual_truth_spatial_query_manifests.is_empty() {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_visual_truth_spatial_query_manifests {
      let readiness = derive_osu_visual_truth_spatial_query_action_readiness(manifest_lineage);
      let manifest = manifest_lineage.manifest.as_ref();
      output.push_str(&format!(
        "- query_artifact={} status={} action_eligibility={} pixel_point={} refusal_reason={} issue={}\n",
        manifest_lineage.artifact.artifact_id,
        manifest.as_ref().map(|value| value.status.as_str()).unwrap_or("n/a"),
        readiness.action_eligibility,
        readiness.pixel_point.as_deref().unwrap_or("n/a"),
        readiness.refusal_reason.as_deref().unwrap_or("n/a"),
        readiness.issue.as_deref().unwrap_or("n/a"),
      ));
    }
  }

  output.push_str("\nOsu Visual Truth Query Wired Live Action:\n");
  if osu_query_wired_live_action_summaries.is_empty() {
    output.push_str("- none\n");
  } else {
    for summary in osu_query_wired_live_action_summaries {
      output.push_str(&format!(
        "- operation_result_artifact={} query_artifact={} attempted={} action_eligibility={} pixel_point={} window_point={} refusal_reason={} operation_status={} operation_message={} dispatch_command={} dispatch_outcome={} target_app={} target_title={} readiness_class={} issue={}\n",
        summary.operation_result_artifact_id.as_deref().unwrap_or("n/a"),
        summary.query_artifact_id.as_deref().unwrap_or("n/a"),
        summary.attempted,
        summary.action_eligibility,
        summary.pixel_point.as_deref().unwrap_or("n/a"),
        summary.window_point.as_deref().unwrap_or("n/a"),
        summary.refusal_reason.as_deref().unwrap_or("n/a"),
        summary.operation_status.as_deref().unwrap_or("n/a"),
        summary.operation_message.as_deref().unwrap_or("n/a"),
        summary.dispatch_command.as_deref().unwrap_or("n/a"),
        summary.dispatch_outcome.as_deref().unwrap_or("n/a"),
        summary.target_app.as_deref().unwrap_or("n/a"),
        summary.target_title.as_deref().unwrap_or("n/a"),
        summary.readiness_class.as_deref().unwrap_or("n/a"),
        summary.issue.as_deref().unwrap_or("n/a"),
      ));
    }
  }

  output.push_str("\nOsu Detection Eval Witness:\n");
  if osu_detection_eval_witness_manifests.is_empty()
    && osu_detection_eval_witness_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_detection_eval_witness_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        output.push_str(&format!(
          "- witness_artifact={} status={} reason={} total_frames={} label_matched={} spatial_matched={} spatial_unscored={} spurious={} projection_kind={} frame_witness_count={} detector_model_id={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.status,
          manifest.reason.as_deref().unwrap_or("n/a"),
          manifest.total_frames,
          manifest.label_matched_frames,
          manifest.spatial_matched_frames,
          manifest.spatial_unscored_frames,
          manifest.spurious_detection_count,
          manifest.projection_kind,
          manifest.frame_witness_count,
          manifest.detector_model_id.as_deref().unwrap_or("n/a"),
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in osu_detection_eval_witness_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- witness_inspect_artifact={} status={} frame_witness_count={} warnings={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.status,
          report.frame_witness_count,
          report.warnings.len(),
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nOsu Detection Eval Quality:\n");
  if osu_detection_eval_quality_manifests.is_empty()
    && osu_detection_eval_quality_inspect_reports.is_empty()
  {
    output.push_str("- none\n");
  } else {
    for manifest_lineage in osu_detection_eval_quality_manifests {
      if let Some(manifest) = &manifest_lineage.manifest {
        let derived = derive_osu_detection_eval_quality_verdict_summary(manifest_lineage);
        output.push_str(&format!(
          "- quality_artifact={} witness_status={} status={} verdict={} label_recall={} spatial_recall={} spurious={} derived_verdict={} issue={}\n",
          manifest_lineage.artifact.artifact_id,
          manifest.witness_status,
          manifest.status,
          manifest.verdict,
          manifest
            .label_recall
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "n/a".to_string()),
          manifest
            .spatial_recall
            .map(|value| format!("{value:.3}"))
            .unwrap_or_else(|| "n/a".to_string()),
          manifest
            .spurious_detection_count
            .map(|value| value.to_string())
            .unwrap_or_else(|| "n/a".to_string()),
          derived.verdict,
          manifest_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
    for report_lineage in osu_detection_eval_quality_inspect_reports {
      if let Some(report) = &report_lineage.report {
        output.push_str(&format!(
          "- quality_inspect_artifact={} verdict={} label_recall_available={} spatial_recall_available={} issue={}\n",
          report_lineage.artifact.artifact_id,
          report.verdict,
          report.label_recall_available,
          report.spatial_recall_available,
          report_lineage.issue.as_deref().unwrap_or("n/a"),
        ));
      }
    }
  }

  output.push_str("\nMC-19 Query Wired Live Action:\n");
  if minecraft_query_wired_live_action_summaries.is_empty() {
    output.push_str("- none\n");
  } else {
    for summary in minecraft_query_wired_live_action_summaries {
      output.push_str(&format!(
        "- operation_result_artifact={} query_artifact={} attempted={} action_eligibility={} window_point={} refusal_reason={} operation_status={} operation_message={} dispatch_command={} dispatch_outcome={} target_app={} target_title={} mc14_action_eligibility={} readiness_class={} issue={}\n",
        summary.operation_result_artifact_id.as_deref().unwrap_or("n/a"),
        summary.query_artifact_id.as_deref().unwrap_or("n/a"),
        summary.attempted,
        summary.action_eligibility,
        summary.window_point.as_deref().unwrap_or("n/a"),
        summary.refusal_reason.as_deref().unwrap_or("n/a"),
        summary.operation_status.as_deref().unwrap_or("n/a"),
        summary.operation_message.as_deref().unwrap_or("n/a"),
        summary.dispatch_command.as_deref().unwrap_or("n/a"),
        summary.dispatch_outcome.as_deref().unwrap_or("n/a"),
        summary.target_app.as_deref().unwrap_or("n/a"),
        summary.target_title.as_deref().unwrap_or("n/a"),
        summary.mc14_action_eligibility.as_deref().unwrap_or("n/a"),
        summary.readiness_class.as_deref().unwrap_or("n/a"),
        summary.issue.as_deref().unwrap_or("n/a"),
      ));
    }
  }

  output.push_str("\nCandidate Action Execution Lineage:\n");
  if candidate_action_execution_lineage.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in candidate_action_execution_lineage {
      output.push_str(&format!(
        "- artifact={} status={} closure_state={} execution_id={} source_decision={} operation_result_artifact={} candidate={} resolver={} selected={} input_delivery={} selected_path={} operation_status={} verification={} semantic_matched={} readiness={} blocker={} side_effect={} consent={} by={} consent_provenance={} consent_grade={} issue={}\n",
        lineage.artifact.artifact_id,
        render_candidate_action_execution_status(&lineage.status),
        render_candidate_action_execution_closure_state(&lineage.closure_state),
        lineage.execution_id.as_deref().unwrap_or("n/a"),
        lineage
          .source_candidate_action_decision_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage
          .operation_result_artifact
          .as_ref()
          .and_then(|artifact| artifact.path.as_deref())
          .unwrap_or("n/a"),
        lineage.candidate_local_id.as_deref().unwrap_or("n/a"),
        lineage.resolver_operation.as_deref().unwrap_or("n/a"),
        lineage.selected_method.as_deref().unwrap_or("n/a"),
        lineage.input_delivery.as_deref().unwrap_or("n/a"),
        lineage.selected_path.as_deref().unwrap_or("n/a"),
        lineage.operation_status.as_deref().unwrap_or("n/a"),
        lineage.verification.as_deref().unwrap_or("n/a"),
        render_optional_bool(lineage.semantic_matched),
        lineage.readiness.as_deref().unwrap_or("n/a"),
        lineage.readiness_blocker.as_deref().unwrap_or("n/a"),
        lineage.side_effect.as_deref().unwrap_or("n/a"),
        lineage.consent_id.as_deref().unwrap_or("n/a"),
        lineage.consent_granted_by.as_deref().unwrap_or("n/a"),
        lineage.consent_provenance.as_deref().unwrap_or("n/a"),
        lineage.consent_grade.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a"),
      ));
      output.push_str(&format!(
        "  attempts={} succeeded={}\n",
        lineage
          .attempts
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
        lineage
          .attempts_succeeded
          .map(|value| value.to_string())
          .unwrap_or_else(|| "n/a".to_string()),
      ));
      if !lineage.known_limits.is_empty() {
        output.push_str(&format!(
          "  known_limits={}\n",
          lineage.known_limits.join(" | ")
        ));
      }
    }
  }

  output
}

fn render_detector_status(
  status: &crate::run_read::DetectorRecognitionLineageStatus,
) -> &'static str {
  match status {
    crate::run_read::DetectorRecognitionLineageStatus::Ready => "ready",
    crate::run_read::DetectorRecognitionLineageStatus::MissingCaptureArtifact => {
      "missing_capture_artifact"
    }
    crate::run_read::DetectorRecognitionLineageStatus::MissingEvidence => "missing_evidence",
    crate::run_read::DetectorRecognitionLineageStatus::CaptureArtifactUnresolved => {
      "capture_artifact_unresolved"
    }
    crate::run_read::DetectorRecognitionLineageStatus::Malformed => "malformed",
  }
}

fn render_candidate_promotion_status(status: &CandidatePromotionLineageStatus) -> &'static str {
  match status {
    CandidatePromotionLineageStatus::Ready => "ready",
    CandidatePromotionLineageStatus::MissingSourceRecognitionArtifact => {
      "missing_source_recognition_artifact"
    }
    CandidatePromotionLineageStatus::SourceRecognitionArtifactUnresolved => {
      "source_recognition_artifact_unresolved"
    }
    CandidatePromotionLineageStatus::MissingCaptureArtifact => "missing_capture_artifact",
    CandidatePromotionLineageStatus::CaptureArtifactUnresolved => "capture_artifact_unresolved",
    CandidatePromotionLineageStatus::MissingRecognitionEvidence => "missing_recognition_evidence",
    CandidatePromotionLineageStatus::Malformed => "malformed",
  }
}

fn render_candidate_action_decision_status(
  status: &CandidateActionDecisionLineageStatus,
) -> &'static str {
  match status {
    CandidateActionDecisionLineageStatus::Ready => "ready",
    CandidateActionDecisionLineageStatus::MissingSourceCandidatePromotionArtifact => {
      "missing_source_candidate_promotion_artifact"
    }
    CandidateActionDecisionLineageStatus::SourceCandidatePromotionArtifactUnresolved => {
      "source_candidate_promotion_artifact_unresolved"
    }
    CandidateActionDecisionLineageStatus::Malformed => "malformed",
  }
}

fn render_candidate_action_execution_status(
  status: &CandidateActionExecutionLineageStatus,
) -> &'static str {
  match status {
    CandidateActionExecutionLineageStatus::Ready => "ready",
    CandidateActionExecutionLineageStatus::BlockedNotReady => "blocked_not_ready",
    CandidateActionExecutionLineageStatus::MissingSourceCandidateActionDecisionArtifact => {
      "missing_source_candidate_action_decision_artifact"
    }
    CandidateActionExecutionLineageStatus::SourceCandidateActionDecisionArtifactUnresolved => {
      "source_candidate_action_decision_artifact_unresolved"
    }
    CandidateActionExecutionLineageStatus::MissingOperationResultArtifact => {
      "missing_operation_result_artifact"
    }
    CandidateActionExecutionLineageStatus::OperationResultArtifactUnresolved => {
      "operation_result_artifact_unresolved"
    }
    CandidateActionExecutionLineageStatus::Malformed => "malformed",
  }
}

fn render_candidate_action_execution_closure_state(
  state: &CandidateActionExecutionClosureState,
) -> &'static str {
  match state {
    CandidateActionExecutionClosureState::EvidenceClosed => "evidence_closed",
    CandidateActionExecutionClosureState::SemanticOpen => "semantic_open",
    CandidateActionExecutionClosureState::BlockedByReadiness => "blocked_by_readiness",
  }
}

fn render_optional_bool(value: Option<bool>) -> &'static str {
  match value {
    Some(true) => "true",
    Some(false) => "false",
    None => "n/a",
  }
}

fn render_projection_visibility(
  visibility: &auv_game_minecraft::types::ProjectionVisibility,
) -> &'static str {
  match visibility {
    auv_game_minecraft::types::ProjectionVisibility::Visible => "visible",
    auv_game_minecraft::types::ProjectionVisibility::BehindCamera => "behind_camera",
    auv_game_minecraft::types::ProjectionVisibility::OutOfFrustum => "out_of_frustum",
    auv_game_minecraft::types::ProjectionVisibility::OutsideWindow => "outside_window",
  }
}

fn render_minecraft_projected_point(
  projected_point: Option<&auv_game_minecraft::types::MinecraftProjectedPoint>,
) -> String {
  match projected_point {
    Some(projected_point) => {
      let screen_point = projected_point
        .screen_point
        .as_ref()
        .map(|point| format!("{},{}", point.x, point.y))
        .unwrap_or_else(|| "n/a".to_string());
      format!(
        "screen={} visibility={} radius_px={} confidence={} basis={}",
        screen_point,
        render_projection_visibility(&projected_point.visibility),
        projected_point.match_radius_px,
        projected_point.confidence,
        projected_point.basis_frame_id,
      )
    }
    None => "n/a".to_string(),
  }
}

fn render_training_compatibility_status(
  status: &auv_game_minecraft::TrainingCompatibilityStatus,
) -> &'static str {
  match status {
    auv_game_minecraft::TrainingCompatibilityStatus::Ready => "ready",
    auv_game_minecraft::TrainingCompatibilityStatus::Partial => "partial",
    auv_game_minecraft::TrainingCompatibilityStatus::Blocked => "blocked",
  }
}

fn render_failure_layer(layer: Option<FailureLayer>) -> &'static str {
  match layer {
    Some(FailureLayer::GroundingFailed) => "grounding_failed",
    Some(FailureLayer::CandidateExpired) => "candidate_expired",
    Some(FailureLayer::ControlFailed) => "control_failed",
    Some(FailureLayer::VerificationUnreliable) => "verification_unreliable",
    Some(FailureLayer::StateChangedNoMatch) => "state_changed_no_match",
    Some(FailureLayer::SemanticMismatch) => "semantic_mismatch",
    None => "n/a",
  }
}

fn render_verification_method(method: &VerificationMethod) -> String {
  match method {
    VerificationMethod::TextVisible => "text_visible".to_string(),
    VerificationMethod::AxText => "ax_text".to_string(),
    VerificationMethod::StateChanged => "state_changed".to_string(),
    VerificationMethod::CandidateAlive => "candidate_alive".to_string(),
    VerificationMethod::SemanticMatch => "semantic_match".to_string(),
    VerificationMethod::NoProgressBoundary => "no_progress_boundary".to_string(),
    VerificationMethod::Custom { name } => format!("custom:{name}"),
  }
}

fn render_observation_source(source: ObservationSource) -> &'static str {
  match source {
    ObservationSource::Ax => "ax",
    ObservationSource::Ocr => "ocr",
    ObservationSource::Visual => "visual",
    ObservationSource::Merged => "merged",
  }
}

fn render_recognition_source(source: crate::contract::RecognitionSource) -> &'static str {
  match source {
    crate::contract::RecognitionSource::OcrText => "ocr_text",
    crate::contract::RecognitionSource::OcrRow => "ocr_row",
    crate::contract::RecognitionSource::VisualRow => "visual_row",
    crate::contract::RecognitionSource::SegmentedRegion => "segmented_region",
    crate::contract::RecognitionSource::IconMatch => "icon_match",
    crate::contract::RecognitionSource::Custom => "custom",
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use super::{inspect_run, render_run_text};
  use crate::contract::{
    OBSERVATION_SNAPSHOT_API_VERSION, ObservationSnapshot, ObservationSource, RecognitionScope,
    RecognitionSource, RecognitionSurface, VERIFICATION_RESULT_API_VERSION, VerificationMethod,
    VerificationResult,
  };
  use crate::run_read::{
    ArtifactRefLineage, CandidateActionDecisionLineage, CandidateActionDecisionLineageStatus,
    CandidateActionExecutionClosureState, CandidateActionExecutionLineage,
    CandidateActionExecutionLineageStatus, CandidatePromotionLineage,
    CandidatePromotionLineageStatus, DetectorRecognitionArtifactRefLineage,
    DetectorRecognitionLineage, DetectorRecognitionLineageStatus,
    MinecraftTelemetrySampleArtifactLineage, MinecraftTrainingJobInspectReportLineage,
    MinecraftTrainingJobManifestLineage, MinecraftTrainingLaunchInspectReportLineage,
    MinecraftTrainingLaunchManifestLineage, MinecraftTrainingPackageInspectReportLineage,
    MinecraftTrainingPackageInspectReportSummary, MinecraftTrainingPackageManifestLineage,
    MinecraftTrainingPackageManifestSummary,
    MinecraftTrainingResultArtifactFetchInspectReportLineage,
    MinecraftTrainingResultArtifactFetchManifestLineage,
    MinecraftTrainingResultHoldoutPreviewInspectReportLineage,
    MinecraftTrainingResultHoldoutPreviewManifestLineage,
    MinecraftTrainingResultInspectReportLineage, MinecraftTrainingResultManifestLineage,
    MinecraftTrainingResultSemanticInspectReportLineage,
    MinecraftTrainingResultSemanticManifestLineage,
    MinecraftTrainingResultSpatialQueryInspectReportLineage,
    MinecraftTrainingResultSpatialQueryManifestLineage, OsuVisualTruthSpatialQueryManifestLineage,
  };
  use auv_game_minecraft::{
    HoldoutFrameSelection, HoldoutFrameWitness, HoldoutPreviewStatus, TrainingCompatibilityStatus,
    TrainingCompatibilityViewReport, TrainingPackageCounts,
    TrainingResultHoldoutPreviewInspectReport, TrainingResultHoldoutPreviewManifest,
  };
  use auv_tracing_driver::store::CanonicalRun;
  use auv_tracing_driver::trace::{
    ARTIFACT_API_VERSION, ArtifactId, ArtifactRecordV1Alpha1, EVENT_API_VERSION, EventId,
    EventRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION,
    SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };

  #[test]
  fn render_run_text_includes_run_span_event_artifact_verification_and_observation_records() {
    let run_id = RunId::new("run_inspect_test");
    let root_span_id = SpanId::new("span_root");
    let event_id = EventId::new("event_test");
    let artifact_id = ArtifactId::new("artifact_test");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_test"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("inspection summary".to_string()),
        failure: None,
      },
      spans: vec![SpanRecordV1Alpha1 {
        api_version: SPAN_API_VERSION.to_string(),
        span_id: root_span_id.clone(),
        parent_span_id: None,
        name: "auv.inspect.span".to_string(),
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        attributes: BTreeMap::new(),
        summary: None,
        failure: None,
      }],
      events: vec![
        EventRecordV1Alpha1 {
          api_version: EVENT_API_VERSION.to_string(),
          event_id,
          span_id: root_span_id.clone(),
          name: "inspect.event".to_string(),
          timestamp_millis: 1,
          attributes: BTreeMap::new(),
          message: Some("event message".to_string()),
          artifact_ids: vec![artifact_id.clone()],
        },
        EventRecordV1Alpha1 {
          api_version: EVENT_API_VERSION.to_string(),
          event_id: EventId::new("event_command_verification"),
          span_id: root_span_id.clone(),
          name: "command.verification".to_string(),
          timestamp_millis: 1,
          attributes: BTreeMap::new(),
          message: Some(
            "activation-only; semantic success requires a separate verification result".to_string(),
          ),
          artifact_ids: Vec::new(),
        },
        EventRecordV1Alpha1 {
          api_version: EVENT_API_VERSION.to_string(),
          event_id: EventId::new("event_command_known_limit"),
          span_id: root_span_id.clone(),
          name: "command.known_limit".to_string(),
          timestamp_millis: 1,
          attributes: BTreeMap::new(),
          message: Some("input delivery does not verify target UI state".to_string()),
          artifact_ids: Vec::new(),
        },
      ],
      artifacts: vec![ArtifactRecordV1Alpha1 {
        api_version: ARTIFACT_API_VERSION.to_string(),
        artifact_id: artifact_id.clone(),
        span_id: root_span_id,
        event_id: None,
        role: "driver.output".to_string(),
        mime_type: "text/plain".to_string(),
        path: "artifacts/output.txt".to_string(),
        sha256: None,
        attributes: BTreeMap::new(),
        summary: Some("output".to_string()),
      }],
    };
    let verifications = vec![VerificationResult {
      api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
      method: VerificationMethod::SemanticMatch,
      executed: true,
      state_changed: true,
      semantic_matched: Some(true),
      failure_layer: None,
      evidence: Vec::new(),
      consumed_candidate_ref: None,
      consumed_node_ref: None,
      consumed_recognition_artifact_ref: None,
      consumed_recognition_id: None,
      consumed_recognized_item_id: None,
      observed_label: Some("Now Playing".to_string()),
    }];
    let observation_snapshots = vec![ObservationSnapshot {
      api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
      snapshot_id: "snapshot_1".to_string(),
      run_id: run_id.clone(),
      span_id: SpanId::new("span_root"),
      captured_at_millis: 1,
      source: ObservationSource::Visual,
      scope: RecognitionScope {
        surface: RecognitionSurface::Window,
        display_ref: None,
        native_display_id: None,
        app_bundle_id: Some("com.example.music".to_string()),
        window_title: Some("Example Music".to_string()),
        window_number: None,
        region_hint: None,
        capture_artifact: None,
        capture_contract_artifact: None,
      },
      capture_contract_ref: None,
      evidence: Vec::new(),
      nodes: Vec::new(),
      detail: serde_json::json!({"producer": "scroll_scan"}),
      known_limits: vec!["visual only".to_string()],
    }];
    let detector_recognition_lineage = vec![DetectorRecognitionLineage {
      artifact: DetectorRecognitionArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_detector_recognition"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_detector_recognition")),
        role: Some("detector-recognition".to_string()),
        path: Some("artifacts/detector-recognition.json".to_string()),
        summary: Some("detector recognition".to_string()),
        resolved: true,
      },
      status: DetectorRecognitionLineageStatus::Ready,
      recognition_id: Some("recognition_detector_1".to_string()),
      source: Some(RecognitionSource::Custom),
      backend: Some("ultralytics-inference".to_string()),
      model_id: Some("games-balatro-ui".to_string()),
      execution_provider: Some("cpu".to_string()),
      class_label_source_kind: Some("override_file".to_string()),
      runtime_projection_kind: Some("identity_source_image_pixels".to_string()),
      capture_artifact: Some(DetectorRecognitionArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_capture"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_capture")),
        role: Some("capture-image".to_string()),
        path: Some("artifacts/capture.png".to_string()),
        summary: Some("capture".to_string()),
        resolved: true,
      }),
      capture_contract_artifact: Some(DetectorRecognitionArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_contract"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_contract")),
        role: Some("capture-contract".to_string()),
        path: Some("artifacts/capture-contract.json".to_string()),
        summary: Some("contract".to_string()),
        resolved: true,
      }),
      evidence_artifacts: vec![
        DetectorRecognitionArtifactRefLineage {
          run_id: run_id.clone(),
          artifact_id: ArtifactId::new("artifact_capture"),
          span_id: SpanId::new("span_root"),
          captured_event_id: Some(EventId::new("event_capture")),
          role: Some("capture-image".to_string()),
          path: Some("artifacts/capture.png".to_string()),
          summary: Some("capture".to_string()),
          resolved: true,
        },
        DetectorRecognitionArtifactRefLineage {
          run_id: run_id.clone(),
          artifact_id: ArtifactId::new("artifact_contract"),
          span_id: SpanId::new("span_root"),
          captured_event_id: Some(EventId::new("event_contract")),
          role: Some("capture-contract".to_string()),
          path: Some("artifacts/capture-contract.json".to_string()),
          summary: Some("contract".to_string()),
          resolved: true,
        },
      ],
      all_count: Some(2),
      filtered_count: Some(1),
      best_item_id: None,
      known_limits: vec![
        "projection basis is unavailable outside capture-integrated runtime".to_string(),
        "detector RecognitionResult is recognition evidence only, not candidate-ready output"
          .to_string(),
      ],
      issue: None,
    }];
    let candidate_promotion_lineage = vec![CandidatePromotionLineage {
      artifact: ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_candidate_promotion"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_candidate_promotion")),
        role: Some("candidate-promotion".to_string()),
        path: Some("artifacts/candidate-promotion.json".to_string()),
        summary: Some("candidate promotion".to_string()),
        resolved: true,
      },
      status: CandidatePromotionLineageStatus::Ready,
      promotion_id: Some("promotion_end_turn".to_string()),
      source_recognition_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_detector_recognition"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_detector_recognition")),
        role: Some("detector-recognition".to_string()),
        path: Some("artifacts/detector-recognition.json".to_string()),
        summary: Some("detector recognition".to_string()),
        resolved: true,
      }),
      capture_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_capture"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_capture")),
        role: Some("capture-image".to_string()),
        path: Some("artifacts/capture.png".to_string()),
        summary: Some("capture".to_string()),
        resolved: true,
      }),
      promotion_input_recognition_id: Some("recognition_detector_1".to_string()),
      observed_recognition_ids: vec![
        "recognition_detector_0".to_string(),
        "recognition_detector_1".to_string(),
      ],
      recognition_source: Some(RecognitionSource::Custom),
      projection_kind: Some("identity_window_addressable".to_string()),
      stability_kind: Some("stable".to_string()),
      stability_observed_frames: Some(2),
      stability_reason: None,
      freshness_present: Some(true),
      freshness_source_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_capture"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_capture")),
        role: Some("capture-image".to_string()),
        path: Some("artifacts/capture.png".to_string()),
        summary: Some("capture".to_string()),
        resolved: true,
      }),
      freshness_source_operation_id: Some("observe.window.capture".to_string()),
      permission_granted: Some(true),
      permission_granted_by: Some("human-review".to_string()),
      permission_scope_note: Some("fixture promotion".to_string()),
      consent_id: Some("consent_promotion_end_turn".to_string()),
      consent_provenance: Some("human_gesture".to_string()),
      consent_grade: Some("human_approved".to_string()),
      consent_scope: Some("candidate_promotion_only".to_string()),
      consent_approved_action: Some("promote_recognition_to_candidate".to_string()),
      consent_recognition_id: Some("recognition_detector_1".to_string()),
      decision_kind: Some("promoted".to_string()),
      refusal_reasons: Vec::new(),
      promoted_candidate_local_ids: vec!["promoted-item_end_turn".to_string()],
      known_limits: vec![
        "candidate promotion artifact records gate decisions only; runtime action consumption remains deferred".to_string(),
      ],
      issue: None,
    }];
    let candidate_action_decision_lineage = vec![CandidateActionDecisionLineage {
      artifact: ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_candidate_action_decision"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_candidate_action_decision")),
        role: Some("candidate-action-decision".to_string()),
        path: Some("artifacts/candidate-action-decision.json".to_string()),
        summary: Some("candidate action decision".to_string()),
        resolved: true,
      },
      status: CandidateActionDecisionLineageStatus::Ready,
      decision_id: Some("decision_end_turn".to_string()),
      source_candidate_promotion_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_candidate_promotion"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_candidate_promotion")),
        role: Some("candidate-promotion".to_string()),
        path: Some("artifacts/candidate-promotion.json".to_string()),
        summary: Some("candidate promotion".to_string()),
        resolved: true,
      }),
      source_promotion_id: Some("promotion_end_turn".to_string()),
      candidate_local_id: Some("promoted-item_end_turn".to_string()),
      resolver_operation: Some("candidate.action.decide_only".to_string()),
      selected_method: Some("pointer-click".to_string()),
      primary_method: Some("pointer-click".to_string()),
      fallback_allowed: Some(false),
      fallback_used: Some(false),
      fallback_reason: None,
      policy: Some("candidate-coordinate-pointer".to_string()),
      cursor_disturbance: Some("warp-visible".to_string()),
      press_mechanism: Some("pointer-click".to_string()),
      side_effect: Some("none_decide_only".to_string()),
      input_delivery: Some("not_attempted".to_string()),
      operation_result: Some("not_produced".to_string()),
      verification_result: Some("not_produced".to_string()),
      known_limits: vec![
        "L8a records an ActionResolverDecision only; it does not call auv-driver or produce InputActionResult".to_string(),
      ],
      issue: None,
    }];
    let candidate_action_execution_lineage = vec![CandidateActionExecutionLineage {
      artifact: ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_candidate_action_execution"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_candidate_action_execution")),
        role: Some("candidate-action-execution".to_string()),
        path: Some("artifacts/candidate-action-execution.json".to_string()),
        summary: Some("candidate action execution".to_string()),
        resolved: true,
      },
      status: CandidateActionExecutionLineageStatus::Ready,
      execution_id: Some("execution_end_turn".to_string()),
      source_candidate_action_decision_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_candidate_action_decision"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_candidate_action_decision")),
        role: Some("candidate-action-decision".to_string()),
        path: Some("artifacts/candidate-action-decision.json".to_string()),
        summary: Some("candidate action decision".to_string()),
        resolved: true,
      }),
      source_candidate_promotion_artifact: None,
      operation_result_artifact: Some(ArtifactRefLineage {
        run_id: run_id.clone(),
        artifact_id: ArtifactId::new("artifact_operation_result"),
        span_id: SpanId::new("span_root"),
        captured_event_id: Some(EventId::new("event_operation_result")),
        role: Some("operation-result".to_string()),
        path: Some("artifacts/operation-result.json".to_string()),
        summary: Some("operation result".to_string()),
        resolved: true,
      }),
      source_promotion_id: Some("promotion_end_turn".to_string()),
      source_decision_id: Some("decision_end_turn".to_string()),
      candidate_local_id: Some("promoted-item_end_turn".to_string()),
      resolver_operation: Some("candidate.action.decide_only".to_string()),
      selected_method: Some("pointer-click".to_string()),
      input_delivery: Some("attempted".to_string()),
      selected_path: Some("window_targeted_mouse".to_string()),
      attempts: Some(1),
      attempts_succeeded: Some(1),
      operation_status: Some("completed".to_string()),
      verification: Some("activation_only".to_string()),
      closure_state: CandidateActionExecutionClosureState::SemanticOpen,
      semantic_matched: None,
      readiness: Some("ready".to_string()),
      readiness_blocker: None,
      consent_id: Some("consent_execute_end_turn".to_string()),
      consent_granted_by: Some("human-review".to_string()),
      consent_provenance: Some("human_gesture".to_string()),
      consent_grade: Some("human_approved".to_string()),
      side_effect: Some("single_input_delivered".to_string()),
      known_limits: vec![
        "activation_only verification records input delivery, not semantic success".to_string(),
      ],
      issue: None,
    }];

    let minecraft_projection_artifacts =
      vec![auv_game_minecraft::artifact::MinecraftProjectionArtifact {
        spatial_frame_id: "frame-1".to_string(),
        world_tick: 42,
        monotonic_timestamp_ms: 1_000,
        viewport_bounds: auv_game_minecraft::artifact::ProjectionViewportBounds {
          x: 0.0,
          y: 0.0,
          width: 800.0,
          height: 600.0,
        },
        projected_point: Some(auv_game_minecraft::types::MinecraftProjectedPoint {
          screen_point: Some(auv_driver::geometry::Point::new(320.0, 240.0)),
          visibility: auv_game_minecraft::types::ProjectionVisibility::Visible,
          match_radius_px: 12.0,
          basis_frame_id: "frame-1".to_string(),
          confidence: 1.0,
        }),
        screenshot_artifact_ref: Some("artifact://screenshot-1".to_string()),
        mc_capture_skew_ms: Some(180),
        visibility: auv_game_minecraft::types::ProjectionVisibility::Visible,
        raycast_block_id: Some("minecraft:stone".to_string()),
        screen_state: Some("menu".to_string()),
        resource_pack_ids: vec!["vanilla".to_string()],
        mismatch_refusal_reason: Some(
          auv_game_minecraft::verify::MismatchRefusalReason::MenuLoadingScreen,
        ),
        verification_reference: Some("verification-1".to_string()),
      }];
    let minecraft_telemetry_sample_artifacts = vec![MinecraftTelemetrySampleArtifactLineage {
      artifact: crate::run_read::ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: auv_tracing_driver::trace::ArtifactId::new("artifact_mc1".to_string()),
        span_id: auv_tracing_driver::trace::SpanId::new("span_mc1".to_string()),
        captured_event_id: None,
        role: Some("telemetry-sample".to_string()),
        path: Some("artifacts/telemetry.jsonl".to_string()),
        summary: Some("durable minecraft telemetry sample".to_string()),
        resolved: true,
      },
      line_count: Some(1),
      byte_size: Some(16),
      issue: None,
    }];
    let minecraft_training_package_manifests = vec![MinecraftTrainingPackageManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_mc7_package"),
        span_id: SpanId::new("span_mc7_package"),
        captured_event_id: None,
        role: Some("minecraft-3dgs-training-package".to_string()),
        path: Some("artifacts/minecraft-3dgs-training-package-run.json".to_string()),
        summary: Some("training package manifest".to_string()),
        resolved: true,
      },
      manifest: Some(MinecraftTrainingPackageManifestSummary {
        schema_version: 1,
        source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec![
          "/tmp/bundle-a/run.json".to_string(),
          "/tmp/bundle-b/run.json".to_string(),
        ],
        source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
        counts: TrainingPackageCounts {
          frames: 6,
          images: 6,
          compatibility_exported_frames: 4,
          compatibility_skipped_frames: 2,
        },
        compatibility_views: vec![TrainingCompatibilityViewReport {
          view_name: "nerfstudio".to_string(),
          status: TrainingCompatibilityStatus::Partial,
          exported_frame_count: 4,
          skipped_frame_count: 2,
          transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
          export_report_path: "compat/nerfstudio/export_report.json".to_string(),
          exported_frame_indices: vec![1, 2, 3, 4],
          frame_decisions: Vec::new(),
          skip_reason_counts: Vec::new(),
          warnings: vec!["frame 5 skipped".to_string()],
          used_legacy_view_translation_fallback_frame_indices: vec![2],
          known_limits: vec!["legacy translation fallback used".to_string()],
        }],
        known_limits: vec!["canonical package only; no trained splat is present".to_string()],
      }),
      issue: None,
    }];
    let minecraft_training_package_inspect_reports =
      vec![MinecraftTrainingPackageInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_package_inspect"),
          span_id: SpanId::new("span_mc7_package"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-package-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-package-inspect.json".to_string()),
          summary: Some("training package inspect report".to_string()),
          resolved: true,
        },
        report: Some(MinecraftTrainingPackageInspectReportSummary {
          schema_version: 1,
          training_package_manifest_path: "/tmp/package/run.json".to_string(),
          scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
          source_bundle_manifest_paths: vec![
            "/tmp/bundle-a/run.json".to_string(),
            "/tmp/bundle-b/run.json".to_string(),
          ],
          source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
          counts: TrainingPackageCounts {
            frames: 6,
            images: 6,
            compatibility_exported_frames: 4,
            compatibility_skipped_frames: 2,
          },
          compatibility_views: vec![TrainingCompatibilityViewReport {
            view_name: "nerfstudio".to_string(),
            status: TrainingCompatibilityStatus::Partial,
            exported_frame_count: 4,
            skipped_frame_count: 2,
            transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
            export_report_path: "compat/nerfstudio/export_report.json".to_string(),
            exported_frame_indices: vec![1, 2, 3, 4],
            frame_decisions: Vec::new(),
            skip_reason_counts: Vec::new(),
            warnings: vec!["frame 5 skipped".to_string()],
            used_legacy_view_translation_fallback_frame_indices: vec![2],
            known_limits: vec!["legacy translation fallback used".to_string()],
          }],
          warnings: vec!["frame 6 missing screenshot".to_string()],
          known_limits: vec!["synthetic validation only".to_string()],
        }),
        issue: None,
      }];

    let minecraft_training_launch_manifests = vec![MinecraftTrainingLaunchManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_mc7_launch"),
        span_id: SpanId::new("span_mc7_launch"),
        captured_event_id: None,
        role: Some("minecraft-3dgs-training-launch-plan".to_string()),
        path: Some("artifacts/minecraft-3dgs-training-launch-plan.json".to_string()),
        summary: Some("training launch manifest".to_string()),
        resolved: true,
      },
      manifest: Some(crate::run_read::MinecraftTrainingLaunchManifestSummary {
        schema_version: 1,
        source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
        source_training_package_inspect_report_path: "/tmp/package/inspect_report.json".to_string(),
        source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string(), "/tmp/bundle-b/run.json".to_string()],
        source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
        counts: TrainingPackageCounts { frames: 6, images: 6, compatibility_exported_frames: 4, compatibility_skipped_frames: 2 },
        compatibility_view_name: "nerfstudio".to_string(),
        trainer_backend: "nerfstudio.splatfacto".to_string(),
        training_data_dir: "/tmp/package/compat/nerfstudio".to_string(),
        transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
        export_report_path: "compat/nerfstudio/export_report.json".to_string(),
        suggested_output_dir: "/tmp/launch/trainer-output/nerfstudio-splatfacto".to_string(),
        launch_command: "ns-train splatfacto --data /tmp/package/compat/nerfstudio --output-dir /tmp/launch/trainer-output/nerfstudio-splatfacto".to_string(),
        known_limits: vec!["launch prep only".to_string()],
      }),
      issue: None,
    }];
    let minecraft_training_launch_inspect_reports =
      vec![MinecraftTrainingLaunchInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_launch_inspect"),
          span_id: SpanId::new("span_mc7_launch"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-launch-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-launch-inspect.json".to_string()),
          summary: Some("training launch inspect".to_string()),
          resolved: true,
        },
        report: Some(
          crate::run_read::MinecraftTrainingLaunchInspectReportSummary {
            schema_version: 1,
            training_launch_manifest_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
              .to_string(),
            source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec![
              "/tmp/bundle-a/run.json".to_string(),
              "/tmp/bundle-b/run.json".to_string(),
            ],
            source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
            compatibility_status: "Partial".to_string(),
            trainer_readiness: "Blocked".to_string(),
            readiness_blocker: Some("TrainerCommandUnavailable".to_string()),
            probe_command: "ns-train --help".to_string(),
            probe_succeeded: false,
            exported_frame_count: 4,
            skipped_frame_count: 2,
            transforms_present: true,
            warnings: vec!["ns-train unavailable".to_string()],
            known_limits: vec!["synthetic only".to_string()],
          },
        ),
        issue: None,
      }];
    let minecraft_training_job_manifests = vec![MinecraftTrainingJobManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_mc7_job"),
        span_id: SpanId::new("span_mc7_job"),
        captured_event_id: None,
        role: Some("minecraft-3dgs-training-job".to_string()),
        path: Some("artifacts/minecraft-3dgs-training-job.json".to_string()),
        summary: Some("training job manifest".to_string()),
        resolved: true,
      },
      manifest: Some(crate::run_read::MinecraftTrainingJobManifestSummary {
        schema_version: 1,
        source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
        source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
        source_training_package_inspect_report_path: "/tmp/package/inspect_report.json".to_string(),
        source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string(), "/tmp/bundle-b/run.json".to_string()],
        source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
        counts: TrainingPackageCounts { frames: 6, images: 6, compatibility_exported_frames: 4, compatibility_skipped_frames: 2 },
        compatibility_view_name: "nerfstudio".to_string(),
        provider_backend: "remote-command-provider".to_string(),
        trainer_backend: "nerfstudio.splatfacto".to_string(),
        job_backend: "remote".to_string(),
        job_submission_endpoint: "https://jobs.example/api".to_string(),
        job_submission_command: "submit-training-job".to_string(),
        submission_recorded_at_millis: Some(1),
        accepted_by_provider: true,
        training_data_dir: "/tmp/package/compat/nerfstudio".to_string(),
        transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
        export_report_path: "compat/nerfstudio/export_report.json".to_string(),
        suggested_output_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
        launch_command: "ns-train splatfacto --data /tmp/package/compat/nerfstudio --output-dir /tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
        status: "submitted".to_string(),
        job_id: Some("job-123".to_string()),
        job_url: Some("https://jobs.example/job-123".to_string()),
        readiness_blocker: None,
        known_limits: vec!["remote submission only".to_string()],
      }),
      issue: None,
    }];
    let minecraft_training_job_inspect_reports = vec![MinecraftTrainingJobInspectReportLineage {
      artifact: ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_mc7_job_inspect"),
        span_id: SpanId::new("span_mc7_job"),
        captured_event_id: None,
        role: Some("minecraft-3dgs-training-job-inspect".to_string()),
        path: Some("artifacts/minecraft-3dgs-training-job-inspect.json".to_string()),
        summary: Some("training job inspect".to_string()),
        resolved: true,
      },
      report: Some(crate::run_read::MinecraftTrainingJobInspectReportSummary {
        schema_version: 1,
        training_launch_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
        source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
          .to_string(),
        source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
        source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec![
          "/tmp/bundle-a/run.json".to_string(),
          "/tmp/bundle-b/run.json".to_string(),
        ],
        source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
        provider_backend: "remote-command-provider".to_string(),
        job_backend: "remote".to_string(),
        trainer_backend: "nerfstudio.splatfacto".to_string(),
        job_submission_endpoint: "https://jobs.example/api".to_string(),
        job_submission_command: "submit-training-job".to_string(),
        submission_recorded_at_millis: Some(1),
        accepted_by_provider: true,
        status: "submitted".to_string(),
        job_id: Some("job-123".to_string()),
        job_url: Some("https://jobs.example/job-123".to_string()),
        readiness_blocker: None,
        probe_command: "submit-training-job --help".to_string(),
        probe_succeeded: true,
        exported_frame_count: 4,
        skipped_frame_count: 2,
        transforms_present: true,
        warnings: vec!["manual remote audit required".to_string()],
        known_limits: vec!["job execution not consumed here".to_string()],
      }),
      issue: None,
    }];
    let minecraft_training_result_manifests = vec![MinecraftTrainingResultManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_mc7_result"),
        span_id: SpanId::new("span_mc7_result"),
        captured_event_id: None,
        role: Some("minecraft-3dgs-training-result".to_string()),
        path: Some("artifacts/minecraft-3dgs-training-result.json".to_string()),
        summary: Some("training result manifest".to_string()),
        resolved: true,
      },
      manifest: Some(crate::run_read::MinecraftTrainingResultManifestSummary {
        schema_version: 1,
        source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
        source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
          .to_string(),
        source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
        source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec![
          "/tmp/bundle-a/run.json".to_string(),
          "/tmp/bundle-b/run.json".to_string(),
        ],
        source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
        trainer_backend: "nerfstudio.splatfacto".to_string(),
        job_backend: "remote".to_string(),
        job_submission_endpoint: "https://jobs.example/api".to_string(),
        source_job_status: "submitted".to_string(),
        status: "succeeded".to_string(),
        status_message: Some("provider succeeded".to_string()),
        job_id: "job-123".to_string(),
        job_url: Some("https://jobs.example/job-123".to_string()),
        result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
        result_artifacts: vec![crate::run_read::MinecraftTrainingResultArtifactSummary {
          relative_path: "config.yml".to_string(),
          absolute_path: "/tmp/job/trainer-output/nerfstudio-splatfacto/config.yml".to_string(),
          readable: true,
          byte_size: Some(128),
        }],
        exported_frame_count: 4,
        skipped_frame_count: 2,
        known_limits: vec!["quality not graded".to_string()],
      }),
      issue: None,
    }];
    let minecraft_training_result_inspect_reports =
      vec![MinecraftTrainingResultInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_result_inspect"),
          span_id: SpanId::new("span_mc7_result"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-result-inspect.json".to_string()),
          summary: Some("training result inspect".to_string()),
          resolved: true,
        },
        report: Some(
          crate::run_read::MinecraftTrainingResultInspectReportSummary {
            schema_version: 1,
            training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
              .to_string(),
            source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json"
              .to_string(),
            source_training_launch_plan_path:
              "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec![
              "/tmp/bundle-a/run.json".to_string(),
              "/tmp/bundle-b/run.json".to_string(),
            ],
            source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
            trainer_backend: "nerfstudio.splatfacto".to_string(),
            job_backend: "remote".to_string(),
            job_submission_endpoint: "https://jobs.example/api".to_string(),
            source_job_status: "submitted".to_string(),
            status: "succeeded".to_string(),
            status_message: Some("provider succeeded".to_string()),
            status_reason: None,
            job_id: "job-123".to_string(),
            job_url: Some("https://jobs.example/job-123".to_string()),
            result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
            result_dir_exists: true,
            key_result_artifacts_present: true,
            result_artifact_count: 1,
            warnings: vec!["manual quality review pending".to_string()],
            known_limits: vec!["quality not graded".to_string()],
          },
        ),
        issue: None,
      }];
    let minecraft_training_result_artifact_fetch_manifests = vec![
      MinecraftTrainingResultArtifactFetchManifestLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_result_artifact_manifest"),
          span_id: SpanId::new("span_mc7_result_artifact"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-artifact-manifest".to_string()),
          path: Some(
            "artifacts/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
          ),
          summary: Some("training result artifact fetch manifest".to_string()),
          resolved: true,
        },
        manifest: Some(crate::run_read::MinecraftTrainingResultArtifactFetchManifestSummary {
          schema_version: 1,
          source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
            .to_string(),
          source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json"
            .to_string(),
          source_training_launch_plan_path:
            "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
          source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
          source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
          source_bundle_manifest_paths: vec![
            "/tmp/bundle-a/run.json".to_string(),
            "/tmp/bundle-b/run.json".to_string(),
          ],
          source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
          trainer_backend: "nerfstudio.splatfacto".to_string(),
          job_backend: "remote".to_string(),
          source_job_status: "submitted".to_string(),
          source_result_status: "succeeded".to_string(),
          source_result_status_reason: None,
          source_result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
          normalized_result_dir:
            "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result".to_string(),
          normalized_artifacts: vec![
            crate::run_read::MinecraftTrainingResultNormalizedArtifactSummary {
              kind: "config".to_string(),
              relative_path: "config.yml".to_string(),
              absolute_path:
                "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result/config.yml"
                  .to_string(),
              readable: true,
              byte_size: Some(128),
            },
            crate::run_read::MinecraftTrainingResultNormalizedArtifactSummary {
              kind: "models_directory".to_string(),
              relative_path: "nerfstudio_models".to_string(),
              absolute_path: "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result/nerfstudio_models".to_string(),
              readable: true,
              byte_size: None,
            },
            crate::run_read::MinecraftTrainingResultNormalizedArtifactSummary {
              kind: "status_snapshot".to_string(),
              relative_path: "job_status.json".to_string(),
              absolute_path: "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result/job_status.json".to_string(),
              readable: true,
              byte_size: Some(32),
            },
          ],
          known_limits: vec!["normalized artifacts only".to_string()],
        }),
        issue: None,
      },
    ];
    let minecraft_training_result_artifact_fetch_inspect_reports =
      vec![MinecraftTrainingResultArtifactFetchInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_result_artifact_inspect"),
          span_id: SpanId::new("span_mc7_result_artifact"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-artifact-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-result-artifact-inspect.json".to_string()),
          summary: Some("training result artifact fetch inspect".to_string()),
          resolved: true,
        },
        report: Some(
          crate::run_read::MinecraftTrainingResultArtifactFetchInspectReportSummary {
            schema_version: 1,
            training_result_artifact_fetch_manifest_path:
              "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
            source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
              .to_string(),
            source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json"
              .to_string(),
            source_training_launch_plan_path:
              "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec![
              "/tmp/bundle-a/run.json".to_string(),
              "/tmp/bundle-b/run.json".to_string(),
            ],
            source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
            trainer_backend: "nerfstudio.splatfacto".to_string(),
            job_backend: "remote".to_string(),
            source_job_status: "submitted".to_string(),
            source_result_status: "succeeded".to_string(),
            source_result_status_reason: None,
            fetch_status: "succeeded".to_string(),
            fetch_reason: None,
            source_result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
            normalized_result_dir:
              "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result".to_string(),
            source_result_dir_exists: true,
            required_artifacts_present: true,
            normalized_artifact_count: 3,
            warnings: vec!["manual downstream quality review pending".to_string()],
            known_limits: vec!["normalized artifacts only".to_string()],
          },
        ),
        issue: None,
      }];

    let minecraft_training_result_semantic_manifests =
      vec![MinecraftTrainingResultSemanticManifestLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc10_semantic_manifest"),
          span_id: SpanId::new("span_mc10_semantic"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-semantic".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-result-semantic.json".to_string()),
          summary: Some("training result semantic manifest".to_string()),
          resolved: true,
        },
        manifest: Some(
          crate::run_read::MinecraftTrainingResultSemanticManifestSummary {
            schema_version: 1,
            source_training_result_artifact_manifest_path:
              "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
            source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
              .to_string(),
            source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json"
              .to_string(),
            source_training_launch_plan_path:
              "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
            source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec![
              "/tmp/bundle-a/run.json".to_string(),
              "/tmp/bundle-b/run.json".to_string(),
            ],
            source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
            trainer_backend: "nerfstudio.splatfacto".to_string(),
            job_backend: "remote".to_string(),
            source_result_status: "succeeded".to_string(),
            normalized_result_dir:
              "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result".to_string(),
            semantic_status: "ready".to_string(),
            semantic_reason: None,
            config_path:
              "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result/config.yml"
                .to_string(),
            models_dir_path:
              "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result/nerfstudio_models"
                .to_string(),
            status_snapshot_path: Some(
              "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result/job_status.json"
                .to_string(),
            ),
            config_trainer: Some("nerfstudio.splatfacto".to_string()),
            checkpoint_files: vec![
              crate::run_read::MinecraftTrainingResultSemanticCheckpointSummary {
                relative_path: "step-000001.ckpt".to_string(),
                byte_size: 32,
              },
            ],
            checkpoint_count: 1,
            known_limits: vec!["semantic gate only".to_string()],
          },
        ),
        issue: None,
      }];
    let minecraft_training_result_semantic_inspect_reports =
      vec![MinecraftTrainingResultSemanticInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc10_semantic_inspect"),
          span_id: SpanId::new("span_mc10_semantic"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-semantic-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-result-semantic-inspect.json".to_string()),
          summary: Some("training result semantic inspect".to_string()),
          resolved: true,
        },
        report: Some(
          crate::run_read::MinecraftTrainingResultSemanticInspectReportSummary {
            schema_version: 1,
            training_result_semantic_manifest_path:
              "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
            source_training_result_artifact_manifest_path:
              "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
            source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
              .to_string(),
            source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json"
              .to_string(),
            source_training_launch_plan_path:
              "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
            source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec![
              "/tmp/bundle-a/run.json".to_string(),
              "/tmp/bundle-b/run.json".to_string(),
            ],
            source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
            trainer_backend: "nerfstudio.splatfacto".to_string(),
            job_backend: "remote".to_string(),
            source_result_status: "succeeded".to_string(),
            normalized_result_dir:
              "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result".to_string(),
            semantic_status: "ready".to_string(),
            semantic_reason: None,
            config_yaml_parsed: true,
            config_trainer: Some("nerfstudio.splatfacto".to_string()),
            config_backend_matches: true,
            models_dir_readable: true,
            status_snapshot_present: true,
            checkpoint_count: 1,
            warnings: vec![],
            known_limits: vec!["semantic inspect only".to_string()],
          },
        ),
        issue: None,
      }];

    let holdout_witness = HoldoutFrameWitness {
      frame_index: 6,
      spatial_frame_id: "frame-355416-47699343801916".to_string(),
      screenshot_path: "/tmp/scene-packet/frames/frame_000006.png".to_string(),
      frame_json_path: "/tmp/scene-packet/frames/frame_000006.json".to_string(),
    };
    let holdout_manifest_value = TrainingResultHoldoutPreviewManifest {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_semantic_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
      source_training_result_artifact_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
        .to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path:
        "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec![
        "/tmp/bundle-a/run.json".to_string(),
        "/tmp/bundle-b/run.json".to_string(),
      ],
      source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir:
        "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result".to_string(),
      holdout_frame_index: 6,
      holdout_frame: Some(holdout_witness.clone()),
      basis_checkpoint_path: Some(
        "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result/nerfstudio_models/step-000001.ckpt"
          .to_string(),
      ),
      holdout_screenshot_path: Some(holdout_witness.screenshot_path.clone()),
      reference_overlay_path: Some(
        "/tmp/holdout/holdout_overlay_frame_000006.png".to_string(),
      ),
      status: HoldoutPreviewStatus::Ready,
      reason: None,
      known_limits: vec!["holdout preview only".to_string()],
    };
    let holdout_inspect_value = TrainingResultHoldoutPreviewInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_holdout_preview_manifest_path:
        "/tmp/holdout/minecraft-3dgs-training-result-holdout-preview.json".to_string(),
      training_result_semantic_manifest_path: holdout_manifest_value
        .training_result_semantic_manifest_path
        .clone(),
      source_training_result_artifact_manifest_path: holdout_manifest_value
        .source_training_result_artifact_manifest_path
        .clone(),
      source_training_result_manifest_path: holdout_manifest_value
        .source_training_result_manifest_path
        .clone(),
      source_training_job_manifest_path: holdout_manifest_value
        .source_training_job_manifest_path
        .clone(),
      source_training_launch_plan_path: holdout_manifest_value
        .source_training_launch_plan_path
        .clone(),
      source_training_package_manifest_path: holdout_manifest_value
        .source_training_package_manifest_path
        .clone(),
      source_scene_packet_manifest_path: holdout_manifest_value
        .source_scene_packet_manifest_path
        .clone(),
      source_bundle_manifest_paths: holdout_manifest_value.source_bundle_manifest_paths.clone(),
      source_run_ids: holdout_manifest_value.source_run_ids.clone(),
      trainer_backend: holdout_manifest_value.trainer_backend.clone(),
      job_backend: holdout_manifest_value.job_backend.clone(),
      normalized_result_dir: holdout_manifest_value.normalized_result_dir.clone(),
      holdout_frame_index: 6,
      holdout_frame: Some(holdout_witness),
      basis_checkpoint_path: holdout_manifest_value.basis_checkpoint_path.clone(),
      holdout_screenshot_path: holdout_manifest_value.holdout_screenshot_path.clone(),
      reference_overlay_path: holdout_manifest_value.reference_overlay_path.clone(),
      status: HoldoutPreviewStatus::Ready,
      reason: None,
      holdout_frame_selection: HoldoutFrameSelection::LastInGame,
      checkpoint_count: 1,
      scene_packet_frame_count: 6,
      warnings: vec![],
      known_limits: vec!["holdout inspect only".to_string()],
    };
    let minecraft_training_result_holdout_preview_manifests =
      vec![MinecraftTrainingResultHoldoutPreviewManifestLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc16_holdout_manifest"),
          span_id: SpanId::new("span_mc16_holdout"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-holdout-preview".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-result-holdout-preview.json".to_string()),
          summary: Some("training result holdout preview manifest".to_string()),
          resolved: true,
        },
        manifest: Some(holdout_manifest_value.into()),
        issue: None,
      }];
    let minecraft_training_result_holdout_preview_inspect_reports =
      vec![MinecraftTrainingResultHoldoutPreviewInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc16_holdout_inspect"),
          span_id: SpanId::new("span_mc16_holdout"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-holdout-preview-inspect".to_string()),
          path: Some(
            "artifacts/minecraft-3dgs-training-result-holdout-preview-inspect.json".to_string(),
          ),
          summary: Some("training result holdout preview inspect".to_string()),
          resolved: true,
        },
        report: Some(holdout_inspect_value.into()),
        issue: None,
      }];

    let minecraft_training_result_spatial_query_manifests =
      vec![MinecraftTrainingResultSpatialQueryManifestLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc12_query_manifest"),
          span_id: SpanId::new("span_mc12_query"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-query".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-result-query.json".to_string()),
          summary: Some("training result spatial query manifest".to_string()),
          resolved: true,
        },
        manifest: Some(
          crate::run_read::MinecraftTrainingResultSpatialQueryManifestSummary {
            schema_version: 1,
            training_result_semantic_manifest_path:
              "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
            source_training_result_artifact_manifest_path:
              "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
            source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
              .to_string(),
            source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json"
              .to_string(),
            source_training_launch_plan_path:
              "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
            source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec![
              "/tmp/bundle-a/run.json".to_string(),
              "/tmp/bundle-b/run.json".to_string(),
            ],
            source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
            trainer_backend: "nerfstudio.splatfacto".to_string(),
            job_backend: "remote".to_string(),
            normalized_result_dir:
              "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result".to_string(),
            query_kind: "block_projection".to_string(),
            target_block: "511,73,728".to_string(),
            target_face: Some("north".to_string()),
            target_semantics: "hit_face_center".to_string(),
            selected_backend: Some("projection_reference".to_string()),
            status: "answered".to_string(),
            reason: None,
            visibility: Some("visible".to_string()),
            screen_point: Some("854.0,480.0".to_string()),
            match_radius_px: Some(8.0),
            confidence: Some(0.9),
            basis_frame_id: Some("frame-355416".to_string()),
            comparison_verdict: Some("reference_only".to_string()),
            known_limits: vec!["projection_reference only".to_string()],
          },
        ),
        issue: None,
      }];
    let minecraft_training_result_spatial_query_inspect_reports =
      vec![MinecraftTrainingResultSpatialQueryInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc12_query_inspect"),
          span_id: SpanId::new("span_mc12_query"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-query-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-result-query-inspect.json".to_string()),
          summary: Some("training result spatial query inspect".to_string()),
          resolved: true,
        },
        report: Some(
          crate::run_read::MinecraftTrainingResultSpatialQueryInspectReportSummary {
            schema_version: 1,
            training_result_spatial_query_manifest_path:
              "/tmp/query/minecraft-3dgs-training-result-query.json".to_string(),
            training_result_semantic_manifest_path:
              "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
            source_training_result_artifact_manifest_path:
              "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
            source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
              .to_string(),
            source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json"
              .to_string(),
            source_training_launch_plan_path:
              "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
            source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec![
              "/tmp/bundle-a/run.json".to_string(),
              "/tmp/bundle-b/run.json".to_string(),
            ],
            source_run_ids: vec!["run_a".to_string(), "run_b".to_string()],
            trainer_backend: "nerfstudio.splatfacto".to_string(),
            job_backend: "remote".to_string(),
            normalized_result_dir:
              "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result".to_string(),
            query_kind: "block_projection".to_string(),
            target_block: "511,73,728".to_string(),
            target_face: Some("north".to_string()),
            target_semantics: "hit_face_center".to_string(),
            selected_backend: Some("projection_reference".to_string()),
            status: "answered".to_string(),
            reason: None,
            visibility: Some("visible".to_string()),
            screen_point: Some("854.0,480.0".to_string()),
            match_radius_px: Some(8.0),
            confidence: Some(0.9),
            basis_frame_id: Some("frame-355416".to_string()),
            comparison_verdict: Some("reference_only".to_string()),
            provider_status: "blocked".to_string(),
            provider_reason: None,
            provider_message: None,
            reference_status: "answered".to_string(),
            reference_reason: None,
            reference_basis_frame_id: Some("frame-355416".to_string()),
            reference_source_frame_json_path: Some(
              "/tmp/scene-packet/frames/frame_000001.json".to_string(),
            ),
            reference_screenshot_path: None,
            scene_packet_frame_count: 12,
            warnings: vec![],
            known_limits: vec!["query inspect only".to_string()],
          },
        ),
        issue: None,
      }];

    let output = render_run_text(
      &run,
      &verifications,
      &observation_snapshots,
      &detector_recognition_lineage,
      &candidate_promotion_lineage,
      &candidate_action_decision_lineage,
      &candidate_action_execution_lineage,
      &minecraft_projection_artifacts,
      &minecraft_telemetry_sample_artifacts,
      &[],
      &minecraft_training_package_manifests,
      &minecraft_training_package_inspect_reports,
      &minecraft_training_launch_manifests,
      &minecraft_training_launch_inspect_reports,
      &minecraft_training_job_manifests,
      &minecraft_training_job_inspect_reports,
      &minecraft_training_result_manifests,
      &minecraft_training_result_inspect_reports,
      &minecraft_training_result_artifact_fetch_manifests,
      &minecraft_training_result_artifact_fetch_inspect_reports,
      &minecraft_training_result_semantic_manifests,
      &minecraft_training_result_semantic_inspect_reports,
      &minecraft_training_result_holdout_preview_manifests,
      &minecraft_training_result_holdout_preview_inspect_reports,
      &[],
      &[],
      &minecraft_training_result_spatial_query_manifests,
      &minecraft_training_result_spatial_query_inspect_reports,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("Run run_inspect_test"));
    assert!(output.contains("Type: command"));
    assert!(output.contains("Status: ok"));
    assert!(output.contains("auv.inspect.span"));
    assert!(output.contains("inspect.event"));
    assert!(output.contains("artifact_test"));
    assert!(output.contains("Command Boundary Claims:"));
    assert!(output.contains(
      "verification=activation-only; semantic success requires a separate verification result"
    ));
    assert!(output.contains("known_limit=input delivery does not verify target UI state"));
    assert!(output.contains("Verifications:"));
    assert!(output.contains("method=semantic_match"));
    assert!(output.contains("Observations:"));
    assert!(output.contains("snapshot_1"));
    assert!(output.contains("Detector Recognition Lineage:"));
    assert!(output.contains("artifact=artifact_detector_recognition"));
    assert!(output.contains("status=ready"));
    assert!(output.contains("model=games-balatro-ui"));
    assert!(output.contains("backend=ultralytics-inference"));
    assert!(output.contains("capture=artifacts/capture.png"));
    assert!(output.contains("known_limits=projection basis is unavailable outside capture-integrated runtime | detector RecognitionResult is recognition evidence only, not candidate-ready output"));
    assert!(output.contains("Candidate Promotion Lineage:"));
    assert!(output.contains("artifact=artifact_candidate_promotion"));
    assert!(output.contains("promotion_id=promotion_end_turn"));
    assert!(output.contains("decision=promoted"));
    assert!(output.contains("projection=identity_window_addressable"));
    assert!(output.contains("source_recognition=artifacts/detector-recognition.json"));
    assert!(output.contains("freshness_source=artifacts/capture.png"));
    assert!(output.contains("consent_scope=candidate_promotion_only"));
    assert!(output.contains("consent_provenance=human_gesture"));
    assert!(output.contains("consent_grade=human_approved"));
    assert!(output.contains("permission_by=human-review"));
    assert!(output.contains("Candidate Action Decision Lineage:"));
    assert!(output.contains("artifact=artifact_candidate_action_decision"));
    assert!(output.contains("decision_id=decision_end_turn"));
    assert!(output.contains("resolver=candidate.action.decide_only"));
    assert!(output.contains("selected=pointer-click"));
    assert!(output.contains("side_effect=none_decide_only"));
    assert!(output.contains("input_delivery=not_attempted"));
    assert!(output.contains("operation_result=not_produced"));
    assert!(output.contains("verification_result=not_produced"));
    assert!(output.contains("cursor=warp-visible"));
    assert!(output.contains("MC-2 Projection Artifacts:"));
    assert!(output.contains("frame=frame-1"));
    assert!(output.contains("screenshot_artifact_ref=artifact://screenshot-1"));
    assert!(output.contains("capture_skew_ms=180"));
    assert!(output.contains("verification_reference=verification-1"));
    assert!(output.contains(
      "projected_point=screen=320,240 visibility=visible radius_px=12 confidence=1 basis=frame-1"
    ));
    assert!(output.contains("MC-6 Spatial Bundles:"));
    assert!(output.contains("MC-7 Training Packages:"));
    assert!(output.contains("manifest_artifact=artifact_mc7_package"));
    assert!(output.contains("compatibility_status=partial"));
    assert!(output.contains("exported=4"));
    assert!(output.contains("skipped=2"));
    assert!(output.contains("transforms=present"));
    assert!(output.contains("paired_report_artifact=artifact_mc7_package_inspect"));
    assert!(output.contains("known_limits=canonical package only; no trained splat is present"));
    assert!(output.contains("MC-7 Training Launches:"));
    assert!(output.contains("manifest_artifact=artifact_mc7_launch"));
    assert!(output.contains("paired_report_artifact=artifact_mc7_launch_inspect"));
    assert!(output.contains("trainer_readiness=Blocked"));
    assert!(output.contains("readiness_blocker=TrainerCommandUnavailable"));
    assert!(output.contains("launch_command=ns-train splatfacto --data /tmp/package/compat/nerfstudio --output-dir /tmp/launch/trainer-output/nerfstudio-splatfacto"));
    assert!(output.contains("MC-7 Training Jobs:"));
    assert!(output.contains("manifest_artifact=artifact_mc7_job"));
    assert!(output.contains("paired_report_artifact=artifact_mc7_job_inspect"));
    assert!(output.contains("provider_backend=remote-command-provider"));
    assert!(output.contains("job_backend=remote"));
    assert!(output.contains("status=submitted"));
    assert!(output.contains("accepted_by_provider=true"));
    assert!(output.contains("job_id=job-123"));
    assert!(output.contains("job_submission_endpoint=https://jobs.example/api"));
    assert!(output.contains("MC-7 Training Results:"));
    assert!(output.contains("manifest_artifact=artifact_mc7_result"));
    assert!(output.contains("paired_report_artifact=artifact_mc7_result_inspect"));
    assert!(output.contains("source_job_status=submitted"));
    assert!(output.contains("provider_status=succeeded"));
    assert!(output.contains("status_message=provider succeeded"));
    assert!(output.contains("status_reason=n/a"));
    assert!(output.contains("local_result_observation result_dir_exists=true"));
    assert!(output.contains("key_result_artifacts_present=true"));
    assert!(output.contains("result_artifact_count=1"));
    assert!(output.contains("MC-7 Training Result Artifacts:"));
    assert!(output.contains("manifest_artifact=artifact_mc7_result_artifact_manifest"));
    assert!(output.contains("paired_report_artifact=artifact_mc7_result_artifact_inspect"));
    assert!(output.contains("fetch_status=succeeded"));
    assert!(output.contains("required_artifacts_present=true"));
    assert!(output.contains("normalized_artifact_count=3"));
    assert!(output.contains("kind=config relative_path=config.yml readable=true byte_size=128"));
    assert!(output.contains(
      "kind=models_directory relative_path=nerfstudio_models readable=true byte_size=n/a"
    ));
    assert!(
      output
        .contains("kind=status_snapshot relative_path=job_status.json readable=true byte_size=32")
    );
    assert!(output.contains("normalized artifacts only"));
    assert!(output.contains("MC-10 Training Result Semantics:"));
    assert!(output.contains("manifest_artifact=artifact_mc10_semantic_manifest"));
    assert!(output.contains("paired_report_artifact=artifact_mc10_semantic_inspect"));
    assert!(output.contains("semantic_status=ready"));
    assert!(output.contains("semantic_reason=n/a"));
    assert!(output.contains("config_backend_matches=true"));
    assert!(output.contains("checkpoint_count=1"));
    assert!(output.contains("MC-16 Training Result Holdout Preview:"));
    assert!(output.contains("manifest_artifact=artifact_mc16_holdout_manifest"));
    assert!(output.contains("paired_report_artifact=artifact_mc16_holdout_inspect"));
    assert!(output.contains("holdout_frame_index=6"));
    assert!(output.contains("spatial_frame_id=frame-355416-47699343801916"));
    assert!(output.contains("status=ready"));
    assert!(output.contains("holdout_frame_selection=last_in_game"));
    let mc10_section = output
      .find("MC-10 Training Result Semantics:")
      .expect("mc10 section");
    let mc16_section = output
      .find("MC-16 Training Result Holdout Preview:")
      .expect("mc16 section");
    let mc12_section = output
      .find("MC-12 Training Result Spatial Query:")
      .expect("mc12 section");
    assert!(mc10_section < mc16_section);
    assert!(mc16_section < mc12_section);
    assert!(output.contains("MC-12 Training Result Spatial Query:"));
    assert!(output.contains("manifest_artifact=artifact_mc12_query_manifest"));
    assert!(output.contains("paired_report_artifact=artifact_mc12_query_inspect"));
    assert!(output.contains("selected_backend=projection_reference"));
    assert!(output.contains("visibility=visible"));
    assert!(output.contains("target_block=511,73,728"));
    assert!(output.contains("comparison_verdict=reference_only"));
    assert!(output.contains("provider_status=blocked"));
    assert!(output.contains("reference_status=answered"));
    assert!(output.contains("scene_packet_frame_count=12"));
    assert!(output.contains("MC-14 Training Result Spatial Query Action Readiness:"));
    assert!(output.contains("query_artifact=artifact_mc12_query_manifest"));
    assert!(output.contains("action_eligibility=click_ready"));
    assert!(output.contains("paired_inspect_artifact=artifact_mc12_query_inspect"));

    assert!(output.contains("normalized_artifacts=3"));
    assert!(output.contains("Candidate Action Execution Lineage:"));
    assert!(output.contains("artifact=artifact_candidate_action_execution"));
    assert!(output.contains("execution_id=execution_end_turn"));
    assert!(output.contains("input_delivery=attempted"));
    assert!(output.contains("selected_path=window_targeted_mouse"));
    assert!(output.contains("operation_status=completed"));
    assert!(output.contains("verification=activation_only"));
    assert!(output.contains("closure_state=semantic_open"));
    assert!(output.contains("semantic_matched=n/a"));
    assert!(output.contains("side_effect=single_input_delivered"));
    assert!(output.contains("consent=consent_execute_end_turn"));
    assert!(output.contains("consent_provenance=human_gesture"));
    assert!(output.contains("consent_grade=human_approved"));
  }

  #[test]
  fn render_run_text_renders_training_orphan_and_issue_entries() {
    let run_id = RunId::new("run_inspect_orphan_test");
    let root_span_id = SpanId::new("span_orphan_root");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_orphan_test"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id,
        attributes: BTreeMap::new(),
        summary: Some("orphan inspect summary".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[MinecraftTrainingJobInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_job_orphan"),
          span_id: SpanId::new("span_mc7_job_orphan"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-job-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-job-inspect-orphan.json".to_string()),
          summary: Some("training job orphan inspect".to_string()),
          resolved: true,
        },
        report: None,
        issue: Some("json parse error: expected value".to_string()),
      }],
      &[],
      &[],
      &[MinecraftTrainingResultArtifactFetchManifestLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_result_artifact_manifest_orphan"),
          span_id: SpanId::new("span_mc7_result_artifact_orphan"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-artifact-manifest".to_string()),
          path: Some(
            "artifacts/minecraft-3dgs-training-result-artifact-manifest-orphan.json".to_string(),
          ),
          summary: Some("training result artifact orphan manifest".to_string()),
          resolved: true,
        },
        manifest: None,
        issue: Some("json parse error: expected value".to_string()),
      }],
      &[MinecraftTrainingResultArtifactFetchInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_result_artifact_orphan"),
          span_id: SpanId::new("span_mc7_result_artifact_orphan"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-artifact-inspect".to_string()),
          path: Some(
            "artifacts/minecraft-3dgs-training-result-artifact-inspect-orphan.json".to_string(),
          ),
          summary: Some("training result artifact orphan inspect".to_string()),
          resolved: true,
        },
        report: None,
        issue: Some("json parse error: expected value".to_string()),
      }],
      &[],
      &[MinecraftTrainingResultSemanticInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc10_semantic_orphan"),
          span_id: SpanId::new("span_mc10_semantic_orphan"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-semantic-inspect".to_string()),
          path: Some(
            "artifacts/minecraft-3dgs-training-result-semantic-inspect-orphan.json".to_string(),
          ),
          summary: Some("training result semantic orphan inspect".to_string()),
          resolved: true,
        },
        report: None,
        issue: Some("json parse error: expected value".to_string()),
      }],
      &[],
      &[MinecraftTrainingResultHoldoutPreviewInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc16_holdout_orphan"),
          span_id: SpanId::new("span_mc16_holdout_orphan"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-holdout-preview-inspect".to_string()),
          path: Some(
            "artifacts/minecraft-3dgs-training-result-holdout-preview-inspect-orphan.json"
              .to_string(),
          ),
          summary: Some("training result holdout preview orphan inspect".to_string()),
          resolved: true,
        },
        report: None,
        issue: Some("json parse error: expected value".to_string()),
      }],
      &[],
      &[],
      &[],
      &[MinecraftTrainingResultSpatialQueryInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc12_query_orphan"),
          span_id: SpanId::new("span_mc12_query_orphan"),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-query-inspect".to_string()),
          path: Some(
            "artifacts/minecraft-3dgs-training-result-query-inspect-orphan.json".to_string(),
          ),
          summary: Some("training result spatial query orphan inspect".to_string()),
          resolved: true,
        },
        report: None,
        issue: Some("json parse error: expected value".to_string()),
      }],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("MC-7 Training Jobs:"));
    assert!(output.contains("inspect_artifact=artifact_mc7_job_orphan"));
    assert!(output.contains("path=artifacts/minecraft-3dgs-training-job-inspect-orphan.json"));
    assert!(output.contains("issue=json parse error: expected value"));
    assert!(output.contains("MC-7 Training Result Artifacts:"));
    assert!(output.contains("inspect_artifact=artifact_mc7_result_artifact_orphan"));
    assert!(
      output.contains("path=artifacts/minecraft-3dgs-training-result-artifact-inspect-orphan.json")
    );
    assert!(output.contains("issue=json parse error: expected value"));
    assert!(output.contains("MC-10 Training Result Semantics:"));
    assert!(output.contains("inspect_artifact=artifact_mc10_semantic_orphan"));
    assert!(
      output.contains("path=artifacts/minecraft-3dgs-training-result-semantic-inspect-orphan.json")
    );
    assert!(output.contains("MC-16 Training Result Holdout Preview:"));
    assert!(output.contains("inspect_artifact=artifact_mc16_holdout_orphan"));
    assert!(output.contains(
      "path=artifacts/minecraft-3dgs-training-result-holdout-preview-inspect-orphan.json"
    ));
    assert!(output.contains("MC-12 Training Result Spatial Query:"));
    assert!(output.contains("inspect_artifact=artifact_mc12_query_orphan"));
    assert!(
      output.contains("path=artifacts/minecraft-3dgs-training-result-query-inspect-orphan.json")
    );
  }

  #[test]
  fn render_run_text_renders_spatial_query_action_readiness_three_states() {
    use crate::run_read::MinecraftTrainingResultSpatialQueryManifestSummary;

    let run_id = RunId::new("run_inspect_mc14_three_states");
    let root_span_id = SpanId::new("span_mc14_root");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_mc14_three_states"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("mc14 three states".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };

    fn query_manifest(
      run: &CanonicalRun,
      span_id: SpanId,
      artifact_id: &str,
      summary: MinecraftTrainingResultSpatialQueryManifestSummary,
    ) -> MinecraftTrainingResultSpatialQueryManifestLineage {
      MinecraftTrainingResultSpatialQueryManifestLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new(artifact_id),
          span_id,
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-query".to_string()),
          path: Some(format!("artifacts/{artifact_id}.json")),
          summary: Some("spatial query manifest".to_string()),
          resolved: true,
        },
        manifest: Some(summary),
        issue: None,
      }
    }

    fn base_summary() -> MinecraftTrainingResultSpatialQueryManifestSummary {
      MinecraftTrainingResultSpatialQueryManifestSummary {
        schema_version: 1,
        training_result_semantic_manifest_path: "/tmp/semantic.json".to_string(),
        source_training_result_artifact_manifest_path: "/tmp/artifact.json".to_string(),
        source_training_result_manifest_path: "/tmp/result.json".to_string(),
        source_training_job_manifest_path: "/tmp/job.json".to_string(),
        source_training_launch_plan_path: "/tmp/launch.json".to_string(),
        source_training_package_manifest_path: "/tmp/package.json".to_string(),
        source_scene_packet_manifest_path: "/tmp/scene-packet.json".to_string(),
        source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
        source_run_ids: vec!["run-1".to_string()],
        trainer_backend: "nerfstudio.splatfacto".to_string(),
        job_backend: "remote".to_string(),
        normalized_result_dir: "/tmp/normalized".to_string(),
        query_kind: "block_projection".to_string(),
        target_block: "511,73,728".to_string(),
        target_face: None,
        target_semantics: "hit_face_center".to_string(),
        selected_backend: Some("projection_reference".to_string()),
        status: "answered".to_string(),
        reason: None,
        visibility: Some("visible".to_string()),
        screen_point: Some("854.0,480.0".to_string()),
        match_radius_px: Some(8.0),
        confidence: Some(0.9),
        basis_frame_id: Some("frame-1".to_string()),
        comparison_verdict: Some("reference_only".to_string()),
        known_limits: vec![],
      }
    }

    let click_ready = base_summary();
    let mut outside_window = base_summary();
    outside_window.selected_backend = Some("command_provider".to_string());
    outside_window.visibility = Some("outside_window".to_string());
    outside_window.screen_point = None;
    outside_window.comparison_verdict = Some("divergent".to_string());

    let mut absent = base_summary();
    absent.target_block = "9,9,9".to_string();
    absent.selected_backend = None;
    absent.status = "failed".to_string();
    absent.reason = Some("target_block_absent_from_scene_packet".to_string());
    absent.visibility = None;
    absent.screen_point = None;
    absent.comparison_verdict = Some("not_comparable".to_string());

    let manifests = vec![
      query_manifest(
        &run,
        root_span_id.clone(),
        "artifact_mc14_click_ready",
        click_ready,
      ),
      query_manifest(
        &run,
        root_span_id.clone(),
        "artifact_mc14_outside_window",
        outside_window,
      ),
      query_manifest(&run, root_span_id.clone(), "artifact_mc14_absent", absent),
    ];

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &manifests,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("MC-14 Training Result Spatial Query Action Readiness:"));
    assert!(output.contains("query_artifact=artifact_mc14_click_ready"));
    assert!(output.contains("action_eligibility=click_ready"));
    assert!(output.contains("query_artifact=artifact_mc14_outside_window"));
    assert!(output.contains("action_eligibility=answer_non_clickable"));
    assert!(output.contains("refusal_reason=visibility=outside_window"));
    assert!(output.contains("query_artifact=artifact_mc14_absent"));
    assert!(output.contains("action_eligibility=not_consumable"));
    assert!(output.contains("readiness_class=not_consumable"));
    assert!(
      output.contains("refusal_reason=status=failed reason=target_block_absent_from_scene_packet")
    );
  }

  #[test]
  fn render_run_text_renders_osu_visual_truth_action_readiness_three_states() {
    use crate::run_read::OsuVisualTruthSpatialQueryManifestSummary;

    let run_id = RunId::new("run_inspect_osu_mc14_three_states");
    let root_span_id = SpanId::new("span_osu_mc14_root");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_osu_mc14_three_states"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("osu mc14 three states".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };

    fn query_manifest(
      run: &CanonicalRun,
      span_id: SpanId,
      artifact_id: &str,
      summary: OsuVisualTruthSpatialQueryManifestSummary,
    ) -> OsuVisualTruthSpatialQueryManifestLineage {
      OsuVisualTruthSpatialQueryManifestLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new(artifact_id),
          span_id,
          captured_event_id: None,
          role: Some("osu-visual-truth-spatial-query".to_string()),
          path: Some(format!("artifacts/{artifact_id}.json")),
          summary: Some("osu spatial query manifest".to_string()),
          resolved: true,
        },
        manifest: Some(summary),
        issue: None,
      }
    }

    fn base_summary() -> OsuVisualTruthSpatialQueryManifestSummary {
      OsuVisualTruthSpatialQueryManifestSummary {
        schema_version: 1,
        visual_truth_semantic_manifest_path: "/tmp/semantic.json".to_string(),
        source_run_artifact_dir: "/tmp/run".to_string(),
        object_index: 0,
        capture_phase: "before_dispatch".to_string(),
        object_kind: Some("circle".to_string()),
        query_backend: "playfield_projection_reference".to_string(),
        status: "answered".to_string(),
        reason: None,
        pixel_visibility: Some("inside_capture".to_string()),
        pixel_x: Some(400.0),
        pixel_y: Some(300.0),
        match_radius_px: Some(20.0),
        capture_width: Some(800),
        capture_height: Some(600),
        known_limits: vec![],
      }
    }

    let click_ready = base_summary();
    let mut outside_capture = base_summary();
    outside_capture.pixel_visibility = Some("outside_capture".to_string());

    let mut absent = base_summary();
    absent.status = "failed".to_string();
    absent.reason = Some("target_absent_from_visual_truth".to_string());
    absent.pixel_visibility = None;
    absent.pixel_x = None;
    absent.pixel_y = None;

    let manifests = vec![
      query_manifest(
        &run,
        root_span_id.clone(),
        "artifact_osu_click_ready",
        click_ready,
      ),
      query_manifest(
        &run,
        root_span_id.clone(),
        "artifact_osu_outside_capture",
        outside_capture,
      ),
      query_manifest(&run, root_span_id.clone(), "artifact_osu_absent", absent),
    ];

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &manifests,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("Osu Visual Truth Spatial Query Action Readiness:"));
    assert!(output.contains("query_artifact=artifact_osu_click_ready"));
    assert!(output.contains("action_eligibility=click_ready"));
    assert!(output.contains("query_artifact=artifact_osu_outside_capture"));
    assert!(output.contains("action_eligibility=answer_non_clickable"));
    assert!(output.contains("refusal_reason=pixel_visibility=outside_capture"));
    assert!(output.contains("query_artifact=artifact_osu_absent"));
    assert!(output.contains("action_eligibility=not_consumable"));
    assert!(output.contains("refusal_reason=status=failed reason=target_absent_from_visual_truth"));
  }

  #[test]
  fn render_run_text_renders_query_wired_live_action_three_gates() {
    use crate::run_read::MinecraftQueryWiredLiveActionSummary;

    let run_id = RunId::new("run_inspect_mc19_three_gates");
    let root_span_id = SpanId::new("span_mc19_root");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_mc19_three_gates"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("mc19 three gates".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };

    let summaries = vec![
      MinecraftQueryWiredLiveActionSummary {
        operation_result_artifact_id: Some("artifact_mc19_click_ready_op".to_string()),
        query_artifact_id: Some("artifact_mc19_click_ready_query".to_string()),
        attempted: true,
        action_eligibility: "click_ready".to_string(),
        window_point: Some("854.0,480.0".to_string()),
        refusal_reason: None,
        operation_status: Some("completed".to_string()),
        operation_message: Some("mock live click dispatched".to_string()),
        target_app: Some("net.minecraft.client".to_string()),
        target_title: Some("Minecraft".to_string()),
        dispatch_command: Some("input.clickWindowPoint".to_string()),
        dispatch_outcome: Some("failed: main visible window was not found".to_string()),
        mc14_action_eligibility: Some("click_ready".to_string()),
        readiness_class: Some("ready".to_string()),
        issue: None,
      },
      MinecraftQueryWiredLiveActionSummary {
        operation_result_artifact_id: Some("artifact_mc19_outside_op".to_string()),
        query_artifact_id: Some("artifact_mc19_outside_query".to_string()),
        attempted: false,
        action_eligibility: "answer_non_clickable".to_string(),
        window_point: None,
        refusal_reason: Some("visibility=outside_window".to_string()),
        operation_status: Some("completed".to_string()),
        operation_message: Some("visibility=outside_window".to_string()),
        target_app: Some("net.minecraft.client".to_string()),
        target_title: Some("Minecraft".to_string()),
        dispatch_command: None,
        dispatch_outcome: None,
        mc14_action_eligibility: Some("answer_non_clickable".to_string()),
        readiness_class: Some("non_actionable".to_string()),
        issue: None,
      },
      MinecraftQueryWiredLiveActionSummary {
        operation_result_artifact_id: Some("artifact_mc19_absent_op".to_string()),
        query_artifact_id: Some("artifact_mc19_absent_query".to_string()),
        attempted: false,
        action_eligibility: "not_consumable".to_string(),
        window_point: None,
        refusal_reason: Some(
          "status=failed reason=target_block_absent_from_scene_packet".to_string(),
        ),
        operation_status: Some("completed".to_string()),
        operation_message: Some(
          "status=failed reason=target_block_absent_from_scene_packet".to_string(),
        ),
        target_app: Some("net.minecraft.client".to_string()),
        target_title: Some("Minecraft".to_string()),
        dispatch_command: None,
        dispatch_outcome: None,
        mc14_action_eligibility: Some("not_consumable".to_string()),
        readiness_class: Some("not_consumable".to_string()),
        issue: None,
      },
    ];

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &summaries,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("MC-19 Query Wired Live Action:"));
    assert!(output.contains("operation_result_artifact=artifact_mc19_click_ready_op"));
    assert!(output.contains("attempted=true"));
    assert!(output.contains("action_eligibility=click_ready"));
    assert!(output.contains("readiness_class=ready"));
    assert!(output.contains("dispatch_command=input.clickWindowPoint"));
    assert!(output.contains("operation_result_artifact=artifact_mc19_outside_op"));
    assert!(output.contains("refusal_reason=visibility=outside_window"));
    assert!(output.contains("operation_result_artifact=artifact_mc19_absent_op"));
    assert!(output.contains("action_eligibility=not_consumable"));
    assert!(
      output.contains("refusal_reason=status=failed reason=target_block_absent_from_scene_packet")
    );
  }

  #[test]
  fn render_run_text_renders_osu_query_wired_live_action_three_gates() {
    use crate::run_read::OsuQueryWiredLiveActionSummary;

    let run_id = RunId::new("run_inspect_osu_wired_three_gates");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_osu_wired_three_gates"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: SpanId::new("span_osu_wired_root"),
        attributes: BTreeMap::new(),
        summary: Some("osu wired three gates".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };

    let summaries = vec![
      OsuQueryWiredLiveActionSummary {
        operation_result_artifact_id: Some("artifact_osu_click_ready_op".to_string()),
        query_artifact_id: Some("artifact_osu_click_ready_query".to_string()),
        attempted: true,
        action_eligibility: "click_ready".to_string(),
        pixel_point: Some("400.0,300.0".to_string()),
        window_point: Some("400.000,300.000".to_string()),
        refusal_reason: None,
        operation_status: Some("completed".to_string()),
        operation_message: Some("mock live click dispatched".to_string()),
        target_app: Some("osu!".to_string()),
        target_title: Some("osu".to_string()),
        dispatch_command: Some("input.clickWindowPoint".to_string()),
        dispatch_outcome: Some("failed: main visible window was not found".to_string()),
        readiness_class: Some("ready".to_string()),
        issue: None,
      },
      OsuQueryWiredLiveActionSummary {
        operation_result_artifact_id: Some("artifact_osu_outside_op".to_string()),
        query_artifact_id: Some("artifact_osu_outside_query".to_string()),
        attempted: false,
        action_eligibility: "answer_non_clickable".to_string(),
        pixel_point: Some("900.0,300.0".to_string()),
        window_point: None,
        refusal_reason: Some("pixel_visibility=outside_capture".to_string()),
        operation_status: Some("completed".to_string()),
        operation_message: Some("pixel_visibility=outside_capture".to_string()),
        target_app: Some("osu!".to_string()),
        target_title: Some("osu".to_string()),
        dispatch_command: None,
        dispatch_outcome: None,
        readiness_class: Some("non_actionable".to_string()),
        issue: None,
      },
      OsuQueryWiredLiveActionSummary {
        operation_result_artifact_id: Some("artifact_osu_absent_op".to_string()),
        query_artifact_id: Some("artifact_osu_absent_query".to_string()),
        attempted: false,
        action_eligibility: "not_consumable".to_string(),
        pixel_point: None,
        window_point: None,
        refusal_reason: Some("status=failed reason=target_absent_from_visual_truth".to_string()),
        operation_status: Some("completed".to_string()),
        operation_message: Some("status=failed reason=target_absent_from_visual_truth".to_string()),
        target_app: Some("osu!".to_string()),
        target_title: Some("osu".to_string()),
        dispatch_command: None,
        dispatch_outcome: None,
        readiness_class: Some("not_consumable".to_string()),
        issue: None,
      },
    ];

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &summaries,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("Osu Visual Truth Query Wired Live Action:"));
    assert!(output.contains("pixel_point=400.0,300.0"));
    assert!(output.contains("window_point=400.000,300.000"));
    assert!(output.contains("action_eligibility=click_ready"));
    assert!(output.contains("readiness_class=ready"));
    assert!(output.contains("dispatch_command=input.clickWindowPoint"));
    assert!(output.contains("refusal_reason=pixel_visibility=outside_capture"));
    assert!(output.contains("readiness_class=non_actionable"));
    assert!(output.contains("action_eligibility=not_consumable"));
    assert!(output.contains("readiness_class=not_consumable"));
    assert!(output.contains("refusal_reason=status=failed reason=target_absent_from_visual_truth"));
  }

  #[test]
  fn render_run_text_leaves_duplicate_training_launch_reports_unpaired() {
    let run_id = RunId::new("run_inspect_duplicate_launch_reports");
    let root_span_id = SpanId::new("span_duplicate_launch_root");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_duplicate_launch_test"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("duplicate launch reports".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };

    let launch_manifest = MinecraftTrainingLaunchManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_mc7_launch_manifest"),
        span_id: root_span_id.clone(),
        captured_event_id: None,
        role: Some("minecraft-3dgs-training-launch-plan".to_string()),
        path: Some("artifacts/minecraft-3dgs-training-launch-plan.json".to_string()),
        summary: Some("training launch manifest".to_string()),
        resolved: true,
      },
      manifest: Some(crate::run_read::MinecraftTrainingLaunchManifestSummary {
        schema_version: 1,
        source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
        source_training_package_inspect_report_path: "/tmp/package/inspect_report.json".to_string(),
        source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
        source_run_ids: vec!["run_a".to_string()],
        counts: TrainingPackageCounts {
          frames: 2,
          images: 2,
          compatibility_exported_frames: 2,
          compatibility_skipped_frames: 0,
        },
        compatibility_view_name: "nerfstudio".to_string(),
        trainer_backend: "nerfstudio.splatfacto".to_string(),
        training_data_dir: "/tmp/package/compat/nerfstudio".to_string(),
        transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
        export_report_path: "compat/nerfstudio/export_report.json".to_string(),
        suggested_output_dir: "/tmp/launch/out".to_string(),
        launch_command: "ns-train splatfacto".to_string(),
        known_limits: vec![],
      }),
      issue: None,
    };

    let duplicate_reports = vec![
      MinecraftTrainingLaunchInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_launch_report_a"),
          span_id: root_span_id.clone(),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-launch-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-launch-inspect-a.json".to_string()),
          summary: Some("training launch inspect a".to_string()),
          resolved: true,
        },
        report: Some(
          crate::run_read::MinecraftTrainingLaunchInspectReportSummary {
            schema_version: 1,
            training_launch_manifest_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
              .to_string(),
            source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
            source_run_ids: vec!["run_a".to_string()],
            compatibility_status: "Ready".to_string(),
            trainer_readiness: "Ready".to_string(),
            readiness_blocker: None,
            probe_command: "ns-train --help".to_string(),
            probe_succeeded: true,
            exported_frame_count: 2,
            skipped_frame_count: 0,
            transforms_present: true,
            warnings: vec![],
            known_limits: vec![],
          },
        ),
        issue: None,
      },
      MinecraftTrainingLaunchInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_launch_report_b"),
          span_id: root_span_id,
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-launch-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-launch-inspect-b.json".to_string()),
          summary: Some("training launch inspect b".to_string()),
          resolved: true,
        },
        report: Some(
          crate::run_read::MinecraftTrainingLaunchInspectReportSummary {
            schema_version: 1,
            training_launch_manifest_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
              .to_string(),
            source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
            source_run_ids: vec!["run_a".to_string()],
            compatibility_status: "Blocked".to_string(),
            trainer_readiness: "Blocked".to_string(),
            readiness_blocker: Some("TrainerCommandUnavailable".to_string()),
            probe_command: "ns-train --help".to_string(),
            probe_succeeded: false,
            exported_frame_count: 1,
            skipped_frame_count: 1,
            transforms_present: true,
            warnings: vec!["duplicate report".to_string()],
            known_limits: vec![],
          },
        ),
        issue: None,
      },
    ];

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[launch_manifest],
      &duplicate_reports,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("manifest_artifact=artifact_mc7_launch_manifest"));
    assert!(output.contains("paired_report_artifact=n/a"));
    assert!(output.contains("inspect_artifact=artifact_mc7_launch_report_a"));
    assert!(output.contains("inspect_artifact=artifact_mc7_launch_report_b"));
  }

  #[test]
  fn render_run_text_leaves_duplicate_training_job_reports_unpaired() {
    let run_id = RunId::new("run_inspect_duplicate_job_reports");
    let root_span_id = SpanId::new("span_duplicate_job_root");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_duplicate_job_test"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("duplicate job reports".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };

    let job_manifest = MinecraftTrainingJobManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_mc7_job_manifest_dup"),
        span_id: root_span_id.clone(),
        captured_event_id: None,
        role: Some("minecraft-3dgs-training-job".to_string()),
        path: Some("artifacts/minecraft-3dgs-training-job.json".to_string()),
        summary: Some("training job manifest".to_string()),
        resolved: true,
      },
      manifest: Some(crate::run_read::MinecraftTrainingJobManifestSummary {
        schema_version: 1,
        source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
          .to_string(),
        source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
        source_training_package_inspect_report_path: "/tmp/package/inspect_report.json".to_string(),
        source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
        source_run_ids: vec!["run_a".to_string()],
        counts: TrainingPackageCounts {
          frames: 2,
          images: 2,
          compatibility_exported_frames: 2,
          compatibility_skipped_frames: 0,
        },
        compatibility_view_name: "nerfstudio".to_string(),
        provider_backend: "remote-command-provider".to_string(),
        trainer_backend: "nerfstudio.splatfacto".to_string(),
        job_backend: "remote".to_string(),
        job_submission_endpoint: "https://jobs.example/api".to_string(),
        job_submission_command: "submit-training-job".to_string(),
        submission_recorded_at_millis: Some(1),
        accepted_by_provider: true,
        training_data_dir: "/tmp/package/compat/nerfstudio".to_string(),
        transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
        export_report_path: "compat/nerfstudio/export_report.json".to_string(),
        suggested_output_dir: "/tmp/job/out".to_string(),
        launch_command: "ns-train splatfacto".to_string(),
        status: "submitted".to_string(),
        job_id: Some("job-123".to_string()),
        job_url: Some("https://jobs.example/job-123".to_string()),
        readiness_blocker: None,
        known_limits: vec![],
      }),
      issue: None,
    };

    let duplicate_reports = vec![
      MinecraftTrainingJobInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_job_report_a"),
          span_id: root_span_id.clone(),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-job-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-job-inspect-a.json".to_string()),
          summary: Some("training job inspect a".to_string()),
          resolved: true,
        },
        report: Some(crate::run_read::MinecraftTrainingJobInspectReportSummary {
          schema_version: 1,
          training_launch_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
          source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
            .to_string(),
          source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
          source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
          source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
          source_run_ids: vec!["run_a".to_string()],
          provider_backend: "remote-command-provider".to_string(),
          job_backend: "remote".to_string(),
          trainer_backend: "nerfstudio.splatfacto".to_string(),
          job_submission_endpoint: "https://jobs.example/api".to_string(),
          job_submission_command: "submit-training-job".to_string(),
          submission_recorded_at_millis: Some(1),
          accepted_by_provider: true,
          status: "submitted".to_string(),
          job_id: Some("job-123".to_string()),
          job_url: Some("https://jobs.example/job-123".to_string()),
          readiness_blocker: None,
          probe_command: "submit-training-job --help".to_string(),
          probe_succeeded: true,
          exported_frame_count: 2,
          skipped_frame_count: 0,
          transforms_present: true,
          warnings: vec![],
          known_limits: vec![],
        }),
        issue: None,
      },
      MinecraftTrainingJobInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_job_report_b"),
          span_id: root_span_id,
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-job-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-job-inspect-b.json".to_string()),
          summary: Some("training job inspect b".to_string()),
          resolved: true,
        },
        report: Some(crate::run_read::MinecraftTrainingJobInspectReportSummary {
          schema_version: 1,
          training_launch_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
          source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
            .to_string(),
          source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
          source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
          source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
          source_run_ids: vec!["run_a".to_string()],
          provider_backend: "remote-command-provider".to_string(),
          job_backend: "remote".to_string(),
          trainer_backend: "nerfstudio.splatfacto".to_string(),
          job_submission_endpoint: "https://jobs.example/api".to_string(),
          job_submission_command: "submit-training-job".to_string(),
          submission_recorded_at_millis: None,
          accepted_by_provider: false,
          status: "blocked".to_string(),
          job_id: None,
          job_url: None,
          readiness_blocker: Some("MissingAuthentication".to_string()),
          probe_command: "submit-training-job --help".to_string(),
          probe_succeeded: false,
          exported_frame_count: 1,
          skipped_frame_count: 1,
          transforms_present: true,
          warnings: vec!["duplicate report".to_string()],
          known_limits: vec![],
        }),
        issue: None,
      },
    ];

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[job_manifest],
      &duplicate_reports,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("manifest_artifact=artifact_mc7_job_manifest_dup"));
    assert!(output.contains("paired_report_artifact=n/a"));
    assert!(output.contains("inspect_artifact=artifact_mc7_job_report_a"));
    assert!(output.contains("inspect_artifact=artifact_mc7_job_report_b"));
  }

  #[test]
  fn render_run_text_leaves_duplicate_training_package_reports_unpaired() {
    let run_id = RunId::new("run_inspect_duplicate_package_reports");
    let root_span_id = SpanId::new("span_duplicate_package_root");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_duplicate_package_test"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("duplicate package reports".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };

    let package_manifest = MinecraftTrainingPackageManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_mc7_package_manifest_dup"),
        span_id: root_span_id.clone(),
        captured_event_id: None,
        role: Some("minecraft-3dgs-training-package".to_string()),
        path: Some("artifacts/minecraft-3dgs-training-package.json".to_string()),
        summary: Some("training package manifest".to_string()),
        resolved: true,
      },
      manifest: Some(MinecraftTrainingPackageManifestSummary {
        schema_version: 1,
        source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
        source_run_ids: vec!["run_a".to_string()],
        counts: TrainingPackageCounts {
          frames: 2,
          images: 2,
          compatibility_exported_frames: 2,
          compatibility_skipped_frames: 0,
        },
        compatibility_views: vec![TrainingCompatibilityViewReport {
          view_name: "nerfstudio".to_string(),
          status: TrainingCompatibilityStatus::Ready,
          exported_frame_count: 2,
          skipped_frame_count: 0,
          transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
          export_report_path: "compat/nerfstudio/export_report.json".to_string(),
          exported_frame_indices: vec![1, 2],
          frame_decisions: Vec::new(),
          skip_reason_counts: Vec::new(),
          warnings: Vec::new(),
          used_legacy_view_translation_fallback_frame_indices: Vec::new(),
          known_limits: vec![],
        }],
        known_limits: vec![],
      }),
      issue: None,
    };

    let duplicate_reports = vec![
      MinecraftTrainingPackageInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_package_report_a"),
          span_id: root_span_id.clone(),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-package-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-package-inspect-a.json".to_string()),
          summary: Some("training package inspect a".to_string()),
          resolved: true,
        },
        report: Some(MinecraftTrainingPackageInspectReportSummary {
          schema_version: 1,
          training_package_manifest_path: "/tmp/package/run.json".to_string(),
          scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
          source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
          source_run_ids: vec!["run_a".to_string()],
          counts: TrainingPackageCounts {
            frames: 2,
            images: 2,
            compatibility_exported_frames: 2,
            compatibility_skipped_frames: 0,
          },
          compatibility_views: vec![TrainingCompatibilityViewReport {
            view_name: "nerfstudio".to_string(),
            status: TrainingCompatibilityStatus::Ready,
            exported_frame_count: 2,
            skipped_frame_count: 0,
            transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
            export_report_path: "compat/nerfstudio/export_report.json".to_string(),
            exported_frame_indices: vec![1, 2],
            frame_decisions: Vec::new(),
            skip_reason_counts: Vec::new(),
            warnings: Vec::new(),
            used_legacy_view_translation_fallback_frame_indices: Vec::new(),
            known_limits: vec![],
          }],
          warnings: vec![],
          known_limits: vec![],
        }),
        issue: None,
      },
      MinecraftTrainingPackageInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_package_report_b"),
          span_id: root_span_id,
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-package-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-package-inspect-b.json".to_string()),
          summary: Some("training package inspect b".to_string()),
          resolved: true,
        },
        report: Some(MinecraftTrainingPackageInspectReportSummary {
          schema_version: 1,
          training_package_manifest_path: "/tmp/package/run.json".to_string(),
          scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
          source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
          source_run_ids: vec!["run_a".to_string()],
          counts: TrainingPackageCounts {
            frames: 2,
            images: 2,
            compatibility_exported_frames: 1,
            compatibility_skipped_frames: 1,
          },
          compatibility_views: vec![TrainingCompatibilityViewReport {
            view_name: "nerfstudio".to_string(),
            status: TrainingCompatibilityStatus::Partial,
            exported_frame_count: 1,
            skipped_frame_count: 1,
            transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
            export_report_path: "compat/nerfstudio/export_report.json".to_string(),
            exported_frame_indices: vec![1],
            frame_decisions: Vec::new(),
            skip_reason_counts: Vec::new(),
            warnings: vec!["duplicate report".to_string()],
            used_legacy_view_translation_fallback_frame_indices: Vec::new(),
            known_limits: vec![],
          }],
          warnings: vec!["duplicate report".to_string()],
          known_limits: vec![],
        }),
        issue: None,
      },
    ];

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[package_manifest],
      &duplicate_reports,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("manifest_artifact=artifact_mc7_package_manifest_dup"));
    assert!(output.contains("paired_report_artifact=n/a"));
    assert!(output.contains("inspect_artifact=artifact_mc7_package_report_a"));
    assert!(output.contains("inspect_artifact=artifact_mc7_package_report_b"));
  }

  #[test]
  fn render_run_text_leaves_duplicate_training_result_reports_unpaired() {
    let run_id = RunId::new("run_inspect_duplicate_result_reports");
    let root_span_id = SpanId::new("span_duplicate_result_root");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_duplicate_result_test"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("duplicate result reports".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };

    let result_manifest = MinecraftTrainingResultManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_mc7_result_manifest_dup"),
        span_id: root_span_id.clone(),
        captured_event_id: None,
        role: Some("minecraft-3dgs-training-result".to_string()),
        path: Some("artifacts/minecraft-3dgs-training-result.json".to_string()),
        summary: Some("training result manifest".to_string()),
        resolved: true,
      },
      manifest: Some(crate::run_read::MinecraftTrainingResultManifestSummary {
        schema_version: 1,
        source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
        source_training_launch_plan_path: "/tmp/launch/minecraft-3dgs-training-launch-plan.json"
          .to_string(),
        source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
        source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
        source_run_ids: vec!["run_a".to_string()],
        trainer_backend: "nerfstudio.splatfacto".to_string(),
        job_backend: "remote".to_string(),
        job_submission_endpoint: "https://jobs.example/api".to_string(),
        source_job_status: "submitted".to_string(),
        status: "succeeded".to_string(),
        status_message: Some("provider succeeded".to_string()),
        job_id: "job-123".to_string(),
        job_url: Some("https://jobs.example/job-123".to_string()),
        result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
        result_artifacts: vec![crate::run_read::MinecraftTrainingResultArtifactSummary {
          relative_path: "config.yml".to_string(),
          absolute_path: "/tmp/job/trainer-output/nerfstudio-splatfacto/config.yml".to_string(),
          readable: true,
          byte_size: Some(128),
        }],
        exported_frame_count: 2,
        skipped_frame_count: 0,
        known_limits: vec![],
      }),
      issue: None,
    };

    let duplicate_reports = vec![
      MinecraftTrainingResultInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_result_report_a"),
          span_id: root_span_id.clone(),
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-result-inspect-a.json".to_string()),
          summary: Some("training result inspect a".to_string()),
          resolved: true,
        },
        report: Some(
          crate::run_read::MinecraftTrainingResultInspectReportSummary {
            schema_version: 1,
            training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
              .to_string(),
            source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json"
              .to_string(),
            source_training_launch_plan_path:
              "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
            source_run_ids: vec!["run_a".to_string()],
            trainer_backend: "nerfstudio.splatfacto".to_string(),
            job_backend: "remote".to_string(),
            job_submission_endpoint: "https://jobs.example/api".to_string(),
            source_job_status: "submitted".to_string(),
            status: "succeeded".to_string(),
            status_message: Some("provider succeeded".to_string()),
            status_reason: None,
            job_id: "job-123".to_string(),
            job_url: Some("https://jobs.example/job-123".to_string()),
            result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
            result_dir_exists: true,
            key_result_artifacts_present: true,
            result_artifact_count: 1,
            warnings: vec![],
            known_limits: vec![],
          },
        ),
        issue: None,
      },
      MinecraftTrainingResultInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new("artifact_mc7_result_report_b"),
          span_id: root_span_id,
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-inspect".to_string()),
          path: Some("artifacts/minecraft-3dgs-training-result-inspect-b.json".to_string()),
          summary: Some("training result inspect b".to_string()),
          resolved: true,
        },
        report: Some(
          crate::run_read::MinecraftTrainingResultInspectReportSummary {
            schema_version: 1,
            training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
              .to_string(),
            source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json"
              .to_string(),
            source_training_launch_plan_path:
              "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
            source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
            source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
            source_run_ids: vec!["run_a".to_string()],
            trainer_backend: "nerfstudio.splatfacto".to_string(),
            job_backend: "remote".to_string(),
            job_submission_endpoint: "https://jobs.example/api".to_string(),
            source_job_status: "submitted".to_string(),
            status: "failed".to_string(),
            status_message: Some("legacy adapter failure".to_string()),
            status_reason: Some("result_artifacts_missing".to_string()),
            job_id: "job-123".to_string(),
            job_url: Some("https://jobs.example/job-123".to_string()),
            result_dir: "/tmp/job/trainer-output/nerfstudio-splatfacto".to_string(),
            result_dir_exists: true,
            key_result_artifacts_present: false,
            result_artifact_count: 0,
            warnings: vec!["duplicate report".to_string()],
            known_limits: vec![],
          },
        ),
        issue: None,
      },
    ];

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[result_manifest],
      &duplicate_reports,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("manifest_artifact=artifact_mc7_result_manifest_dup"));
    assert!(output.contains("paired_report_artifact=n/a"));
    assert!(output.contains("inspect_artifact=artifact_mc7_result_report_a"));
    assert!(output.contains("inspect_artifact=artifact_mc7_result_report_b"));
  }
  #[test]
  fn render_run_text_leaves_duplicate_holdout_preview_reports_unpaired() {
    let run_id = RunId::new("run_inspect_duplicate_holdout_reports");
    let root_span_id = SpanId::new("span_duplicate_holdout_root");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_duplicate_holdout_test"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id: root_span_id.clone(),
        attributes: BTreeMap::new(),
        summary: Some("duplicate holdout reports".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };

    let holdout_witness = HoldoutFrameWitness {
      frame_index: 6,
      spatial_frame_id: "frame-355416-47699343801916".to_string(),
      screenshot_path: "/tmp/scene-packet/frames/frame_000006.png".to_string(),
      frame_json_path: "/tmp/scene-packet/frames/frame_000006.json".to_string(),
    };
    let holdout_manifest_value = TrainingResultHoldoutPreviewManifest {
      schema_version: 1,
      generated_at_millis: 1,
      training_result_semantic_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-semantic.json".to_string(),
      source_training_result_artifact_manifest_path:
        "/tmp/result/minecraft-3dgs-training-result-artifact-manifest.json".to_string(),
      source_training_result_manifest_path: "/tmp/result/minecraft-3dgs-training-result.json"
        .to_string(),
      source_training_job_manifest_path: "/tmp/job/minecraft-3dgs-training-job.json".to_string(),
      source_training_launch_plan_path:
        "/tmp/launch/minecraft-3dgs-training-launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package/run.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle-a/run.json".to_string()],
      source_run_ids: vec!["run_a".to_string()],
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      normalized_result_dir:
        "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result".to_string(),
      holdout_frame_index: 6,
      holdout_frame: Some(holdout_witness.clone()),
      basis_checkpoint_path: Some(
        "/tmp/job/trainer-output/nerfstudio-splatfacto/normalized-result/nerfstudio_models/step-000001.ckpt"
          .to_string(),
      ),
      holdout_screenshot_path: Some(holdout_witness.screenshot_path.clone()),
      reference_overlay_path: None,
      status: HoldoutPreviewStatus::Ready,
      reason: None,
      known_limits: vec![],
    };

    fn duplicate_holdout_report(
      run: &CanonicalRun,
      root_span_id: SpanId,
      artifact_id: &str,
      path_suffix: &str,
      manifest: &TrainingResultHoldoutPreviewManifest,
      witness: HoldoutFrameWitness,
      warnings: Vec<String>,
    ) -> MinecraftTrainingResultHoldoutPreviewInspectReportLineage {
      MinecraftTrainingResultHoldoutPreviewInspectReportLineage {
        artifact: ArtifactRefLineage {
          run_id: run.run.run_id.clone(),
          artifact_id: ArtifactId::new(artifact_id),
          span_id: root_span_id,
          captured_event_id: None,
          role: Some("minecraft-3dgs-training-result-holdout-preview-inspect".to_string()),
          path: Some(format!(
            "artifacts/minecraft-3dgs-training-result-holdout-preview-inspect-{path_suffix}.json"
          )),
          summary: Some(format!("holdout inspect {path_suffix}")),
          resolved: true,
        },
        report: Some(
          TrainingResultHoldoutPreviewInspectReport {
            schema_version: 1,
            generated_at_millis: 1,
            training_result_holdout_preview_manifest_path:
              "/tmp/holdout/minecraft-3dgs-training-result-holdout-preview.json".to_string(),
            training_result_semantic_manifest_path: manifest
              .training_result_semantic_manifest_path
              .clone(),
            source_training_result_artifact_manifest_path: manifest
              .source_training_result_artifact_manifest_path
              .clone(),
            source_training_result_manifest_path: manifest
              .source_training_result_manifest_path
              .clone(),
            source_training_job_manifest_path: manifest.source_training_job_manifest_path.clone(),
            source_training_launch_plan_path: manifest.source_training_launch_plan_path.clone(),
            source_training_package_manifest_path: manifest
              .source_training_package_manifest_path
              .clone(),
            source_scene_packet_manifest_path: manifest.source_scene_packet_manifest_path.clone(),
            source_bundle_manifest_paths: manifest.source_bundle_manifest_paths.clone(),
            source_run_ids: manifest.source_run_ids.clone(),
            trainer_backend: manifest.trainer_backend.clone(),
            job_backend: manifest.job_backend.clone(),
            normalized_result_dir: manifest.normalized_result_dir.clone(),
            holdout_frame_index: 6,
            holdout_frame: Some(witness),
            basis_checkpoint_path: manifest.basis_checkpoint_path.clone(),
            holdout_screenshot_path: manifest.holdout_screenshot_path.clone(),
            reference_overlay_path: manifest.reference_overlay_path.clone(),
            status: HoldoutPreviewStatus::Ready,
            reason: None,
            holdout_frame_selection: HoldoutFrameSelection::LastInGame,
            checkpoint_count: 1,
            scene_packet_frame_count: 6,
            warnings,
            known_limits: vec![],
          }
          .into(),
        ),
        issue: None,
      }
    }

    let holdout_manifest = MinecraftTrainingResultHoldoutPreviewManifestLineage {
      artifact: ArtifactRefLineage {
        run_id: run.run.run_id.clone(),
        artifact_id: ArtifactId::new("artifact_mc16_holdout_manifest_dup"),
        span_id: root_span_id.clone(),
        captured_event_id: None,
        role: Some("minecraft-3dgs-training-result-holdout-preview".to_string()),
        path: Some("artifacts/minecraft-3dgs-training-result-holdout-preview.json".to_string()),
        summary: Some("holdout preview manifest".to_string()),
        resolved: true,
      },
      manifest: Some(holdout_manifest_value.clone().into()),
      issue: None,
    };

    let duplicate_reports = vec![
      duplicate_holdout_report(
        &run,
        root_span_id.clone(),
        "artifact_mc16_holdout_report_a",
        "a",
        &holdout_manifest_value,
        holdout_witness.clone(),
        vec![],
      ),
      duplicate_holdout_report(
        &run,
        root_span_id,
        "artifact_mc16_holdout_report_b",
        "b",
        &holdout_manifest_value,
        holdout_witness,
        vec!["duplicate report".to_string()],
      ),
    ];

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[holdout_manifest],
      &duplicate_reports,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      None,
      None,
      None,
    );

    assert!(output.contains("MC-16 Training Result Holdout Preview:"));
    assert!(output.contains("manifest_artifact=artifact_mc16_holdout_manifest_dup"));
    assert!(output.contains("paired_report_artifact=n/a"));
    assert!(output.contains("inspect_artifact=artifact_mc16_holdout_report_a"));
    assert!(output.contains("inspect_artifact=artifact_mc16_holdout_report_b"));
  }

  #[test]
  fn render_run_text_renders_mc17_d2_quality_baseline_report() {
    use crate::run_read::{
      MinecraftTrainingResultQualityBaselineReportSummary, QualityBaselineHoldoutWitnessEvidence,
      QualityBaselineRenderQualityEvidence, QualityBaselineSpatialQueryEvidence,
      derive_minecraft_training_result_quality_verdict,
      quality_baseline_verdict_thresholds_probe_v1,
      quality_baseline_verdict_thresholds_trained_render_v1,
    };
    let run_id = RunId::new("run_inspect_mc17_d2_baseline");
    let root_span_id = SpanId::new("span_mc17_d2_root");
    let run = CanonicalRun {
      run: RunRecordV1Alpha1 {
        api_version: RUN_API_VERSION.to_string(),
        run_id: run_id.clone(),
        trace_id: TraceId::new("trace_mc17_d2_baseline"),
        run_type: RunType::Command,
        state: TraceState::Ended,
        status_code: TraceStatusCode::Ok,
        started_at_millis: 1,
        finished_at_millis: Some(2),
        root_span_id,
        attributes: BTreeMap::new(),
        summary: Some("mc17 d2 baseline".to_string()),
        failure: None,
      },
      spans: vec![],
      events: vec![],
      artifacts: vec![],
    };
    let report = MinecraftTrainingResultQualityBaselineReportSummary {
      profile_id: "mc17-d2-primary-v1".to_string(),
      training_result_semantic_manifest_path:
        ".tmp/mc10-smoke-review/semantic/minecraft-3dgs-training-result-semantic.json".to_string(),
      evidence_coverage: "complete".to_string(),
      spatial_query: Some(QualityBaselineSpatialQueryEvidence {
        status: "answered".to_string(),
        visibility: Some("visible".to_string()),
        screen_point: Some("854.0,480.0".to_string()),
        selected_backend: Some("projection_reference".to_string()),
        comparison_verdict: Some("reference_only".to_string()),
        basis_frame_id: Some("frame-355416".to_string()),
        target_block: "511,73,728".to_string(),
        target_face: Some("north".to_string()),
        target_semantics: "hit_face_center".to_string(),
      }),
      holdout_witness: Some(QualityBaselineHoldoutWitnessEvidence {
        status: "ready".to_string(),
        holdout_frame_index: 6,
        basis_checkpoint_path: Some(
          "/tmp/normalized/nerfstudio_models/step-000001.ckpt".to_string(),
        ),
        holdout_screenshot_path: Some("/tmp/frame_000006.png".to_string()),
        spatial_frame_id: Some("frame-355416-47699343801916".to_string()),
      }),
      render_quality: Some(QualityBaselineRenderQualityEvidence {
        status: "ready".to_string(),
        verdict: "measured_only".to_string(),
        image_size_match: true,
        l1_mean: Some(0.0),
        mse: Some(0.0),
        psnr: None,
        known_limits: vec!["metrics evidence only".to_string()],
      }),
      trust_notes: vec![
        "MC-12 projection_reference answers are scene-packet reference geometry only; they are not Gaussian-native inference".to_string(),
      ],
      issue: None,
    };
    let probe_thresholds = quality_baseline_verdict_thresholds_probe_v1().expect("probe");
    let trained_thresholds =
      quality_baseline_verdict_thresholds_trained_render_v1().expect("trained");
    let probe_verdict =
      derive_minecraft_training_result_quality_verdict(&report, &probe_thresholds);
    let trained_verdict =
      derive_minecraft_training_result_quality_verdict(&report, &trained_thresholds);

    let output = render_run_text(
      &run,
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      &[],
      Some(&report),
      Some(&probe_verdict),
      Some(&trained_verdict),
    );

    assert!(output.contains("MC-17 Quality Baseline Report:"));
    assert!(output.contains("MC-17 Quality Verdict:"));
    assert!(output.contains("profile_id=mc17-d2-primary-v1 evidence_coverage=complete"));
    assert!(output.contains("spatial_query_status=answered visibility=visible"));
    assert!(output.contains("holdout_status=ready holdout_frame_index=6"));
    assert!(output.contains("render_quality_status=ready verdict=measured_only"));
    assert!(output.contains("l1_mean=0 mse=0"));
    assert!(output.contains("trust_notes="));
    assert!(output.contains("render_evidence_mode=screenshot_copy_probe"));
    assert!(output.contains("quality_verdict=pass"));
    assert!(output.contains("render_evidence_mode=trained_render"));
    assert!(
      output.contains("screenshot_copy_probe thresholds judge pipeline wiring only"),
      "verdict trust_notes should surface probe disclaimer"
    );
  }

  #[test]
  fn osu_visual_truth_probe_live_store_inspect_acceptance() {
    use std::fs;
    use std::path::PathBuf;

    use auv_game_osu::CapturePhase;
    use auv_tracing_driver::recording::RunRecordingBackend;
    use auv_tracing_driver::store::LocalStore;

    use crate::osu::{
      run_osu_visual_truth_semantic_validation, run_osu_visual_truth_spatial_query,
    };

    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("crates/auv-game-osu/tests/fixtures/osu_visual_truth_probe");
    use std::env;
    use std::time::{SystemTime, UNIX_EPOCH};

    let stamp = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .expect("clock")
      .as_nanos();
    let work = env::temp_dir().join(format!("auv-osu-probe-work-{stamp}"));
    fs::create_dir_all(&work).expect("work dir");
    for name in ["visual_truth_manifest.json", "projection.json"] {
      fs::copy(fixture_root.join(name), work.join(name)).expect("copy fixture");
    }

    let store_root = env::temp_dir().join(format!("auv-osu-probe-store-{stamp}"));
    fs::create_dir_all(&store_root).expect("store dir");
    let store = LocalStore::new(store_root.clone()).expect("store");
    let recording = RunRecordingBackend::local_only(store.clone()).handle();

    let semantic =
      run_osu_visual_truth_semantic_validation(&recording, work.clone(), work.join("semantic-out"))
        .expect("semantic validation should record");

    let semantic_output = inspect_run(&store, semantic.run_id.as_str())
      .unwrap_or_else(|error| panic!("semantic inspect: {error}"));
    assert!(semantic_output.contains("Osu Visual Truth Semantic:"));
    assert!(semantic_output.contains("semantic_status=ready"));

    let query = run_osu_visual_truth_spatial_query(
      &recording,
      semantic.value.manifest_path,
      0,
      CapturePhase::BeforeDispatch,
      None,
      work.join("query-out"),
    )
    .expect("spatial query should record");

    let query_output = inspect_run(&store, query.run_id.as_str())
      .unwrap_or_else(|error| panic!("query inspect: {error}"));
    assert!(query_output.contains("Osu Visual Truth Spatial Query:"));
    assert!(query_output.contains("status=answered"));
    assert!(query_output.contains("Osu Visual Truth Spatial Query Action Readiness:"));
    assert!(query_output.contains("action_eligibility=click_ready"));
    let _ = fs::remove_dir_all(&work);
    let _ = fs::remove_dir_all(&store_root);
  }

  #[test]
  fn mc19_d5_live_store_inspect_acceptance() {
    use std::path::PathBuf;
    let store_root = PathBuf::from(".tmp/mc19-live/store");
    if !store_root.exists() {
      return;
    }
    let store = auv_tracing_driver::store::LocalStore::new(store_root).expect("store");
    let cases = [
      (
        "run_1782590245467_18186_0",
        "attempted=true",
        "action_eligibility=click_ready",
        "dispatch_command=input.clickWindowPoint",
      ),
      (
        "run_1782590246310_18190_0",
        "attempted=false",
        "visibility=outside_window",
        "dispatch_command=n/a",
      ),
      (
        "run_1782590246843_18194_0",
        "attempted=false",
        "refusal_reason=status=failed reason=target_block_absent_from_scene_packet",
        "dispatch_command=n/a",
      ),
    ];
    for (run_id, attempted, eligibility_or_refusal, dispatch) in cases {
      let output = inspect_run(&store, run_id).unwrap_or_else(|error| panic!("{run_id}: {error}"));
      assert!(
        output.contains("MC-19 Query Wired Live Action:"),
        "{run_id}"
      );
      assert!(output.contains(attempted), "{run_id} missing {attempted}");
      assert!(
        output.contains(eligibility_or_refusal),
        "{run_id} missing {eligibility_or_refusal}"
      );
      assert!(output.contains(dispatch), "{run_id} missing {dispatch}");
      eprintln!("--- MC-19 inspect {run_id} ---");
      for line in output.lines() {
        if line.contains("MC-19 Query Wired Live Action:")
          || line.starts_with("- operation_result_artifact=")
        {
          eprintln!("{line}");
        }
      }
    }
  }
  #[test]
  fn mc17_d2_live_store_inspect_acceptance() {
    use std::path::PathBuf;
    let store_root = PathBuf::from(".tmp/mc17-d2-live/store");
    if !store_root.exists() {
      return;
    }
    let store = auv_tracing_driver::store::LocalStore::new(store_root).expect("store");
    let run_id = "run_1782594531314_61141_0";
    let output = inspect_run(&store, run_id).unwrap_or_else(|error| panic!("{run_id}: {error}"));
    assert!(
      output.contains("MC-17 Quality Baseline Report:"),
      "{run_id}"
    );
    assert!(
      output.contains("profile_id=mc17-d2-primary-v1 evidence_coverage=complete"),
      "{run_id}"
    );
    assert!(output.contains("spatial_query_status=answered"), "{run_id}");
    assert!(
      output.contains("holdout_status=ready holdout_frame_index=6"),
      "{run_id}"
    );
    assert!(
      output.contains("render_quality_status=ready verdict=measured_only"),
      "{run_id}"
    );
    assert!(output.contains("l1_mean=0 mse=0"), "{run_id}");
    assert!(output.contains("trust_notes="), "{run_id}");
    assert!(output.contains("MC-17 Quality Verdict:"), "{run_id}");
    assert!(
      output.contains("render_evidence_mode=screenshot_copy_probe")
        && output.contains("quality_verdict=pass"),
      "{run_id}"
    );
  }
  #[test]
  fn render_run_text_renders_balatro_card_detection_probe() {
    use std::path::PathBuf;

    use auv_game_balatro::ObjectZone;
    use auv_tracing_driver::recording::RunRecordingBackend;
    use auv_tracing_driver::store::LocalStore;

    use crate::balatro::run_balatro_consumption_probe_chain;

    let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
      .join("crates/auv-game-balatro/tests/fixtures/balatro_consumption_probe");
    let store_root = std::env::temp_dir().join(format!(
      "auv-balatro-probe-store-{}",
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos()
    ));
    let work_dir = std::env::temp_dir().join(format!(
      "auv-balatro-probe-work-{}",
      std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos()
    ));
    std::fs::create_dir_all(&store_root).expect("store dir");
    std::fs::create_dir_all(&work_dir).expect("work dir");

    let store = LocalStore::new(store_root.clone()).expect("store");
    let recording = RunRecordingBackend::local_only(store.clone()).handle();

    let chain = run_balatro_consumption_probe_chain(
      &recording,
      fixture_root.clone(),
      fixture_root.join("expected_slots.json"),
      auv_game_balatro::SlotId::new(ObjectZone::Hand, 0),
      work_dir.clone(),
    )
    .expect("probe chain");

    let output =
      inspect_run(&store, chain.run_id.as_str()).unwrap_or_else(|error| panic!("inspect: {error}"));

    assert!(output.contains("Balatro Card Detection Semantic:"));
    assert!(output.contains("semantic_status=ready"));
    assert!(output.contains("Balatro Card Detection Spatial Query:"));
    assert!(output.contains("status=answered"));
    assert!(output.contains("Balatro Card Detection Eval Witness:"));
    assert!(output.contains("status=ready"));
    assert!(output.contains("Balatro Card Detection Quality:"));
    assert!(output.contains("witness_status=ready"));
    assert!(output.contains("verdict=measured_only"));
    assert!(output.contains("quality_backend=ultralytics_onnx_entities"));

    let _ = std::fs::remove_dir_all(&store_root);
    let _ = std::fs::remove_dir_all(&work_dir);
  }
}
