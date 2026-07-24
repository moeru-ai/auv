use std::collections::BTreeMap;
use std::fs;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::{ImageFormat, ImageReader, Limits};

pub mod help;
pub mod projection_workflow;
pub mod query_live_action;

use auv_game_minecraft::dataset::{PROJECTION_BUNDLE_ROLE, SPATIAL_FRAME_BUNDLE_ROLE};
use auv_game_minecraft::{
  BundleArtifactId, MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, MinecraftArtifactReadError, MinecraftProjectionArtifact,
  MinecraftProjector, MinecraftSpatialFrame, ScenePacketInputs, ScenePacketOutput, SourceRunReference, SpatialBundleInputs,
  SpatialBundleSourceArtifact, TextureSweepInputs, TextureSweepPreparationInputs, TextureSweepPreparationOutput, TextureSweepReport,
  TextureSweepSampleBuildInputs, TextureSweepSampleBuildOutput, TextureSweepThresholds, build_texture_sweep_samples_from_bundles,
  evaluate_texture_sweep, export_3dgs_scene_packet, export_spatial_bundle, prepare_texture_sweep_resource_packs,
};
use auv_runtime::model::AuvResult;
use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, ArtifactUri, ByteLength, ContentType, Context, EventPayload, ReadArtifactError, RunId, RunSnapshot,
  RunStore, read_artifact_bytes,
};

pub const MINECRAFT_SPATIAL_BUNDLE_PURPOSE: &str = "auv.minecraft.spatial_bundle";

const MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT: u64 = 32 * 1024 * 1024;
const MINECRAFT_IMAGE_DECODE_ALLOCATION_LIMIT: u64 = 256 * 1024 * 1024;
const MINECRAFT_IMAGE_DIMENSION_LIMIT: u32 = 16_384;

#[derive(serde::Serialize)]
struct MinecraftArtifactPublicationFailed {
  artifact_purpose: &'static str,
  error: String,
}

impl EventPayload for MinecraftArtifactPublicationFailed {
  const NAME: &'static str = "auv.minecraft.artifact_publication_failed";
  const VERSION: u32 = 1;
}

fn keep_artifact_receipt<E: std::fmt::Display>(
  artifact_purpose: &'static str,
  result: Result<Option<ArtifactMetadata>, E>,
) -> Option<ArtifactMetadata> {
  match result {
    Ok(receipt) => receipt,
    Err(error) => {
      auv_tracing::emit_event!(MinecraftArtifactPublicationFailed {
        artifact_purpose,
        error: error.to_string(),
      });
      None
    }
  }
}

pub async fn run_minecraft_3dgs_scene_packet_export(
  bundle_manifest_paths: Vec<PathBuf>,
  output_dir: PathBuf,
) -> AuvResult<ScenePacketOutput> {
  let result = export_3dgs_scene_packet(ScenePacketInputs {
    bundle_manifest_paths,
    output_dir,
  })?;
  let context = Context::current();
  keep_artifact_receipt(
    auv_game_minecraft::scene_packet::MINECRAFT_SCENE_PACKET_PURPOSE,
    auv_game_minecraft::scene_packet::publish_minecraft_scene_packet(Some(&context), &result.manifest).await,
  );
  Ok(result)
}

pub async fn run_minecraft_texture_sweep_preparation(
  sidecar_run_dir: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<TextureSweepPreparationOutput> {
  prepare_texture_sweep_resource_packs(TextureSweepPreparationInputs {
    sidecar_run_dir,
    output_dir,
  })
}

pub async fn run_minecraft_texture_sweep_sample_build(
  bundle_manifest_paths: Vec<PathBuf>,
  output_path: PathBuf,
) -> AuvResult<TextureSweepSampleBuildOutput> {
  build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
    bundle_manifest_paths,
    output_path,
  })
}

pub async fn run_minecraft_spatial_bundle_export(
  store: Arc<dyn RunStore>,
  source_run_id: String,
  output_dir: PathBuf,
  // NOTICE(minecraft-bundle-exporter-commit): exporter commit metadata is not
  // canonical RunSnapshot provenance. Remove this argument when the command
  // frontend signature is in an owner-approved write slice.
  _git_commit: Option<String>,
) -> AuvResult<auv_game_minecraft::SpatialBundleOutput> {
  let source_run_id = source_run_id.parse::<RunId>().map_err(|error| format!("invalid Minecraft source run id: {error}"))?;
  let snapshot = store
    .load_snapshot(source_run_id)
    .await
    .map_err(|error| format!("failed to read Minecraft source run {source_run_id}: {error}"))?
    .ok_or_else(|| format!("Minecraft source run {source_run_id} was not found"))?;
  let staging = tempfile::Builder::new()
    .prefix("auv-minecraft-bundle-source-")
    .tempdir()
    .map_err(|error| format!("failed to create exclusive Minecraft bundle source staging directory: {error}"))?;
  let artifacts = read_spatial_bundle_artifacts(store.as_ref(), &snapshot)
    .await
    .and_then(|artifacts| stage_spatial_bundle_artifacts(artifacts, staging.path()));
  let result = match artifacts {
    Ok(artifacts) => export_spatial_bundle(SpatialBundleInputs {
      output_dir,
      source_run: source_run_reference(&snapshot),
      exported_at_millis: auv_runtime::model::now_millis(),
      artifacts,
    }),
    Err(error) => Err(error),
  };
  let cleanup = staging.close().map_err(|error| format!("failed to remove exclusive Minecraft bundle source staging directory: {error}"));
  let result = match (result, cleanup) {
    (Ok(result), Ok(())) => result,
    (Err(error), Ok(())) => return Err(error),
    (Ok(_), Err(cleanup_error)) => return Err(cleanup_error),
    (Err(error), Err(cleanup_error)) => return Err(format!("{error}; additionally, {cleanup_error}")),
  };
  projection_workflow::publish_json_artifact(MINECRAFT_SPATIAL_BUNDLE_PURPOSE, &result.manifest).await?;
  Ok(result)
}

enum ValidatedMinecraftBundleArtifact {
  Screenshot {
    source_uri: ArtifactUri,
    bundle_artifact_id: BundleArtifactId,
    bytes: Vec<u8>,
  },
  SpatialFrame {
    source_uri: ArtifactUri,
    bundle_artifact_id: BundleArtifactId,
    frame: Box<MinecraftSpatialFrame>,
    screenshot_bundle_artifact_id: Option<BundleArtifactId>,
  },
  Projection {
    source_uri: ArtifactUri,
    bundle_artifact_id: BundleArtifactId,
    projection: Box<MinecraftProjectionArtifact>,
  },
  Overlay {
    source_uri: ArtifactUri,
    bundle_artifact_id: BundleArtifactId,
    bytes: Vec<u8>,
  },
}

async fn read_spatial_bundle_artifacts(store: &dyn RunStore, snapshot: &RunSnapshot) -> AuvResult<Vec<ValidatedMinecraftBundleArtifact>> {
  validate_minecraft_bundle_snapshot_authority(store, snapshot)
    .map_err(|error| format!("failed to validate Minecraft bundle source snapshot: {}: {error}", error.code()))?;
  let mut artifacts = Vec::new();
  for published in snapshot.artifacts().values() {
    let metadata = published.metadata();
    let uri = metadata.uri();
    let bundle_artifact_id = BundleArtifactId::new(format!("bundle-{:06}", artifacts.len() + 1))?;
    let artifact = match metadata.purpose().as_str() {
      projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE => ValidatedMinecraftBundleArtifact::Screenshot {
        source_uri: uri.clone(),
        bundle_artifact_id,
        bytes: read_minecraft_screenshot(store, snapshot, uri)
          .await
          .map_err(|error| minecraft_bundle_read_error("screenshot", uri, error))?,
      },
      projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE => ValidatedMinecraftBundleArtifact::SpatialFrame {
        source_uri: uri.clone(),
        bundle_artifact_id,
        frame: Box::new(
          read_minecraft_spatial_frame(store, snapshot, uri)
            .await
            .map_err(|error| minecraft_bundle_read_error("spatial-frame", uri, error))?,
        ),
        screenshot_bundle_artifact_id: None,
      },
      auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE => ValidatedMinecraftBundleArtifact::Projection {
        source_uri: uri.clone(),
        bundle_artifact_id,
        projection: Box::new(
          auv_game_minecraft::artifact::read_minecraft_projection(store, snapshot, uri)
            .await
            .map_err(|error| minecraft_bundle_read_error("projection", uri, error))?,
        ),
      },
      projection_workflow::MINECRAFT_OVERLAY_PURPOSE => ValidatedMinecraftBundleArtifact::Overlay {
        source_uri: uri.clone(),
        bundle_artifact_id,
        bytes: read_minecraft_projection_overlay(store, snapshot, uri)
          .await
          .map_err(|error| minecraft_bundle_read_error("projection-overlay", uri, error))?,
      },
      _ => continue,
    };
    artifacts.push(artifact);
  }
  resolve_spatial_frame_screenshot_bundle_ids(store, snapshot, &mut artifacts).await?;
  Ok(artifacts)
}

async fn resolve_spatial_frame_screenshot_bundle_ids(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  artifacts: &mut [ValidatedMinecraftBundleArtifact],
) -> AuvResult<()> {
  let screenshot_ids = artifacts
    .iter()
    .filter_map(|artifact| match artifact {
      ValidatedMinecraftBundleArtifact::Screenshot {
        source_uri,
        bundle_artifact_id,
        ..
      } => Some((source_uri.clone(), bundle_artifact_id.clone())),
      _ => None,
    })
    .collect::<BTreeMap<_, _>>();

  for artifact in artifacts {
    let ValidatedMinecraftBundleArtifact::SpatialFrame {
      frame,
      screenshot_bundle_artifact_id,
      ..
    } = artifact
    else {
      continue;
    };
    let Some(reference) = frame.screenshot_artifact_ref.as_deref() else {
      continue;
    };
    let uri = reference
      .parse::<ArtifactUri>()
      .map_err(|error| format!("Minecraft spatial frame screenshot reference {reference:?} is not a canonical ArtifactUri: {error}"))?;
    let bundle_artifact_id = match screenshot_ids.get(&uri) {
      Some(bundle_artifact_id) => bundle_artifact_id,
      None => match read_minecraft_screenshot(store, snapshot, &uri).await {
        Err(error) => return Err(minecraft_bundle_read_error("referenced screenshot", &uri, error)),
        Ok(_) => {
          return Err(format!("validated Minecraft screenshot artifact {uri} was not assigned a bundle-local identity"));
        }
      },
    };
    *screenshot_bundle_artifact_id = Some(bundle_artifact_id.clone());
  }
  Ok(())
}

#[derive(Clone, Copy)]
struct SpatialBundleStagingSemantics {
  role: &'static str,
  file_name: &'static str,
}

fn stage_spatial_bundle_artifacts(
  artifacts: Vec<ValidatedMinecraftBundleArtifact>,
  staging_dir: &Path,
) -> AuvResult<Vec<SpatialBundleSourceArtifact>> {
  artifacts.into_iter().map(|artifact| stage_spatial_bundle_artifact(artifact, staging_dir)).collect()
}

fn stage_spatial_bundle_artifact(artifact: ValidatedMinecraftBundleArtifact, staging_dir: &Path) -> AuvResult<SpatialBundleSourceArtifact> {
  let (source_artifact_uri, bundle_artifact_id, screenshot_bundle_artifact_id, semantics, bytes) = match artifact {
    ValidatedMinecraftBundleArtifact::Screenshot {
      source_uri,
      bundle_artifact_id,
      bytes,
    } => (
      source_uri,
      bundle_artifact_id,
      None,
      SpatialBundleStagingSemantics {
        role: "minecraft-screenshot",
        file_name: "screenshot.png",
      },
      bytes,
    ),
    ValidatedMinecraftBundleArtifact::SpatialFrame {
      source_uri,
      bundle_artifact_id,
      frame,
      screenshot_bundle_artifact_id,
    } => (
      source_uri,
      bundle_artifact_id,
      screenshot_bundle_artifact_id,
      SpatialBundleStagingSemantics {
        role: SPATIAL_FRAME_BUNDLE_ROLE,
        file_name: "spatial-frame.json",
      },
      encode_bundle_json(frame.as_ref(), "spatial frame")?,
    ),
    ValidatedMinecraftBundleArtifact::Projection {
      source_uri,
      bundle_artifact_id,
      projection,
    } => (
      source_uri,
      bundle_artifact_id,
      None,
      SpatialBundleStagingSemantics {
        role: PROJECTION_BUNDLE_ROLE,
        file_name: "projection.json",
      },
      encode_bundle_json(projection.as_ref(), "projection")?,
    ),
    ValidatedMinecraftBundleArtifact::Overlay {
      source_uri,
      bundle_artifact_id,
      bytes,
    } => (
      source_uri,
      bundle_artifact_id,
      None,
      SpatialBundleStagingSemantics {
        role: "minecraft-overlay",
        file_name: "projection-overlay.png",
      },
      bytes,
    ),
  };
  let artifact_dir = staging_dir.join(bundle_artifact_id.as_str());
  fs::create_dir(&artifact_dir)
    .map_err(|error| format!("failed to exclusively create Minecraft bundle staging directory {}: {error}", artifact_dir.display()))?;
  let source_path = artifact_dir.join(semantics.file_name);
  let mut source = OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(&source_path)
    .map_err(|error| format!("failed to exclusively create Minecraft bundle staging input {}: {error}", source_path.display()))?;
  source.write_all(&bytes).map_err(|error| format!("failed to stage Minecraft bundle input at {}: {error}", source_path.display()))?;
  source.flush().map_err(|error| format!("failed to flush Minecraft bundle staging input {}: {error}", source_path.display()))?;

  Ok(SpatialBundleSourceArtifact {
    source_artifact_uri: source_artifact_uri.into(),
    bundle_artifact_id,
    role: semantics.role.to_string(),
    source_file: source_path,
    screenshot_bundle_artifact_id,
  })
}

fn encode_bundle_json(value: &impl serde::Serialize, kind: &str) -> AuvResult<Vec<u8>> {
  serialize_json_bounded(value, MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, &format!("validated Minecraft {kind} bundle input"))
}

async fn read_minecraft_screenshot(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<Vec<u8>, MinecraftArtifactReadError> {
  let bytes = read_minecraft_bundle_artifact_bytes(
    store,
    snapshot,
    uri,
    projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE,
    "image/png",
    MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT,
  )
  .await?;
  validate_minecraft_png(uri, &bytes, "screenshot")?;
  Ok(bytes)
}

async fn read_minecraft_spatial_frame(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<MinecraftSpatialFrame, MinecraftArtifactReadError> {
  let bytes = read_minecraft_bundle_artifact_bytes(
    store,
    snapshot,
    uri,
    projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE,
    "application/json",
    MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT,
  )
  .await?;
  let frame: MinecraftSpatialFrame = serde_json::from_slice(&bytes).map_err(|source| MinecraftArtifactReadError::MalformedJson {
    uri: uri.clone(),
    source,
  })?;
  MinecraftProjector::new(frame.clone()).map_err(|message| MinecraftArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message,
  })?;
  Ok(frame)
}

async fn read_minecraft_projection_overlay(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
) -> Result<Vec<u8>, MinecraftArtifactReadError> {
  let bytes = read_minecraft_bundle_artifact_bytes(
    store,
    snapshot,
    uri,
    projection_workflow::MINECRAFT_OVERLAY_PURPOSE,
    "image/png",
    MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT,
  )
  .await?;
  validate_minecraft_png(uri, &bytes, "projection-overlay")?;
  Ok(bytes)
}

// The canonical Minecraft error enum carries rich typed transport failures;
// preserving that contract here is more useful than hiding it behind a local error.
#[allow(clippy::result_large_err)]
fn validate_minecraft_png(uri: &ArtifactUri, bytes: &[u8], kind: &str) -> Result<(), MinecraftArtifactReadError> {
  let mut dimensions_reader = ImageReader::with_format(std::io::Cursor::new(bytes), ImageFormat::Png);
  dimensions_reader.limits(minecraft_image_decode_limits());
  let (width, height) = dimensions_reader.into_dimensions().map_err(|error| MinecraftArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message: format!("{kind} PNG payload dimensions violated decode limits: {error}"),
  })?;
  let decoded_byte_length =
    minecraft_decoded_image_buffer_length(width, height).map_err(|message| MinecraftArtifactReadError::InvalidPayload {
      uri: uri.clone(),
      message: format!("{kind} PNG payload {message}"),
    })?;
  validate_minecraft_image_buffer(width, height, decoded_byte_length, &format!("{kind} decoded PNG")).map_err(|message| {
    MinecraftArtifactReadError::InvalidPayload {
      uri: uri.clone(),
      message,
    }
  })?;

  let mut reader = ImageReader::with_format(std::io::Cursor::new(bytes), ImageFormat::Png);
  reader.limits(minecraft_image_decode_limits());
  reader.decode().map(|_| ()).map_err(|error| MinecraftArtifactReadError::InvalidPayload {
    uri: uri.clone(),
    message: format!("{kind} PNG payload violated decode limits or could not be decoded: {error}"),
  })
}

fn minecraft_image_decode_limits() -> Limits {
  let mut limits = Limits::default();
  limits.max_image_width = Some(MINECRAFT_IMAGE_DIMENSION_LIMIT);
  limits.max_image_height = Some(MINECRAFT_IMAGE_DIMENSION_LIMIT);
  limits.max_alloc = Some(MINECRAFT_IMAGE_DECODE_ALLOCATION_LIMIT);
  limits
}

fn validate_minecraft_image_buffer(width: u32, height: u32, byte_length: usize, label: &str) -> AuvResult<()> {
  if width > MINECRAFT_IMAGE_DIMENSION_LIMIT || height > MINECRAFT_IMAGE_DIMENSION_LIMIT {
    return Err(format!("{label} dimensions {width}x{height} exceed the {MINECRAFT_IMAGE_DIMENSION_LIMIT}-pixel per-axis limit"));
  }
  let byte_length = u64::try_from(byte_length).map_err(|_| format!("{label} byte length does not fit u64"))?;
  if byte_length > MINECRAFT_IMAGE_DECODE_ALLOCATION_LIMIT {
    return Err(format!("{label} buffer is {byte_length} bytes, exceeding the {MINECRAFT_IMAGE_DECODE_ALLOCATION_LIMIT}-byte limit"));
  }
  Ok(())
}

fn minecraft_decoded_image_buffer_length(width: u32, height: u32) -> AuvResult<usize> {
  let byte_length = u64::from(width)
    .checked_mul(u64::from(height))
    .and_then(|pixels| pixels.checked_mul(8))
    .ok_or_else(|| format!("decoded dimensions {width}x{height} overflow the image byte-length calculation"))?;
  usize::try_from(byte_length).map_err(|_| format!("decoded dimensions {width}x{height} do not fit this process"))
}

async fn read_minecraft_bundle_artifact_bytes(
  store: &dyn RunStore,
  snapshot: &RunSnapshot,
  uri: &ArtifactUri,
  expected_purpose: &'static str,
  expected_content_type: &'static str,
  byte_limit: u64,
) -> Result<Vec<u8>, MinecraftArtifactReadError> {
  let expected_purpose = ArtifactPurpose::parse(expected_purpose).map_err(|source| MinecraftArtifactReadError::InvalidExpectedPurpose {
    value: expected_purpose,
    source,
  })?;
  let expected_content_type =
    ContentType::parse(expected_content_type).expect("Minecraft bundle readers use compile-time validated content types");
  read_artifact_bytes(
    store,
    snapshot,
    uri,
    &expected_purpose,
    &expected_content_type,
    ByteLength::new(byte_limit).expect("Minecraft bundle reader limit must be non-zero"),
  )
  .await
  .map_err(Into::into)
}

fn serialize_json_bounded(value: &impl serde::Serialize, byte_limit: u64, label: &str) -> AuvResult<Vec<u8>> {
  let mut output = BoundedBytes::new(label, byte_limit);
  serde_json::to_writer_pretty(&mut output, value).map_err(|error| format!("failed to serialize {label}: {error}"))?;
  Ok(output.into_inner())
}

struct BoundedBytes {
  label: String,
  byte_limit: u64,
  bytes: Vec<u8>,
}

impl BoundedBytes {
  fn new(label: &str, byte_limit: u64) -> Self {
    Self {
      label: label.to_string(),
      byte_limit,
      bytes: Vec::new(),
    }
  }

  fn into_inner(self) -> Vec<u8> {
    self.bytes
  }
}

impl Write for BoundedBytes {
  fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
    let next_length =
      self.bytes.len().checked_add(buffer.len()).ok_or_else(|| std::io::Error::other(format!("{} length overflow", self.label)))?;
    let next_length = u64::try_from(next_length).map_err(|_| std::io::Error::other(format!("{} length does not fit u64", self.label)))?;
    if next_length > self.byte_limit {
      return Err(std::io::Error::other(format!("{} is {next_length} bytes, exceeding the {}-byte limit", self.label, self.byte_limit)));
    }
    self.bytes.try_reserve(buffer.len()).map_err(std::io::Error::other)?;
    self.bytes.extend_from_slice(buffer);
    Ok(buffer.len())
  }

  fn flush(&mut self) -> std::io::Result<()> {
    Ok(())
  }
}

#[allow(clippy::result_large_err)]
fn validate_minecraft_bundle_snapshot_authority(store: &dyn RunStore, snapshot: &RunSnapshot) -> Result<(), MinecraftArtifactReadError> {
  let store_authority = store.authority_id();
  if snapshot.authority_id() != store_authority {
    return Err(
      ReadArtifactError::SnapshotAuthorityMismatch {
        snapshot_authority: snapshot.authority_id(),
        store_authority,
      }
      .into(),
    );
  }
  Ok(())
}

fn minecraft_bundle_read_error(kind: &str, uri: &ArtifactUri, error: MinecraftArtifactReadError) -> String {
  format!("failed to read typed Minecraft {kind} artifact {uri}: {}: {error}", error.code())
}

fn source_run_reference(snapshot: &RunSnapshot) -> SourceRunReference {
  SourceRunReference {
    authority_id: snapshot.authority_id().into(),
    run_id: snapshot.run_id().into(),
    through_revision: snapshot.through_revision().into(),
  }
}

pub async fn run_minecraft_texture_sweep_eval(
  samples_path: PathBuf,
  output_dir: PathBuf,
  require_real_source: bool,
) -> AuvResult<TextureSweepReport> {
  evaluate_texture_sweep(&TextureSweepInputs {
    samples_path,
    output_dir,
    thresholds: TextureSweepThresholds::mc6_v0(),
    require_real_source,
  })
}

pub fn current_git_commit() -> Option<String> {
  let output = std::process::Command::new("git").args(["rev-parse", "HEAD"]).output().ok()?;
  if !output.status.success() {
    return None;
  }
  let commit = String::from_utf8(output.stdout).ok()?.trim().to_string();
  (!commit.is_empty()).then_some(commit)
}

pub fn read_spatial_bundle_manifest(path: PathBuf) -> AuvResult<auv_game_minecraft::SpatialBundleManifest> {
  let bytes = fs::read(&path).map_err(|error| format!("failed to read minecraft spatial bundle manifest {}: {error}", path.display()))?;
  serde_json::from_slice(&bytes).map_err(|error| format!("failed to parse minecraft spatial bundle manifest {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  use auv_tracing::{
    ArtifactBody, ArtifactId, ArtifactPurpose, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, ByteLength,
    CommitError, CommitResult, ContentType, IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage,
    RunCommitRequest, RunRevision, RunStore, RunSubscription, Sha256Digest, StoreArtifactRequest,
  };
  use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
  use sha2::{Digest, Sha256};

  use super::*;

  // ROOT CAUSE:
  //
  // Typed Minecraft publishers returned store errors after the domain operation
  // had already completed.
  //
  // Before the fix, a failed recording changed a completed operation into an
  // application failure and made a caller likely to retry its side effects.
  // The direct result is now independent from the recording route.
  #[tokio::test]
  async fn direct_texture_sweep_prep_returns_domain_output() {
    let root = std::env::temp_dir().join(format!("auv-minecraft-direct-{}", auv_tracing::RunId::new()));
    let sidecar_run_dir = root.join("sidecar");
    let output_dir = root.join("out");

    let result = run_minecraft_texture_sweep_preparation(sidecar_run_dir.clone(), output_dir.clone())
      .await
      .expect("direct preparation should return its domain output");

    assert_eq!(result.output_dir, output_dir);
    assert_eq!(result.manifest.sidecar_run_dir, sidecar_run_dir.to_string_lossy());
    assert!(result.manifest_path.is_file());
    assert!(result.runbook_path.is_file());
    fs::remove_dir_all(root).expect("remove direct preparation fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_malformed_spatial_frame_before_writing_bundle() {
    let root = bundle_test_root("malformed-frame");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE,
      "application/json",
      b"{not-json".to_vec(),
    )
    .await;

    let error = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("malformed spatial frame must fail export");

    assert!(error.contains("malformed"), "unexpected malformed-frame error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove malformed-frame fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_corrupt_screenshot_before_writing_bundle() {
    let root = bundle_test_root("corrupt-screenshot");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    write_source_artifact(store.as_ref(), run_id, projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE, "image/png", b"not-a-png".to_vec())
      .await;

    let error = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("corrupt screenshot must fail export");

    assert!(error.contains("PNG payload"), "unexpected corrupt-screenshot error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove corrupt-screenshot fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_wrong_minecraft_content_type_before_writing_bundle() {
    let root = bundle_test_root("wrong-content-type");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE,
      "application/json",
      png_bytes([8, 16, 32]),
    )
    .await;

    let error = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("wrong screenshot content type must fail export");

    assert!(error.contains("content type"), "unexpected wrong-content-type error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove wrong-content-type fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_digest_mismatch_before_writing_bundle() {
    let root = bundle_test_root("digest-mismatch");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let body = png_bytes([8, 16, 32]);
    let uri =
      write_source_artifact(store.as_ref(), run_id, projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE, "image/png", body.clone()).await;
    let mut corrupt_body = body;
    corrupt_body[0] ^= 1;
    let controlled = Arc::new(ControlledArtifactStore::new(store, uri, corrupt_body));

    let error = run_minecraft_spatial_bundle_export(controlled, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("digest mismatch must fail export");

    assert!(error.contains("digest mismatch"), "unexpected digest-mismatch error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove digest-mismatch fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_length_mismatch_before_writing_bundle() {
    let root = bundle_test_root("length-mismatch");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let body = png_bytes([8, 16, 32]);
    let uri =
      write_source_artifact(store.as_ref(), run_id, projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE, "image/png", body.clone()).await;
    let mut short_body = body;
    short_body.pop().expect("projection body should be non-empty");
    let controlled = Arc::new(ControlledArtifactStore::new(store, uri, short_body));

    let error = run_minecraft_spatial_bundle_export(controlled, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("length mismatch must fail export");

    assert!(error.contains("length mismatch"), "unexpected length-mismatch error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove length-mismatch fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_decodes_all_supported_artifacts_and_uses_bundle_local_semantics() {
    let root = bundle_test_root("multi-artifact");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let frame = bundle_test_frame();
    let projection = auv_game_minecraft::MinecraftProjectionArtifact::for_frame(&frame, None, None);
    write_source_artifact(store.as_ref(), run_id, projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE, "image/png", png_bytes([8, 16, 32]))
      .await;
    write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE,
      "application/json",
      serde_json::to_vec(&frame).expect("spatial frame should encode"),
    )
    .await;
    write_source_artifact(
      store.as_ref(),
      run_id,
      auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE,
      "application/json",
      serde_json::to_vec(&projection).expect("projection should encode"),
    )
    .await;
    write_source_artifact(store.as_ref(), run_id, projection_workflow::MINECRAFT_OVERLAY_PURPOSE, "image/png", png_bytes([64, 32, 16]))
      .await;

    let output = run_minecraft_spatial_bundle_export(store.clone(), run_id.to_string(), output_dir.clone(), None)
      .await
      .expect("valid typed artifacts should export");

    assert_eq!(output.manifest.counts.screenshots, 1);
    assert_eq!(output.manifest.counts.spatial_frames, 2);
    assert_eq!(output.manifest.counts.overlays, 1);
    assert_eq!(output.manifest.artifacts.len(), 4);
    assert_eq!(output.manifest.source_run.authority_id, store.authority_id().into());
    assert_eq!(output.manifest.source_run.run_id, run_id.into());
    for artifact in &output.manifest.artifacts {
      assert_eq!(artifact.source_artifact_uri.run_id(), run_id.to_string());
      assert!(!artifact.bundle_artifact_id.as_str().contains(&run_id.to_string()));
      assert!(output_dir.join(&artifact.bundle_path).is_file(), "bundle artifact was not written: {artifact:?}");
    }
    fs::remove_dir_all(root).expect("remove multi-artifact fixture");
  }

  // ROOT CAUSE:
  //
  // Canonical screenshot references and bundle-local IDs shared one string
  // field, so the exporter rewrote a valid ArtifactUri into a fabricated URI.
  //
  // Before the fix, typed lineage was lost at staging. The fix validates
  // canonical lineage and records the local screenshot relationship separately.
  #[tokio::test]
  async fn spatial_bundle_export_preserves_canonical_screenshot_reference_and_links_bundle_artifact() {
    let root = bundle_test_root("canonical-screenshot-reference");
    let output_dir = root.join("bundle");
    let scene_packet_dir = root.join("scene-packet");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let screenshot_uri =
      write_source_artifact(store.as_ref(), run_id, projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE, "image/png", png_bytes([8, 16, 32]))
        .await;
    let mut frame = bundle_test_frame();
    frame.screenshot_artifact_ref = Some(screenshot_uri.to_string());
    frame.screen_state = Some("in_game".to_string());
    frame.resource_pack_ids = vec!["file/test-pack".to_string()];
    write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE,
      "application/json",
      serde_json::to_vec(&frame).expect("spatial frame should encode"),
    )
    .await;

    let bundle = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect("canonical screenshot reference should export");
    let screenshot_record =
      bundle.manifest.artifacts.iter().find(|artifact| artifact.role == "minecraft-screenshot").expect("bundle screenshot record");
    let frame_record =
      bundle.manifest.artifacts.iter().find(|artifact| artifact.role == SPATIAL_FRAME_BUNDLE_ROLE).expect("bundle frame record");
    let staged_frame: MinecraftSpatialFrame =
      serde_json::from_slice(&fs::read(output_dir.join(&frame_record.bundle_path)).expect("read bundled frame"))
        .expect("decode bundled frame");
    let screenshot_uri_string = screenshot_uri.to_string();
    assert_eq!(staged_frame.screenshot_artifact_ref.as_deref(), Some(screenshot_uri_string.as_str()));
    assert_eq!(frame_record.screenshot_bundle_artifact_id.as_ref(), Some(&screenshot_record.bundle_artifact_id));
    assert_ne!(screenshot_record.bundle_artifact_id.as_str(), screenshot_uri.artifact_id().to_string());

    let scene_packet = run_minecraft_3dgs_scene_packet_export(vec![output_dir.join("run.json")], scene_packet_dir.clone())
      .await
      .expect("scene packet export");
    assert_eq!(scene_packet.manifest.counts.screenshots, 1);
    assert_eq!(scene_packet.manifest.counts.missing_screenshots, 0);
    assert!(scene_packet_dir.join(scene_packet.manifest.frames[0].screenshot_path.as_ref().expect("screenshot path")).is_file());
    fs::remove_dir_all(root).expect("remove canonical-screenshot-reference fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_dangling_canonical_screenshot_reference() {
    let root = bundle_test_root("dangling-screenshot-reference");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let mut frame = bundle_test_frame();
    frame.screenshot_artifact_ref = Some(ArtifactUri::from_ids(run_id, ArtifactId::new()).to_string());
    write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE,
      "application/json",
      serde_json::to_vec(&frame).expect("spatial frame should encode"),
    )
    .await;

    let error = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("dangling screenshot reference must fail export");

    assert!(error.contains("dangling"), "unexpected dangling screenshot error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove dangling-screenshot-reference fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_screenshot_reference_to_wrong_purpose() {
    let root = bundle_test_root("wrong-purpose-screenshot-reference");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let referenced_frame_uri = write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE,
      "application/json",
      serde_json::to_vec(&bundle_test_frame()).expect("referenced frame should encode"),
    )
    .await;
    let mut frame = bundle_test_frame();
    frame.spatial_frame_id = "frame-with-mismatched-screenshot".to_string();
    frame.screenshot_artifact_ref = Some(referenced_frame_uri.to_string());
    write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SPATIAL_FRAME_PURPOSE,
      "application/json",
      serde_json::to_vec(&frame).expect("spatial frame should encode"),
    )
    .await;

    let error = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("wrong-purpose screenshot reference must fail export");

    assert!(error.contains("wrong_purpose"), "unexpected mismatched screenshot error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove wrong-purpose-screenshot-reference fixture");
  }

  #[tokio::test]
  async fn spatial_bundle_export_rejects_png_dimensions_outside_decode_limit() {
    let root = bundle_test_root("oversized-png-dimensions");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    let image = RgbImage::from_pixel(20_000, 1, Rgb([8, 16, 32]));
    let mut encoded = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image).write_to(&mut encoded, ImageFormat::Png).expect("wide PNG should encode");
    write_source_artifact(store.as_ref(), run_id, projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE, "image/png", encoded.into_inner())
      .await;

    let error = run_minecraft_spatial_bundle_export(store, run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("out-of-policy image dimensions must fail export");

    assert!(error.contains("decode limits"), "unexpected image limit error: {error}");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove oversized-png-dimensions fixture");
  }

  // ROOT CAUSE:
  //
  // Committed image metadata previously had no byte budget, so export opened
  // and reserved for attacker-controlled payloads before validating size.
  //
  // Before the fix, oversized images reached the store stream. The fix rejects
  // committed lengths over policy before opening or allocating for the body.
  #[tokio::test]
  async fn spatial_bundle_export_rejects_oversized_committed_image_before_store_open() {
    const TEST_IMAGE_BYTE_LIMIT: usize = 32 * 1024 * 1024;

    let root = bundle_test_root("oversized-committed-image");
    let output_dir = root.join("bundle");
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let run_id = RunId::new();
    write_source_artifact(
      store.as_ref(),
      run_id,
      projection_workflow::MINECRAFT_SCREENSHOT_PURPOSE,
      "image/png",
      vec![0; TEST_IMAGE_BYTE_LIMIT + 1],
    )
    .await;
    let counting = Arc::new(CountingArtifactStore::new(store));

    let error = run_minecraft_spatial_bundle_export(counting.clone(), run_id.to_string(), output_dir.clone(), None)
      .await
      .expect_err("oversized committed image must fail export");

    assert!(error.contains("payload_too_large"), "unexpected oversized committed image error: {error}");
    assert_eq!(counting.open_count(), 0, "committed image limit must be checked before opening its stream");
    assert!(!output_dir.join("run.json").exists());
    fs::remove_dir_all(root).expect("remove oversized-committed-image fixture");
  }

  // ROOT CAUSE:
  //
  // Predictable staging paths were created permissively, so an existing
  // directory or symlink could redirect a later artifact write.
  //
  // Before the fix, staging reused attacker-controlled filesystem entries.
  // The fix requires exclusive creation for both the directory and the file.
  #[test]
  fn spatial_bundle_staging_rejects_precreated_artifact_directory() {
    let staging = tempfile::tempdir().expect("exclusive staging fixture");
    let artifact_dir = staging.path().join("bundle-000001");
    fs::create_dir(&artifact_dir).expect("precreate colliding artifact directory");
    let sentinel = artifact_dir.join("sentinel");
    fs::write(&sentinel, b"untouched").expect("write staging sentinel");

    let error =
      stage_spatial_bundle_artifact(staging_test_screenshot(), staging.path()).expect_err("precreated artifact directory must fail staging");

    assert!(error.contains("exclusively create"), "unexpected staging collision error: {error}");
    assert_eq!(fs::read(&sentinel).expect("read staging sentinel"), b"untouched");
    assert!(!artifact_dir.join("screenshot.png").exists());
  }

  #[cfg(unix)]
  #[test]
  fn spatial_bundle_staging_rejects_symlinked_artifact_directory() {
    let staging = tempfile::tempdir().expect("exclusive staging fixture");
    let redirected = tempfile::tempdir().expect("redirect target fixture");
    std::os::unix::fs::symlink(redirected.path(), staging.path().join("bundle-000001")).expect("create staging redirect");

    let error =
      stage_spatial_bundle_artifact(staging_test_screenshot(), staging.path()).expect_err("symlinked artifact directory must fail staging");

    assert!(error.contains("exclusively create"), "unexpected staging symlink error: {error}");
    assert!(!redirected.path().join("screenshot.png").exists());
  }

  fn staging_test_screenshot() -> ValidatedMinecraftBundleArtifact {
    ValidatedMinecraftBundleArtifact::Screenshot {
      source_uri: ArtifactUri::from_ids(RunId::new(), ArtifactId::new()),
      bundle_artifact_id: BundleArtifactId::new("bundle-000001").expect("valid bundle artifact id"),
      bytes: vec![1, 2, 3],
    }
  }

  fn bundle_test_root(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("auv-minecraft-bundle-{label}-{}", RunId::new()));
    fs::create_dir_all(&root).expect("bundle fixture root should write");
    root
  }

  fn bundle_test_frame() -> auv_game_minecraft::MinecraftSpatialFrame {
    auv_game_minecraft::MinecraftSpatialFrame {
      spatial_frame_id: "frame-bundle-test".to_string(),
      world_tick: 42,
      monotonic_timestamp_ms: 5_000,
      telemetry_session_id: None,
      viewport: auv_game_minecraft::Viewport::new(64, 64),
      view_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      player_pose: auv_game_minecraft::PlayerPose {
        eye_position: auv_game_minecraft::Vec3::new(0.0, 64.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: None,
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: None,
      resource_pack_ids: Vec::new(),
    }
  }

  fn png_bytes(color: [u8; 3]) -> Vec<u8> {
    let image = RgbImage::from_pixel(8, 8, Rgb(color));
    let mut output = std::io::Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image).write_to(&mut output, ImageFormat::Png).expect("PNG should encode");
    output.into_inner()
  }

  async fn write_source_artifact(store: &MemoryRunStore, run_id: RunId, purpose: &str, content_type: &str, body: Vec<u8>) -> ArtifactUri {
    let artifact_id = ArtifactId::new();
    let request = StoreArtifactRequest::new(
      store.authority_id(),
      run_id,
      IdempotencyKey::new(),
      artifact_id,
      None,
      ArtifactPurpose::parse(purpose).expect("artifact purpose"),
      ContentType::parse(content_type).expect("content type"),
      ByteLength::new(body.len() as u64).expect("byte length"),
      Sha256Digest::new(Sha256::digest(&body).into()),
      auv_tracing::Attributes::empty(),
    );
    store.write_artifact(request, Box::pin(futures_util::io::Cursor::new(body))).await.expect("source artifact should write");
    ArtifactUri::from_ids(run_id, artifact_id)
  }

  struct ControlledArtifactStore {
    inner: Arc<MemoryRunStore>,
    overridden_uri: ArtifactUri,
    body: Vec<u8>,
  }

  struct CountingArtifactStore {
    inner: Arc<MemoryRunStore>,
    opens: AtomicUsize,
  }

  impl CountingArtifactStore {
    fn new(inner: Arc<MemoryRunStore>) -> Self {
      Self {
        inner,
        opens: AtomicUsize::new(0),
      }
    }

    fn open_count(&self) -> usize {
      self.opens.load(Ordering::SeqCst)
    }
  }

  impl ControlledArtifactStore {
    fn new(inner: Arc<MemoryRunStore>, overridden_uri: ArtifactUri, body: Vec<u8>) -> Self {
      Self {
        inner,
        overridden_uri,
        body,
      }
    }
  }

  impl RunStore for ControlledArtifactStore {
    fn authority_id(&self) -> AuthorityId {
      self.inner.authority_id()
    }

    fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
      self.inner.commit(request)
    }

    fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
      self.inner.write_artifact(request, body)
    }

    fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
      self.inner.lookup_commit(run_id, key)
    }

    fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
      self.inner.load_snapshot(run_id)
    }

    fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
      self.inner.commits_after(run_id, after, limit)
    }

    fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
      self.inner.subscribe(run_id, after)
    }

    fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
      if uri != self.overridden_uri {
        return self.inner.open_artifact(uri);
      }
      let body = self.body.clone();
      Box::pin(async move { Ok(Box::pin(futures_util::stream::once(async move { Ok(body.into()) })) as ArtifactReader) })
    }
  }

  impl RunStore for CountingArtifactStore {
    fn authority_id(&self) -> AuthorityId {
      self.inner.authority_id()
    }

    fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
      self.inner.commit(request)
    }

    fn write_artifact(&self, request: StoreArtifactRequest, body: ArtifactBody) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
      self.inner.write_artifact(request, body)
    }

    fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
      self.inner.lookup_commit(run_id, key)
    }

    fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<RunSnapshot>, ReadError>> {
      self.inner.load_snapshot(run_id)
    }

    fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
      self.inner.commits_after(run_id, after, limit)
    }

    fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
      self.inner.subscribe(run_id, after)
    }

    fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
      self.opens.fetch_add(1, Ordering::SeqCst);
      self.inner.open_artifact(uri)
    }
  }
}
