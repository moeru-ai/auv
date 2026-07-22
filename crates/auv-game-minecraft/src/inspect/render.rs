//! Minecraft inspect text fragments rendered from canonical typed artifacts.

use auv_tracing::ArtifactUri;

use crate::{
  MinecraftProjectionArtifact, ScenePacketManifest, TrainingLaunchJobManifest, TrainingPackageManifest,
  TrainingResultHoldoutPreviewManifest, TrainingResultHoldoutRenderQualityManifest, TrainingResultManifest, TrainingResultSemanticManifest,
  TrainingResultSpatialQueryManifest,
};

pub(crate) struct PrimaryArtifacts<'a> {
  pub projections: &'a [(ArtifactUri, MinecraftProjectionArtifact)],
  pub scene_packets: &'a [(ArtifactUri, ScenePacketManifest)],
  pub training_packages: &'a [(ArtifactUri, TrainingPackageManifest)],
  pub training_jobs: &'a [(ArtifactUri, TrainingLaunchJobManifest)],
  pub training_results: &'a [(ArtifactUri, TrainingResultManifest)],
  pub semantics: &'a [(ArtifactUri, TrainingResultSemanticManifest)],
  pub holdout_previews: &'a [(ArtifactUri, TrainingResultHoldoutPreviewManifest)],
  pub render_quality: &'a [(ArtifactUri, TrainingResultHoldoutRenderQualityManifest)],
}

pub(crate) fn append_primary_sections(output: &mut String, artifacts: PrimaryArtifacts<'_>) {
  let PrimaryArtifacts {
    projections,
    scene_packets,
    training_packages,
    training_jobs,
    training_results,
    semantics,
    holdout_previews,
    render_quality,
  } = artifacts;

  output.push_str("\nMC-1 Telemetry Samples:\n- none\n");

  output.push_str("\nMC-2 Projection Artifacts:\n");
  if projections.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, projection) in projections {
      output.push_str(&format!(
        "- projection_artifact={uri} spatial_frame_id={} world_tick={} visibility={} screenshot_artifact={} verification_reference={}\n",
        projection.spatial_frame_id,
        projection.world_tick,
        visibility_label(projection.visibility),
        projection.screenshot_artifact_ref.as_deref().unwrap_or("n/a"),
        projection.verification_reference.as_deref().unwrap_or("n/a"),
      ));
    }
  }

  output.push_str("\nMC-7 Scene Packets:\n");
  if scene_packets.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, packet) in scene_packets {
      output.push_str(&format!(
        "- scene_packet_artifact={uri} schema={} frames={} screenshots={} source_runs={}\n",
        packet.schema_version,
        packet.counts.frames,
        packet.counts.screenshots,
        packet.source_run_ids.len(),
      ));
    }
  }

  output.push_str("\nMC-7 Training Packages:\n");
  if training_packages.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, package) in training_packages {
      output.push_str(&format!(
        "- training_package_artifact={uri} schema={} frames={} images={} source_runs={}\n",
        package.schema_version,
        package.counts.frames,
        package.counts.images,
        package.source_run_ids.len(),
      ));
    }
  }

  output.push_str("\nMC-7 Training Jobs:\n");
  if training_jobs.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, job) in training_jobs {
      output.push_str(&format!(
        "- training_job_artifact={uri} schema={} status={} provider_backend={} job_id={} source_runs={}\n",
        job.schema_version,
        job.status.as_str(),
        job.provider_backend,
        job.job_id.as_deref().unwrap_or("n/a"),
        job.source_run_ids.len(),
      ));
    }
  }

  output.push_str("\nMC-7 Training Results:\n");
  if training_results.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, result) in training_results {
      output.push_str(&format!(
        "- training_result_artifact={uri} schema={} status={} job_id={} result_artifacts={} source_runs={}\n",
        result.schema_version,
        result.status.as_str(),
        result.job_id,
        result.result_artifacts.len(),
        result.source_run_ids.len(),
      ));
    }
  }

  output.push_str("\nMC-10 Training Result Semantic:\n");
  if semantics.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, semantic) in semantics {
      output.push_str(&format!(
        "- semantic_artifact={uri} schema={} status={} reason={} checkpoints={} source_runs={}\n",
        semantic.schema_version,
        semantic.semantic_status,
        semantic.semantic_reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        semantic.checkpoint_count,
        semantic.source_run_ids.len(),
      ));
    }
  }

  output.push_str("\nMC-16 Holdout Preview:\n");
  if holdout_previews.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, preview) in holdout_previews {
      output.push_str(&format!(
        "- holdout_preview_artifact={uri} schema={} status={} reason={} frame_index={} checkpoint={} screenshot={}\n",
        preview.schema_version,
        preview.status,
        preview.reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        preview.holdout_frame_index,
        preview.basis_checkpoint_path.as_deref().unwrap_or("n/a"),
        preview.holdout_screenshot_path.as_deref().unwrap_or("n/a"),
      ));
    }
  }

  output.push_str("\nMC-17 Holdout Render Quality:\n");
  if render_quality.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, quality) in render_quality {
      output.push_str(&format!(
        "- render_quality_artifact={uri} schema={} status={} verdict={} frame_index={} image_size_match={}\n",
        quality.schema_version,
        quality.status,
        quality.verdict.as_str(),
        quality.holdout_frame_index,
        quality.image_size_match,
      ));
    }
  }
}

pub(crate) fn append_quality_and_spatial_sections(
  output: &mut String,
  spatial_queries: &[(ArtifactUri, TrainingResultSpatialQueryManifest)],
  holdout_previews: &[(ArtifactUri, TrainingResultHoldoutPreviewManifest)],
  render_quality: &[(ArtifactUri, TrainingResultHoldoutRenderQualityManifest)],
) {
  output.push_str("\nMC-12 Training Result Spatial Query:\n");
  if spatial_queries.is_empty() {
    output.push_str("- none\n");
  } else {
    for (uri, query) in spatial_queries {
      output.push_str(&format!(
        "- spatial_query_artifact={uri} status={} reason={} target_block={},{},{} visibility={} screen_point={} backend={} comparison={}\n",
        query.status.as_str(),
        query.reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        query.target_block.x,
        query.target_block.y,
        query.target_block.z,
        query.visibility.map(visibility_label).unwrap_or("n/a"),
        query.screen_point.map(|point| format!("{},{}", point.x, point.y)).unwrap_or_else(|| "n/a".to_string()),
        query.selected_backend.map(|backend| backend.as_str()).unwrap_or("n/a"),
        query.comparison_verdict.map(|verdict| verdict.as_str()).unwrap_or("n/a"),
      ));
    }
  }

  output.push_str("\nMC-17 Quality Baseline Report:\n");
  output.push_str(&format!(
    "- spatial_query_artifacts={} holdout_preview_artifacts={} render_quality_artifacts={}\n",
    spatial_queries.len(),
    holdout_previews.len(),
    render_quality.len(),
  ));
}

fn visibility_label(visibility: crate::ProjectionVisibility) -> &'static str {
  match visibility {
    crate::ProjectionVisibility::Visible => "visible",
    crate::ProjectionVisibility::BehindCamera => "behind_camera",
    crate::ProjectionVisibility::OutOfFrustum => "out_of_frustum",
    crate::ProjectionVisibility::OutsideWindow => "outside_window",
  }
}
