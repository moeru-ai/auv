//! Product inspect sections assembled from app-owned and product-owned readers.
//!
//! Query-wired sections remain product-owned because they depend on
//! `OperationResult` adapters. Ordinary app-specific sections are supplied by
//! `auv-game-*` factories.
//!
//! Product CLI snapshot inspection uses canonical app readers. The temporary
//! text projection composes those sections with legacy core/osu readers until
//! the remaining Task 22 migrations land. Viewer app-specific cards still
//! fetch named JSON extensions by key, not first-class Minecraft routes.

use std::sync::Arc;

use auv_inspect_model::{InspectComposer, InspectError, InspectSection, InspectSectionOutput};
use auv_runtime::inspect::{CorePrefixSection, CoreSuffixSection};
use auv_tracing::{RunSnapshot, RunStore};
use auv_tracing_driver::store::{CanonicalRun, LocalStore};

use super::query_wired_minecraft::{append_minecraft_query_wired_section, collect_minecraft_query_wired_live_action_summaries};
use super::query_wired_osu::append_osu_query_wired_section;
use crate::run_read::list_osu_query_wired_live_action_summaries;

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ProductInspectSection {
  pub id: &'static str,
  pub text: String,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ProductInspectDocument {
  #[serde(flatten)]
  canonical: auv_inspect_model::InspectDocument,
  pub sections: Vec<ProductInspectSection>,
}

impl ProductInspectDocument {
  pub fn render_text(&self) -> String {
    self.sections.iter().map(|section| section.text.as_str()).collect()
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProductInspectTextDocument {
  pub sections: Vec<ProductInspectSection>,
}

impl ProductInspectTextDocument {
  pub fn render_text(&self) -> String {
    let mut output = String::new();
    for section in &self.sections {
      output.push_str(&section.text);
      if !section.text.ends_with('\n') {
        output.push('\n');
      }
    }
    output
  }
}

#[derive(Debug)]
pub enum ProductInspectError {
  Legacy(InspectError),
  Minecraft(auv_game_minecraft::MinecraftArtifactReadError),
  Balatro(auv_game_balatro::BalatroArtifactReadError),
  MissingLegacySection(&'static str),
  UnexpectedSection(&'static str),
}

impl std::fmt::Display for ProductInspectError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Legacy(error) => write!(formatter, "legacy inspect composition failed: {error}"),
      Self::Minecraft(error) => write!(formatter, "Minecraft inspection failed: {error}"),
      Self::Balatro(error) => write!(formatter, "Balatro inspection failed: {error}"),
      Self::MissingLegacySection(id) => write!(formatter, "legacy inspect composition omitted required section {id}"),
      Self::UnexpectedSection(id) => write!(formatter, "product inspect composition returned unexpected section {id}"),
    }
  }
}

impl std::error::Error for ProductInspectError {}

impl From<InspectError> for ProductInspectError {
  fn from(value: InspectError) -> Self {
    Self::Legacy(value)
  }
}

impl From<auv_game_minecraft::MinecraftArtifactReadError> for ProductInspectError {
  fn from(value: auv_game_minecraft::MinecraftArtifactReadError) -> Self {
    Self::Minecraft(value)
  }
}

impl From<auv_game_balatro::BalatroArtifactReadError> for ProductInspectError {
  fn from(value: auv_game_balatro::BalatroArtifactReadError) -> Self {
    Self::Balatro(value)
  }
}

pub async fn build_product_inspect_document(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<ProductInspectDocument, ProductInspectError> {
  let sections = collect_canonical_app_sections(store, snapshot).await?;
  Ok(ProductInspectDocument {
    canonical: auv_inspect_model::InspectDocument::from(snapshot),
    sections,
  })
}

/// Composes the staged product text view without adapting canonical app
/// artifacts back into the retired path-backed record format.
pub async fn build_product_inspect_text_document(
  legacy_store: &LocalStore,
  legacy_run: &CanonicalRun,
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<ProductInspectTextDocument, ProductInspectError> {
  let legacy = build_product_inspect_composer()?.collect_document(legacy_store, legacy_run)?;
  let mut legacy_sections = legacy.sections.into_iter().map(|section| (section.id, section)).collect::<std::collections::BTreeMap<_, _>>();
  let canonical_sections = collect_canonical_app_sections(store, snapshot).await?;
  let mut canonical_sections =
    canonical_sections.into_iter().map(|section| (section.id, section)).collect::<std::collections::BTreeMap<_, _>>();

  let mut sections = Vec::new();
  for id in [
    "core_prefix",
    "minecraft_primary",
    "balatro_card_detection",
    "minecraft_quality_spatial",
    "osu_visual_truth_primary",
    "osu_query_wired_live_action",
    "osu_detection_eval",
    "minecraft_query_wired_live_action",
    "core_suffix",
  ] {
    if let Some(section) = canonical_sections.remove(id) {
      sections.push(section);
      continue;
    }
    let section = legacy_sections.remove(id).ok_or(ProductInspectError::MissingLegacySection(id))?;
    sections.push(ProductInspectSection {
      id: section.id,
      text: section.text,
    });
  }

  if let Some(id) = canonical_sections.keys().chain(legacy_sections.keys()).next().copied() {
    return Err(ProductInspectError::UnexpectedSection(id));
  }
  Ok(ProductInspectTextDocument { sections })
}

async fn collect_canonical_app_sections(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
) -> Result<Vec<ProductInspectSection>, ProductInspectError> {
  let primary = auv_game_minecraft::inspect_sections_primary(store, snapshot).await?;
  let balatro = auv_game_balatro::inspect::render_balatro_card_detection_text(store, snapshot).await?;
  let quality_spatial = auv_game_minecraft::inspect_sections_quality_spatial(store, snapshot).await?;
  let minecraft_query_wired = collect_minecraft_query_wired_live_action_summaries(store, snapshot).await?;
  let mut minecraft_query_wired_text = String::new();
  append_minecraft_query_wired_section(&mut minecraft_query_wired_text, &minecraft_query_wired);

  let mut sections = Vec::with_capacity(primary.len() + 2 + quality_spatial.len());
  sections.extend(primary.into_iter().map(minecraft_section));
  sections.push(ProductInspectSection {
    id: auv_game_balatro::inspect::BalatroCardDetectionSection::ID,
    text: balatro,
  });
  sections.extend(quality_spatial.into_iter().map(minecraft_section));
  sections.push(ProductInspectSection {
    id: "minecraft_query_wired_live_action",
    text: minecraft_query_wired_text,
  });
  Ok(sections)
}

fn minecraft_section(section: auv_game_minecraft::inspect::MinecraftInspectSection) -> ProductInspectSection {
  ProductInspectSection {
    id: section.id(),
    text: section.into_text(),
  }
}

pub struct OsuQueryWiredLiveActionSection;

impl InspectSection for OsuQueryWiredLiveActionSection {
  fn id(&self) -> &'static str {
    "osu_query_wired_live_action"
  }

  fn collect(&self, store: &LocalStore, run: &CanonicalRun) -> Result<Option<InspectSectionOutput>, InspectError> {
    let summaries = list_osu_query_wired_live_action_summaries(store, run.run.run_id.as_str())?;
    let mut text = String::new();
    append_osu_query_wired_section(&mut text, &summaries);
    Ok(Some(InspectSectionOutput {
      id: self.id(),
      text,
      json: None,
    }))
  }
}

/// LOCKED golden render order (do not invent another).
pub fn build_product_inspect_composer() -> Result<Arc<InspectComposer>, InspectError> {
  let mut sections: Vec<Arc<dyn InspectSection>> = Vec::new();
  // 1. core_prefix
  sections.push(Arc::new(CorePrefixSection));
  // TODO(run-contract-task-22): Retire this legacy composer after core, osu!,
  // and product query-wired readers accept RunStore/RunSnapshot. Canonical
  // Balatro and Minecraft sections are merged by
  // build_product_inspect_text_document.
  // 2. osu A
  sections.extend(auv_game_osu::inspect_sections_primary());
  // 3. osu query-wired (PRODUCT)
  sections.push(Arc::new(OsuQueryWiredLiveActionSection));
  // 4. osu B
  sections.extend(auv_game_osu::inspect_sections_detection_eval());
  // 5. core_suffix
  sections.push(Arc::new(CoreSuffixSection));
  InspectComposer::try_new(sections).map(Arc::new)
}

#[cfg(test)]
mod tests {
  use auv_game_minecraft::scene_packet::publish_minecraft_scene_packet;
  use auv_game_minecraft::training_job::publish_minecraft_training_job;
  use auv_game_minecraft::training_package::publish_minecraft_training_package;
  use auv_game_minecraft::training_result::publish_minecraft_training_result;
  use auv_game_minecraft::training_result_holdout_preview::publish_minecraft_training_holdout_preview;
  use auv_game_minecraft::training_result_holdout_render_quality::publish_minecraft_training_holdout_render_quality;
  use auv_game_minecraft::training_result_semantic::publish_minecraft_training_semantic;
  use auv_game_minecraft::training_result_spatial_query::publish_minecraft_training_spatial_query;
  use auv_tracing::dispatcher;
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure};
  use serde::de::DeserializeOwned;
  use serde_json::{Value, json};

  use super::*;

  #[test]
  fn legacy_composer_keeps_remaining_section_order() {
    let composer = build_product_inspect_composer().expect("product composer");
    let ids = composer.sections().iter().map(|section| section.id()).collect::<Vec<_>>();
    assert_eq!(
      ids,
      [
        "core_prefix",
        "osu_visual_truth_primary",
        "osu_query_wired_live_action",
        "osu_detection_eval",
        "core_suffix",
      ]
    );
  }

  #[tokio::test]
  async fn product_snapshot_inspect_restores_typed_minecraft_sections() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));

    let scene_packet = decode_fixture(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "source_bundle_manifest_paths": ["bundle.json"],
      "source_run_ids": ["run-source"],
      "counts": {"frames": 1, "screenshots": 1, "missing_screenshots": 0},
      "frames": [],
      "known_limits": ["fixture"]
    }));
    let training_package = decode_fixture(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "source_scene_packet_manifest_path": "scene.json",
      "source_bundle_manifest_paths": ["bundle.json"],
      "source_run_ids": ["run-source"],
      "counts": {"frames": 1, "images": 1, "compatibility_exported_frames": 1, "compatibility_skipped_frames": 0},
      "frames": [],
      "compatibility_views": [{
        "view_name": "nerfstudio",
        "status": "ready",
        "exported_frame_count": 1,
        "skipped_frame_count": 0,
        "transforms_path": "transforms.json",
        "export_report_path": "export.json",
        "exported_frame_indices": [0],
        "frame_decisions": [],
        "skip_reason_counts": [],
        "warnings": [],
        "used_legacy_view_translation_fallback_frame_indices": [],
        "known_limits": []
      }],
      "known_limits": ["fixture"]
    }));
    let training_job = decode_fixture(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "source_training_launch_plan_path": "launch.json",
      "source_training_package_manifest_path": "package.json",
      "source_training_package_inspect_report_path": "package-inspect.json",
      "source_scene_packet_manifest_path": "scene.json",
      "source_bundle_manifest_paths": ["bundle.json"],
      "source_run_ids": ["run-source"],
      "counts": {"frames": 1, "images": 1, "compatibility_exported_frames": 1, "compatibility_skipped_frames": 0},
      "compatibility_view_name": "nerfstudio",
      "provider_backend": "fixture",
      "trainer_backend": "nerfstudio",
      "job_backend": "fixture",
      "job_submission_endpoint": "fixture://submit",
      "job_submission_command": "submit",
      "submission_recorded_at_millis": 21,
      "accepted_by_provider": true,
      "training_data_dir": "training-data",
      "transforms_path": "transforms.json",
      "export_report_path": "export.json",
      "suggested_output_dir": "result",
      "launch_command": "train",
      "status": "submitted",
      "job_id": "job-20",
      "job_url": "fixture://jobs/job-20",
      "known_limits": ["fixture"]
    }));
    let training_result = decode_fixture(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "source_training_job_manifest_path": "job.json",
      "source_training_launch_plan_path": "launch.json",
      "source_training_package_manifest_path": "package.json",
      "source_scene_packet_manifest_path": "scene.json",
      "source_bundle_manifest_paths": ["bundle.json"],
      "source_run_ids": ["run-source"],
      "trainer_backend": "nerfstudio",
      "job_backend": "fixture",
      "job_submission_endpoint": "fixture://submit",
      "source_job_status": "submitted",
      "status": "succeeded",
      "status_message": "training complete",
      "job_id": "job-20",
      "job_url": "fixture://jobs/job-20",
      "result_dir": "result",
      "exported_frame_count": 1,
      "skipped_frame_count": 0,
      "result_artifacts": [{"relative_path": "config.yml", "absolute_path": "result/config.yml", "readable": true, "byte_size": 42}],
      "known_limits": ["fixture"]
    }));
    let semantic = decode_fixture(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
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
      "source_result_status": "succeeded",
      "normalized_result_dir": "normalized",
      "semantic_status": "ready",
      "config_path": "config.yml",
      "models_dir_path": "models",
      "status_snapshot_path": "job_status.json",
      "config_trainer": "nerfstudio",
      "checkpoint_files": [{"relative_path": "models/step-000001.ckpt", "byte_size": 42}],
      "checkpoint_count": 1,
      "known_limits": ["fixture"]
    }));
    let spatial_query = decode_fixture(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "training_result_semantic_manifest_path": ".tmp/mc10-smoke-review/semantic/minecraft-3dgs-training-result-semantic.json",
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
    }));
    let holdout_preview = decode_fixture(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "training_result_semantic_manifest_path": ".tmp/mc10-smoke-review/semantic/minecraft-3dgs-training-result-semantic.json",
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
      "basis_checkpoint_path": "models/step-000001.ckpt",
      "holdout_screenshot_path": "holdout.png",
      "reference_overlay_path": "overlay.png",
      "status": "ready",
      "known_limits": ["fixture"]
    }));
    let render_quality = decode_fixture(json!({
      "schema_version": 1,
      "generated_at_millis": 20,
      "training_result_semantic_manifest_path": ".tmp/mc10-smoke-review/semantic/minecraft-3dgs-training-result-semantic.json",
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
      "basis_checkpoint_path": "models/step-000001.ckpt",
      "holdout_screenshot_path": "holdout.png",
      "rendered_image_path": "rendered.png",
      "render_backend": "external_command",
      "image_size_match": true,
      "metrics": {"l1_mean": 0.0, "mse": 0.0, "psnr": 100.0},
      "status": "ready",
      "verdict": "measured_only",
      "known_limits": ["fixture"]
    }));

    publish_minecraft_scene_packet(Some(&root), &scene_packet).await.expect("publish scene packet");
    publish_minecraft_training_package(Some(&root), &training_package).await.expect("publish training package");
    publish_minecraft_training_job(Some(&root), &training_job).await.expect("publish training job");
    publish_minecraft_training_result(Some(&root), &training_result).await.expect("publish training result");
    publish_minecraft_training_semantic(Some(&root), &semantic).await.expect("publish semantic");
    let spatial_query_metadata = publish_minecraft_training_spatial_query(Some(&root), &spatial_query)
      .await
      .expect("publish spatial query")
      .expect("canonical spatial query publication enabled");
    publish_minecraft_training_holdout_preview(Some(&root), &holdout_preview).await.expect("publish holdout preview");
    publish_minecraft_training_holdout_render_quality(Some(&root), &render_quality).await.expect("publish render quality");
    dispatch.flush().await.expect("flush canonical artifacts");
    let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("snapshot");

    let document = build_product_inspect_document(store.as_ref(), &snapshot).await.expect("product inspect document");
    let text = document.render_text();
    assert!(text.contains("MC-6 Spatial Bundles:"));
    assert!(text.contains("schema=1 source_runs=1 screenshots=1 spatial_frames=1 missing_screenshots=0"));
    assert!(text.contains("source_scene_packet=scene.json source_runs=1 frames=1 images=1"));
    assert!(text.contains("MC-7 Training Launches:"));
    assert!(text.contains("source_training_package=package.json source_scene_packet=scene.json source_runs=1"));
    assert!(text.contains("launch_command=train"));
    assert!(text.contains("source_training_launch_plan=launch.json source_runs=1 frames=1 images=1"));
    assert!(text.contains("submission_recorded_at_millis=21 job_id=job-20 job_url=fixture://jobs/job-20"));
    assert!(text.contains("source_training_job=job.json source_training_launch_plan=launch.json source_runs=1"));
    assert!(text.contains("source_job_status=submitted status=succeeded status_message=training complete"));
    assert!(text.contains("MC-7 Training Result Artifacts:"));
    assert!(text.contains("result_dir=result result_artifacts=1 readable=1 recorded_bytes=42"));
    assert!(text.contains("source_training_result_artifact=result-artifacts.json source_training_result=result.json source_runs=1"));
    assert!(text.contains("semantic_status=ready semantic_reason=n/a normalized_result_dir=normalized"));
    assert!(text.contains(
      "config_path=config.yml models_dir_path=models status_snapshot_path=job_status.json config_trainer=nerfstudio checkpoints=1"
    ));
    assert!(text.contains("training_result_semantic_manifest=.tmp/mc10-smoke-review/semantic/minecraft-3dgs-training-result-semantic.json"));
    assert!(text.contains("checkpoint=present screenshot=present reference_overlay=present spatial_frame=n/a"));
    assert!(text.contains("holdout_preview_manifest=preview.json"));
    assert!(text.contains("checkpoint=present screenshot=present rendered_image=present"));
    assert!(text.contains("MC-17 Quality Verdict:"));
    assert!(text.contains("quality_verdict=pass"));
    assert!(text.contains("query_kind=block_projection"));
    assert!(text.contains("target_face=north target_semantics=hit_face_center"));
    assert!(text.contains("match_radius_px=8 confidence=1 backend=projection_reference comparison=match basis_frame=frame-20"));
    assert!(text.contains("MC-14 Training Result Spatial Query Action Readiness:"));
    assert!(text.contains("target_block=511,73,728 status=answered visibility=visible selected_backend=projection_reference"));
    assert!(text.contains("action_eligibility=click_ready"));
    assert!(text.contains("window_point=12,34"));
    assert!(text.contains("MC-19 Query Wired Live Action:"));
    assert!(text.contains(&format!("query_artifact={}", spatial_query_metadata.uri())));
    assert!(text.contains("operation_evidence=not_recorded"));
    assert!(text.contains("operation_status=n/a operation_message=n/a"));
    assert!(text.contains("mc14_action_eligibility=click_ready readiness_class=ready window_point=12,34"));
    assert!(text.contains(&format!("source_readiness=ready source_query_artifact={}", spatial_query_metadata.uri())));
    assert!(!text.contains("operation_result_artifact="));
    assert!(!text.contains("source_readiness_ref="));
    assert!(!text.contains("manifest_artifact="));
    assert!(!text.contains("paired_report_artifact="));
    assert!(!text.contains(" role="));
    assert!(!text.contains(" issue="));
  }

  fn decode_fixture<T: DeserializeOwned>(value: Value) -> T {
    serde_json::from_value(value).expect("typed Minecraft fixture")
  }
}
