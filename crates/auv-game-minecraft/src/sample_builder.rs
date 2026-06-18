use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::dataset::{SpatialBundleDirectory, SpatialBundleManifest};
use crate::measurement::{TextureSweepSample, TextureSweepSampleSet, TextureSweepSampleSource};
use crate::types::{MinecraftSpatialFrame, ProjectionVisibility};
use crate::{MinecraftBlockTarget, MinecraftProjector};

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
  first_timestamp_ms: Option<u64>,
  last_timestamp_ms: Option<u64>,
  frames: Vec<MinecraftSpatialFrame>,
  refused_noise_count: usize,
}

impl ProfileFrames {
  fn record_observation(&mut self, frame: &MinecraftSpatialFrame) {
    self.first_timestamp_ms = Some(
      self
        .first_timestamp_ms
        .map_or(frame.monotonic_timestamp_ms, |timestamp| {
          timestamp.min(frame.monotonic_timestamp_ms)
        }),
    );
    self.last_timestamp_ms = Some(
      self
        .last_timestamp_ms
        .map_or(frame.monotonic_timestamp_ms, |timestamp| {
          timestamp.max(frame.monotonic_timestamp_ms)
        }),
    );
  }

  fn observed_duration_seconds(&self) -> f64 {
    let (Some(first_timestamp), Some(last_timestamp)) =
      (self.first_timestamp_ms, self.last_timestamp_ms)
    else {
      return 0.0;
    };
    if last_timestamp >= first_timestamp {
      (last_timestamp - first_timestamp) as f64 / 1000.0
    } else {
      0.0
    }
  }
}

pub fn build_texture_sweep_samples_from_bundles(
  inputs: TextureSweepSampleBuildInputs,
) -> SampleBuildResult<TextureSweepSampleBuildOutput> {
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
    if frames.frames.is_empty() {
      continue;
    }
    samples.extend(samples_for_profile(frames)?);
  }
  if samples.is_empty() {
    return Err(
      "MC-6 texture sweep sample builder found no usable spatial frames in the supplied bundles"
        .to_string(),
    );
  }

  let sample_set = TextureSweepSampleSet {
    source: Some(TextureSweepSampleSource {
      generated_at_millis: auv_tracing_driver::now_millis(),
      generator: TEXTURE_SWEEP_SAMPLE_BUILDER_GENERATOR.to_string(),
      source_run_ids: source_run_ids.into_iter().collect(),
      bundle_manifest_paths: inputs
        .bundle_manifest_paths
        .iter()
        .map(|path| path.to_string_lossy().into_owned())
        .collect(),
      known_limits: known_limits.into_iter().collect(),
    }),
    samples,
  };
  if let Some(parent) = inputs.output_path.parent() {
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create MC-6 texture sweep sample output directory {}: {error}",
        parent.display()
      )
    })?;
  }
  let json = serde_json::to_string_pretty(&sample_set)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize MC-6 texture sweep samples: {error}"))?;
  fs::write(&inputs.output_path, json.as_bytes()).map_err(|error| {
    format!(
      "failed to write MC-6 texture sweep samples {}: {error}",
      inputs.output_path.display()
    )
  })?;

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
  let bundle_dir = manifest_path.parent().ok_or_else(|| {
    format!(
      "MC-6 spatial bundle manifest {} has no parent directory",
      manifest_path.display()
    )
  })?;
  for artifact in &manifest.artifacts {
    if artifact.directory != SpatialBundleDirectory::SpatialFrames
      || artifact.role != "minecraft-spatial-frame"
    {
      continue;
    }
    let frame_path = bundle_dir.join(&artifact.bundle_path);
    let frame = read_frame(&frame_path)?;
    let Some((resource_pack, texture_profile)) = classify_profile(&frame)? else {
      continue;
    };
    let entry = profile_frames
      .entry(resource_pack.clone())
      .or_insert_with(|| ProfileFrames {
        resource_pack: resource_pack.clone(),
        texture_profile: texture_profile.clone(),
        first_timestamp_ms: None,
        last_timestamp_ms: None,
        frames: Vec::new(),
        refused_noise_count: 0,
      });
    if entry.texture_profile != texture_profile {
      return Err(format!(
        "resource pack {resource_pack} maps to both {} and {texture_profile}",
        entry.texture_profile
      ));
    }
    entry.record_observation(&frame);
    if is_refused_noise(&frame) {
      entry.refused_noise_count += 1;
    } else {
      entry.frames.push(frame);
    }
  }
  Ok(())
}

fn samples_for_profile(frames: &ProfileFrames) -> SampleBuildResult<Vec<TextureSweepSample>> {
  let observed_duration = frames.observed_duration_seconds();
  let mut samples = Vec::new();
  for frame in &frames.frames {
    let raycast_hit = frame.raycast_hit.as_ref().ok_or_else(|| {
      format!(
        "frame {} in resource pack {} lacks raycast ground truth",
        frame.spatial_frame_id, frames.resource_pack
      )
    })?;
    let projected = MinecraftProjector::new(frame.clone())?
      .project_block_target(&MinecraftBlockTarget::new(raycast_hit.block_pos))?;
    let pose_error_px = match projected.visibility {
      ProjectionVisibility::Visible => {
        let point = projected.screen_point.ok_or_else(|| {
          format!(
            "frame {} projected visible without a screen point",
            frame.spatial_frame_id
          )
        })?;
        let center_x = f64::from(frame.viewport.width) / 2.0;
        let center_y = f64::from(frame.viewport.height) / 2.0;
        ((point.x - center_x).powi(2) + (point.y - center_y).powi(2)).sqrt()
      }
      _ => projected.match_radius_px + f64::from(frame.viewport.width.max(frame.viewport.height)),
    };
    samples.push(TextureSweepSample {
      resource_pack: frames.resource_pack.clone(),
      texture_profile: frames.texture_profile.clone(),
      duration_seconds: observed_duration,
      pose_error_px,
      occlusion_iou: 1.0,
      refused_noise: false,
    });
  }
  for _ in 0..frames.refused_noise_count {
    samples.push(TextureSweepSample {
      resource_pack: frames.resource_pack.clone(),
      texture_profile: frames.texture_profile.clone(),
      duration_seconds: observed_duration,
      pose_error_px: 0.0,
      occlusion_iou: 0.0,
      refused_noise: true,
    });
  }
  Ok(samples)
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
      matched
        .iter()
        .map(|(pack_id, _)| (*pack_id).clone())
        .collect::<Vec<_>>()
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

fn is_refused_noise(frame: &MinecraftSpatialFrame) -> bool {
  !matches!(frame.screen_state.as_deref(), Some("in_game"))
    || frame.raycast_hit.is_none()
    || frame.screenshot_artifact_ref.is_none()
}

fn read_manifest(path: &Path) -> SampleBuildResult<SpatialBundleManifest> {
  let bytes = fs::read(path).map_err(|error| {
    format!(
      "failed to read MC-6 spatial bundle manifest {}: {error}",
      path.display()
    )
  })?;
  serde_json::from_slice::<SpatialBundleManifest>(&bytes).map_err(|error| {
    format!(
      "failed to parse MC-6 spatial bundle manifest {}: {error}",
      path.display()
    )
  })
}

fn read_frame(path: &Path) -> SampleBuildResult<MinecraftSpatialFrame> {
  let bytes = fs::read(path).map_err(|error| {
    format!(
      "failed to read MC-6 spatial frame artifact {}: {error}",
      path.display()
    )
  })?;
  serde_json::from_slice::<MinecraftSpatialFrame>(&bytes).map_err(|error| {
    format!(
      "failed to parse MC-6 spatial frame artifact {}: {error}",
      path.display()
    )
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::dataset::{
    SPATIAL_BUNDLE_SCHEMA_VERSION, SourceRunSummary, SpatialBundleArtifactRecord,
    SpatialBundleCounts,
  };
  use crate::types::{BlockFace, BlockPosition, PlayerPose, RaycastHit, Vec3, Viewport};

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn test_frame(id: &str, pack_id: &str, screen_state: &str) -> MinecraftSpatialFrame {
    test_frame_at(id, pack_id, Some(screen_state), 1_000)
  }

  fn test_frame_at(
    id: &str,
    pack_id: &str,
    screen_state: Option<&str>,
    monotonic_timestamp_ms: u64,
  ) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: id.to_string(),
      world_tick: 1,
      monotonic_timestamp_ms,
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

  fn write_bundle(temp: &tempfile::TempDir, frames: Vec<(&str, MinecraftSpatialFrame)>) -> PathBuf {
    let bundle_dir = temp.path().join("bundle");
    let frames_dir = bundle_dir.join("spatial_frames");
    fs::create_dir_all(&frames_dir).expect("frames dir");
    let mut artifacts = Vec::new();
    for (index, (name, frame)) in frames.iter().enumerate() {
      let file_name = format!("artifact_{index:04}-{name}.json");
      fs::write(
        frames_dir.join(&file_name),
        serde_json::to_vec_pretty(frame).expect("frame json"),
      )
      .expect("frame write");
      artifacts.push(SpatialBundleArtifactRecord {
        artifact_id: format!("artifact_{index:04}"),
        role: "minecraft-spatial-frame".to_string(),
        source_path: format!("artifacts/{name}.json"),
        bundle_path: format!("spatial_frames/{file_name}"),
        directory: SpatialBundleDirectory::SpatialFrames,
        summary: None,
      });
    }
    let manifest = SpatialBundleManifest {
      schema_version: SPATIAL_BUNDLE_SCHEMA_VERSION,
      source_run: SourceRunSummary {
        source_run_id: "run_1".to_string(),
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
    fs::write(
      &manifest_path,
      serde_json::to_vec_pretty(&manifest).expect("manifest json"),
    )
    .expect("manifest write");
    manifest_path
  }

  #[test]
  fn builds_samples_from_real_bundle_spatial_frames() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      vec![
        (
          "rich",
          test_frame("frame-rich", "file/auv-mc6-rich", "in_game"),
        ),
        (
          "refusal",
          test_frame("frame-refusal", "file/auv-mc6-rich", "menu"),
        ),
      ],
    );

    let output = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
      bundle_manifest_paths: vec![manifest_path.clone()],
      output_path: temp.path().join("samples.json"),
    })
    .expect("sample build should succeed");

    let source = output.sample_set.source.as_ref().expect("source");
    assert_eq!(source.generator, TEXTURE_SWEEP_SAMPLE_BUILDER_GENERATOR);
    assert_eq!(source.source_run_ids, vec!["run_1"]);
    assert_eq!(
      source.bundle_manifest_paths,
      vec![manifest_path.to_string_lossy().into_owned()]
    );
    assert_eq!(output.sample_set.samples.len(), 2);
    assert!(
      output
        .sample_set
        .samples
        .iter()
        .any(|sample| sample.refused_noise)
    );
    assert!(output.output_path.is_file());
  }

  #[test]
  fn duration_uses_all_profile_timestamps_even_when_refusals_are_unordered() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      vec![
        (
          "late",
          test_frame_at("frame-late", "file/auv-mc6-rich", Some("in_game"), 31_000),
        ),
        (
          "early-refusal",
          test_frame_at(
            "frame-early-refusal",
            "file/auv-mc6-rich",
            Some("menu"),
            1_000,
          ),
        ),
        (
          "middle",
          test_frame_at("frame-middle", "file/auv-mc6-rich", Some("in_game"), 5_000),
        ),
      ],
    );

    let output = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_path: temp.path().join("samples.json"),
    })
    .expect("sample build should succeed");

    assert_eq!(output.sample_set.samples.len(), 3);
    assert_eq!(
      output
        .sample_set
        .samples
        .iter()
        .filter(|sample| sample.refused_noise)
        .count(),
      1
    );
    assert!(
      output
        .sample_set
        .samples
        .iter()
        .all(|sample| sample.duration_seconds == 30.0)
    );
  }

  #[test]
  fn missing_screen_state_is_refused_noise_not_accepted_metric_data() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      vec![
        (
          "accepted",
          test_frame_at(
            "frame-accepted",
            "file/auv-mc6-rich",
            Some("in_game"),
            1_000,
          ),
        ),
        (
          "legacy-missing-screen-state",
          test_frame_at(
            "frame-legacy-missing-screen-state",
            "file/auv-mc6-rich",
            None,
            11_000,
          ),
        ),
      ],
    );

    let output = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_path: temp.path().join("samples.json"),
    })
    .expect("sample build should succeed");

    let accepted = output
      .sample_set
      .samples
      .iter()
      .filter(|sample| !sample.refused_noise)
      .collect::<Vec<_>>();
    assert_eq!(accepted.len(), 1);
    assert_eq!(
      output
        .sample_set
        .samples
        .iter()
        .filter(|sample| sample.refused_noise)
        .count(),
      1
    );
    assert!(
      output
        .sample_set
        .samples
        .iter()
        .all(|sample| sample.duration_seconds == 10.0)
    );
  }
}
