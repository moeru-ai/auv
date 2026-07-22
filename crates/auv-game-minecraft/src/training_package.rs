use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use auv_tracing::{ArtifactMetadata, ArtifactUri, Context, RunSnapshot, RunStore};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::scene_packet::{ScenePacketCameraRecord, ScenePacketFramePayload, ScenePacketFrameRecord, ScenePacketManifest};
use crate::types::{MinecraftSpatialFrame, Viewport};

pub type TrainingPackageResult<T> = Result<T, String>;

pub const TRAINING_PACKAGE_SCHEMA_VERSION: u32 = 1;
pub const TRAINING_PACKAGE_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
pub const MINECRAFT_TRAINING_PACKAGE_PURPOSE: &str = "auv.minecraft.training.package";

pub async fn publish_minecraft_training_package(
  context: Option<&Context>,
  package: &TrainingPackageManifest,
) -> Result<Option<ArtifactMetadata>, crate::run_read::MinecraftArtifactPublishError> {
  crate::run_read::publish_json_artifact(context, MINECRAFT_TRAINING_PACKAGE_PURPOSE, package, |_| Ok(())).await
}

pub async fn read_minecraft_training_package(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<TrainingPackageManifest, crate::run_read::MinecraftArtifactReadError> {
  crate::run_read::read_json_artifact(store, snapshot, uri, MINECRAFT_TRAINING_PACKAGE_PURPOSE, |_| Ok(())).await
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingPackageInputs {
  pub scene_packet_manifest_path: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingPackageOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub cameras_path: PathBuf,
  pub known_limits_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub compatibility_export_report_path: PathBuf,
  pub compatibility_transforms_path: Option<PathBuf>,
  pub manifest: TrainingPackageManifest,
  pub inspect_report: TrainingPackageInspectReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingPackageManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: TrainingPackageCounts,
  pub frames: Vec<TrainingPackageFrameRecord>,
  pub compatibility_views: Vec<TrainingCompatibilityViewReport>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingPackageCounts {
  pub frames: usize,
  pub images: usize,
  pub compatibility_exported_frames: usize,
  pub compatibility_skipped_frames: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingPackageFrameRecord {
  pub frame_index: usize,
  pub spatial_frame_id: String,
  pub source_run_id: String,
  pub source_bundle_manifest_path: String,
  pub source_scene_packet_frame_json_path: String,
  pub canonical_frame_json_path: String,
  #[serde(default)]
  pub canonical_image_path: Option<String>,
  #[serde(default)]
  pub screen_state: Option<String>,
  #[serde(default)]
  pub resource_pack_ids: Vec<String>,
  #[serde(default)]
  pub primary_file_resource_pack_id: Option<String>,
  pub compatibility_status: TrainingCompatibilityStatus,
  #[serde(default)]
  pub compatibility_skip_reasons: Vec<TrainingCompatibilitySkipReason>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingPackageInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub training_package_manifest_path: String,
  pub scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: TrainingPackageCounts,
  pub compatibility_views: Vec<TrainingCompatibilityViewReport>,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingCompatibilityViewReport {
  pub view_name: String,
  pub status: TrainingCompatibilityStatus,
  pub exported_frame_count: usize,
  pub skipped_frame_count: usize,
  #[serde(default)]
  pub transforms_path: Option<String>,
  pub export_report_path: String,
  pub exported_frame_indices: Vec<usize>,
  pub frame_decisions: Vec<TrainingCompatibilityFrameDecision>,
  pub skip_reason_counts: Vec<TrainingCompatibilitySkipReasonCount>,
  pub warnings: Vec<String>,
  #[serde(default)]
  pub used_legacy_view_translation_fallback_frame_indices: Vec<usize>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingCompatibilityFrameDecision {
  pub frame_index: usize,
  pub spatial_frame_id: String,
  pub source_run_id: String,
  pub status: TrainingCompatibilityStatus,
  #[serde(default)]
  pub skip_reasons: Vec<TrainingCompatibilitySkipReason>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingCompatibilitySkipReasonCount {
  pub reason: TrainingCompatibilitySkipReason,
  pub count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingCompatibilityStatus {
  Ready,
  Partial,
  Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingCompatibilitySkipReason {
  MissingScreenshot,
  NonIngameScreenState,
  NoFileResourcePack,
  MultipleFileResourcePacks,
  InvalidViewMatrix,
  InvalidProjectionMatrix,
  NoninvertibleCameraTransform,
  InvalidIntrinsics,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct NerfstudioTransforms {
  camera_model: &'static str,
  w: u32,
  h: u32,
  fl_x: f64,
  fl_y: f64,
  cx: f64,
  cy: f64,
  k1: f64,
  k2: f64,
  p1: f64,
  p2: f64,
  frames: Vec<NerfstudioFrame>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
struct NerfstudioFrame {
  file_path: String,
  transform_matrix: [[f64; 4]; 4],
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct CompatibilityIntrinsics {
  width: u32,
  height: u32,
  fl_x: f64,
  fl_y: f64,
  cx: f64,
  cy: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct CompatibilityEvaluation {
  status: TrainingCompatibilityStatus,
  skip_reasons: Vec<TrainingCompatibilitySkipReason>,
  camera_to_world: Option<[f64; 16]>,
  intrinsics: Option<CompatibilityIntrinsics>,
  used_legacy_view_translation_fallback: bool,
}

pub fn export_3dgs_training_package(inputs: TrainingPackageInputs) -> TrainingPackageResult<TrainingPackageOutput> {
  let scene_packet_manifest = read_json_file::<ScenePacketManifest>(&inputs.scene_packet_manifest_path, "MC-7 D2 scene packet manifest")?;
  let scene_packet_dir = inputs
    .scene_packet_manifest_path
    .parent()
    .ok_or_else(|| format!("MC-7 D2 scene packet manifest {} has no parent directory", inputs.scene_packet_manifest_path.display()))?;

  let source_cameras_path = scene_packet_dir.join("cameras.json");
  let source_known_limits_path = scene_packet_dir.join("known_limits.json");
  let cameras = read_json_file::<Vec<ScenePacketCameraRecord>>(&source_cameras_path, "MC-7 D2 scene packet cameras JSON")?;
  let source_known_limits = read_json_file::<Vec<String>>(&source_known_limits_path, "MC-7 D2 scene packet known limits JSON")?;
  let camera_records = index_camera_records(&scene_packet_manifest, cameras)?;

  let mut warnings = BTreeSet::new();
  let mut known_limits = BTreeSet::new();
  known_limits.extend(source_known_limits);
  known_limits.insert("MC-7 training package is training-prep only; no trainer/backend or trained splat is included".to_string());

  let mut frame_records = Vec::new();
  let mut frame_decisions = Vec::new();
  let mut exported_frame_indices = Vec::new();
  let mut nerfstudio_frames = Vec::new();
  let mut compatibility_baseline = None::<CompatibilityIntrinsics>;
  let mut legacy_fallback_frame_indices = Vec::new();
  let mut frame_indices = BTreeSet::new();
  let mut image_count = 0usize;

  for frame_record in &scene_packet_manifest.frames {
    if !frame_indices.insert(frame_record.frame_index) {
      return Err(format!(
        "duplicate scene packet frame_index {} in {}",
        frame_record.frame_index,
        inputs.scene_packet_manifest_path.display()
      ));
    }

    let source_frame_json_path = scene_packet_dir.join(&frame_record.frame_json_path);
    let mut frame_payload = read_json_file::<ScenePacketFramePayload>(&source_frame_json_path, "MC-7 D2 scene packet frame JSON")?;
    validate_frame_payload(frame_record, &frame_payload, &source_frame_json_path)?;

    let frame_json_relative_path = format!("frames/frame_{:06}.json", frame_record.frame_index);
    let canonical_image_path = if let Some(source_screenshot_relative_path) = frame_record.screenshot_path.as_deref() {
      let extension = extension_for(source_screenshot_relative_path);
      let relative_path = format!("images/frame_{:06}.{extension}", frame_record.frame_index);
      copy_file(
        &scene_packet_dir.join(source_screenshot_relative_path),
        &inputs.output_dir.join(&relative_path),
        "MC-7 D3 canonical screenshot",
      )?;
      image_count += 1;
      Some(relative_path)
    } else {
      None
    };

    frame_payload.screenshot_path = canonical_image_path.clone();
    write_json(&inputs.output_dir.join(&frame_json_relative_path), &frame_payload, "MC-7 D3 canonical frame JSON")?;

    let primary_file_resource_pack_id = primary_file_resource_pack_id(&frame_record.resource_pack_ids);
    let camera_record = camera_records
      .get(&frame_record.frame_index)
      .ok_or_else(|| format!("missing camera record for frame {}", frame_record.frame_index))?;
    let evaluation = evaluate_compatibility(
      frame_record,
      &frame_payload.spatial_frame,
      camera_record,
      canonical_image_path.as_deref(),
      &mut compatibility_baseline,
      &mut warnings,
    )?;

    if evaluation.used_legacy_view_translation_fallback {
      legacy_fallback_frame_indices.push(frame_record.frame_index);
    }
    if evaluation.status == TrainingCompatibilityStatus::Ready {
      let source_canonical_image = inputs
        .output_dir
        .join(canonical_image_path.as_deref().ok_or_else(|| format!("missing canonical image for frame {}", frame_record.frame_index))?);
      let compatibility_image_relative_path = format!(
        "compat/nerfstudio/images/frame_{:06}.{}",
        frame_record.frame_index,
        extension_for(canonical_image_path.as_deref().expect("ready compatibility frame must have canonical image"),)
      );
      copy_file(
        &source_canonical_image,
        &inputs.output_dir.join(&compatibility_image_relative_path),
        "MC-7 D3 Nerfstudio compatibility screenshot",
      )?;
      exported_frame_indices.push(frame_record.frame_index);
      nerfstudio_frames.push(NerfstudioFrame {
        file_path: format!("images/frame_{:06}.{}", frame_record.frame_index, extension_for(&compatibility_image_relative_path)),
        transform_matrix: matrix_rows(&evaluation.camera_to_world.expect("ready compatibility frame must include camera transform")),
      });
    }

    frame_records.push(TrainingPackageFrameRecord {
      frame_index: frame_record.frame_index,
      spatial_frame_id: frame_record.spatial_frame_id.clone(),
      source_run_id: frame_record.source_run_id.clone(),
      source_bundle_manifest_path: frame_record.source_bundle_manifest_path.clone(),
      source_scene_packet_frame_json_path: frame_record.frame_json_path.clone(),
      canonical_frame_json_path: frame_json_relative_path,
      canonical_image_path,
      screen_state: frame_record.screen_state.clone(),
      resource_pack_ids: frame_record.resource_pack_ids.clone(),
      primary_file_resource_pack_id,
      compatibility_status: evaluation.status,
      compatibility_skip_reasons: evaluation.skip_reasons.clone(),
    });
    frame_decisions.push(TrainingCompatibilityFrameDecision {
      frame_index: frame_record.frame_index,
      spatial_frame_id: frame_record.spatial_frame_id.clone(),
      source_run_id: frame_record.source_run_id.clone(),
      status: evaluation.status,
      skip_reasons: evaluation.skip_reasons,
    });
  }

  let compatibility_status = match nerfstudio_frames.len() {
    0 => TrainingCompatibilityStatus::Blocked,
    count if count == frame_records.len() => TrainingCompatibilityStatus::Ready,
    _ => TrainingCompatibilityStatus::Partial,
  };
  if !legacy_fallback_frame_indices.is_empty() {
    known_limits.insert(format!(
      "MC-7 D3 Nerfstudio compatibility reused the legacy rotation-only view_matrix fallback on frame indices {}",
      legacy_fallback_frame_indices.iter().map(|index| index.to_string()).collect::<Vec<_>>().join(",")
    ));
  }
  let known_limits = known_limits.into_iter().collect::<Vec<_>>();
  let warnings = warnings.into_iter().collect::<Vec<_>>();

  let counts = TrainingPackageCounts {
    frames: frame_records.len(),
    images: image_count,
    compatibility_exported_frames: nerfstudio_frames.len(),
    compatibility_skipped_frames: frame_records.len().saturating_sub(nerfstudio_frames.len()),
  };

  let compatibility_export_report_relative_path = "compat/nerfstudio/export_report.json".to_string();
  let compatibility_transforms_relative_path =
    (compatibility_status != TrainingCompatibilityStatus::Blocked).then_some("compat/nerfstudio/transforms.json".to_string());
  let compatibility_known_limits = if legacy_fallback_frame_indices.is_empty() {
    Vec::new()
  } else {
    known_limits.iter().filter(|value| value.contains("legacy rotation-only view_matrix fallback")).cloned().collect::<Vec<_>>()
  };
  let compatibility_report = TrainingCompatibilityViewReport {
    view_name: "nerfstudio".to_string(),
    status: compatibility_status,
    exported_frame_count: counts.compatibility_exported_frames,
    skipped_frame_count: counts.compatibility_skipped_frames,
    transforms_path: compatibility_transforms_relative_path.clone(),
    export_report_path: compatibility_export_report_relative_path.clone(),
    exported_frame_indices,
    frame_decisions: frame_decisions.clone(),
    skip_reason_counts: summarize_skip_reason_counts(&frame_decisions),
    warnings: warnings.clone(),
    used_legacy_view_translation_fallback_frame_indices: legacy_fallback_frame_indices.clone(),
    known_limits: compatibility_known_limits,
  };

  let generated_at_millis = crate::run_read::now_millis();
  let manifest_path = inputs.output_dir.join("run.json");
  let cameras_path = inputs.output_dir.join("cameras.json");
  let known_limits_path = inputs.output_dir.join("known_limits.json");
  let inspect_report_path = inputs.output_dir.join("inspect_report.json");
  let compatibility_export_report_path = inputs.output_dir.join(&compatibility_export_report_relative_path);
  let compatibility_transforms_path = compatibility_transforms_relative_path.as_ref().map(|path| inputs.output_dir.join(path));

  let manifest = TrainingPackageManifest {
    schema_version: TRAINING_PACKAGE_SCHEMA_VERSION,
    generated_at_millis,
    source_scene_packet_manifest_path: inputs.scene_packet_manifest_path.to_string_lossy().into_owned(),
    source_bundle_manifest_paths: scene_packet_manifest.source_bundle_manifest_paths.clone(),
    source_run_ids: scene_packet_manifest.source_run_ids.clone(),
    counts: counts.clone(),
    frames: frame_records,
    compatibility_views: vec![compatibility_report.clone()],
    known_limits: known_limits.clone(),
  };
  write_json(&manifest_path, &manifest, "MC-7 D3 training package manifest JSON")?;
  write_json(&cameras_path, &camera_records.values().cloned().collect::<Vec<ScenePacketCameraRecord>>(), "MC-7 D3 cameras JSON")?;
  write_json(&known_limits_path, &known_limits, "MC-7 D3 known limits JSON")?;
  write_json(&compatibility_export_report_path, &compatibility_report, "MC-7 D3 Nerfstudio compatibility export report JSON")?;
  if let (Some(transforms_path), Some(intrinsics)) = (compatibility_transforms_path.as_ref(), compatibility_baseline) {
    write_json(
      transforms_path,
      &NerfstudioTransforms {
        camera_model: "OPENCV",
        w: intrinsics.width,
        h: intrinsics.height,
        fl_x: intrinsics.fl_x,
        fl_y: intrinsics.fl_y,
        cx: intrinsics.cx,
        cy: intrinsics.cy,
        k1: 0.0,
        k2: 0.0,
        p1: 0.0,
        p2: 0.0,
        frames: nerfstudio_frames,
      },
      "MC-7 D3 Nerfstudio transforms JSON",
    )?;
  }

  let inspect_report = TrainingPackageInspectReport {
    schema_version: TRAINING_PACKAGE_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    training_package_manifest_path: manifest_path.to_string_lossy().into_owned(),
    scene_packet_manifest_path: inputs.scene_packet_manifest_path.to_string_lossy().into_owned(),
    source_bundle_manifest_paths: scene_packet_manifest.source_bundle_manifest_paths,
    source_run_ids: scene_packet_manifest.source_run_ids,
    counts,
    compatibility_views: vec![compatibility_report],
    warnings,
    known_limits,
  };
  write_json(&inspect_report_path, &inspect_report, "MC-7 D3 inspect report JSON")?;

  Ok(TrainingPackageOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    cameras_path,
    known_limits_path,
    inspect_report_path,
    compatibility_export_report_path,
    compatibility_transforms_path,
    manifest,
    inspect_report,
  })
}

fn index_camera_records(
  manifest: &ScenePacketManifest,
  cameras: Vec<ScenePacketCameraRecord>,
) -> TrainingPackageResult<BTreeMap<usize, ScenePacketCameraRecord>> {
  let mut index = BTreeMap::new();
  for record in cameras {
    let frame_index = record.frame_index;
    if index.insert(frame_index, record).is_some() {
      return Err(format!("duplicate camera record for frame_index {} in scene packet cameras JSON", frame_index));
    }
  }
  if index.len() != manifest.frames.len() {
    return Err(format!(
      "scene packet cameras JSON contained {} record(s), but manifest declared {} frame(s)",
      index.len(),
      manifest.frames.len()
    ));
  }
  for frame in &manifest.frames {
    if !index.contains_key(&frame.frame_index) {
      return Err(format!("scene packet cameras JSON missing frame_index {}", frame.frame_index));
    }
  }
  Ok(index)
}

fn validate_frame_payload(
  frame_record: &ScenePacketFrameRecord,
  frame_payload: &ScenePacketFramePayload,
  path: &Path,
) -> TrainingPackageResult<()> {
  if frame_payload.frame_index != frame_record.frame_index {
    return Err(format!(
      "scene packet frame JSON {} declared frame_index {}, but manifest expected {}",
      path.display(),
      frame_payload.frame_index,
      frame_record.frame_index
    ));
  }
  if frame_payload.spatial_frame.spatial_frame_id != frame_record.spatial_frame_id {
    return Err(format!(
      "scene packet frame JSON {} declared spatial_frame_id {}, but manifest expected {}",
      path.display(),
      frame_payload.spatial_frame.spatial_frame_id,
      frame_record.spatial_frame_id
    ));
  }
  Ok(())
}

fn primary_file_resource_pack_id(resource_pack_ids: &[String]) -> Option<String> {
  let file_resource_packs =
    resource_pack_ids.iter().filter(|resource_pack_id| resource_pack_id.starts_with("file/")).cloned().collect::<Vec<_>>();
  if file_resource_packs.len() == 1 {
    Some(file_resource_packs[0].clone())
  } else {
    None
  }
}

fn evaluate_compatibility(
  frame_record: &ScenePacketFrameRecord,
  spatial_frame: &MinecraftSpatialFrame,
  camera_record: &ScenePacketCameraRecord,
  canonical_image_path: Option<&str>,
  compatibility_baseline: &mut Option<CompatibilityIntrinsics>,
  warnings: &mut BTreeSet<String>,
) -> TrainingPackageResult<CompatibilityEvaluation> {
  if camera_record.frame_index != frame_record.frame_index {
    return Err(format!("camera record frame_index {} did not match frame record {}", camera_record.frame_index, frame_record.frame_index));
  }
  if camera_record.spatial_frame_id != frame_record.spatial_frame_id {
    return Err(format!(
      "camera record spatial_frame_id {} did not match frame record {}",
      camera_record.spatial_frame_id, frame_record.spatial_frame_id
    ));
  }

  let mut skip_reasons = Vec::new();
  if canonical_image_path.is_none() {
    skip_reasons.push(TrainingCompatibilitySkipReason::MissingScreenshot);
  }
  if spatial_frame.screen_state.as_deref() != Some("in_game") {
    skip_reasons.push(TrainingCompatibilitySkipReason::NonIngameScreenState);
  }
  let file_resource_packs =
    spatial_frame.resource_pack_ids.iter().filter(|resource_pack_id| resource_pack_id.starts_with("file/")).cloned().collect::<Vec<_>>();
  match file_resource_packs.len() {
    0 => skip_reasons.push(TrainingCompatibilitySkipReason::NoFileResourcePack),
    1 => {}
    _ => skip_reasons.push(TrainingCompatibilitySkipReason::MultipleFileResourcePacks),
  }
  if !all_finite(&spatial_frame.view_matrix) {
    skip_reasons.push(TrainingCompatibilitySkipReason::InvalidViewMatrix);
  }
  if !all_finite(&spatial_frame.projection_matrix) {
    skip_reasons.push(TrainingCompatibilitySkipReason::InvalidProjectionMatrix);
  }

  let mut camera_to_world = None;
  let mut intrinsics = None;
  let mut used_legacy_view_translation_fallback = false;
  if skip_reasons.is_empty() {
    let (world_to_camera, used_fallback) = match effective_world_to_camera_matrix(spatial_frame) {
      Some(value) => value,
      None => {
        skip_reasons.push(TrainingCompatibilitySkipReason::NoninvertibleCameraTransform);
        (
          [
            0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
          ],
          false,
        )
      }
    };
    used_legacy_view_translation_fallback = used_fallback;
    if skip_reasons.is_empty() {
      camera_to_world = invert_affine_matrix(&world_to_camera);
      if camera_to_world.is_none() {
        skip_reasons.push(TrainingCompatibilitySkipReason::NoninvertibleCameraTransform);
      }
    }
    if skip_reasons.is_empty() {
      intrinsics = intrinsics_from_projection(spatial_frame.viewport, &spatial_frame.projection_matrix);
      if intrinsics.is_none() {
        skip_reasons.push(TrainingCompatibilitySkipReason::InvalidIntrinsics);
      }
    }
  }
  if skip_reasons.is_empty() {
    let current_intrinsics = intrinsics.expect("validated intrinsics");
    if let Some(baseline) = compatibility_baseline {
      if !intrinsics_match(*baseline, current_intrinsics) {
        warnings.insert(format!(
          "frame {} had incompatible intrinsics for the fixed Nerfstudio transforms.json top-level camera fields and was skipped",
          frame_record.frame_index
        ));
        skip_reasons.push(TrainingCompatibilitySkipReason::InvalidIntrinsics);
        intrinsics = None;
        camera_to_world = None;
      }
    } else {
      *compatibility_baseline = Some(current_intrinsics);
    }
  }

  let status = if skip_reasons.is_empty() {
    TrainingCompatibilityStatus::Ready
  } else {
    TrainingCompatibilityStatus::Blocked
  };
  if !skip_reasons.is_empty() {
    for reason in &skip_reasons {
      warnings.insert(format!("frame {} compatibility skipped because of {}", frame_record.frame_index, skip_reason_label(*reason)));
    }
  }
  Ok(CompatibilityEvaluation {
    status,
    skip_reasons,
    camera_to_world,
    intrinsics,
    used_legacy_view_translation_fallback,
  })
}

fn effective_world_to_camera_matrix(spatial_frame: &MinecraftSpatialFrame) -> Option<([f64; 16], bool)> {
  let mut matrix = spatial_frame.view_matrix;
  let used_legacy_view_translation_fallback = uses_rotation_only_view_matrix(spatial_frame);
  if used_legacy_view_translation_fallback {
    let eye = spatial_frame.player_pose.eye_position;
    let translated = multiply_mat3_vec3(&matrix, [eye.x, eye.y, eye.z]);
    matrix[12] = -translated[0];
    matrix[13] = -translated[1];
    matrix[14] = -translated[2];
  }
  is_affine_matrix(&matrix).then_some((matrix, used_legacy_view_translation_fallback))
}

fn uses_rotation_only_view_matrix(spatial_frame: &MinecraftSpatialFrame) -> bool {
  const EPSILON: f64 = 1e-6;

  spatial_frame.view_matrix[12].abs() <= EPSILON
    && spatial_frame.view_matrix[13].abs() <= EPSILON
    && spatial_frame.view_matrix[14].abs() <= EPSILON
    && (spatial_frame.player_pose.eye_position.x.abs() > EPSILON
      || spatial_frame.player_pose.eye_position.y.abs() > EPSILON
      || spatial_frame.player_pose.eye_position.z.abs() > EPSILON)
}

fn intrinsics_from_projection(viewport: Viewport, projection_matrix: &[f64; 16]) -> Option<CompatibilityIntrinsics> {
  let width = viewport.width;
  let height = viewport.height;
  if width == 0 || height == 0 {
    return None;
  }
  let width_f64 = f64::from(width);
  let height_f64 = f64::from(height);
  let fl_x = projection_matrix[0] * width_f64 / 2.0;
  let fl_y = projection_matrix[5] * height_f64 / 2.0;
  let cx = width_f64 / 2.0;
  let cy = height_f64 / 2.0;
  [fl_x, fl_y, cx, cy].iter().all(|value| value.is_finite() && *value > 0.0).then_some(CompatibilityIntrinsics {
    width,
    height,
    fl_x,
    fl_y,
    cx,
    cy,
  })
}

fn intrinsics_match(left: CompatibilityIntrinsics, right: CompatibilityIntrinsics) -> bool {
  const EPSILON: f64 = 1e-6;

  left.width == right.width
    && left.height == right.height
    && (left.fl_x - right.fl_x).abs() <= EPSILON
    && (left.fl_y - right.fl_y).abs() <= EPSILON
    && (left.cx - right.cx).abs() <= EPSILON
    && (left.cy - right.cy).abs() <= EPSILON
}

fn invert_affine_matrix(matrix: &[f64; 16]) -> Option<[f64; 16]> {
  let inverse_3x3 = invert_3x3(matrix)?;
  let translation = [matrix[12], matrix[13], matrix[14]];
  let inverse_translation = multiply_3x3_vector(&inverse_3x3, translation).map(|value| -value);

  Some([
    inverse_3x3[0],
    inverse_3x3[1],
    inverse_3x3[2],
    0.0,
    inverse_3x3[3],
    inverse_3x3[4],
    inverse_3x3[5],
    0.0,
    inverse_3x3[6],
    inverse_3x3[7],
    inverse_3x3[8],
    0.0,
    inverse_translation[0],
    inverse_translation[1],
    inverse_translation[2],
    1.0,
  ])
}

fn invert_3x3(matrix: &[f64; 16]) -> Option<[f64; 9]> {
  let a00 = matrix[0];
  let a01 = matrix[4];
  let a02 = matrix[8];
  let a10 = matrix[1];
  let a11 = matrix[5];
  let a12 = matrix[9];
  let a20 = matrix[2];
  let a21 = matrix[6];
  let a22 = matrix[10];

  let c00 = a11 * a22 - a12 * a21;
  let c01 = -(a10 * a22 - a12 * a20);
  let c02 = a10 * a21 - a11 * a20;
  let c10 = -(a01 * a22 - a02 * a21);
  let c11 = a00 * a22 - a02 * a20;
  let c12 = -(a00 * a21 - a01 * a20);
  let c20 = a01 * a12 - a02 * a11;
  let c21 = -(a00 * a12 - a02 * a10);
  let c22 = a00 * a11 - a01 * a10;

  let determinant = a00 * c00 + a01 * c01 + a02 * c02;
  if !determinant.is_finite() || determinant.abs() <= 1e-9 {
    return None;
  }

  let inverse_determinant = 1.0 / determinant;
  Some([
    c00 * inverse_determinant,
    c01 * inverse_determinant,
    c02 * inverse_determinant,
    c10 * inverse_determinant,
    c11 * inverse_determinant,
    c12 * inverse_determinant,
    c20 * inverse_determinant,
    c21 * inverse_determinant,
    c22 * inverse_determinant,
  ])
}

fn multiply_3x3_vector(matrix: &[f64; 9], vector: [f64; 3]) -> [f64; 3] {
  [
    matrix[0] * vector[0] + matrix[3] * vector[1] + matrix[6] * vector[2],
    matrix[1] * vector[0] + matrix[4] * vector[1] + matrix[7] * vector[2],
    matrix[2] * vector[0] + matrix[5] * vector[1] + matrix[8] * vector[2],
  ]
}

fn multiply_mat3_vec3(matrix: &[f64; 16], vector: [f64; 3]) -> [f64; 3] {
  [
    matrix[0] * vector[0] + matrix[4] * vector[1] + matrix[8] * vector[2],
    matrix[1] * vector[0] + matrix[5] * vector[1] + matrix[9] * vector[2],
    matrix[2] * vector[0] + matrix[6] * vector[1] + matrix[10] * vector[2],
  ]
}

fn matrix_rows(matrix: &[f64; 16]) -> [[f64; 4]; 4] {
  [
    [matrix[0], matrix[4], matrix[8], matrix[12]],
    [matrix[1], matrix[5], matrix[9], matrix[13]],
    [matrix[2], matrix[6], matrix[10], matrix[14]],
    [matrix[3], matrix[7], matrix[11], matrix[15]],
  ]
}

fn summarize_skip_reason_counts(frame_decisions: &[TrainingCompatibilityFrameDecision]) -> Vec<TrainingCompatibilitySkipReasonCount> {
  let mut counts = BTreeMap::<TrainingCompatibilitySkipReason, usize>::new();
  for decision in frame_decisions {
    for reason in &decision.skip_reasons {
      *counts.entry(*reason).or_default() += 1;
    }
  }
  counts.into_iter().map(|(reason, count)| TrainingCompatibilitySkipReasonCount { reason, count }).collect()
}

fn all_finite(values: &[f64; 16]) -> bool {
  values.iter().all(|value| value.is_finite())
}

fn is_affine_matrix(matrix: &[f64; 16]) -> bool {
  const EPSILON: f64 = 1e-6;

  matrix[3].abs() <= EPSILON && matrix[7].abs() <= EPSILON && matrix[11].abs() <= EPSILON && (matrix[15] - 1.0).abs() <= EPSILON
}

fn skip_reason_label(reason: TrainingCompatibilitySkipReason) -> &'static str {
  match reason {
    TrainingCompatibilitySkipReason::MissingScreenshot => "missing_screenshot",
    TrainingCompatibilitySkipReason::NonIngameScreenState => "non_ingame_screen_state",
    TrainingCompatibilitySkipReason::NoFileResourcePack => "no_file_resource_pack",
    TrainingCompatibilitySkipReason::MultipleFileResourcePacks => "multiple_file_resource_packs",
    TrainingCompatibilitySkipReason::InvalidViewMatrix => "invalid_view_matrix",
    TrainingCompatibilitySkipReason::InvalidProjectionMatrix => "invalid_projection_matrix",
    TrainingCompatibilitySkipReason::NoninvertibleCameraTransform => "noninvertible_camera_transform",
    TrainingCompatibilitySkipReason::InvalidIntrinsics => "invalid_intrinsics",
  }
}

fn extension_for(path: &str) -> String {
  Path::new(path)
    .extension()
    .and_then(|extension| extension.to_str())
    .filter(|extension| !extension.trim().is_empty())
    .unwrap_or("png")
    .to_string()
}

fn copy_file(source: &Path, destination: &Path, label: &str) -> TrainingPackageResult<()> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent).map_err(|error| format!("failed to create {label} directory {}: {error}", parent.display()))?;
  }
  fs::copy(source, destination)
    .map_err(|error| format!("failed to copy {label} from {} to {}: {error}", source.display(), destination.display()))?;
  Ok(())
}

fn write_json(path: &Path, value: &impl Serialize, label: &str) -> TrainingPackageResult<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|error| format!("failed to create {label} directory {}: {error}", parent.display()))?;
  }
  let json = serde_json::to_string_pretty(value)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize {label}: {error}"))?;
  fs::write(path, json.as_bytes()).map_err(|error| format!("failed to write {label} {}: {error}", path.display()))
}

fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> TrainingPackageResult<T> {
  let file = fs::File::open(path).map_err(|error| format!("failed to open {label} {}: {error}", path.display()))?;
  serde_json::from_reader(BufReader::new(file)).map_err(|error| format!("failed to parse {label} {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
  use super::*;

  use image::{Rgba, RgbaImage};
  use tempfile::TempDir;

  use crate::scene_packet::{ScenePacketCounts, ScenePacketFramePayload};
  use crate::types::{BlockFace, BlockPosition, PlayerPose, RaycastHit, Vec3, Viewport};

  struct SyntheticFrameSpec {
    frame_index: usize,
    source_run_id: String,
    source_bundle_manifest_path: String,
    spatial_frame_id: String,
    monotonic_timestamp_ms: u64,
    screen_state: Option<String>,
    resource_pack_ids: Vec<String>,
    screenshot: ScreenshotDisposition,
    view_matrix: [f64; 16],
    projection_matrix: [f64; 16],
    viewport: Viewport,
    player_eye: Vec3,
  }

  enum ScreenshotDisposition {
    Present,
    MissingPath,
    MissingFile,
  }

  #[test]
  fn exports_happy_path_training_package_and_nerfstudio_view() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scene_packet_manifest_path = write_scene_packet_fixture(
      &temp,
      vec![
        synthetic_frame(1, "run_1", "file/auv-mc6-rich"),
        synthetic_frame(2, "run_2", "file/auv-mc6-rich"),
        synthetic_frame(3, "run_3", "file/auv-mc6-flat"),
        synthetic_frame(4, "run_4", "file/auv-mc6-flat"),
        synthetic_frame(5, "run_5", "file/auv-mc6-repetitive"),
        synthetic_frame(6, "run_6", "file/auv-mc6-repetitive"),
      ],
    );

    let output = export_3dgs_training_package(TrainingPackageInputs {
      scene_packet_manifest_path,
      output_dir: temp.path().join("training-package"),
    })
    .expect("training package export should succeed");

    assert_eq!(output.manifest.counts.frames, 6);
    assert_eq!(output.manifest.counts.images, 6);
    assert_eq!(output.manifest.counts.compatibility_exported_frames, 6);
    assert_eq!(output.inspect_report.compatibility_views[0].status, TrainingCompatibilityStatus::Ready);
    assert!(output.output_dir.join("run.json").is_file());
    assert!(output.output_dir.join("frames/frame_000001.json").is_file());
    assert!(output.output_dir.join("images/frame_000001.png").is_file());
    assert!(output.output_dir.join("cameras.json").is_file());
    assert!(output.output_dir.join("known_limits.json").is_file());
    assert!(output.output_dir.join("inspect_report.json").is_file());
    assert!(output.output_dir.join("compat/nerfstudio/transforms.json").is_file());
    assert!(output.output_dir.join("compat/nerfstudio/export_report.json").is_file());
  }

  #[test]
  fn partial_compatibility_keeps_canonical_package_and_reports_skips() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scene_packet_manifest_path = write_scene_packet_fixture(
      &temp,
      vec![
        synthetic_frame(1, "run_1", "file/auv-mc6-rich"),
        SyntheticFrameSpec {
          screenshot: ScreenshotDisposition::MissingPath,
          ..synthetic_frame(2, "run_2", "file/auv-mc6-rich")
        },
        SyntheticFrameSpec {
          screen_state: Some("menu".to_string()),
          ..synthetic_frame(3, "run_3", "file/auv-mc6-flat")
        },
        SyntheticFrameSpec {
          resource_pack_ids: vec!["fabric".to_string()],
          ..synthetic_frame(4, "run_4", "file/auv-mc6-flat")
        },
        SyntheticFrameSpec {
          resource_pack_ids: vec![
            "file/auv-mc6-repetitive".to_string(),
            "fabric".to_string(),
            "file/auv-mc6-shadow".to_string(),
          ],
          ..synthetic_frame(5, "run_5", "file/auv-mc6-repetitive")
        },
      ],
    );

    let output = export_3dgs_training_package(TrainingPackageInputs {
      scene_packet_manifest_path,
      output_dir: temp.path().join("training-package"),
    })
    .expect("training package export should succeed");

    let report = &output.inspect_report.compatibility_views[0];
    assert_eq!(report.status, TrainingCompatibilityStatus::Partial);
    assert_eq!(report.exported_frame_count, 1);
    assert_eq!(report.skipped_frame_count, 4);
    assert!(output.output_dir.join("frames/frame_000002.json").is_file());
    assert!(output.output_dir.join("compat/nerfstudio/export_report.json").is_file());
    assert!(output.output_dir.join("compat/nerfstudio/transforms.json").is_file());
    assert!(
      report
        .frame_decisions
        .iter()
        .find(|decision| decision.frame_index == 2)
        .expect("frame 2 decision")
        .skip_reasons
        .contains(&TrainingCompatibilitySkipReason::MissingScreenshot)
    );
    assert!(
      report
        .frame_decisions
        .iter()
        .find(|decision| decision.frame_index == 3)
        .expect("frame 3 decision")
        .skip_reasons
        .contains(&TrainingCompatibilitySkipReason::NonIngameScreenState)
    );
  }

  #[test]
  fn blocked_compatibility_omits_transforms_but_writes_export_report() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scene_packet_manifest_path = write_scene_packet_fixture(
      &temp,
      vec![
        SyntheticFrameSpec {
          screenshot: ScreenshotDisposition::MissingPath,
          ..synthetic_frame(1, "run_1", "file/auv-mc6-rich")
        },
        SyntheticFrameSpec {
          screen_state: Some("menu".to_string()),
          ..synthetic_frame(2, "run_2", "file/auv-mc6-flat")
        },
      ],
    );

    let output = export_3dgs_training_package(TrainingPackageInputs {
      scene_packet_manifest_path,
      output_dir: temp.path().join("training-package"),
    })
    .expect("training package export should succeed");

    let report = &output.inspect_report.compatibility_views[0];
    assert_eq!(report.status, TrainingCompatibilityStatus::Blocked);
    assert!(!output.output_dir.join("compat/nerfstudio/transforms.json").exists());
    assert!(output.output_dir.join("compat/nerfstudio/export_report.json").is_file());
  }

  #[test]
  fn hard_fails_when_cameras_json_is_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, vec![synthetic_frame(1, "run_1", "file/auv-mc6-rich")]);
    fs::remove_file(scene_packet_manifest_path.parent().unwrap().join("cameras.json")).expect("remove cameras");

    let error = export_3dgs_training_package(TrainingPackageInputs {
      scene_packet_manifest_path,
      output_dir: temp.path().join("training-package"),
    })
    .expect_err("missing cameras json should fail");

    assert!(error.contains("failed to open MC-7 D2 scene packet cameras JSON"));
  }

  #[test]
  fn hard_fails_when_frame_json_is_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scene_packet_manifest_path = write_scene_packet_fixture(&temp, vec![synthetic_frame(1, "run_1", "file/auv-mc6-rich")]);
    fs::remove_file(scene_packet_manifest_path.parent().unwrap().join("frames/frame_000001.json")).expect("remove frame");

    let error = export_3dgs_training_package(TrainingPackageInputs {
      scene_packet_manifest_path,
      output_dir: temp.path().join("training-package"),
    })
    .expect_err("missing frame json should fail");

    assert!(error.contains("failed to open MC-7 D2 scene packet frame JSON"));
  }

  #[test]
  fn hard_fails_when_screenshot_path_claims_file_but_file_is_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scene_packet_manifest_path = write_scene_packet_fixture(
      &temp,
      vec![SyntheticFrameSpec {
        screenshot: ScreenshotDisposition::MissingFile,
        ..synthetic_frame(1, "run_1", "file/auv-mc6-rich")
      }],
    );

    let error = export_3dgs_training_package(TrainingPackageInputs {
      scene_packet_manifest_path,
      output_dir: temp.path().join("training-package"),
    })
    .expect_err("missing physical screenshot should fail");

    assert!(error.contains("failed to copy MC-7 D3 canonical screenshot"));
  }

  #[test]
  fn rotation_only_view_matrix_uses_legacy_eye_position_fallback() {
    let temp = tempfile::tempdir().expect("temp dir");
    let scene_packet_manifest_path = write_scene_packet_fixture(
      &temp,
      vec![SyntheticFrameSpec {
        view_matrix: [
          0.719950, 0.115742, -0.684307, 0.0, -0.0, 0.985996, 0.166769, 0.0, 0.694026, -0.120065, 0.709867, 0.0, 0.0, 0.0, 0.0, 1.0,
        ],
        projection_matrix: [
          0.802706, 0.0, -0.0, -0.0, 0.0, 1.428148, -0.0, -0.0, 0.0, 0.0, -1.000130, -1.0, -0.0, -0.0, -0.100007, -0.0,
        ],
        viewport: Viewport::new(1708, 960),
        player_eye: Vec3::new(511.028439, 73.62, 728.652906),
        ..synthetic_frame(1, "run_1", "file/auv-mc6-rich")
      }],
    );

    let output = export_3dgs_training_package(TrainingPackageInputs {
      scene_packet_manifest_path,
      output_dir: temp.path().join("training-package"),
    })
    .expect("training package export should succeed");

    let report = &output.inspect_report.compatibility_views[0];
    assert_eq!(report.status, TrainingCompatibilityStatus::Ready);
    assert_eq!(report.used_legacy_view_translation_fallback_frame_indices, vec![1]);
    assert!(output.inspect_report.known_limits.iter().any(|value| value.contains("legacy rotation-only view_matrix fallback")));
  }

  fn synthetic_frame(frame_index: usize, source_run_id: &str, file_pack: &str) -> SyntheticFrameSpec {
    SyntheticFrameSpec {
      frame_index,
      source_run_id: source_run_id.to_string(),
      source_bundle_manifest_path: format!("/tmp/{source_run_id}/run.json"),
      spatial_frame_id: format!("frame-{frame_index}"),
      monotonic_timestamp_ms: frame_index as u64 * 1_000,
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: vec!["fabric".to_string(), file_pack.to_string()],
      screenshot: ScreenshotDisposition::Present,
      view_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      viewport: Viewport::new(800, 600),
      player_eye: Vec3::new(0.0, 0.0, 0.0),
    }
  }

  fn write_scene_packet_fixture(temp: &TempDir, specs: Vec<SyntheticFrameSpec>) -> PathBuf {
    let scene_packet_dir = temp.path().join("scene-packet");
    let frames_dir = scene_packet_dir.join("frames");
    fs::create_dir_all(&frames_dir).expect("frames dir");

    let mut manifest_frames = Vec::new();
    let mut frame_payloads = Vec::new();
    let mut cameras = Vec::new();
    let mut screenshot_count = 0usize;
    let mut missing_screenshot_count = 0usize;

    for spec in specs {
      let screenshot_relative_path = match spec.screenshot {
        ScreenshotDisposition::Present | ScreenshotDisposition::MissingFile => Some(format!("frames/frame_{:06}.png", spec.frame_index)),
        ScreenshotDisposition::MissingPath => None,
      };
      let screenshot_artifact_id = screenshot_relative_path.as_ref().map(|_| format!("artifact_{}", spec.frame_index));

      let spatial_frame = MinecraftSpatialFrame {
        spatial_frame_id: spec.spatial_frame_id.clone(),
        world_tick: spec.frame_index as u64,
        monotonic_timestamp_ms: spec.monotonic_timestamp_ms,
        telemetry_session_id: Some(format!("session-{}", spec.frame_index)),
        viewport: spec.viewport,
        view_matrix: spec.view_matrix,
        projection_matrix: spec.projection_matrix,
        player_pose: PlayerPose {
          eye_position: spec.player_eye,
          yaw: 0.0,
          pitch: 0.0,
        },
        raycast_hit: Some(RaycastHit {
          block_pos: BlockPosition::new(0, 0, 0),
          face: BlockFace::North,
          block_id: "minecraft:stone".to_string(),
        }),
        nearby_blocks: Vec::new(),
        nearby_entities: Vec::new(),
        inventory_summary: Vec::new(),
        screenshot_artifact_ref: screenshot_artifact_id.as_ref().map(|artifact_id| format!("artifact://{artifact_id}")),
        mc_capture_skew_ms: Some(0),
        screen_state: spec.screen_state.clone(),
        resource_pack_ids: spec.resource_pack_ids.clone(),
      };
      let frame_json_relative_path = format!("frames/frame_{:06}.json", spec.frame_index);
      let frame_payload = ScenePacketFramePayload {
        frame_index: spec.frame_index,
        source_run_id: spec.source_run_id.clone(),
        source_bundle_manifest_path: spec.source_bundle_manifest_path.clone(),
        source_frame_artifact_id: format!("frame-artifact-{}", spec.frame_index),
        source_frame_bundle_path: format!("spatial_frames/frame_{}.json", spec.frame_index),
        screenshot_artifact_id: screenshot_artifact_id.clone(),
        screenshot_path: screenshot_relative_path.clone(),
        spatial_frame,
      };
      write_json(&scene_packet_dir.join(&frame_json_relative_path), &frame_payload, "MC-7 D2 test frame JSON").expect("frame payload write");

      match spec.screenshot {
        ScreenshotDisposition::Present => {
          write_png(&scene_packet_dir.join(screenshot_relative_path.as_ref().expect("present screenshot path should exist")));
          screenshot_count += 1;
        }
        ScreenshotDisposition::MissingPath => {
          missing_screenshot_count += 1;
        }
        ScreenshotDisposition::MissingFile => {
          screenshot_count += 1;
        }
      }

      manifest_frames.push(ScenePacketFrameRecord {
        frame_index: spec.frame_index,
        spatial_frame_id: spec.spatial_frame_id.clone(),
        source_run_id: spec.source_run_id.clone(),
        source_bundle_manifest_path: spec.source_bundle_manifest_path.clone(),
        source_frame_artifact_id: format!("frame-artifact-{}", spec.frame_index),
        source_frame_bundle_path: format!("spatial_frames/frame_{}.json", spec.frame_index),
        frame_json_path: frame_json_relative_path,
        screenshot_artifact_id,
        screenshot_path: screenshot_relative_path,
        monotonic_timestamp_ms: spec.monotonic_timestamp_ms,
        viewport: spec.viewport,
        screen_state: spec.screen_state,
        resource_pack_ids: spec.resource_pack_ids,
      });
      frame_payloads.push(frame_payload);
    }

    for payload in &frame_payloads {
      cameras.push(ScenePacketCameraRecord {
        frame_index: payload.frame_index,
        spatial_frame_id: payload.spatial_frame.spatial_frame_id.clone(),
        monotonic_timestamp_ms: payload.spatial_frame.monotonic_timestamp_ms,
        viewport: payload.spatial_frame.viewport,
        view_matrix: payload.spatial_frame.view_matrix,
        projection_matrix: payload.spatial_frame.projection_matrix,
        player_pose: payload.spatial_frame.player_pose,
        raycast_hit: payload.spatial_frame.raycast_hit.clone(),
      });
    }

    let manifest = ScenePacketManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_bundle_manifest_paths: manifest_frames.iter().map(|frame| frame.source_bundle_manifest_path.clone()).collect(),
      source_run_ids: manifest_frames.iter().map(|frame| frame.source_run_id.clone()).collect(),
      counts: ScenePacketCounts {
        frames: manifest_frames.len(),
        screenshots: screenshot_count,
        missing_screenshots: missing_screenshot_count,
      },
      frames: manifest_frames,
      known_limits: vec!["MC-7 scene packet is 3DGS input material only; no trained splat is present".to_string()],
    };
    write_json(&scene_packet_dir.join("run.json"), &manifest, "MC-7 D2 test manifest JSON").expect("manifest write");
    write_json(&scene_packet_dir.join("cameras.json"), &cameras, "MC-7 D2 test cameras JSON").expect("cameras write");
    write_json(&scene_packet_dir.join("known_limits.json"), &manifest.known_limits, "MC-7 D2 test known limits JSON")
      .expect("known limits write");

    scene_packet_dir.join("run.json")
  }

  fn write_png(path: &Path) {
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent).expect("png parent");
    }
    let mut image = RgbaImage::new(2, 2);
    for pixel in image.pixels_mut() {
      *pixel = Rgba([255, 0, 0, 255]);
    }
    image.save(path).expect("png save");
  }
}
