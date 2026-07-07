use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::dataset::{SpatialBundleDirectory, SpatialBundleManifest};
use crate::types::{MinecraftSpatialFrame, PlayerPose, RaycastHit, Viewport};

pub type ScenePacketResult<T> = Result<T, String>;

pub const SCENE_PACKET_SCHEMA_VERSION: u32 = 1;
pub const SCENE_PACKET_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;

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
  pub inspect_report_path: PathBuf,
  pub manifest: ScenePacketManifest,
  pub inspect_report: ScenePacketInspectReport,
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

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenePacketInspectCounts {
  pub frames: usize,
  pub screenshots: usize,
  pub missing_screenshots: usize,
  pub camera_records: usize,
  pub source_runs: usize,
  pub resource_pack_profiles: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenePacketResourcePackCoverage {
  pub resource_pack_id: String,
  pub frame_count: usize,
  pub source_run_ids: Vec<String>,
  pub screen_states: Vec<String>,
  #[serde(default)]
  pub first_timestamp_ms: Option<u64>,
  #[serde(default)]
  pub last_timestamp_ms: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenePacketAnomalies {
  pub missing_screenshot_frame_indices: Vec<usize>,
  pub non_ingame_frame_indices: Vec<usize>,
  pub frames_without_file_resource_pack: Vec<usize>,
  pub frames_with_multiple_file_resource_packs: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScenePacketInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: ScenePacketInspectCounts,
  pub resource_pack_coverage: Vec<ScenePacketResourcePackCoverage>,
  pub anomalies: ScenePacketAnomalies,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
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

#[derive(Serialize)]
struct ScenePacketFramePayloadRef<'a> {
  pub frame_index: usize,
  pub source_run_id: &'a str,
  pub source_bundle_manifest_path: &'a str,
  pub source_frame_artifact_id: &'a str,
  pub source_frame_bundle_path: &'a str,
  #[serde(default)]
  pub screenshot_artifact_id: Option<&'a str>,
  #[serde(default)]
  pub screenshot_path: Option<&'a str>,
  pub spatial_frame: &'a MinecraftSpatialFrame,
}

pub fn export_3dgs_scene_packet(inputs: ScenePacketInputs) -> ScenePacketResult<ScenePacketOutput> {
  if inputs.bundle_manifest_paths.is_empty() {
    return Err("at least one MC-7 source spatial bundle manifest is required".to_string());
  }

  let frames_dir = inputs.output_dir.join("frames");
  fs::create_dir_all(&frames_dir)
    .map_err(|error| format!("failed to create MC-7 scene packet frames directory {}: {error}", frames_dir.display()))?;

  let mut frames = Vec::new();
  let mut source_run_ids = BTreeSet::new();
  let mut known_limits = BTreeSet::new();
  let mut warnings = BTreeSet::new();
  let mut screenshot_count = 0;
  let mut missing_screenshot_count = 0;
  let mut camera_record_count = 0;
  let mut anomalies = ScenePacketAnomalies::default();
  let mut resource_pack_coverage = BTreeMap::<String, ResourcePackCoverageAccumulator>::new();
  let cameras_path = inputs.output_dir.join("cameras.json");
  let mut camera_writer = JsonArrayWriter::create(&cameras_path, "MC-7 scene packet cameras JSON")?;

  for manifest_path in &inputs.bundle_manifest_paths {
    let manifest = read_manifest(manifest_path)?;
    let bundle_dir =
      manifest_path.parent().ok_or_else(|| format!("MC-7 source bundle manifest {} has no parent directory", manifest_path.display()))?;
    source_run_ids.insert(manifest.source_run.source_run_id.clone());
    known_limits.extend(manifest.known_limits.iter().cloned());

    let screenshots = manifest
      .artifacts
      .iter()
      .filter(|artifact| artifact.directory == SpatialBundleDirectory::Screenshots)
      .map(|artifact| (artifact.artifact_id.clone(), artifact.clone()))
      .collect::<BTreeMap<_, _>>();

    for artifact in &manifest.artifacts {
      if artifact.directory != SpatialBundleDirectory::SpatialFrames || artifact.role != "minecraft-spatial-frame" {
        continue;
      }

      let frame_index = frames.len() + 1;
      let frame_source_path = bundle_dir.join(&artifact.bundle_path);
      let spatial_frame = read_frame(&frame_source_path)?;
      let frame_json_path = format!("frames/frame_{frame_index:06}.json");
      let screenshot_artifact_id =
        spatial_frame.screenshot_artifact_ref.as_deref().and_then(|artifact_ref| artifact_ref.strip_prefix("artifact://"));
      let screenshot = screenshot_artifact_id.and_then(|artifact_id| screenshots.get(artifact_id));
      let (screenshot_artifact_id, screenshot_path) = if let Some(screenshot) = screenshot {
        let path = format!("frames/frame_{frame_index:06}.{}", extension_for(&screenshot.bundle_path));
        copy_file(&bundle_dir.join(&screenshot.bundle_path), &inputs.output_dir.join(&path), "MC-7 scene packet screenshot")?;
        screenshot_count += 1;
        (Some(screenshot.artifact_id.clone()), Some(path))
      } else {
        missing_screenshot_count += 1;
        anomalies.missing_screenshot_frame_indices.push(frame_index);
        (None, None)
      };

      let screen_state = spatial_frame.screen_state.clone();
      if screen_state.as_deref() != Some("in_game") {
        anomalies.non_ingame_frame_indices.push(frame_index);
        warnings.insert(format!(
          "frame {frame_index} from source run {} had non-ingame screen_state {:?}",
          manifest.source_run.source_run_id,
          screen_state.as_deref().unwrap_or("missing")
        ));
      }

      let file_resource_packs =
        spatial_frame.resource_pack_ids.iter().filter(|resource_pack_id| resource_pack_id.starts_with("file/")).cloned().collect::<Vec<_>>();
      match file_resource_packs.len() {
        0 => {
          anomalies.frames_without_file_resource_pack.push(frame_index);
          warnings.insert(format!("frame {frame_index} from source run {} had no file/* resource pack", manifest.source_run.source_run_id));
        }
        1 => {
          let coverage =
            resource_pack_coverage.entry(file_resource_packs[0].clone()).or_insert_with(ResourcePackCoverageAccumulator::default);
          coverage.frame_count += 1;
          coverage.source_run_ids.insert(manifest.source_run.source_run_id.clone());
          coverage.screen_states.insert(screen_state.clone().unwrap_or_else(|| "missing".to_string()));
          coverage.first_timestamp_ms = Some(
            coverage
              .first_timestamp_ms
              .map_or(spatial_frame.monotonic_timestamp_ms, |value| value.min(spatial_frame.monotonic_timestamp_ms)),
          );
          coverage.last_timestamp_ms = Some(
            coverage.last_timestamp_ms.map_or(spatial_frame.monotonic_timestamp_ms, |value| value.max(spatial_frame.monotonic_timestamp_ms)),
          );
        }
        _ => {
          anomalies.frames_with_multiple_file_resource_packs.push(frame_index);
          warnings.insert(format!(
            "frame {frame_index} from source run {} had multiple file/* resource packs: {}",
            manifest.source_run.source_run_id,
            file_resource_packs.join(",")
          ));
        }
      }

      let source_run_id = manifest.source_run.source_run_id.as_str();
      let source_bundle_manifest_path = manifest_path.to_string_lossy().into_owned();
      let payload = ScenePacketFramePayloadRef {
        frame_index,
        source_run_id,
        source_bundle_manifest_path: source_bundle_manifest_path.as_str(),
        source_frame_artifact_id: artifact.artifact_id.as_str(),
        source_frame_bundle_path: artifact.bundle_path.as_str(),
        screenshot_artifact_id: screenshot_artifact_id.as_deref(),
        screenshot_path: screenshot_path.as_deref(),
        spatial_frame: &spatial_frame,
      };
      write_json(&inputs.output_dir.join(&frame_json_path), &payload)?;

      camera_writer.push(&ScenePacketCameraRecord {
        frame_index,
        spatial_frame_id: spatial_frame.spatial_frame_id.clone(),
        monotonic_timestamp_ms: spatial_frame.monotonic_timestamp_ms,
        viewport: spatial_frame.viewport,
        view_matrix: spatial_frame.view_matrix,
        projection_matrix: spatial_frame.projection_matrix,
        player_pose: spatial_frame.player_pose,
        raycast_hit: spatial_frame.raycast_hit.clone(),
      })?;
      camera_record_count += 1;
      frames.push(ScenePacketFrameRecord {
        frame_index,
        spatial_frame_id: spatial_frame.spatial_frame_id,
        source_run_id: source_run_id.to_string(),
        source_bundle_manifest_path,
        source_frame_artifact_id: artifact.artifact_id.clone(),
        source_frame_bundle_path: artifact.bundle_path.clone(),
        frame_json_path,
        screenshot_artifact_id,
        screenshot_path,
        monotonic_timestamp_ms: spatial_frame.monotonic_timestamp_ms,
        viewport: spatial_frame.viewport,
        screen_state,
        resource_pack_ids: spatial_frame.resource_pack_ids,
      });
    }
  }

  if frames.is_empty() {
    return Err("MC-7 scene packet export found no minecraft-spatial-frame artifacts in the supplied bundles".to_string());
  }
  if missing_screenshot_count > 0 {
    known_limits.insert(format!("{missing_screenshot_count} scene packet frame(s) had no copied screenshot artifact"));
  }
  known_limits.insert("MC-7 scene packet is 3DGS input material only; no trained splat is present".to_string());

  let source_bundle_manifest_paths = inputs.bundle_manifest_paths.iter().map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>();
  let source_run_ids = source_run_ids.into_iter().collect::<Vec<_>>();
  let known_limits = known_limits.into_iter().collect::<Vec<_>>();

  let manifest = ScenePacketManifest {
    schema_version: SCENE_PACKET_SCHEMA_VERSION,
    generated_at_millis: auv_tracing_driver::now_millis(),
    source_bundle_manifest_paths: source_bundle_manifest_paths.clone(),
    source_run_ids: source_run_ids.clone(),
    counts: ScenePacketCounts {
      frames: frames.len(),
      screenshots: screenshot_count,
      missing_screenshots: missing_screenshot_count,
    },
    frames,
    known_limits: known_limits.clone(),
  };

  let manifest_path = inputs.output_dir.join("run.json");
  let known_limits_path = inputs.output_dir.join("known_limits.json");
  let inspect_report_path = inputs.output_dir.join("inspect_report.json");
  write_json(&manifest_path, &manifest)?;
  camera_writer.finish()?;
  write_json(&known_limits_path, &manifest.known_limits)?;

  let inspect_report = ScenePacketInspectReport {
    schema_version: SCENE_PACKET_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis: manifest.generated_at_millis,
    scene_packet_manifest_path: manifest_path.to_string_lossy().into_owned(),
    source_bundle_manifest_paths,
    source_run_ids: source_run_ids.clone(),
    counts: ScenePacketInspectCounts {
      frames: manifest.counts.frames,
      screenshots: manifest.counts.screenshots,
      missing_screenshots: manifest.counts.missing_screenshots,
      camera_records: camera_record_count,
      source_runs: source_run_ids.len(),
      resource_pack_profiles: resource_pack_coverage.len(),
    },
    resource_pack_coverage: resource_pack_coverage
      .into_iter()
      .map(|(resource_pack_id, coverage)| ScenePacketResourcePackCoverage {
        resource_pack_id,
        frame_count: coverage.frame_count,
        source_run_ids: coverage.source_run_ids.into_iter().collect(),
        screen_states: coverage.screen_states.into_iter().collect(),
        first_timestamp_ms: coverage.first_timestamp_ms,
        last_timestamp_ms: coverage.last_timestamp_ms,
      })
      .collect(),
    anomalies,
    warnings: warnings.into_iter().collect(),
    known_limits: known_limits.clone(),
  };
  write_json(&inspect_report_path, &inspect_report)?;

  Ok(ScenePacketOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    cameras_path,
    known_limits_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

#[derive(Default)]
struct ResourcePackCoverageAccumulator {
  frame_count: usize,
  source_run_ids: BTreeSet<String>,
  screen_states: BTreeSet<String>,
  first_timestamp_ms: Option<u64>,
  last_timestamp_ms: Option<u64>,
}

fn read_manifest(path: &Path) -> ScenePacketResult<SpatialBundleManifest> {
  read_json_file(path, "MC-7 source bundle manifest")
}

fn read_frame(path: &Path) -> ScenePacketResult<MinecraftSpatialFrame> {
  read_json_file(path, "MC-7 source spatial frame")
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
    fs::create_dir_all(parent).map_err(|error| format!("failed to create {label} directory {}: {error}", parent.display()))?;
  }
  fs::copy(source, destination)
    .map_err(|error| format!("failed to copy {label} from {} to {}: {error}", source.display(), destination.display()))?;
  Ok(())
}

fn write_json(path: &Path, value: &impl Serialize) -> ScenePacketResult<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent)
      .map_err(|error| format!("failed to create MC-7 scene packet JSON directory {}: {error}", parent.display()))?;
  }
  let json = serde_json::to_string_pretty(value)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize MC-7 scene packet JSON: {error}"))?;
  fs::write(path, json.as_bytes()).map_err(|error| format!("failed to write MC-7 scene packet JSON {}: {error}", path.display()))
}

fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> ScenePacketResult<T> {
  let file = fs::File::open(path).map_err(|error| format!("failed to open {label} {}: {error}", path.display()))?;
  serde_json::from_reader(BufReader::new(file)).map_err(|error| format!("failed to parse {label} {}: {error}", path.display()))
}

struct JsonArrayWriter {
  path: PathBuf,
  writer: BufWriter<fs::File>,
  first: bool,
}

impl JsonArrayWriter {
  fn create(path: &Path, label: &str) -> ScenePacketResult<Self> {
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent).map_err(|error| format!("failed to create {label} directory {}: {error}", parent.display()))?;
    }
    let file = fs::File::create(path).map_err(|error| format!("failed to create {label} {}: {error}", path.display()))?;
    let mut writer = BufWriter::new(file);
    writer.write_all(b"[\n").map_err(|error| format!("failed to start {label} {}: {error}", path.display()))?;
    Ok(Self {
      path: path.to_path_buf(),
      writer,
      first: true,
    })
  }

  fn push(&mut self, value: &impl Serialize) -> ScenePacketResult<()> {
    if !self.first {
      self.writer.write_all(b",\n").map_err(|error| format!("failed to append JSON array separator {}: {error}", self.path.display()))?;
    }
    self.first = false;
    serde_json::to_writer_pretty(&mut self.writer, value)
      .map_err(|error| format!("failed to serialize MC-7 scene packet JSON array entry {}: {error}", self.path.display()))
  }

  fn finish(mut self) -> ScenePacketResult<()> {
    self.writer.write_all(b"\n]\n").map_err(|error| format!("failed to finish JSON array {}: {error}", self.path.display()))?;
    self.writer.flush().map_err(|error| format!("failed to flush JSON array {}: {error}", self.path.display()))
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::dataset::{SPATIAL_BUNDLE_SCHEMA_VERSION, SourceRunSummary, SpatialBundleArtifactRecord, SpatialBundleCounts};
  use crate::types::{BlockFace, BlockPosition, PlayerPose, RaycastHit, Vec3, Viewport};

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn frame(
    spatial_frame_id: &str,
    monotonic_timestamp_ms: u64,
    screen_state: Option<&str>,
    resource_pack_ids: &[&str],
  ) -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: spatial_frame_id.to_string(),
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
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: Some(10),
      screen_state: screen_state.map(ToString::to_string),
      resource_pack_ids: resource_pack_ids.iter().map(ToString::to_string).collect(),
    }
  }

  #[derive(Clone, Copy)]
  enum ScreenshotDisposition {
    Present,
    MissingRef,
    MissingArtifact,
    MissingFile,
  }

  struct BundleFrameSpec {
    label: String,
    frame: MinecraftSpatialFrame,
    screenshot: ScreenshotDisposition,
  }

  fn write_bundle(
    temp: &tempfile::TempDir,
    bundle_name: &str,
    run_id: &str,
    known_limits: Vec<String>,
    frames: Vec<BundleFrameSpec>,
  ) -> PathBuf {
    let bundle_dir = temp.path().join(bundle_name);
    let screenshots_dir = bundle_dir.join("screenshots");
    let frames_dir = bundle_dir.join("spatial_frames");
    fs::create_dir_all(&screenshots_dir).expect("screenshots dir");
    fs::create_dir_all(&frames_dir).expect("frames dir");
    let mut artifacts = Vec::new();

    for (index, mut frame_spec) in frames.into_iter().enumerate() {
      let screenshot_artifact_id = format!("artifact_{:04}", index * 2 + 1);
      let frame_artifact_id = format!("artifact_{:04}", index * 2 + 2);
      let screenshot_file_name = format!("{screenshot_artifact_id}-{}.png", frame_spec.label);
      let frame_file_name = format!("{frame_artifact_id}-{}.json", frame_spec.label);
      let screenshot_bundle_path = format!("screenshots/{screenshot_file_name}");
      let frame_bundle_path = format!("spatial_frames/{frame_file_name}");

      frame_spec.frame.screenshot_artifact_ref = match frame_spec.screenshot {
        ScreenshotDisposition::Present | ScreenshotDisposition::MissingArtifact | ScreenshotDisposition::MissingFile => {
          Some(format!("artifact://{screenshot_artifact_id}"))
        }
        ScreenshotDisposition::MissingRef => None,
      };

      if matches!(frame_spec.screenshot, ScreenshotDisposition::Present | ScreenshotDisposition::MissingFile) {
        artifacts.push(SpatialBundleArtifactRecord {
          artifact_id: screenshot_artifact_id.clone(),
          role: "minecraft-screenshot".to_string(),
          source_path: format!("artifacts/{screenshot_file_name}"),
          bundle_path: screenshot_bundle_path.clone(),
          directory: SpatialBundleDirectory::Screenshots,
          summary: None,
        });
      }
      if matches!(frame_spec.screenshot, ScreenshotDisposition::Present) {
        fs::write(screenshots_dir.join(&screenshot_file_name), b"png").expect("screenshot");
      }

      fs::write(frames_dir.join(&frame_file_name), serde_json::to_vec_pretty(&frame_spec.frame).expect("frame json")).expect("frame write");
      artifacts.push(SpatialBundleArtifactRecord {
        artifact_id: frame_artifact_id,
        role: "minecraft-spatial-frame".to_string(),
        source_path: format!("artifacts/{frame_file_name}"),
        bundle_path: frame_bundle_path,
        directory: SpatialBundleDirectory::SpatialFrames,
        summary: None,
      });
    }

    let manifest = SpatialBundleManifest {
      schema_version: SPATIAL_BUNDLE_SCHEMA_VERSION,
      source_run: SourceRunSummary {
        source_run_id: run_id.to_string(),
        source_operation: "auv.minecraft.bridge".to_string(),
        source_run_type: "execute".to_string(),
        source_status: "ok".to_string(),
        generated_at_millis: 1,
        auv_git_commit: None,
        exporter_git_commit: None,
      },
      counts: SpatialBundleCounts {
        screenshots: artifacts.iter().filter(|artifact| artifact.directory == SpatialBundleDirectory::Screenshots).count(),
        spatial_frames: artifacts.iter().filter(|artifact| artifact.directory == SpatialBundleDirectory::SpatialFrames).count(),
        ..SpatialBundleCounts::default()
      },
      artifacts,
      known_limits,
    };
    let manifest_path = bundle_dir.join("run.json");
    fs::write(&manifest_path, serde_json::to_vec_pretty(&manifest).expect("manifest json")).expect("manifest write");
    manifest_path
  }

  #[test]
  fn exports_scene_packet_from_spatial_bundle() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      "bundle",
      "run_1",
      vec!["bundle limit".to_string()],
      vec![BundleFrameSpec {
        label: "frame-rich".to_string(),
        frame: frame("frame-rich", 1_000, Some("in_game"), &["fabric", "file/auv-mc6-rich"]),
        screenshot: ScreenshotDisposition::Present,
      }],
    );

    let output = export_3dgs_scene_packet(ScenePacketInputs {
      bundle_manifest_paths: vec![manifest_path.clone()],
      output_dir: temp.path().join("scene-packet"),
    })
    .expect("scene packet export should succeed");

    assert_eq!(output.manifest.schema_version, SCENE_PACKET_SCHEMA_VERSION);
    assert_eq!(output.manifest.source_run_ids, vec!["run_1"]);
    assert_eq!(output.manifest.source_bundle_manifest_paths, vec![manifest_path.to_string_lossy().into_owned()]);
    assert_eq!(output.manifest.counts.frames, 1);
    assert_eq!(output.manifest.counts.screenshots, 1);
    assert!(output.manifest_path.is_file());
    assert!(output.cameras_path.is_file());
    assert!(output.known_limits_path.is_file());
    assert!(output.inspect_report_path.is_file());
    assert_eq!(output.inspect_report.scene_packet_manifest_path, output.manifest_path.to_string_lossy());
    assert_eq!(output.inspect_report.counts.camera_records, 1);
    assert_eq!(output.inspect_report.counts.resource_pack_profiles, 1);
    assert_eq!(output.inspect_report.resource_pack_coverage[0].resource_pack_id, "file/auv-mc6-rich");
    assert!(
      output.inspect_report.known_limits.contains(&"MC-7 scene packet is 3DGS input material only; no trained splat is present".to_string())
    );
    assert!(!output.inspect_report.known_limits.iter().any(|limit| limit.contains("MC-7 D1 scene packet")));
    let cameras: Vec<ScenePacketCameraRecord> =
      serde_json::from_slice(&fs::read(&output.cameras_path).expect("cameras json should read")).expect("cameras json should parse");
    assert_eq!(cameras.len(), 1);
    let inspect_report: ScenePacketInspectReport =
      serde_json::from_slice(&fs::read(&output.inspect_report_path).expect("inspect report should read"))
        .expect("inspect report should parse");
    assert_eq!(inspect_report, output.inspect_report);
    assert!(output.output_dir.join(&output.manifest.frames[0].frame_json_path).is_file());
    assert!(output.output_dir.join(output.manifest.frames[0].screenshot_path.as_ref().expect("screenshot path")).is_file());
  }

  #[test]
  fn rejects_bundle_without_spatial_frames() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(&temp, "bundle", "run_1", Vec::new(), Vec::new());

    let error = export_3dgs_scene_packet(ScenePacketInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_dir: temp.path().join("scene-packet"),
    })
    .expect_err("missing spatial frames should fail");

    assert!(error.contains("no minecraft-spatial-frame artifacts"));
  }

  #[test]
  fn exports_inspect_report_for_synthetic_six_bundle_shape() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifests = vec![
      write_bundle(
        &temp,
        "bundle-1",
        "run_1",
        vec!["bundle limit".to_string()],
        vec![BundleFrameSpec {
          label: "rich-a".to_string(),
          frame: frame("frame-rich-a", 1_000, Some("in_game"), &["fabric", "file/auv-mc6-rich"]),
          screenshot: ScreenshotDisposition::Present,
        }],
      ),
      write_bundle(
        &temp,
        "bundle-2",
        "run_2",
        vec!["bundle limit".to_string()],
        vec![BundleFrameSpec {
          label: "rich-b".to_string(),
          frame: frame("frame-rich-b", 2_000, Some("in_game"), &["fabric", "file/auv-mc6-rich"]),
          screenshot: ScreenshotDisposition::Present,
        }],
      ),
      write_bundle(
        &temp,
        "bundle-3",
        "run_3",
        vec!["bundle limit".to_string()],
        vec![BundleFrameSpec {
          label: "flat-a".to_string(),
          frame: frame("frame-flat-a", 3_000, Some("in_game"), &["fabric", "file/auv-mc6-flat-color"]),
          screenshot: ScreenshotDisposition::Present,
        }],
      ),
      write_bundle(
        &temp,
        "bundle-4",
        "run_4",
        vec!["bundle limit".to_string()],
        vec![BundleFrameSpec {
          label: "flat-b".to_string(),
          frame: frame("frame-flat-b", 4_000, Some("in_game"), &["fabric", "file/auv-mc6-flat-color"]),
          screenshot: ScreenshotDisposition::Present,
        }],
      ),
      write_bundle(
        &temp,
        "bundle-5",
        "run_5",
        vec!["bundle limit".to_string()],
        vec![BundleFrameSpec {
          label: "repetitive-a".to_string(),
          frame: frame("frame-repetitive-a", 5_000, Some("in_game"), &["fabric", "file/auv-mc6-repetitive"]),
          screenshot: ScreenshotDisposition::Present,
        }],
      ),
      write_bundle(
        &temp,
        "bundle-6",
        "run_6",
        vec!["bundle limit".to_string()],
        vec![BundleFrameSpec {
          label: "repetitive-b".to_string(),
          frame: frame("frame-repetitive-b", 6_000, Some("in_game"), &["fabric", "file/auv-mc6-repetitive"]),
          screenshot: ScreenshotDisposition::Present,
        }],
      ),
    ];

    let output = export_3dgs_scene_packet(ScenePacketInputs {
      bundle_manifest_paths: manifests,
      output_dir: temp.path().join("scene-packet"),
    })
    .expect("scene packet export should succeed");

    assert_eq!(output.inspect_report.counts.frames, 6);
    assert_eq!(output.inspect_report.counts.screenshots, 6);
    assert_eq!(output.inspect_report.counts.camera_records, 6);
    assert_eq!(output.inspect_report.counts.source_runs, 6);
    assert_eq!(output.inspect_report.counts.resource_pack_profiles, 3);
    assert!(output.inspect_report.anomalies.non_ingame_frame_indices.is_empty());
    assert!(output.inspect_report.anomalies.missing_screenshot_frame_indices.is_empty());
    assert!(output.inspect_report.anomalies.frames_without_file_resource_pack.is_empty());
    assert!(output.inspect_report.anomalies.frames_with_multiple_file_resource_packs.is_empty());
    assert_eq!(output.inspect_report.resource_pack_coverage.len(), 3);
    assert!(output.inspect_report.resource_pack_coverage.iter().all(|row| row.frame_count == 2));
    let cameras: Vec<ScenePacketCameraRecord> =
      serde_json::from_slice(&fs::read(&output.cameras_path).expect("cameras json should read")).expect("cameras json should parse");
    assert_eq!(cameras.len(), output.inspect_report.counts.camera_records);
    assert_eq!(output.manifest.frames[0].frame_index, 1);
    assert!(output.output_dir.join("frames/frame_000001.json").is_file());
  }

  #[test]
  fn missing_screenshot_artifact_ref_or_resolution_continues_into_anomaly() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      "bundle",
      "run_1",
      Vec::new(),
      vec![
        BundleFrameSpec {
          label: "missing-ref".to_string(),
          frame: frame("frame-1", 1_000, Some("in_game"), &["file/auv-mc6-rich"]),
          screenshot: ScreenshotDisposition::MissingRef,
        },
        BundleFrameSpec {
          label: "missing-artifact".to_string(),
          frame: frame("frame-2", 2_000, Some("in_game"), &["file/auv-mc6-rich"]),
          screenshot: ScreenshotDisposition::MissingArtifact,
        },
      ],
    );

    let output = export_3dgs_scene_packet(ScenePacketInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_dir: temp.path().join("scene-packet"),
    })
    .expect("scene packet export should succeed");

    assert_eq!(output.inspect_report.counts.missing_screenshots, 2);
    assert_eq!(output.inspect_report.anomalies.missing_screenshot_frame_indices, vec![1, 2]);
    assert_eq!(output.manifest.frames[0].frame_index, 1);
    assert!(output.output_dir.join("frames/frame_000001.json").is_file());
    assert_eq!(output.manifest.frames[1].frame_index, 2);
    assert!(output.output_dir.join("frames/frame_000002.json").is_file());
  }

  #[test]
  fn no_file_resource_pack_continues_with_anomaly_and_warning() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      "bundle",
      "run_1",
      Vec::new(),
      vec![BundleFrameSpec {
        label: "no-file-pack".to_string(),
        frame: frame("frame-1", 1_000, Some("in_game"), &["fabric"]),
        screenshot: ScreenshotDisposition::Present,
      }],
    );

    let output = export_3dgs_scene_packet(ScenePacketInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_dir: temp.path().join("scene-packet"),
    })
    .expect("scene packet export should succeed");

    assert_eq!(output.inspect_report.anomalies.frames_without_file_resource_pack, vec![1]);
    assert!(output.inspect_report.resource_pack_coverage.is_empty());
    assert!(output.inspect_report.warnings.iter().any(|warning| warning.contains("no file/* resource pack")));
  }

  #[test]
  fn multiple_file_resource_packs_continue_with_anomaly_and_warning() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      "bundle",
      "run_1",
      Vec::new(),
      vec![BundleFrameSpec {
        label: "multi-file-pack".to_string(),
        frame: frame("frame-1", 1_000, Some("in_game"), &["file/auv-a", "fabric", "file/auv-b"]),
        screenshot: ScreenshotDisposition::Present,
      }],
    );

    let output = export_3dgs_scene_packet(ScenePacketInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_dir: temp.path().join("scene-packet"),
    })
    .expect("scene packet export should succeed");

    assert_eq!(output.inspect_report.anomalies.frames_with_multiple_file_resource_packs, vec![1]);
    assert!(output.inspect_report.resource_pack_coverage.is_empty());
    assert!(output.inspect_report.warnings.iter().any(|warning| warning.contains("multiple file/* resource packs")));
  }

  #[test]
  fn non_ingame_screen_state_continues_with_anomaly_and_warning() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      "bundle",
      "run_1",
      Vec::new(),
      vec![BundleFrameSpec {
        label: "menu".to_string(),
        frame: frame("frame-1", 1_000, Some("menu"), &["file/auv-mc6-rich"]),
        screenshot: ScreenshotDisposition::Present,
      }],
    );

    let output = export_3dgs_scene_packet(ScenePacketInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_dir: temp.path().join("scene-packet"),
    })
    .expect("scene packet export should succeed");

    assert_eq!(output.inspect_report.anomalies.non_ingame_frame_indices, vec![1]);
    assert_eq!(output.inspect_report.resource_pack_coverage.len(), 1);
    assert_eq!(output.inspect_report.resource_pack_coverage[0].screen_states, vec!["menu".to_string()]);
    assert!(output.inspect_report.warnings.iter().any(|warning| warning.contains("non-ingame screen_state")));
  }

  #[test]
  fn resolved_screenshot_artifact_with_missing_file_is_hard_error() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_bundle(
      &temp,
      "bundle",
      "run_1",
      Vec::new(),
      vec![BundleFrameSpec {
        label: "missing-file".to_string(),
        frame: frame("frame-1", 1_000, Some("in_game"), &["file/auv-mc6-rich"]),
        screenshot: ScreenshotDisposition::MissingFile,
      }],
    );

    let error = export_3dgs_scene_packet(ScenePacketInputs {
      bundle_manifest_paths: vec![manifest_path],
      output_dir: temp.path().join("scene-packet"),
    })
    .expect_err("missing screenshot file should hard fail");

    assert!(error.contains("failed to copy MC-7 scene packet screenshot"));
  }
}
