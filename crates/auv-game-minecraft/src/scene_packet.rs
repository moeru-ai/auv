use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::{BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

#[cfg(feature = "tracing")]
use auv_tracing::{ArtifactMetadata, Context};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::dataset::{BundleArtifactId, SPATIAL_FRAME_BUNDLE_ROLE, SpatialBundleDirectory, SpatialBundleManifest};
use crate::types::{MinecraftSpatialFrame, PlayerPose, RaycastHit, Viewport};

pub type ScenePacketResult<T> = Result<T, String>;

pub const SCENE_PACKET_SCHEMA_VERSION: u32 = 1;
pub const SCENE_PACKET_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
pub const MINECRAFT_SCENE_PACKET_PURPOSE: &str = "auv.minecraft.scene_packet";

#[cfg(feature = "tracing")]
pub async fn publish_minecraft_scene_packet(
  context: Option<&Context>,
  packet: &ScenePacketManifest,
) -> Result<Option<ArtifactMetadata>, crate::run_read::MinecraftArtifactPublishError> {
  crate::run_read::publish_json_artifact(context, MINECRAFT_SCENE_PACKET_PURPOSE, packet, validate_scene_packet_payload).await
}

#[cfg(feature = "tracing")]
fn validate_scene_packet_payload(packet: &ScenePacketManifest) -> Result<(), String> {
  if packet.schema_version != SCENE_PACKET_SCHEMA_VERSION {
    return Err(format!(
      "unsupported Minecraft scene packet schema_version {} (expected {SCENE_PACKET_SCHEMA_VERSION})",
      packet.schema_version
    ));
  }
  // TODO(minecraft-scene-packet-invariants): Add cross-field checks when the
  // owning manifest contract declares invariants beyond schema_version.
  Ok(())
}

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
  pub source_frame_bundle_artifact_id: BundleArtifactId,
  pub source_frame_bundle_path: PathBuf,
  pub frame_json_path: String,
  #[serde(default)]
  pub screenshot_bundle_artifact_id: Option<BundleArtifactId>,
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
  pub source_frame_bundle_artifact_id: BundleArtifactId,
  pub source_frame_bundle_path: PathBuf,
  #[serde(default)]
  pub screenshot_bundle_artifact_id: Option<BundleArtifactId>,
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
  pub source_frame_bundle_artifact_id: &'a BundleArtifactId,
  pub source_frame_bundle_path: &'a Path,
  #[serde(default)]
  pub screenshot_bundle_artifact_id: Option<&'a BundleArtifactId>,
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
    let source_run_id = manifest.source_run.run_id.to_string();
    source_run_ids.insert(source_run_id.clone());
    known_limits.extend(manifest.known_limits.iter().cloned());

    let screenshots = manifest
      .artifacts
      .iter()
      .filter(|artifact| artifact.directory == SpatialBundleDirectory::Screenshots)
      .map(|artifact| (artifact.bundle_artifact_id.clone(), artifact.clone()))
      .collect::<BTreeMap<_, _>>();

    for artifact in &manifest.artifacts {
      if artifact.directory != SpatialBundleDirectory::SpatialFrames || artifact.role != SPATIAL_FRAME_BUNDLE_ROLE {
        continue;
      }

      let frame_index = frames.len() + 1;
      let frame_source_path = bundle_dir.join(&artifact.bundle_path);
      let spatial_frame = read_frame(&frame_source_path)?;
      let frame_json_path = format!("frames/frame_{frame_index:06}.json");
      let screenshot = artifact.screenshot_bundle_artifact_id.as_ref().and_then(|artifact_id| screenshots.get(artifact_id));
      let (screenshot_bundle_artifact_id, screenshot_path) = if let Some(screenshot) = screenshot {
        let path = format!("frames/frame_{frame_index:06}.{}", extension_for(&screenshot.bundle_path));
        copy_file(&bundle_dir.join(&screenshot.bundle_path), &inputs.output_dir.join(&path), "MC-7 scene packet screenshot")?;
        screenshot_count += 1;
        (Some(screenshot.bundle_artifact_id.clone()), Some(path))
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
          source_run_id,
          screen_state.as_deref().unwrap_or("missing")
        ));
      }

      let file_resource_packs =
        spatial_frame.resource_pack_ids.iter().filter(|resource_pack_id| resource_pack_id.starts_with("file/")).cloned().collect::<Vec<_>>();
      match file_resource_packs.len() {
        0 => {
          anomalies.frames_without_file_resource_pack.push(frame_index);
          warnings.insert(format!("frame {frame_index} from source run {source_run_id} had no file/* resource pack"));
        }
        1 => {
          let coverage =
            resource_pack_coverage.entry(file_resource_packs[0].clone()).or_insert_with(ResourcePackCoverageAccumulator::default);
          coverage.frame_count += 1;
          coverage.source_run_ids.insert(source_run_id.clone());
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
            source_run_id,
            file_resource_packs.join(",")
          ));
        }
      }

      let source_bundle_manifest_path = manifest_path.to_string_lossy().into_owned();
      let payload = ScenePacketFramePayloadRef {
        frame_index,
        source_run_id: source_run_id.as_str(),
        source_bundle_manifest_path: source_bundle_manifest_path.as_str(),
        source_frame_bundle_artifact_id: &artifact.bundle_artifact_id,
        source_frame_bundle_path: &artifact.bundle_path,
        screenshot_bundle_artifact_id: screenshot_bundle_artifact_id.as_ref(),
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
        source_run_id: source_run_id.clone(),
        source_bundle_manifest_path,
        source_frame_bundle_artifact_id: artifact.bundle_artifact_id.clone(),
        source_frame_bundle_path: artifact.bundle_path.clone(),
        frame_json_path,
        screenshot_bundle_artifact_id,
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
    generated_at_millis: crate::now_millis(),
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

fn extension_for(path: &Path) -> String {
  path.extension().and_then(|extension| extension.to_str()).filter(|extension| !extension.trim().is_empty()).unwrap_or("png").to_string()
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
