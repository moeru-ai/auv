use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use auv_driver::geometry::Point;
use serde::de::DeserializeOwned;

use crate::MinecraftProjector;
use crate::artifact::MinecraftProjectionArtifact;
use crate::dataset::{SpatialBundleDirectory, SpatialBundleManifest};
use crate::measurement::{TextureSweepSample, TextureSweepSampleSet, TextureSweepSampleSource};
use crate::types::{MinecraftSpatialFrame, MinecraftTargetSemantics, ProjectionVisibility};
use crate::verify::MismatchRefusalReason;

pub type SampleBuildResult<T> = Result<T, String>;

pub const TEXTURE_SWEEP_SAMPLE_BUILDER_GENERATOR: &str = "mc6.bundle-texture-sweep";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextureSweepSampleBuildInputs {
  pub bundle_manifest_paths: Vec<PathBuf>,
  pub output_path: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextureSweepSampleBuildOutput {
  pub output_path: PathBuf,
  pub sample_set: TextureSweepSampleSet,
}

#[derive(Clone, Debug, PartialEq)]
struct ProfileFrames {
  resource_pack: String,
  texture_profile: String,
  session_windows: BTreeMap<String, SessionWindow>,
  observed_samples: BTreeSet<(String, bool)>,
  samples: Vec<ProfileSampleEntry>,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct SessionWindow {
  first_timestamp_ms: Option<u64>,
  last_timestamp_ms: Option<u64>,
}

impl SessionWindow {
  fn record_accepted_frame(&mut self, frame: &MinecraftSpatialFrame) {
    self.first_timestamp_ms =
      Some(self.first_timestamp_ms.map_or(frame.monotonic_timestamp_ms, |timestamp| timestamp.min(frame.monotonic_timestamp_ms)));
    self.last_timestamp_ms =
      Some(self.last_timestamp_ms.map_or(frame.monotonic_timestamp_ms, |timestamp| timestamp.max(frame.monotonic_timestamp_ms)));
  }

  fn observed_duration_seconds(&self) -> f64 {
    let (Some(first_timestamp), Some(last_timestamp)) = (self.first_timestamp_ms, self.last_timestamp_ms) else {
      return 0.0;
    };
    if last_timestamp >= first_timestamp {
      (last_timestamp - first_timestamp) as f64 / 1000.0
    } else {
      0.0
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
struct ProfileSampleEntry {
  sample: TextureSweepSample,
  session_bucket: String,
}

impl ProfileFrames {
  fn record_sample(&mut self, frame: &MinecraftSpatialFrame, source_run_id: &str, sample: TextureSweepSample) {
    let dedupe_key = (frame.spatial_frame_id.clone(), sample.refused_noise);
    if !self.observed_samples.insert(dedupe_key) {
      return;
    }

    let session_bucket = session_bucket_key(frame, source_run_id);
    if !sample.refused_noise {
      self.session_windows.entry(session_bucket.clone()).or_default().record_accepted_frame(frame);
    }
    self.samples.push(ProfileSampleEntry {
      sample,
      session_bucket,
    });
  }
}

pub fn build_texture_sweep_samples_from_bundles(inputs: TextureSweepSampleBuildInputs) -> SampleBuildResult<TextureSweepSampleBuildOutput> {
  if inputs.bundle_manifest_paths.is_empty() {
    return Err("at least one MC-6 spatial bundle manifest is required".to_string());
  }

  let mut source_run_ids = BTreeSet::new();
  let mut known_limits = BTreeSet::new();
  let mut profile_frames = BTreeMap::<String, ProfileFrames>::new();
  for manifest_path in &inputs.bundle_manifest_paths {
    let manifest = read_manifest(manifest_path)?;
    source_run_ids.insert(manifest.source_run.source_run_id.clone());
    known_limits.extend(manifest.known_limits.iter().cloned());
    collect_manifest_frames(manifest_path, &manifest, &mut profile_frames)?;
  }

  let mut samples = Vec::new();
  for frames in profile_frames.values() {
    if frames.samples.is_empty() {
      continue;
    }
    samples.extend(samples_for_profile(frames));
  }
  if samples.is_empty() {
    return Err("MC-6 texture sweep sample builder found no usable spatial frames in the supplied bundles".to_string());
  }

  let sample_set = TextureSweepSampleSet {
    source: Some(TextureSweepSampleSource {
      generated_at_millis: crate::run_read::now_millis(),
      generator: TEXTURE_SWEEP_SAMPLE_BUILDER_GENERATOR.to_string(),
      source_run_ids: source_run_ids.into_iter().collect(),
      bundle_manifest_paths: inputs.bundle_manifest_paths.iter().map(|path| path.to_string_lossy().into_owned()).collect(),
      known_limits: known_limits.into_iter().collect(),
    }),
    samples,
  };
  if let Some(parent) = inputs.output_path.parent() {
    fs::create_dir_all(parent)
      .map_err(|error| format!("failed to create MC-6 texture sweep sample output directory {}: {error}", parent.display()))?;
  }
  let json = serde_json::to_string_pretty(&sample_set)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize MC-6 texture sweep samples: {error}"))?;
  fs::write(&inputs.output_path, json.as_bytes())
    .map_err(|error| format!("failed to write MC-6 texture sweep samples {}: {error}", inputs.output_path.display()))?;

  Ok(TextureSweepSampleBuildOutput {
    output_path: inputs.output_path,
    sample_set,
  })
}

fn collect_manifest_frames(
  manifest_path: &Path,
  manifest: &SpatialBundleManifest,
  profile_frames: &mut BTreeMap<String, ProfileFrames>,
) -> SampleBuildResult<()> {
  let bundle_dir =
    manifest_path.parent().ok_or_else(|| format!("MC-6 spatial bundle manifest {} has no parent directory", manifest_path.display()))?;
  let projection_refusal_reasons = read_projection_refusal_reasons(bundle_dir, manifest)?;
  for artifact in &manifest.artifacts {
    if artifact.directory != SpatialBundleDirectory::SpatialFrames || artifact.role != "minecraft-spatial-frame" {
      continue;
    }
    let frame_path = bundle_dir.join(&artifact.bundle_path);
    let frame = read_frame(&frame_path)?;
    let Some((resource_pack, texture_profile)) = classify_profile(&frame)? else {
      continue;
    };
    let projection_refusal_reason = projection_refusal_reasons.get(&frame.spatial_frame_id).copied().flatten();
    let entry = profile_frames.entry(resource_pack.clone()).or_insert_with(|| ProfileFrames {
      resource_pack: resource_pack.clone(),
      texture_profile: texture_profile.clone(),
      session_windows: BTreeMap::new(),
      observed_samples: BTreeSet::new(),
      samples: Vec::new(),
    });
    if entry.texture_profile != texture_profile {
      return Err(format!("resource pack {resource_pack} maps to both {} and {texture_profile}", entry.texture_profile));
    }
    let sample = sample_for_frame(&frame, &resource_pack, &texture_profile, projection_refusal_reason)?;
    entry.record_sample(&frame, &manifest.source_run.source_run_id, sample);
  }
  Ok(())
}

fn samples_for_profile(frames: &ProfileFrames) -> Vec<TextureSweepSample> {
  frames
    .samples
    .iter()
    .cloned()
    .map(|mut entry| {
      entry.sample.duration_seconds =
        frames.session_windows.get(&entry.session_bucket).map_or(0.0, SessionWindow::observed_duration_seconds);
      entry.sample
    })
    .collect()
}

fn sample_for_frame(
  frame: &MinecraftSpatialFrame,
  resource_pack: &str,
  texture_profile: &str,
  projection_refusal_reason: Option<MismatchRefusalReason>,
) -> SampleBuildResult<TextureSweepSample> {
  if let Some(reason) = projection_refusal_reason.or_else(|| fallback_refusal_reason(frame)) {
    return Ok(refused_sample(resource_pack, texture_profile, reason));
  }

  let raycast_hit = frame
    .raycast_hit
    .as_ref()
    .ok_or_else(|| format!("frame {} in resource pack {} lacks raycast ground truth", frame.spatial_frame_id, resource_pack))?;
  let target = crate::mc6_projection_target_for_frame(raycast_hit.block_pos, frame, MinecraftTargetSemantics::HitFaceCenter);
  let projected = MinecraftProjector::new(frame.clone())?.project_block_target(&target)?;
  if let Some(reason) = refusal_reason_from_projection(&projected.visibility, projected.screen_point) {
    return Ok(refused_sample(resource_pack, texture_profile, reason));
  }

  Ok(TextureSweepSample {
    resource_pack: resource_pack.to_string(),
    texture_profile: texture_profile.to_string(),
    duration_seconds: 0.0,
    // TODO(mc6-pose-metric): true pose metric needs independent 2D labels or richer verification
    // evidence; bridge-only MC-6 samples intentionally do not encode center-distance as pose error.
    pose_error_px: 0.0,
    occlusion_iou: 1.0,
    refused_noise: false,
    refusal_reason: None,
  })
}

fn read_projection_refusal_reasons(
  bundle_dir: &Path,
  manifest: &SpatialBundleManifest,
) -> SampleBuildResult<BTreeMap<String, Option<MismatchRefusalReason>>> {
  let mut reasons = BTreeMap::new();
  for artifact in &manifest.artifacts {
    if artifact.directory != SpatialBundleDirectory::SpatialFrames || artifact.role != "minecraft-projection" {
      continue;
    }
    let projection_path = bundle_dir.join(&artifact.bundle_path);
    let projection = read_projection_artifact(&projection_path)?;
    if reasons.insert(projection.spatial_frame_id.clone(), projection.mismatch_refusal_reason).is_some() {
      return Err(format!("MC-6 bundle has multiple minecraft-projection artifacts for frame {}", projection.spatial_frame_id));
    }
  }
  Ok(reasons)
}

fn fallback_refusal_reason(frame: &MinecraftSpatialFrame) -> Option<MismatchRefusalReason> {
  match frame.screen_state.as_deref() {
    Some("in_game") => {}
    Some(_) => return Some(MismatchRefusalReason::MenuLoadingScreen),
    None => return Some(MismatchRefusalReason::TelemetryUnreliable),
  }
  if frame.screenshot_artifact_ref.is_none() {
    return Some(MismatchRefusalReason::ScreenshotUnavailable);
  }
  if frame.raycast_hit.is_none() {
    return Some(MismatchRefusalReason::TelemetryUnreliable);
  }
  None
}

fn refusal_reason_from_projection(visibility: &ProjectionVisibility, screen_point: Option<Point>) -> Option<MismatchRefusalReason> {
  match visibility {
    ProjectionVisibility::Visible if screen_point.is_some() => None,
    ProjectionVisibility::Visible => Some(MismatchRefusalReason::TelemetryUnreliable),
    ProjectionVisibility::BehindCamera => Some(MismatchRefusalReason::TargetBehindCamera),
    ProjectionVisibility::OutOfFrustum => Some(MismatchRefusalReason::TargetOutOfFrustum),
    ProjectionVisibility::OutsideWindow => Some(MismatchRefusalReason::ProjectedOutsideWindow),
  }
}

fn refused_sample(resource_pack: &str, texture_profile: &str, reason: MismatchRefusalReason) -> TextureSweepSample {
  TextureSweepSample {
    resource_pack: resource_pack.to_string(),
    texture_profile: texture_profile.to_string(),
    duration_seconds: 0.0,
    pose_error_px: 0.0,
    occlusion_iou: 0.0,
    refused_noise: true,
    refusal_reason: Some(reason),
  }
}

fn session_bucket_key(frame: &MinecraftSpatialFrame, source_run_id: &str) -> String {
  frame.telemetry_session_id.clone().filter(|id| !id.trim().is_empty()).unwrap_or_else(|| source_run_id.to_string())
}

fn classify_profile(frame: &MinecraftSpatialFrame) -> SampleBuildResult<Option<(String, String)>> {
  let mut matched = frame
    .resource_pack_ids
    .iter()
    .filter_map(|pack_id| profile_for_resource_pack_id(pack_id).map(|profile| (pack_id, profile)))
    .collect::<Vec<_>>();
  matched.sort();
  matched.dedup();
  match matched.as_slice() {
    [] => Ok(None),
    [(pack_id, profile)] => Ok(Some(((*pack_id).clone(), (*profile).to_string()))),
    _ => Err(format!(
      "frame {} has multiple MC-6 texture sweep pack ids: {:?}",
      frame.spatial_frame_id,
      matched.iter().map(|(pack_id, _)| (*pack_id).clone()).collect::<Vec<_>>()
    )),
  }
}

fn profile_for_resource_pack_id(pack_id: &str) -> Option<&'static str> {
  if pack_id.ends_with("auv-mc6-rich") {
    Some("rich")
  } else if pack_id.ends_with("auv-mc6-flat-color") {
    Some("flat_color")
  } else if pack_id.ends_with("auv-mc6-repetitive") {
    Some("repetitive")
  } else {
    None
  }
}

fn read_projection_artifact(path: &Path) -> SampleBuildResult<MinecraftProjectionArtifact> {
  read_json_file(path, "MC-6 projection artifact")
}

fn read_manifest(path: &Path) -> SampleBuildResult<SpatialBundleManifest> {
  read_json_file(path, "MC-6 spatial bundle manifest")
}

fn read_frame(path: &Path) -> SampleBuildResult<MinecraftSpatialFrame> {
  read_json_file(path, "MC-6 spatial frame artifact")
}

fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> SampleBuildResult<T> {
  let file = fs::File::open(path).map_err(|error| format!("failed to open {label} {}: {error}", path.display()))?;
  serde_json::from_reader(BufReader::new(file)).map_err(|error| format!("failed to parse {label} {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::artifact::MinecraftProjectionArtifact;
  use crate::dataset::{SPATIAL_BUNDLE_SCHEMA_VERSION, SourceRunSummary, SpatialBundleArtifactRecord, SpatialBundleCounts};
  use crate::types::{BlockFace, BlockPosition, PlayerPose, RaycastHit, Vec3, Viewport};
  use crate::verify::MismatchRefusalReason;

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn test_frame(id: &str, pack_id: &str, screen_state: &str) -> MinecraftSpatialFrame {
    test_frame_at(id, pack_id, Some(screen_state), 1_000)
  }

  fn test_frame_at(id: &str, pack_id: &str, screen_state: Option<&str>, monotonic_timestamp_ms: u64) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: id.to_string(),
      world_tick: 1,
      monotonic_timestamp_ms,
      telemetry_session_id: None,
      viewport: Viewport::new(800, 600),
      view_matrix: identity_matrix(),
      projection_matrix: identity_matrix(),
      player_pose: PlayerPose {
        eye_position: Vec3::new(0.0, 0.0, 0.0),
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
      screenshot_artifact_ref: Some("artifact://screenshot".to_string()),
      mc_capture_skew_ms: Some(10),
      screen_state: screen_state.map(str::to_string),
      resource_pack_ids: vec!["fabric".to_string(), pack_id.to_string()],
    }
  }

  fn with_telemetry_session_id(mut frame: MinecraftSpatialFrame, telemetry_session_id: &str) -> MinecraftSpatialFrame {
    frame.telemetry_session_id = Some(telemetry_session_id.to_string());
    frame
  }

  fn projection_artifact(frame: &MinecraftSpatialFrame, reason: Option<MismatchRefusalReason>) -> MinecraftProjectionArtifact {
    MinecraftProjectionArtifact {
      spatial_frame_id: frame.spatial_frame_id.clone(),
      world_tick: frame.world_tick,
      monotonic_timestamp_ms: frame.monotonic_timestamp_ms,
      screenshot_artifact_ref: frame.screenshot_artifact_ref.clone(),
      mc_capture_skew_ms: frame.mc_capture_skew_ms,
      viewport_bounds: crate::artifact::ProjectionViewportBounds::from_rect(frame.viewport.bounds()),
      projected_point: None,
      visibility: ProjectionVisibility::OutsideWindow,
      raycast_block_id: frame.raycast_hit.as_ref().map(|hit| hit.block_id.clone()),
      screen_state: frame.screen_state.clone(),
      resource_pack_ids: frame.resource_pack_ids.clone(),
      mismatch_refusal_reason: reason,
      verification_reference: None,
    }
  }

  fn write_bundle(
    temp: &tempfile::TempDir,
    frames: Vec<(&str, MinecraftSpatialFrame)>,
    projections: Vec<(&str, MinecraftProjectionArtifact)>,
    source_run_id: &str,
  ) -> PathBuf {
    let bundle_dir = temp.path().join("bundle");
    let frames_dir = bundle_dir.join("spatial_frames");
    fs::create_dir_all(&frames_dir).expect("frames dir");
    let mut artifacts = Vec::new();
    for (index, (name, frame)) in frames.iter().enumerate() {
      let file_name = format!("artifact_{index:04}-{name}.json");
      fs::write(frames_dir.join(&file_name), serde_json::to_vec_pretty(frame).expect("frame json")).expect("frame write");
      artifacts.push(SpatialBundleArtifactRecord {
        artifact_id: format!("artifact_{index:04}"),
        role: "minecraft-spatial-frame".to_string(),
        source_path: format!("artifacts/{name}.json"),
        bundle_path: format!("spatial_frames/{file_name}"),
        directory: SpatialBundleDirectory::SpatialFrames,
        summary: None,
      });
    }
    let base_index = artifacts.len();
    for (offset, (name, projection)) in projections.iter().enumerate() {
      let file_name = format!("artifact_{:04}-{name}.json", base_index + offset);
      fs::write(frames_dir.join(&file_name), serde_json::to_vec_pretty(projection).expect("projection json")).expect("projection write");
      artifacts.push(SpatialBundleArtifactRecord {
        artifact_id: format!("artifact_{:04}", base_index + offset),
        role: "minecraft-projection".to_string(),
        source_path: format!("artifacts/{name}.json"),
        bundle_path: format!("spatial_frames/{file_name}"),
        directory: SpatialBundleDirectory::SpatialFrames,
        summary: None,
      });
    }
    let manifest = SpatialBundleManifest {
      schema_version: SPATIAL_BUNDLE_SCHEMA_VERSION,
      source_run: SourceRunSummary {
        source_run_id: source_run_id.to_string(),
        source_operation: "auv.minecraft.bridge".to_string(),
        source_run_type: "execute".to_string(),
        source_status: "ok".to_string(),
        generated_at_millis: 1,
        auv_git_commit: None,
        exporter_git_commit: None,
      },
      counts: SpatialBundleCounts {
        spatial_frames: artifacts.len(),
        ..SpatialBundleCounts::default()
      },
      artifacts,
      known_limits: vec!["bundle limit".to_string()],
    };
    let manifest_path = bundle_dir.join("run.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).expect("manifest json")).expect("manifest write");
    manifest_path
  }

  #[test]
  fn visible_off_center_target_does_not_turn_center_distance_into_pose_error() {
    let sample = sample_for_frame(&test_frame("frame-rich", "file/auv-mc6-rich", "in_game"), "file/auv-mc6-rich", "rich", None)
      .expect("sample build should succeed");

    assert!(!sample.refused_noise);
    assert_eq!(sample.pose_error_px, 0.0);
    assert_eq!(sample.occlusion_iou, 1.0);
  }

  #[test]
  fn builds_samples_from_real_bundle_spatial_frames() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      vec![
        ("rich", test_frame("frame-rich", "file/auv-mc6-rich", "in_game")),
        ("refusal", test_frame("frame-refusal", "file/auv-mc6-rich", "menu")),
      ],
      Vec::new(),
      "run_1",
    );

    let output = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
      bundle_manifest_paths: vec![manifest_path.clone()],
      output_path: temp.path().join("samples.json"),
    })
    .expect("sample build should succeed");

    let source = output.sample_set.source.as_ref().expect("source");
    assert_eq!(source.generator, TEXTURE_SWEEP_SAMPLE_BUILDER_GENERATOR);
    assert_eq!(source.source_run_ids, vec!["run_1"]);
    assert_eq!(source.bundle_manifest_paths, vec![manifest_path.to_string_lossy().into_owned()]);
    assert_eq!(output.sample_set.samples.len(), 2);
    assert!(output.sample_set.samples.iter().any(|sample| sample.refused_noise));
    assert!(output.output_path.is_file());
  }

  #[test]
  fn duration_uses_only_accepted_frames_even_when_refusals_are_unordered() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      vec![
        ("late", test_frame_at("frame-late", "file/auv-mc6-rich", Some("in_game"), 31_000)),
        ("early-refusal", test_frame_at("frame-early-refusal", "file/auv-mc6-rich", Some("menu"), 1_000)),
        ("middle", test_frame_at("frame-middle", "file/auv-mc6-rich", Some("in_game"), 5_000)),
      ],
      Vec::new(),
      "run_1",
    );

    let output = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_path: temp.path().join("samples.json"),
    })
    .expect("sample build should succeed");

    assert_eq!(output.sample_set.samples.len(), 3);
    assert_eq!(output.sample_set.samples.iter().filter(|sample| sample.refused_noise).count(), 1);
    let accepted = output.sample_set.samples.iter().filter(|sample| !sample.refused_noise).collect::<Vec<_>>();
    assert_eq!(accepted.len(), 2);
    assert!(accepted.iter().all(|sample| sample.duration_seconds == 26.0));
    assert!(output.sample_set.samples.iter().filter(|sample| sample.refused_noise).all(|sample| sample.duration_seconds == 26.0));
  }

  #[test]
  fn missing_screen_state_is_refused_noise_not_accepted_metric_data() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      vec![
        ("accepted", test_frame_at("frame-accepted", "file/auv-mc6-rich", Some("in_game"), 1_000)),
        ("legacy-missing-screen-state", test_frame_at("frame-legacy-missing-screen-state", "file/auv-mc6-rich", None, 11_000)),
      ],
      Vec::new(),
      "run_1",
    );

    let output = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_path: temp.path().join("samples.json"),
    })
    .expect("sample build should succeed");

    let accepted = output.sample_set.samples.iter().filter(|sample| !sample.refused_noise).collect::<Vec<_>>();
    assert_eq!(accepted.len(), 1);
    assert_eq!(output.sample_set.samples.iter().filter(|sample| sample.refused_noise).count(), 1);
    assert_eq!(accepted[0].duration_seconds, 0.0);
    let refusal = output.sample_set.samples.iter().find(|sample| sample.refused_noise).expect("refusal sample");
    assert_eq!(refusal.refusal_reason, Some(MismatchRefusalReason::TelemetryUnreliable));
    assert_eq!(refusal.duration_seconds, 0.0);
  }

  #[test]
  fn duplicate_accepted_observation_for_same_frame_is_deduped() {
    let temp = tempfile::tempdir().expect("temp dir");
    let shared = with_telemetry_session_id(test_frame("frame-shared", "file/auv-mc6-rich", "in_game"), "session-a");
    let manifest_path = write_bundle(&temp, vec![("shared-a", shared.clone()), ("shared-b", shared)], Vec::new(), "run_1");

    let output = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_path: temp.path().join("samples.json"),
    })
    .expect("sample build should succeed");

    assert_eq!(output.sample_set.samples.len(), 1);
    assert_eq!(output.sample_set.samples[0].duration_seconds, 0.0);
  }

  #[test]
  fn duration_does_not_cross_telemetry_sessions() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      vec![
        ("session-a", with_telemetry_session_id(test_frame_at("frame-a", "file/auv-mc6-rich", Some("in_game"), 1_000), "session-a")),
        ("session-b", with_telemetry_session_id(test_frame_at("frame-b", "file/auv-mc6-rich", Some("in_game"), 61_000), "session-b")),
      ],
      Vec::new(),
      "run_1",
    );

    let output = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_path: temp.path().join("samples.json"),
    })
    .expect("sample build should succeed");

    assert_eq!(output.sample_set.samples.len(), 2);
    assert!(output.sample_set.samples.iter().all(|sample| sample.duration_seconds == 0.0));
  }

  #[test]
  fn projection_refusal_reason_beats_fallback_and_counts_as_refusal() {
    let temp = tempfile::tempdir().expect("temp dir");
    let frame = test_frame("frame-menu", "file/auv-mc6-rich", "in_game");
    let manifest_path = write_bundle(
      &temp,
      vec![("frame", frame.clone())],
      vec![("projection", projection_artifact(&frame, Some(MismatchRefusalReason::MenuLoadingScreen)))],
      "run_1",
    );

    let output = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_path: temp.path().join("samples.json"),
    })
    .expect("sample build should succeed");

    assert_eq!(output.sample_set.samples.len(), 1);
    let sample = &output.sample_set.samples[0];
    assert!(sample.refused_noise);
    assert_eq!(sample.refusal_reason, Some(MismatchRefusalReason::MenuLoadingScreen));
  }

  #[test]
  fn duplicate_projection_artifacts_for_same_frame_are_rejected() {
    let temp = tempfile::tempdir().expect("temp dir");
    let frame = test_frame("frame-rich", "file/auv-mc6-rich", "in_game");
    let manifest_path = write_bundle(
      &temp,
      vec![("frame", frame.clone())],
      vec![
        ("projection-a", projection_artifact(&frame, Some(MismatchRefusalReason::MenuLoadingScreen))),
        ("projection-b", projection_artifact(&frame, Some(MismatchRefusalReason::TargetOccluded))),
      ],
      "run_1",
    );

    let error = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_path: temp.path().join("samples.json"),
    })
    .expect_err("duplicate projection artifacts should fail");

    assert!(error.contains("multiple minecraft-projection artifacts"));
  }
}
