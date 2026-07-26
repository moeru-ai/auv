use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use auv_driver::geometry::Point;
use serde::de::DeserializeOwned;

use crate::MinecraftProjector;
use crate::artifact::MinecraftProjectionArtifact;
use crate::dataset::{PROJECTION_BUNDLE_ROLE, SPATIAL_FRAME_BUNDLE_ROLE, SpatialBundleDirectory, SpatialBundleManifest};
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
    source_run_ids.insert(manifest.source_run.run_id.to_string());
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
      generated_at_millis: crate::now_millis(),
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
    if artifact.directory != SpatialBundleDirectory::SpatialFrames || artifact.role != SPATIAL_FRAME_BUNDLE_ROLE {
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
    entry.record_sample(&frame, &manifest.source_run.run_id.to_string(), sample);
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
    if artifact.directory != SpatialBundleDirectory::SpatialFrames || artifact.role != PROJECTION_BUNDLE_ROLE {
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
