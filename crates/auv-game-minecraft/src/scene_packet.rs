use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::dataset::{SpatialBundleDirectory, SpatialBundleManifest};
use crate::types::{MinecraftSpatialFrame, PlayerPose, RaycastHit, Viewport};

pub type ScenePacketResult<T> = Result<T, String>;

pub const SCENE_PACKET_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenePacketInputs {
  pub bundle_manifest_paths: Vec<PathBuf>,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenePacketOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub cameras_path: PathBuf,
  pub known_limits_path: PathBuf,
  pub manifest: ScenePacketManifest,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenePacketManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: ScenePacketCounts,
  pub frames: Vec<ScenePacketFrameRecord>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenePacketCounts {
  pub frames: usize,
  pub screenshots: usize,
  pub missing_screenshots: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenePacketFrameRecord {
  pub frame_index: usize,
  pub spatial_frame_id: String,
  pub source_run_id: String,
  pub source_bundle_manifest_path: String,
  pub source_frame_artifact_id: String,
  pub source_frame_bundle_path: String,
  pub frame_json_path: String,
  #[serde(default)]
  pub screenshot_artifact_id: Option<String>,
  #[serde(default)]
  pub screenshot_path: Option<String>,
  pub monotonic_timestamp_ms: u64,
  pub viewport: Viewport,
  #[serde(default)]
  pub screen_state: Option<String>,
  #[serde(default)]
  pub resource_pack_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenePacketFramePayload {
  pub frame_index: usize,
  pub source_run_id: String,
  pub source_bundle_manifest_path: String,
  pub source_frame_artifact_id: String,
  pub source_frame_bundle_path: String,
  #[serde(default)]
  pub screenshot_artifact_id: Option<String>,
  #[serde(default)]
  pub screenshot_path: Option<String>,
  pub spatial_frame: MinecraftSpatialFrame,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScenePacketCameraRecord {
  pub frame_index: usize,
  pub spatial_frame_id: String,
  pub monotonic_timestamp_ms: u64,
  pub viewport: Viewport,
  pub view_matrix: [f64; 16],
  pub projection_matrix: [f64; 16],
  pub player_pose: PlayerPose,
  #[serde(default)]
  pub raycast_hit: Option<RaycastHit>,
}

pub fn export_3dgs_scene_packet(inputs: ScenePacketInputs) -> ScenePacketResult<ScenePacketOutput> {
  if inputs.bundle_manifest_paths.is_empty() {
    return Err("at least one MC-7 source spatial bundle manifest is required".to_string());
  }

  let frames_dir = inputs.output_dir.join("frames");
  fs::create_dir_all(&frames_dir).map_err(|error| {
    format!(
      "failed to create MC-7 scene packet frames directory {}: {error}",
      frames_dir.display()
    )
  })?;

  let mut frames = Vec::new();
  let mut cameras = Vec::new();
  let mut source_run_ids = BTreeSet::new();
  let mut known_limits = BTreeSet::new();
  let mut screenshot_count = 0;
  let mut missing_screenshot_count = 0;

  for manifest_path in &inputs.bundle_manifest_paths {
    let manifest = read_manifest(manifest_path)?;
    let bundle_dir = manifest_path.parent().ok_or_else(|| {
      format!(
        "MC-7 source bundle manifest {} has no parent directory",
        manifest_path.display()
      )
    })?;
    source_run_ids.insert(manifest.source_run.source_run_id.clone());
    known_limits.extend(manifest.known_limits.iter().cloned());

    let screenshots = manifest
      .artifacts
      .iter()
      .filter(|artifact| artifact.directory == SpatialBundleDirectory::Screenshots)
      .map(|artifact| (artifact.artifact_id.clone(), artifact.clone()))
      .collect::<BTreeMap<_, _>>();

    for artifact in &manifest.artifacts {
      if artifact.directory != SpatialBundleDirectory::SpatialFrames
        || artifact.role != "minecraft-spatial-frame"
      {
        continue;
      }

      let frame_index = frames.len() + 1;
      let frame_source_path = bundle_dir.join(&artifact.bundle_path);
      let spatial_frame = read_frame(&frame_source_path)?;
      let frame_json_path = format!("frames/frame_{frame_index:06}.json");
      let screenshot = spatial_frame
        .screenshot_artifact_ref
        .as_deref()
        .and_then(|artifact_ref| artifact_ref.strip_prefix("artifact://"))
        .and_then(|artifact_id| screenshots.get(artifact_id));
      let (screenshot_artifact_id, screenshot_path) = if let Some(screenshot) = screenshot {
        let path = format!(
          "frames/frame_{frame_index:06}.{}",
          extension_for(&screenshot.bundle_path)
        );
        copy_file(
          &bundle_dir.join(&screenshot.bundle_path),
          &inputs.output_dir.join(&path),
          "MC-7 scene packet screenshot",
        )?;
        screenshot_count += 1;
        (Some(screenshot.artifact_id.clone()), Some(path))
      } else {
        missing_screenshot_count += 1;
        (None, None)
      };

      let payload = ScenePacketFramePayload {
        frame_index,
        source_run_id: manifest.source_run.source_run_id.clone(),
        source_bundle_manifest_path: manifest_path.to_string_lossy().into_owned(),
        source_frame_artifact_id: artifact.artifact_id.clone(),
        source_frame_bundle_path: artifact.bundle_path.clone(),
        screenshot_artifact_id: screenshot_artifact_id.clone(),
        screenshot_path: screenshot_path.clone(),
        spatial_frame: spatial_frame.clone(),
      };
      write_json(&inputs.output_dir.join(&frame_json_path), &payload)?;

      cameras.push(ScenePacketCameraRecord {
        frame_index,
        spatial_frame_id: spatial_frame.spatial_frame_id.clone(),
        monotonic_timestamp_ms: spatial_frame.monotonic_timestamp_ms,
        viewport: spatial_frame.viewport,
        view_matrix: spatial_frame.view_matrix,
        projection_matrix: spatial_frame.projection_matrix,
        player_pose: spatial_frame.player_pose,
        raycast_hit: spatial_frame.raycast_hit.clone(),
      });
      frames.push(ScenePacketFrameRecord {
        frame_index,
        spatial_frame_id: spatial_frame.spatial_frame_id,
        source_run_id: manifest.source_run.source_run_id.clone(),
        source_bundle_manifest_path: manifest_path.to_string_lossy().into_owned(),
        source_frame_artifact_id: artifact.artifact_id.clone(),
        source_frame_bundle_path: artifact.bundle_path.clone(),
        frame_json_path,
        screenshot_artifact_id,
        screenshot_path,
        monotonic_timestamp_ms: spatial_frame.monotonic_timestamp_ms,
        viewport: spatial_frame.viewport,
        screen_state: spatial_frame.screen_state,
        resource_pack_ids: spatial_frame.resource_pack_ids,
      });
    }
  }

  if frames.is_empty() {
    return Err(
      "MC-7 scene packet export found no minecraft-spatial-frame artifacts in the supplied bundles"
        .to_string(),
    );
  }
  if missing_screenshot_count > 0 {
    known_limits.insert(format!(
      "{missing_screenshot_count} scene packet frame(s) had no copied screenshot artifact"
    ));
  }
  known_limits.insert(
    "MC-7 D1 scene packet is 3DGS input material only; no trained splat is present".to_string(),
  );

  let manifest = ScenePacketManifest {
    schema_version: SCENE_PACKET_SCHEMA_VERSION,
    generated_at_millis: auv_tracing_driver::now_millis(),
    source_bundle_manifest_paths: inputs
      .bundle_manifest_paths
      .iter()
      .map(|path| path.to_string_lossy().into_owned())
      .collect(),
    source_run_ids: source_run_ids.into_iter().collect(),
    counts: ScenePacketCounts {
      frames: frames.len(),
      screenshots: screenshot_count,
      missing_screenshots: missing_screenshot_count,
    },
    frames,
    known_limits: known_limits.into_iter().collect(),
  };

  let manifest_path = inputs.output_dir.join("run.json");
  let cameras_path = inputs.output_dir.join("cameras.json");
  let known_limits_path = inputs.output_dir.join("known_limits.json");
  write_json(&manifest_path, &manifest)?;
  write_json(&cameras_path, &cameras)?;
  write_json(&known_limits_path, &manifest.known_limits)?;

  Ok(ScenePacketOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    cameras_path,
    known_limits_path,
    manifest,
  })
}

fn read_manifest(path: &Path) -> ScenePacketResult<SpatialBundleManifest> {
  let bytes = fs::read(path).map_err(|error| {
    format!(
      "failed to read MC-7 source bundle manifest {}: {error}",
      path.display()
    )
  })?;
  serde_json::from_slice::<SpatialBundleManifest>(&bytes).map_err(|error| {
    format!(
      "failed to parse MC-7 source bundle manifest {}: {error}",
      path.display()
    )
  })
}

fn read_frame(path: &Path) -> ScenePacketResult<MinecraftSpatialFrame> {
  let bytes = fs::read(path).map_err(|error| {
    format!(
      "failed to read MC-7 source spatial frame {}: {error}",
      path.display()
    )
  })?;
  serde_json::from_slice::<MinecraftSpatialFrame>(&bytes).map_err(|error| {
    format!(
      "failed to parse MC-7 source spatial frame {}: {error}",
      path.display()
    )
  })
}

fn extension_for(path: &str) -> String {
  Path::new(path)
    .extension()
    .and_then(|extension| extension.to_str())
    .filter(|extension| !extension.trim().is_empty())
    .unwrap_or("png")
    .to_string()
}

fn copy_file(source: &Path, destination: &Path, label: &str) -> ScenePacketResult<()> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create {label} directory {}: {error}",
        parent.display()
      )
    })?;
  }
  fs::copy(source, destination).map_err(|error| {
    format!(
      "failed to copy {label} from {} to {}: {error}",
      source.display(),
      destination.display()
    )
  })?;
  Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> ScenePacketResult<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create MC-7 scene packet JSON directory {}: {error}",
        parent.display()
      )
    })?;
  }
  let json = serde_json::to_string_pretty(value)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize MC-7 scene packet JSON: {error}"))?;
  fs::write(path, json.as_bytes()).map_err(|error| {
    format!(
      "failed to write MC-7 scene packet JSON {}: {error}",
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

  fn frame() -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "frame-rich".to_string(),
      world_tick: 1,
      monotonic_timestamp_ms: 1_000,
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
      screenshot_artifact_ref: Some("artifact://artifact_0001".to_string()),
      mc_capture_skew_ms: Some(10),
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: vec!["fabric".to_string(), "file/auv-mc6-rich".to_string()],
    }
  }

  fn write_bundle(temp: &tempfile::TempDir, with_frame: bool) -> PathBuf {
    let bundle_dir = temp.path().join("bundle");
    let screenshots_dir = bundle_dir.join("screenshots");
    let frames_dir = bundle_dir.join("spatial_frames");
    fs::create_dir_all(&screenshots_dir).expect("screenshots dir");
    fs::create_dir_all(&frames_dir).expect("frames dir");
    fs::write(screenshots_dir.join("artifact_0001-frame.png"), b"png").expect("screenshot");

    let mut artifacts = vec![SpatialBundleArtifactRecord {
      artifact_id: "artifact_0001".to_string(),
      role: "minecraft-screenshot".to_string(),
      source_path: "artifacts/frame.png".to_string(),
      bundle_path: "screenshots/artifact_0001-frame.png".to_string(),
      directory: SpatialBundleDirectory::Screenshots,
      summary: None,
    }];
    if with_frame {
      fs::write(
        frames_dir.join("artifact_0002-frame.json"),
        serde_json::to_vec_pretty(&frame()).expect("frame json"),
      )
      .expect("frame write");
      artifacts.push(SpatialBundleArtifactRecord {
        artifact_id: "artifact_0002".to_string(),
        role: "minecraft-spatial-frame".to_string(),
        source_path: "artifacts/frame.json".to_string(),
        bundle_path: "spatial_frames/artifact_0002-frame.json".to_string(),
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
        screenshots: 1,
        spatial_frames: usize::from(with_frame),
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
  fn exports_scene_packet_from_spatial_bundle() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(&temp, true);

    let output = export_3dgs_scene_packet(ScenePacketInputs {
      bundle_manifest_paths: vec![manifest_path.clone()],
      output_dir: temp.path().join("scene-packet"),
    })
    .expect("scene packet export should succeed");

    assert_eq!(output.manifest.schema_version, SCENE_PACKET_SCHEMA_VERSION);
    assert_eq!(output.manifest.source_run_ids, vec!["run_1"]);
    assert_eq!(
      output.manifest.source_bundle_manifest_paths,
      vec![manifest_path.to_string_lossy().into_owned()]
    );
    assert_eq!(output.manifest.counts.frames, 1);
    assert_eq!(output.manifest.counts.screenshots, 1);
    assert!(output.manifest_path.is_file());
    assert!(output.cameras_path.is_file());
    assert!(output.known_limits_path.is_file());
    assert!(
      output
        .output_dir
        .join(&output.manifest.frames[0].frame_json_path)
        .is_file()
    );
    assert!(
      output
        .output_dir
        .join(
          output.manifest.frames[0]
            .screenshot_path
            .as_ref()
            .expect("screenshot path")
        )
        .is_file()
    );
  }

  #[test]
  fn rejects_bundle_without_spatial_frames() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(&temp, false);

    let error = export_3dgs_scene_packet(ScenePacketInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_dir: temp.path().join("scene-packet"),
    })
    .expect_err("missing spatial frames should fail");

    assert!(error.contains("no minecraft-spatial-frame artifacts"));
  }
}
