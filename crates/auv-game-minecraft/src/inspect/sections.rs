//! Minecraft InspectSection factories (primary + quality/spatial).

use std::sync::Arc;

use auv_inspect_model::{InspectError, InspectSection, InspectSectionOutput};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

use super::render::{append_primary_sections, append_quality_and_spatial_sections};
use crate::run_read::{
  collect_quality_baseline_evidence_for_run, derive_minecraft_training_result_quality_baseline_report,
  derive_minecraft_training_result_quality_verdict, extract_minecraft_holdout_render_quality_inspect_reports,
  extract_minecraft_holdout_render_quality_manifests, extract_minecraft_projection_artifacts, extract_minecraft_spatial_bundle_manifests,
  extract_minecraft_telemetry_sample_artifacts, extract_minecraft_training_job_inspect_reports, extract_minecraft_training_job_manifests,
  extract_minecraft_training_launch_inspect_reports, extract_minecraft_training_launch_manifests,
  extract_minecraft_training_package_inspect_reports, extract_minecraft_training_package_manifests,
  extract_minecraft_training_result_artifact_fetch_inspect_reports, extract_minecraft_training_result_artifact_fetch_manifests,
  extract_minecraft_training_result_holdout_preview_inspect_reports, extract_minecraft_training_result_holdout_preview_manifests,
  extract_minecraft_training_result_inspect_reports, extract_minecraft_training_result_manifests,
  extract_minecraft_training_result_semantic_inspect_reports, extract_minecraft_training_result_semantic_manifests,
  extract_minecraft_training_result_spatial_query_inspect_reports, extract_minecraft_training_result_spatial_query_manifests,
  quality_baseline_profile_v1, quality_baseline_verdict_thresholds_probe_v1, quality_baseline_verdict_thresholds_trained_render_v1,
};

pub struct MinecraftPrimarySection;

impl InspectSection for MinecraftPrimarySection {
  fn id(&self) -> &'static str {
    "minecraft_primary"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    Ok(Some(InspectSectionOutput {
      id: self.id(),
      text: render_minecraft_primary_text(store, run)?,
      json: None,
    }))
  }
}

pub struct MinecraftQualitySpatialSection;

impl InspectSection for MinecraftQualitySpatialSection {
  fn id(&self) -> &'static str {
    "minecraft_quality_spatial"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    Ok(Some(InspectSectionOutput {
      id: self.id(),
      text: render_minecraft_quality_spatial_text(store, run)?,
      json: None,
    }))
  }
}

pub fn render_minecraft_primary_text(store: &LocalStore, run: &CanonicalRun) -> Result<String, InspectError> {
  let projection = extract_minecraft_projection_artifacts(store, run)?;
  let telemetry = extract_minecraft_telemetry_sample_artifacts(store, run)?;
  let spatial = extract_minecraft_spatial_bundle_manifests(store, run)?;
  let pkg_m = extract_minecraft_training_package_manifests(store, run)?;
  let pkg_r = extract_minecraft_training_package_inspect_reports(store, run)?;
  let launch_m = extract_minecraft_training_launch_manifests(store, run)?;
  let launch_r = extract_minecraft_training_launch_inspect_reports(store, run)?;
  let job_m = extract_minecraft_training_job_manifests(store, run)?;
  let job_r = extract_minecraft_training_job_inspect_reports(store, run)?;
  let result_m = extract_minecraft_training_result_manifests(store, run)?;
  let result_r = extract_minecraft_training_result_inspect_reports(store, run)?;
  let fetch_m = extract_minecraft_training_result_artifact_fetch_manifests(store, run)?;
  let fetch_r = extract_minecraft_training_result_artifact_fetch_inspect_reports(store, run)?;
  let sem_m = extract_minecraft_training_result_semantic_manifests(store, run)?;
  let sem_r = extract_minecraft_training_result_semantic_inspect_reports(store, run)?;
  let holdout_m = extract_minecraft_training_result_holdout_preview_manifests(store, run)?;
  let holdout_r = extract_minecraft_training_result_holdout_preview_inspect_reports(store, run)?;
  let rq_m = extract_minecraft_holdout_render_quality_manifests(store, run)?;
  let rq_r = extract_minecraft_holdout_render_quality_inspect_reports(store, run)?;
  let mut output = String::new();
  append_primary_sections(
    &mut output,
    &projection,
    &telemetry,
    &spatial,
    &pkg_m,
    &pkg_r,
    &launch_m,
    &launch_r,
    &job_m,
    &job_r,
    &result_m,
    &result_r,
    &fetch_m,
    &fetch_r,
    &sem_m,
    &sem_r,
    &holdout_m,
    &holdout_r,
    &rq_m,
    &rq_r,
  );
  Ok(output)
}

pub fn render_minecraft_quality_spatial_text(store: &LocalStore, run: &CanonicalRun) -> Result<String, InspectError> {
  let spatial_m = extract_minecraft_training_result_spatial_query_manifests(store, run)?;
  let spatial_r = extract_minecraft_training_result_spatial_query_inspect_reports(store, run)?;
  let run_id = run.run.run_id.as_str();
  let quality_baseline_report = quality_baseline_profile_v1().ok().and_then(|profile| {
    collect_quality_baseline_evidence_for_run(store, run_id, &profile).ok().map(|bundle| {
      derive_minecraft_training_result_quality_baseline_report(
        &profile,
        bundle.spatial_query.as_ref(),
        bundle.holdout_preview.as_ref(),
        bundle.render_quality.as_ref(),
        &bundle.collection_issues,
      )
    })
  });
  let (quality_verdict_probe, quality_verdict_trained_render) = quality_baseline_report.as_ref().map_or((None, None), |report| {
    let probe = quality_baseline_verdict_thresholds_probe_v1()
      .ok()
      .map(|thresholds| derive_minecraft_training_result_quality_verdict(report, &thresholds));
    let trained_render = quality_baseline_verdict_thresholds_trained_render_v1()
      .ok()
      .map(|thresholds| derive_minecraft_training_result_quality_verdict(report, &thresholds));
    (probe, trained_render)
  });
  let mut output = String::new();
  append_quality_and_spatial_sections(
    &mut output,
    &spatial_m,
    &spatial_r,
    quality_baseline_report.as_ref(),
    quality_verdict_probe.as_ref(),
    quality_verdict_trained_render.as_ref(),
  );
  Ok(output)
}

pub fn inspect_sections_primary() -> Vec<Arc<dyn InspectSection>> {
  vec![Arc::new(MinecraftPrimarySection)]
}

pub fn inspect_sections_quality_spatial() -> Vec<Arc<dyn InspectSection>> {
  vec![Arc::new(MinecraftQualitySpatialSection)]
}
