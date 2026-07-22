//! Minecraft inspect text fragments rendered from typed inspection outcomes.

use auv_driver::geometry::Point;

use super::quality::{MinecraftQualityBaseline, MinecraftQualityVerdict};
use super::sections::{MinecraftPrimaryInspection, MinecraftQualitySpatialInspection};
use crate::{
  BlockFace, MinecraftProjectedPoint, MinecraftTargetSemantics, ProjectionVisibility, TrainingCompatibilityStatus,
  TrainingResultHoldoutRenderQualityManifest, TrainingResultSpatialQueryKind,
};

pub(crate) fn append_primary_sections(output: &mut String, inspection: &MinecraftPrimaryInspection) {
  output.push_str("\nMC-1 Telemetry Samples:\n- none\n");

  output.push_str("\nMC-2 Projection Artifacts:\n");
  if inspection.projections.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in &inspection.projections {
      let projection = &artifact.payload;
      output.push_str(&format!(
        "- artifact={} frame={} tick={} timestamp_ms={} screenshot_artifact_ref={} capture_skew_ms={} viewport={}x{}@{},{} visibility={} raycast={} screen_state={} refusal_reason={} verification_reference={} projected_point={}\n",
        artifact.uri,
        projection.spatial_frame_id,
        projection.world_tick,
        projection.monotonic_timestamp_ms,
        projection.screenshot_artifact_ref.as_deref().unwrap_or("n/a"),
        projection.mc_capture_skew_ms.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
        projection.viewport_bounds.width,
        projection.viewport_bounds.height,
        projection.viewport_bounds.x,
        projection.viewport_bounds.y,
        visibility_label(projection.visibility),
        projection.raycast_block_id.as_deref().unwrap_or("n/a"),
        projection.screen_state.as_deref().unwrap_or("n/a"),
        projection.mismatch_refusal_reason.map(|reason| format!("{reason:?}")).unwrap_or_else(|| "n/a".to_string()),
        projection.verification_reference.as_deref().unwrap_or("n/a"),
        projected_point_label(projection.projected_point.as_ref()),
      ));
    }
  }

  output.push_str("\nMC-6 Spatial Bundles:\n");
  if inspection.scene_packets.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in &inspection.scene_packets {
      let packet = &artifact.payload;
      output.push_str(&format!(
        "- artifact={} schema={} source_runs={} screenshots={} spatial_frames={} missing_screenshots={}\n",
        artifact.uri,
        packet.schema_version,
        packet.source_run_ids.len(),
        packet.counts.screenshots,
        packet.counts.frames,
        packet.counts.missing_screenshots,
      ));
      append_known_limits(output, &packet.known_limits);
    }
  }

  output.push_str("\nMC-7 Training Packages:\n");
  if inspection.training_packages.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in &inspection.training_packages {
      let package = &artifact.payload;
      let primary_view = package.compatibility_views.first();
      output.push_str(&format!(
        "- artifact={} schema={} source_scene_packet={} source_runs={} frames={} images={} compatibility_view={} compatibility_status={} exported={} skipped={} transforms={}\n",
        artifact.uri,
        package.schema_version,
        package.source_scene_packet_manifest_path,
        package.source_run_ids.len(),
        package.counts.frames,
        package.counts.images,
        primary_view.map(|view| view.view_name.as_str()).unwrap_or("n/a"),
        primary_view.map(|view| compatibility_status_label(view.status)).unwrap_or("n/a"),
        primary_view.map(|view| view.exported_frame_count.to_string()).unwrap_or_else(|| "n/a".to_string()),
        primary_view.map(|view| view.skipped_frame_count.to_string()).unwrap_or_else(|| "n/a".to_string()),
        primary_view.and_then(|view| view.transforms_path.as_ref()).map(|_| "present").unwrap_or("none"),
      ));
      append_known_limits(output, &package.known_limits);
    }
  }

  output.push_str("\nMC-7 Training Launches:\n");
  if inspection.training_jobs.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in &inspection.training_jobs {
      let job = &artifact.payload;
      output.push_str(&format!(
        "- artifact={} schema={} source_training_package={} source_scene_packet={} source_runs={} frames={} images={} trainer_backend={} compatibility_view={} exported={} skipped={} transforms={} launch_command={}\n",
        artifact.uri,
        job.schema_version,
        job.source_training_package_manifest_path,
        job.source_scene_packet_manifest_path,
        job.source_run_ids.len(),
        job.counts.frames,
        job.counts.images,
        job.trainer_backend,
        job.compatibility_view_name,
        job.counts.compatibility_exported_frames,
        job.counts.compatibility_skipped_frames,
        job.transforms_path.as_ref().map(|_| "present").unwrap_or("none"),
        job.launch_command,
      ));
    }
  }

  output.push_str("\nMC-7 Training Jobs:\n");
  if inspection.training_jobs.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in &inspection.training_jobs {
      let job = &artifact.payload;
      output.push_str(&format!(
        "- artifact={} schema={} source_training_launch_plan={} source_runs={} frames={} images={} status={} provider_backend={} trainer_backend={} job_backend={} accepted_by_provider={} submission_recorded_at_millis={} job_id={} job_url={} readiness_blocker={} job_submission_endpoint={} job_submission_command={} exported={} skipped={} transforms={}\n",
        artifact.uri,
        job.schema_version,
        job.source_training_launch_plan_path,
        job.source_run_ids.len(),
        job.counts.frames,
        job.counts.images,
        job.status.as_str(),
        job.provider_backend,
        job.trainer_backend,
        job.job_backend,
        job.accepted_by_provider,
        optional_display(job.submission_recorded_at_millis),
        job.job_id.as_deref().unwrap_or("n/a"),
        job.job_url.as_deref().unwrap_or("n/a"),
        job.readiness_blocker.map(|blocker| blocker.as_str()).unwrap_or("n/a"),
        job.job_submission_endpoint,
        job.job_submission_command,
        job.counts.compatibility_exported_frames,
        job.counts.compatibility_skipped_frames,
        presence_label(job.transforms_path.as_ref()),
      ));
      append_known_limits(output, &job.known_limits);
    }
  }

  output.push_str("\nMC-7 Training Results:\n");
  if inspection.training_results.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in &inspection.training_results {
      let result = &artifact.payload;
      output.push_str(&format!(
        "- artifact={} schema={} source_training_job={} source_training_launch_plan={} source_runs={} trainer_backend={} job_backend={} source_job_status={} status={} status_message={} job_id={} job_url={} result_dir={} exported={} skipped={} result_artifacts={}\n",
        artifact.uri,
        result.schema_version,
        result.source_training_job_manifest_path,
        result.source_training_launch_plan_path,
        result.source_run_ids.len(),
        result.trainer_backend,
        result.job_backend,
        result.source_job_status.as_str(),
        result.status.as_str(),
        result.status_message.as_deref().unwrap_or("n/a"),
        result.job_id,
        result.job_url.as_deref().unwrap_or("n/a"),
        result.result_dir,
        result.exported_frame_count,
        result.skipped_frame_count,
        result.result_artifacts.len(),
      ));
      append_known_limits(output, &result.known_limits);
    }
  }

  output.push_str("\nMC-7 Training Result Artifacts:\n");
  if inspection.training_results.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in &inspection.training_results {
      let result = &artifact.payload;
      let readable = result.result_artifacts.iter().filter(|record| record.readable).count();
      let recorded_bytes = result.result_artifacts.iter().filter_map(|record| record.byte_size).sum::<u64>();
      output.push_str(&format!(
        "- training_result_artifact={} result_dir={} result_artifacts={} readable={} recorded_bytes={}\n",
        artifact.uri,
        result.result_dir,
        result.result_artifacts.len(),
        readable,
        recorded_bytes,
      ));
    }
  }

  output.push_str("\nMC-10 Training Result Semantics:\n");
  if inspection.semantics.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in &inspection.semantics {
      let semantic = &artifact.payload;
      output.push_str(&format!(
        "- artifact={} schema={} source_training_result_artifact={} source_training_result={} source_runs={} trainer_backend={} job_backend={} source_result_status={} semantic_status={} semantic_reason={} normalized_result_dir={} config_path={} models_dir_path={} status_snapshot_path={} config_trainer={} checkpoints={}\n",
        artifact.uri,
        semantic.schema_version,
        semantic.source_training_result_artifact_manifest_path,
        semantic.source_training_result_manifest_path,
        semantic.source_run_ids.len(),
        semantic.trainer_backend,
        semantic.job_backend,
        semantic.source_result_status.as_str(),
        semantic.semantic_status.as_str(),
        semantic.semantic_reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        semantic.normalized_result_dir,
        semantic.config_path,
        semantic.models_dir_path,
        semantic.status_snapshot_path.as_deref().unwrap_or("n/a"),
        semantic.config_trainer.as_deref().unwrap_or("n/a"),
        semantic.checkpoint_count,
      ));
      append_known_limits(output, &semantic.known_limits);
    }
  }

  output.push_str("\nMC-16 Training Result Holdout Preview:\n");
  if inspection.holdout_previews.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in &inspection.holdout_previews {
      let preview = &artifact.payload;
      output.push_str(&format!(
        "- artifact={} schema={} training_result_semantic_manifest={} source_training_result_artifact={} source_runs={} status={} reason={} frame_index={} checkpoint={} screenshot={} reference_overlay={} spatial_frame={}\n",
        artifact.uri,
        preview.schema_version,
        preview.training_result_semantic_manifest_path,
        preview.source_training_result_artifact_manifest_path,
        preview.source_run_ids.len(),
        preview.status.as_str(),
        preview.reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        preview.holdout_frame_index,
        presence_label(preview.basis_checkpoint_path.as_ref()),
        presence_label(preview.holdout_screenshot_path.as_ref()),
        presence_label(preview.reference_overlay_path.as_ref()),
        preview.holdout_frame.as_ref().map(|frame| frame.spatial_frame_id.as_str()).unwrap_or("n/a"),
      ));
      append_known_limits(output, &preview.known_limits);
    }
  }

  output.push_str("\nMC-17 Holdout Render Quality:\n");
  if inspection.render_quality.is_empty() {
    output.push_str("- none\n");
  } else {
    for artifact in &inspection.render_quality {
      append_render_quality(output, &artifact.uri.to_string(), &artifact.payload);
    }
  }
}

pub(crate) fn append_quality_and_spatial_sections(output: &mut String, inspection: &MinecraftQualitySpatialInspection) {
  append_quality_baseline_report(output, inspection.quality_baseline());

  output.push_str("\nMC-17 Quality Verdict:\n");
  append_quality_verdict(output, &inspection.quality_baseline().verdicts.probe);
  append_quality_verdict(output, &inspection.quality_baseline().verdicts.trained_render);

  output.push_str("\nMC-12 Training Result Spatial Query:\n");
  if inspection.spatial_queries.is_empty() {
    output.push_str("- none\n");
  } else {
    for query in &inspection.spatial_queries {
      let manifest = &query.artifact.payload;
      output.push_str(&format!(
        "- artifact={} schema={} training_result_semantic_manifest={} source_training_result_artifact={} source_runs={} query_kind={} status={} reason={} target_block={},{},{} target_face={} target_semantics={} visibility={} screen_point={} match_radius_px={} confidence={} backend={} comparison={} basis_frame={}\n",
        query.artifact.uri,
        manifest.schema_version,
        manifest.training_result_semantic_manifest_path,
        manifest.source_training_result_artifact_manifest_path,
        manifest.source_run_ids.len(),
        spatial_query_kind_label(manifest.query_kind),
        manifest.status.as_str(),
        manifest.reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
        manifest.target_block.x,
        manifest.target_block.y,
        manifest.target_block.z,
        manifest.target_face.map(block_face_label).unwrap_or("n/a"),
        target_semantics_label(manifest.target_semantics),
        manifest.visibility.map(visibility_label).unwrap_or("n/a"),
        manifest.screen_point.map(point_label).unwrap_or_else(|| "n/a".to_string()),
        optional_display(manifest.match_radius_px),
        optional_display(manifest.confidence),
        manifest.selected_backend.map(|backend| backend.as_str()).unwrap_or("n/a"),
        manifest.comparison_verdict.map(|verdict| verdict.as_str()).unwrap_or("n/a"),
        manifest.basis_frame_id.as_deref().unwrap_or("n/a"),
      ));
      append_known_limits(output, &manifest.known_limits);
    }
  }

  output.push_str("\nMC-14 Training Result Spatial Query Action Readiness:\n");
  if inspection.spatial_queries.is_empty() {
    output.push_str("- none\n");
  } else {
    for query in &inspection.spatial_queries {
      let eligibility = query.readiness.eligibility.as_str();
      let readiness_class = auv_query_readiness::map_action_eligibility_to_readiness_class(eligibility);
      let window_point = query.readiness.window_point.map(Point::from).map(point_label).unwrap_or_else(|| "n/a".to_string());
      let manifest = &query.artifact.payload;
      output.push_str(&format!(
        "- query_artifact={} target_block={},{},{} status={} visibility={} selected_backend={} action_eligibility={} readiness_class={} window_point={} refusal_reason={}\n",
        query.artifact.uri,
        manifest.target_block.x,
        manifest.target_block.y,
        manifest.target_block.z,
        manifest.status.as_str(),
        manifest.visibility.map(visibility_label).unwrap_or("n/a"),
        manifest.selected_backend.map(|backend| backend.as_str()).unwrap_or("n/a"),
        eligibility,
        readiness_class.as_deref().unwrap_or("n/a"),
        window_point,
        query.readiness.refusal_reason.as_deref().unwrap_or("n/a"),
      ));
    }
  }
}

fn append_quality_baseline_report(output: &mut String, baseline: &MinecraftQualityBaseline) {
  output.push_str("\nMC-17 Quality Baseline Report:\n");
  let spatial = baseline.spatial_query.as_ref();
  let holdout = baseline.holdout_witness.as_ref();
  let render = baseline.render_quality.as_ref();
  output.push_str(&format!(
    "- profile_id={} evidence_coverage={} spatial_query_artifact={} spatial_query_status={} visibility={} screen_point={} holdout_artifact={} holdout_status={} holdout_frame_index={} checkpoint={} render_quality_artifact={} render_quality_status={} verdict={} image_size_match={} l1_mean={} mse={} psnr={} profile_mismatches={}\n",
    baseline.profile_id,
    baseline.evidence_coverage.as_str(),
    spatial.map(|artifact| artifact.uri.to_string()).unwrap_or_else(|| "n/a".to_string()),
    spatial.map(|artifact| artifact.payload.status.as_str()).unwrap_or("n/a"),
    spatial.and_then(|artifact| artifact.payload.visibility).map(visibility_label).unwrap_or("n/a"),
    spatial.and_then(|artifact| artifact.payload.screen_point).map(point_label).unwrap_or_else(|| "n/a".to_string()),
    holdout.map(|artifact| artifact.uri.to_string()).unwrap_or_else(|| "n/a".to_string()),
    holdout.map(|artifact| artifact.payload.status.as_str()).unwrap_or("n/a"),
    holdout.map(|artifact| artifact.payload.holdout_frame_index.to_string()).unwrap_or_else(|| "n/a".to_string()),
    holdout.map(|artifact| presence_label(artifact.payload.basis_checkpoint_path.as_ref())).unwrap_or("n/a"),
    render.map(|artifact| artifact.uri.to_string()).unwrap_or_else(|| "n/a".to_string()),
    render.map(|artifact| artifact.payload.status.as_str()).unwrap_or("n/a"),
    render.map(|artifact| artifact.payload.verdict.as_str()).unwrap_or("n/a"),
    render.map(|artifact| artifact.payload.image_size_match.to_string()).unwrap_or_else(|| "n/a".to_string()),
    render.and_then(|artifact| artifact.payload.metrics.as_ref()).and_then(|metrics| metrics.l1_mean).map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
    render.and_then(|artifact| artifact.payload.metrics.as_ref()).and_then(|metrics| metrics.mse).map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
    render.and_then(|artifact| artifact.payload.metrics.as_ref()).and_then(|metrics| metrics.psnr).map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
    if baseline.mismatched_stages.is_empty() {
      "none".to_string()
    } else {
      baseline.mismatched_stages.iter().map(|stage| stage.as_str()).collect::<Vec<_>>().join(",")
    },
  ));
  if !baseline.trust_notes.is_empty() {
    output.push_str(&format!("  trust_notes={}\n", baseline.trust_notes.join(" | ")));
  }
}

fn append_quality_verdict(output: &mut String, verdict: &MinecraftQualityVerdict) {
  let stage_checks = verdict
    .stage_checks
    .iter()
    .map(|check| {
      let reason = check.reasons.first().map(|reason| format!(" reason={reason}")).unwrap_or_default();
      format!("{}={}{}", check.stage.as_str(), check.outcome.as_str(), reason)
    })
    .collect::<Vec<_>>()
    .join(" ");
  output.push_str(&format!(
    "- profile_id={} render_evidence_mode={} evidence_coverage={} quality_verdict={} {}\n",
    verdict.profile_id,
    verdict.render_evidence_mode.as_str(),
    verdict.evidence_coverage.as_str(),
    verdict.quality_verdict.as_str(),
    stage_checks,
  ));
  if !verdict.trust_notes.is_empty() {
    output.push_str(&format!("  trust_notes={}\n", verdict.trust_notes.join(" | ")));
  }
}

fn append_render_quality(output: &mut String, uri: &str, quality: &TrainingResultHoldoutRenderQualityManifest) {
  let metrics = quality.metrics.as_ref();
  output.push_str(&format!(
    "- artifact={} schema={} training_result_semantic_manifest={} holdout_preview_manifest={} source_training_result_artifact={} source_runs={} status={} reason={} verdict={} frame_index={} checkpoint={} screenshot={} rendered_image={} image_size_match={} render_backend={} l1_mean={} mse={} psnr={}\n",
    uri,
    quality.schema_version,
    quality.training_result_semantic_manifest_path,
    quality.holdout_preview_manifest_path,
    quality.source_training_result_artifact_manifest_path,
    quality.source_run_ids.len(),
    quality.status.as_str(),
    quality.reason.map(|reason| reason.as_str()).unwrap_or("n/a"),
    quality.verdict.as_str(),
    quality.holdout_frame_index,
    presence_label(quality.basis_checkpoint_path.as_ref()),
    presence_label(quality.holdout_screenshot_path.as_ref()),
    presence_label(quality.rendered_image_path.as_ref()),
    quality.image_size_match,
    quality.render_backend.as_str(),
    metrics.and_then(|value| value.l1_mean).map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
    metrics.and_then(|value| value.mse).map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
    metrics.and_then(|value| value.psnr).map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
  ));
  append_known_limits(output, &quality.known_limits);
}

fn append_known_limits(output: &mut String, known_limits: &[String]) {
  if !known_limits.is_empty() {
    output.push_str(&format!("  known_limits={}\n", known_limits.join(" | ")));
  }
}

fn presence_label<T>(value: Option<&T>) -> &'static str {
  if value.is_some() { "present" } else { "none" }
}

fn optional_display(value: Option<impl std::fmt::Display>) -> String {
  value.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string())
}

fn point_label(point: Point) -> String {
  format!("{},{}", point.x, point.y)
}

fn projected_point_label(projected_point: Option<&MinecraftProjectedPoint>) -> String {
  let Some(projected_point) = projected_point else {
    return "n/a".to_string();
  };
  format!(
    "screen={} visibility={} radius_px={} confidence={} basis={}",
    projected_point.screen_point.map(point_label).unwrap_or_else(|| "n/a".to_string()),
    visibility_label(projected_point.visibility),
    projected_point.match_radius_px,
    projected_point.confidence,
    projected_point.basis_frame_id,
  )
}

fn compatibility_status_label(status: TrainingCompatibilityStatus) -> &'static str {
  match status {
    TrainingCompatibilityStatus::Ready => "ready",
    TrainingCompatibilityStatus::Partial => "partial",
    TrainingCompatibilityStatus::Blocked => "blocked",
  }
}

fn block_face_label(face: BlockFace) -> &'static str {
  match face {
    BlockFace::Up => "up",
    BlockFace::Down => "down",
    BlockFace::North => "north",
    BlockFace::South => "south",
    BlockFace::East => "east",
    BlockFace::West => "west",
  }
}

fn target_semantics_label(semantics: MinecraftTargetSemantics) -> &'static str {
  match semantics {
    MinecraftTargetSemantics::HitFaceCenter => "hit_face_center",
    MinecraftTargetSemantics::BlockCenter => "block_center",
  }
}

fn spatial_query_kind_label(kind: TrainingResultSpatialQueryKind) -> &'static str {
  match kind {
    TrainingResultSpatialQueryKind::BlockProjection => "block_projection",
  }
}

fn visibility_label(visibility: ProjectionVisibility) -> &'static str {
  match visibility {
    ProjectionVisibility::Visible => "visible",
    ProjectionVisibility::BehindCamera => "behind_camera",
    ProjectionVisibility::OutOfFrustum => "out_of_frustum",
    ProjectionVisibility::OutsideWindow => "outside_window",
  }
}
