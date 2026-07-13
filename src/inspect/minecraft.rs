//! Minecraft donor render sections for inspect output.

use super::*;

pub(super) fn append_primary_sections(
  output: &mut String,
  minecraft_projection_artifacts: &[auv_game_minecraft::MinecraftProjectionArtifact],
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
) {
  output.push_str("\nMC-1 Telemetry Samples:\n");
  if minecraft_telemetry_sample_artifacts.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in minecraft_telemetry_sample_artifacts {
      output.push_str(&format!(
        "- artifact={} line_count={} bytes={} path={} issue={}\n",
        lineage.artifact.artifact_id,
        lineage.line_count.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
        lineage.byte_size.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
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
  if minecraft_training_package_manifests.is_empty() && minecraft_training_package_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
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
          output.push_str(&format!("  known_limits={}\n", manifest.known_limits.join(" | ")));
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
          output.push_str(&format!("  inspect_known_limits={}\n", report.known_limits.join(" | ")));
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
  if minecraft_training_launch_manifests.is_empty() && minecraft_training_launch_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_launch_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_launch_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| {
            report.source_training_package_manifest_path == manifest.source_training_package_manifest_path
              && report.source_scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
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
          output.push_str(&format!("  known_limits={}\n", manifest.known_limits.join(" | ")));
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
            output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
          output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
  if minecraft_training_job_manifests.is_empty() && minecraft_training_job_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_job_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_job_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| {
            report.source_training_launch_plan_path == manifest.source_training_launch_plan_path
              && report.source_training_package_manifest_path == manifest.source_training_package_manifest_path
              && report.source_scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
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
          output.push_str(&format!("  known_limits={}\n", manifest.known_limits.join(" | ")));
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
            output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
          output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
  if minecraft_training_result_manifests.is_empty() && minecraft_training_result_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_result_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_result_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| {
            report.source_training_job_manifest_path == manifest.source_training_job_manifest_path
              && report.source_training_launch_plan_path == manifest.source_training_launch_plan_path
              && report.source_scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
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
          output.push_str(&format!("  known_limits={}\n", manifest.known_limits.join(" | ")));
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
            output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
          output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
  if minecraft_training_result_artifact_fetch_manifests.is_empty() && minecraft_training_result_artifact_fetch_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_result_artifact_fetch_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_result_artifact_fetch_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| {
            report.source_training_result_manifest_path == manifest.source_training_result_manifest_path
              && report.source_training_job_manifest_path == manifest.source_training_job_manifest_path
              && report.source_run_ids == manifest.source_run_ids
          })
        })
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
              artifact.byte_size.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
              artifact.absolute_path,
            ));
          }
        }
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!("  known_limits={}\n", manifest.known_limits.join(" | ")));
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
            output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
          output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
  if minecraft_training_result_semantic_manifests.is_empty() && minecraft_training_result_semantic_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_result_semantic_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_result_semantic_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| {
            report.source_training_result_artifact_manifest_path == manifest.source_training_result_artifact_manifest_path
              && report.source_training_result_manifest_path == manifest.source_training_result_manifest_path
              && report.source_training_job_manifest_path == manifest.source_training_job_manifest_path
              && report.source_training_launch_plan_path == manifest.source_training_launch_plan_path
              && report.source_scene_packet_manifest_path == manifest.source_scene_packet_manifest_path
              && report.source_run_ids == manifest.source_run_ids
          })
        })
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
              .map(|checkpoint| format!("relative_path={} byte_size={}", checkpoint.relative_path, checkpoint.byte_size))
              .collect::<Vec<_>>()
              .join(" | ")
          ));
        }
        if !manifest.known_limits.is_empty() {
          output.push_str(&format!("  known_limits={}\n", manifest.known_limits.join(" | ")));
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
            output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
          output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
  if minecraft_training_result_holdout_preview_manifests.is_empty() && minecraft_training_result_holdout_preview_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_result_holdout_preview_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_result_holdout_preview_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| holdout_preview_manifest_matches_report(manifest, report))
        })
      });
      if let Some(report_lineage) = paired_report {
        rendered_report_artifacts.insert(report_lineage.artifact.artifact_id.to_string());
      }
      if let Some(manifest) = &manifest_lineage.manifest {
        let spatial_frame_id = manifest.holdout_frame.as_ref().map(|witness| witness.spatial_frame_id.as_str()).unwrap_or("n/a");
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
          output.push_str(&format!("  known_limits={}\n", manifest.known_limits.join(" | ")));
        }
        if let Some(report) = paired_report.and_then(|lineage| lineage.report.as_ref()) {
          output.push_str(&format!(
            "  paired_report schema={} holdout_frame_selection={} checkpoint_count={} scene_packet_frame_count={} warnings={} issue={}\n",
            report.schema_version,
            report.holdout_frame_selection,
            report.checkpoint_count,
            report.scene_packet_frame_count,
            report.warnings.len(),
            paired_report.and_then(|lineage| lineage.issue.as_deref()).unwrap_or("n/a"),
          ));
          if !report.warnings.is_empty() {
            output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
          }
          if !report.known_limits.is_empty() {
            output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
          output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
  if minecraft_holdout_render_quality_manifests.is_empty() && minecraft_holdout_render_quality_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_holdout_render_quality_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_holdout_render_quality_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| holdout_render_quality_manifest_matches_report(manifest, report))
        })
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
              metrics.l1_mean.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
              metrics.mse.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
              metrics.psnr.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
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
          output.push_str(&format!("  known_limits={}\n", manifest.known_limits.join(" | ")));
        }
        if let Some(report) = paired_report.and_then(|lineage| lineage.report.as_ref()) {
          output.push_str(&format!(
            "  paired_report schema={} image_size_match={} verdict={} warnings={} issue={}\n",
            report.schema_version,
            report.image_size_match,
            report.verdict,
            report.warnings.len(),
            paired_report.and_then(|lineage| lineage.issue.as_deref()).unwrap_or("n/a"),
          ));
          if !report.warnings.is_empty() {
            output.push_str(&format!("  warnings={}\n", report.warnings.join(" | ")));
          }
          if !report.known_limits.is_empty() {
            output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
              metrics.l1_mean.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
              metrics.mse.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
              metrics.psnr.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
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
          output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
}

pub(super) fn append_quality_and_spatial_sections(
  output: &mut String,
  minecraft_training_result_spatial_query_manifests: &[MinecraftTrainingResultSpatialQueryManifestLineage],
  minecraft_training_result_spatial_query_inspect_reports: &[MinecraftTrainingResultSpatialQueryInspectReportLineage],
  quality_baseline_report: Option<&MinecraftTrainingResultQualityBaselineReportSummary>,
  quality_verdict_probe: Option<&MinecraftTrainingResultQualityVerdictSummary>,
  quality_verdict_trained_render: Option<&MinecraftTrainingResultQualityVerdictSummary>,
) {
  output.push_str("\nMC-17 Quality Baseline Report:\n");
  if let Some(report) = quality_baseline_report {
    let spatial_query_status = report.spatial_query.as_ref().map(|evidence| evidence.status.as_str()).unwrap_or("n/a");
    let spatial_visibility = report.spatial_query.as_ref().and_then(|evidence| evidence.visibility.as_deref()).unwrap_or("n/a");
    let spatial_screen_point = report.spatial_query.as_ref().and_then(|evidence| evidence.screen_point.as_deref()).unwrap_or("n/a");
    let holdout_status = report.holdout_witness.as_ref().map(|evidence| evidence.status.as_str()).unwrap_or("n/a");
    let holdout_frame_index =
      report.holdout_witness.as_ref().map(|evidence| evidence.holdout_frame_index.to_string()).unwrap_or_else(|| "n/a".to_string());
    let basis_checkpoint_path =
      report.holdout_witness.as_ref().and_then(|evidence| evidence.basis_checkpoint_path.as_deref()).unwrap_or("n/a");
    let render_quality_status = report.render_quality.as_ref().map(|evidence| evidence.status.as_str()).unwrap_or("n/a");
    let render_verdict = report.render_quality.as_ref().map(|evidence| evidence.verdict.as_str()).unwrap_or("n/a");
    let l1_mean = report
      .render_quality
      .as_ref()
      .and_then(|evidence| evidence.l1_mean)
      .map(|value| value.to_string())
      .unwrap_or_else(|| "n/a".to_string());
    let mse =
      report.render_quality.as_ref().and_then(|evidence| evidence.mse).map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string());
    let psnr =
      report.render_quality.as_ref().and_then(|evidence| evidence.psnr).map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string());
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
      output.push_str(&format!("  trust_notes={}\n", report.trust_notes.join(" | ")));
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
  if minecraft_training_result_spatial_query_manifests.is_empty() && minecraft_training_result_spatial_query_inspect_reports.is_empty() {
    output.push_str("- none\n");
  } else {
    let mut rendered_report_artifacts = BTreeSet::new();
    for manifest_lineage in minecraft_training_result_spatial_query_manifests {
      let paired_report = manifest_lineage.manifest.as_ref().and_then(|manifest| {
        unique_matching_report(minecraft_training_result_spatial_query_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| spatial_query_manifest_matches_report(manifest, report))
        })
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
          output.push_str(&format!("  known_limits={}\n", manifest.known_limits.join(" | ")));
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
            output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
          output.push_str(&format!("  known_limits={}\n", report.known_limits.join(" | ")));
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
        unique_matching_report(minecraft_training_result_spatial_query_inspect_reports, |lineage| {
          lineage.report.as_ref().is_some_and(|report| spatial_query_manifest_matches_report(manifest, report))
        })
      });
      let readiness = derive_minecraft_training_result_spatial_query_action_readiness(manifest_lineage);
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
}

pub(super) fn append_query_wired_section(
  output: &mut String,
  minecraft_query_wired_live_action_summaries: &[MinecraftQueryWiredLiveActionSummary],
) {
  output.push_str("\nMC-19 Query Wired Live Action:\n");
  if minecraft_query_wired_live_action_summaries.is_empty() {
    output.push_str("- none\n");
  } else {
    for summary in minecraft_query_wired_live_action_summaries {
      output.push_str(&format!(
        "- operation_result_artifact={} query_artifact={} attempted={} action_eligibility={} window_point={} refusal_reason={} operation_status={} operation_message={} dispatch_command={} dispatch_outcome={} target_app={} target_title={} mc14_action_eligibility={} readiness_class={} source_readiness_ref={} verification_outcome={} verification_source={} verification_reason={} issue={}\n",
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
        summary.source_readiness_ref.as_deref().unwrap_or("n/a"),
        summary.verification_outcome.as_str(),
        summary.verification_source.as_deref().unwrap_or("n/a"),
        summary.verification_reason.as_deref().unwrap_or("n/a"),
        summary.issue.as_deref().unwrap_or("n/a"),
      ));
    }
  }
}
