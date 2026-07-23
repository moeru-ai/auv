//! Direct Minecraft projection workflows used by CLI and library frontends.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use auv_game_minecraft::evidence::{ProjectionEvidence, ScreenshotCapture, build_projection_evidence};
use auv_game_minecraft::{
  BlockPosition, MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, MinecraftBlockTarget, MinecraftProjectionArtifact, MinecraftSpatialFrame,
  MinecraftTargetSemantics, MismatchRefusalReason, TailFrameWaitConfig, bind_capture_to_frame, mc6_projection_target_for_frame,
};
use auv_runtime::contract::VerificationResult;
use auv_runtime::model::AuvResult;
use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, Attributes, ByteLength, ContentType, Context, EventPayload, NewArtifact, Sha256Digest,
};
use futures_util::io::Cursor as AsyncCursor;
use image::{DynamicImage, ExtendedColorType, ImageEncoder, ImageFormat, ImageReader, RgbImage, codecs::png::PngEncoder};
use sha2::{Digest, Sha256};

use super::query_live_action::DirectWindowPointClickExecutor;
use super::{
  BoundedBytes, MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT, minecraft_decoded_image_buffer_length, minecraft_image_decode_limits,
  serialize_json_bounded, validate_minecraft_image_buffer,
};

pub const MINECRAFT_SCREENSHOT_PURPOSE: &str = "auv.minecraft.screenshot";
pub const MINECRAFT_SPATIAL_FRAME_PURPOSE: &str = "auv.minecraft.spatial_frame";
pub const MINECRAFT_OVERLAY_PURPOSE: &str = "auv.minecraft.projection_overlay";
// TODO(minecraft-projection-calibration-reader-v1): add a typed canonical
// reader when calibration inspection becomes an owner-approved active slice.
pub const MINECRAFT_PROJECTION_CALIBRATION_PURPOSE: &str = "auv.minecraft.projection_calibration";

const LIVE_CLICK_POST_FRAME_WAIT: TailFrameWaitConfig = TailFrameWaitConfig::new(750, 25);

#[derive(Clone, Debug)]
/// Canonical publications required with artifact authority and absent otherwise.
pub struct MinecraftProjectionPublications {
  pub screenshot: Option<ArtifactMetadata>,
  pub spatial_frame: Option<ArtifactMetadata>,
  pub projection: Option<ArtifactMetadata>,
  pub overlay: Option<ArtifactMetadata>,
  pub calibration: Option<ArtifactMetadata>,
}

impl MinecraftProjectionPublications {
  pub fn artifacts(&self) -> impl Iterator<Item = &ArtifactMetadata> {
    [
      self.screenshot.as_ref(),
      self.spatial_frame.as_ref(),
      self.projection.as_ref(),
      self.overlay.as_ref(),
      self.calibration.as_ref(),
    ]
    .into_iter()
    .flatten()
  }
}

#[derive(Clone, Debug)]
/// Typed inputs for binding telemetry to a supplied or freshly captured image.
pub struct MinecraftProjectionBridgeInputs {
  pub telemetry_sample: PathBuf,
  pub screenshot: Option<PathBuf>,
  pub capture_target_app: Option<String>,
  pub capture_target_title: Option<String>,
  pub target_block: BlockPosition,
  pub capture_skew_ms: Option<i64>,
  pub screenshot_is_minecraft_window: bool,
}

#[derive(Clone, Debug)]
/// Direct projection evidence plus canonical publications when recording is available.
pub struct MinecraftProjectionBridgeOutput {
  pub evidence: ProjectionEvidence,
  pub publications: MinecraftProjectionPublications,
}

#[derive(Clone, Debug)]
/// Typed inputs for one offline projection calibration pass.
pub struct MinecraftProjectionCalibrationInputs {
  pub frame_path: PathBuf,
  pub screenshot: PathBuf,
  pub target_block: BlockPosition,
  pub target_semantics: MinecraftTargetSemantics,
  pub screenshot_is_minecraft_window: bool,
}

#[derive(Clone, Debug, serde::Serialize)]
/// Calibration summary retained as a direct value and optional run artifact.
pub struct MinecraftProjectionCalibrationArtifact {
  pub frame_id: String,
  pub target_block: String,
  pub target_semantics: String,
  pub raycast_hit_block_pos: Option<String>,
  pub raycast_hit_face: Option<String>,
  pub refusal_reason: Option<MismatchRefusalReason>,
  pub overlay_ref: Option<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug)]
/// Direct calibration evidence and summary.
pub struct MinecraftProjectionCalibrationOutput {
  pub evidence: ProjectionEvidence,
  pub calibration: MinecraftProjectionCalibrationArtifact,
  pub publications: MinecraftProjectionPublications,
}

#[derive(Clone, Debug)]
/// Typed inputs for projected live click and post-action world verification.
pub struct MinecraftLiveClickInputs {
  pub telemetry_sample: PathBuf,
  pub post_telemetry_sample: Option<PathBuf>,
  pub screenshot: PathBuf,
  pub target_block: BlockPosition,
  pub target_app: String,
  pub target_title: String,
  pub capture_skew_ms: Option<i64>,
  pub screenshot_is_minecraft_window: bool,
}

#[derive(Clone, Debug)]
/// Typed driver and verification results from one live-click attempt.
pub struct MinecraftLiveClickOutput {
  pub projection: MinecraftProjectionArtifact,
  pub input_action: auv_driver::InputActionResult,
  pub verification: VerificationResult,
  pub input_summary: String,
  pub publications: MinecraftProjectionPublications,
}

#[derive(serde::Serialize)]
struct MinecraftLiveClickVerificationEvent {
  verification: VerificationResult,
}

impl EventPayload for MinecraftLiveClickVerificationEvent {
  const NAME: &'static str = "auv.minecraft.live_click.verification";
  const VERSION: u32 = 1;
}

/// Binds one telemetry frame to an image and evaluates projection evidence.
pub async fn run_minecraft_projection_bridge(inputs: MinecraftProjectionBridgeInputs) -> AuvResult<MinecraftProjectionBridgeOutput> {
  let frame = auv_game_minecraft::read_latest_spatial_frame_from_tail(&inputs.telemetry_sample)?
    .ok_or_else(|| format!("no valid minecraft frame found in {}", inputs.telemetry_sample.display()))?;
  let screenshot = match inputs.screenshot.as_deref() {
    Some(path) => load_screenshot(path)?,
    None => capture_target_screenshot(
      inputs.capture_target_app.as_deref().ok_or_else(|| "bridge capture requires target app".to_string())?,
      inputs.capture_target_title.as_deref(),
    )?,
  };
  let target = mc6_projection_target_for_frame(inputs.target_block, &frame, MinecraftTargetSemantics::HitFaceCenter);
  let projected = project_capture(frame, screenshot, &target, inputs.capture_skew_ms, inputs.screenshot_is_minecraft_window).await?;
  Ok(MinecraftProjectionBridgeOutput {
    evidence: projected.evidence,
    publications: projected.publications,
  })
}

/// Evaluates one saved frame/image pair and returns its calibration summary.
pub async fn run_minecraft_calibrate_projection(
  inputs: MinecraftProjectionCalibrationInputs,
) -> AuvResult<MinecraftProjectionCalibrationOutput> {
  let frame = read_spatial_frame(&inputs.frame_path)?;
  let screenshot = load_screenshot(&inputs.screenshot)?;
  let target = mc6_projection_target_for_frame(inputs.target_block, &frame, inputs.target_semantics);
  let projected = project_capture(frame, screenshot, &target, Some(0), inputs.screenshot_is_minecraft_window).await?;
  let refusal_reason = match &projected.evidence {
    ProjectionEvidence::Bound { .. } => None,
    ProjectionEvidence::Refused { refusal, .. } => refusal.reason,
  };
  let frame = &projected.bound_frame;
  let calibration = MinecraftProjectionCalibrationArtifact {
    frame_id: frame.spatial_frame_id.clone(),
    target_block: format!("{},{},{}", inputs.target_block.x, inputs.target_block.y, inputs.target_block.z),
    target_semantics: match inputs.target_semantics {
      MinecraftTargetSemantics::HitFaceCenter => "hit_face_center",
      MinecraftTargetSemantics::BlockCenter => "block_center",
    }
    .to_string(),
    raycast_hit_block_pos: frame.raycast_hit.as_ref().map(|hit| format!("{},{},{}", hit.block_pos.x, hit.block_pos.y, hit.block_pos.z)),
    raycast_hit_face: frame.raycast_hit.as_ref().map(|hit| format!("{:?}", hit.face)),
    refusal_reason,
    overlay_ref: projected.publications.overlay.as_ref().map(|artifact| artifact.uri().to_string()),
    known_limits: vec![
      "geometry gate is visual-review driven; this artifact does not assert numeric pass/fail".to_string(),
      "MC-6 hit-face-center applies only when raycast_hit.block_pos matches target_block".to_string(),
    ],
  };
  let mut publications = projected.publications;
  publications.calibration = publish_json_artifact(MINECRAFT_PROJECTION_CALIBRATION_PURPOSE, &calibration).await?;
  Ok(MinecraftProjectionCalibrationOutput {
    evidence: projected.evidence,
    calibration,
    publications,
  })
}

/// Projects, dispatches one typed window click, and verifies the world delta.
pub async fn run_minecraft_live_click(inputs: MinecraftLiveClickInputs) -> AuvResult<MinecraftLiveClickOutput> {
  let pre_frame = auv_game_minecraft::read_latest_spatial_frame_from_tail(&inputs.telemetry_sample)?
    .ok_or_else(|| format!("no valid minecraft frame found in {}", inputs.telemetry_sample.display()))?;
  let projected = project_capture(
    pre_frame.clone(),
    load_screenshot(&inputs.screenshot)?,
    &MinecraftBlockTarget::new(inputs.target_block),
    inputs.capture_skew_ms,
    inputs.screenshot_is_minecraft_window,
  )
  .await?;
  let projection = projected.evidence.artifact().clone();
  let projected_point = match &projected.evidence {
    ProjectionEvidence::Bound { artifact, .. } => {
      artifact.projected_point.clone().ok_or_else(|| "minecraft live click bound projection is missing projected point".to_string())?
    }
    ProjectionEvidence::Refused { refusal, .. } => {
      return Err(format!("minecraft live click refused before input dispatch: {:?}", refusal.reason));
    }
  };
  let window_point = auv_game_minecraft::projected_window_point(&projected_point)
    .ok_or_else(|| "projected minecraft point is not window-clickable".to_string())?;
  let executor = DirectWindowPointClickExecutor::new(inputs.target_app, inputs.target_title);
  let (input_summary, input_action) = executor.click(window_point)?;
  let context = Context::current();
  let input_action_artifact = auv_runtime::run_read::publish_input_action_result(Some(&context), &input_action)
    .await
    .map_err(|error| format!("failed to publish Minecraft live-click input action result: {error}"))?;
  require_enabled_publication(&context, auv_runtime::run_read::INPUT_ACTION_RESULT_PURPOSE, input_action_artifact)?;

  let post_sample_path = inputs.post_telemetry_sample.as_deref().unwrap_or(&inputs.telemetry_sample);
  let post_frame = auv_game_minecraft::read_latest_spatial_frame_newer_than(
    post_sample_path,
    pre_frame.monotonic_timestamp_ms,
    LIVE_CLICK_POST_FRAME_WAIT,
  )?
  .ok_or_else(|| format!("no valid minecraft post frame found in {}", post_sample_path.display()))?;
  let post_frame_artifact = publish_json_artifact(MINECRAFT_SPATIAL_FRAME_PURPOSE, &post_frame).await?;
  let evidence =
    projected.publications.spatial_frame.iter().chain(post_frame_artifact.iter()).map(|artifact| artifact.uri().clone()).collect();
  let world_diff_request =
    auv_game_minecraft::verify::WorldDiffRequest::new(MinecraftBlockTarget::new(inputs.target_block)).allow_same_block_state_change();
  let verification = super::verification::map_world_diff_verdict_to_verification_result(
    &auv_game_minecraft::verify::evaluate_world_diff(&pre_frame, &post_frame, &world_diff_request),
    evidence,
  );
  context.in_scope(|| {
    auv_tracing::emit_event!(MinecraftLiveClickVerificationEvent {
      verification: verification.clone(),
    });
  });

  Ok(MinecraftLiveClickOutput {
    projection,
    input_action,
    verification,
    input_summary,
    publications: projected.publications,
  })
}

struct ProjectedCapture {
  bound_frame: MinecraftSpatialFrame,
  evidence: ProjectionEvidence,
  publications: MinecraftProjectionPublications,
}

async fn project_capture(
  frame: MinecraftSpatialFrame,
  screenshot: RgbImage,
  target: &MinecraftBlockTarget,
  capture_skew_ms: Option<i64>,
  screenshot_is_minecraft_window: bool,
) -> AuvResult<ProjectedCapture> {
  let screenshot_artifact = publish_png(MINECRAFT_SCREENSHOT_PURPOSE, &screenshot).await?;
  let screenshot_ref = screenshot_artifact.as_ref().map(|artifact| artifact.uri().to_string());
  // NOTICE(minecraft-optional-capture-ref): the domain evidence builder still
  // requires a String capture reference. Disabled and telemetry-only runs use
  // an empty transient value which is cleared before evidence can be returned
  // or published. Remove this adapter when ScreenshotCapture accepts Option.
  let evidence_ref = screenshot_ref.clone().unwrap_or_default();
  let capture_timestamp_ms = capture_timestamp(frame.monotonic_timestamp_ms, capture_skew_ms);
  let mut bound = bind_capture_to_frame(frame, evidence_ref.clone(), capture_timestamp_ms);
  let mut evidence = build_projection_evidence(
    bound.frame.clone(),
    ScreenshotCapture {
      screenshot_dimensions: Some((screenshot.width(), screenshot.height())),
      image: screenshot,
      artifact_ref: evidence_ref,
      capture_monotonic_timestamp_ms: capture_timestamp_ms,
      is_minecraft_window: screenshot_is_minecraft_window,
    },
    target,
    Some(250),
  )?;
  if screenshot_ref.is_none() {
    bound.frame.screenshot_artifact_ref = None;
    match &mut evidence {
      ProjectionEvidence::Bound { artifact, .. } | ProjectionEvidence::Refused { artifact, .. } => {
        artifact.screenshot_artifact_ref = None;
      }
    }
  }
  let spatial_frame = publish_json_artifact(MINECRAFT_SPATIAL_FRAME_PURPOSE, &bound.frame).await?;
  let context = Context::current();
  let projection = auv_game_minecraft::artifact::publish_minecraft_projection(Some(&context), evidence.artifact())
    .await
    .map_err(|error| format!("failed to publish Minecraft projection artifact: {error}"))?;
  let projection = require_enabled_publication(&context, auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE, projection)?;
  let overlay = match &evidence {
    ProjectionEvidence::Bound { overlay, .. } => publish_png(MINECRAFT_OVERLAY_PURPOSE, overlay).await?,
    ProjectionEvidence::Refused { .. } => None,
  };
  Ok(ProjectedCapture {
    bound_frame: bound.frame,
    evidence,
    publications: MinecraftProjectionPublications {
      screenshot: screenshot_artifact,
      spatial_frame,
      projection,
      overlay,
      calibration: None,
    },
  })
}

fn capture_timestamp(frame_timestamp_ms: u64, skew_ms: Option<i64>) -> u64 {
  match skew_ms {
    Some(skew) if skew >= 0 => frame_timestamp_ms.saturating_sub(skew as u64),
    Some(skew) => frame_timestamp_ms.saturating_add(skew.unsigned_abs()),
    None => frame_timestamp_ms,
  }
}

fn read_spatial_frame(path: &Path) -> AuvResult<MinecraftSpatialFrame> {
  let bytes = std::fs::read(path).map_err(|error| format!("failed to read minecraft spatial frame {}: {error}", path.display()))?;
  serde_json::from_slice(&bytes).map_err(|error| format!("failed to parse minecraft spatial frame {}: {error}", path.display()))
}

fn load_screenshot(path: &Path) -> AuvResult<RgbImage> {
  let metadata =
    std::fs::metadata(path).map_err(|error| format!("failed to inspect screenshot {} before opening: {error}", path.display()))?;
  if !metadata.is_file() {
    return Err(format!("screenshot {} is not a regular file", path.display()));
  }
  let expected_length = metadata.len();
  if expected_length > MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT {
    return Err(format!(
      "screenshot {} is {expected_length} bytes, exceeding the {MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT}-byte limit",
      path.display()
    ));
  }
  let expected_capacity =
    usize::try_from(expected_length).map_err(|_| format!("screenshot {} length does not fit this process", path.display()))?;
  let mut bytes = Vec::new();
  bytes.try_reserve_exact(expected_capacity).map_err(|error| format!("failed to reserve screenshot {} bytes: {error}", path.display()))?;
  let file = File::open(path).map_err(|error| format!("failed to open screenshot {}: {error}", path.display()))?;
  file
    .take(MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT + 1)
    .read_to_end(&mut bytes)
    .map_err(|error| format!("failed to read screenshot {}: {error}", path.display()))?;
  let actual_length = u64::try_from(bytes.len()).map_err(|_| format!("screenshot {} read length does not fit u64", path.display()))?;
  if actual_length > MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT {
    return Err(format!("screenshot {} exceeded the {MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT}-byte limit while reading", path.display()));
  }
  if actual_length != expected_length {
    return Err(format!("screenshot {} length changed while reading: expected {expected_length}, read {actual_length}", path.display()));
  }

  let mut dimensions_reader = ImageReader::with_format(std::io::Cursor::new(bytes.as_slice()), ImageFormat::Png);
  dimensions_reader.limits(minecraft_image_decode_limits());
  let (width, height) = dimensions_reader
    .into_dimensions()
    .map_err(|error| format!("failed to decode bounded PNG screenshot dimensions {}: {error}", path.display()))?;
  let decoded_byte_length = minecraft_decoded_image_buffer_length(width, height)?;
  validate_minecraft_image_buffer(width, height, decoded_byte_length, "decoded Minecraft screenshot")?;

  let mut reader = ImageReader::with_format(std::io::Cursor::new(bytes), ImageFormat::Png);
  reader.limits(minecraft_image_decode_limits());
  let image = reader.decode().map_err(|error| format!("failed to decode bounded PNG screenshot {}: {error}", path.display()))?;
  Ok(image.into_rgb8())
}

fn capture_target_screenshot(target_app: &str, target_title: Option<&str>) -> AuvResult<RgbImage> {
  let session = auv_driver::open_local().map_err(|error| error.to_string())?;
  let window = session
    .window()
    .resolve(auv_driver::WindowSelector {
      app: Some(auv_driver::App::bundle_id(target_app)),
      title: target_title.map(|title| auv_driver::TextMatcher::Contains(title.to_string())),
      main_visible: true,
    })
    .map_err(|error| error.to_string())?;
  let capture = session.window().capture(&window).map_err(|error| error.to_string())?;
  validate_minecraft_image_buffer(
    capture.image.width(),
    capture.image.height(),
    capture.image.as_raw().len(),
    "captured Minecraft screenshot",
  )?;
  Ok(DynamicImage::ImageRgba8(capture.image).into_rgb8())
}

pub(crate) async fn publish_json_artifact<T: serde::Serialize>(purpose: &'static str, value: &T) -> AuvResult<Option<ArtifactMetadata>> {
  let context = Context::current();
  if !context.can_publish_artifacts() {
    return Ok(None);
  }
  let bytes = serialize_json_bounded(value, MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, &format!("{purpose} artifact"))?;
  publish_bytes(&context, purpose, "application/json", bytes).await.map(Some)
}

async fn publish_png(purpose: &'static str, image: &RgbImage) -> AuvResult<Option<ArtifactMetadata>> {
  let context = Context::current();
  if !context.can_publish_artifacts() {
    return Ok(None);
  }
  validate_minecraft_image_buffer(image.width(), image.height(), image.as_raw().len(), purpose)?;
  let mut output = BoundedBytes::new(purpose, MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT);
  PngEncoder::new(&mut output)
    .write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgb8)
    .map_err(|error| format!("failed to encode {purpose} artifact: {error}"))?;
  publish_bytes(&context, purpose, "image/png", output.into_inner()).await.map(Some)
}

async fn publish_bytes(context: &Context, purpose: &'static str, content_type: &'static str, bytes: Vec<u8>) -> AuvResult<ArtifactMetadata> {
  let byte_length = u64::try_from(bytes.len()).map_err(|_| format!("{purpose} artifact length does not fit u64"))?;
  let artifact = NewArtifact::new(
    ArtifactPurpose::parse(purpose).map_err(|error| format!("invalid {purpose} artifact purpose: {error}"))?,
    ContentType::parse(content_type).map_err(|error| format!("invalid {purpose} artifact content type {content_type}: {error}"))?,
    ByteLength::new(byte_length).map_err(|error| format!("invalid {purpose} artifact byte length: {error}"))?,
    Sha256Digest::new(Sha256::digest(&bytes).into()),
    Attributes::empty(),
    AsyncCursor::new(bytes),
  );
  let published = context
    .in_scope(|| auv_tracing::emit_artifact!(artifact))
    .await
    .map_err(|error| format!("failed to publish {purpose} artifact: {error}"))?;
  published.ok_or_else(|| format!("enabled publication of {purpose} returned no artifact receipt"))
}

fn require_enabled_publication(
  context: &Context,
  purpose: &'static str,
  publication: Option<ArtifactMetadata>,
) -> AuvResult<Option<ArtifactMetadata>> {
  if context.can_publish_artifacts() && publication.is_none() {
    return Err(format!("enabled publication of {purpose} returned no artifact receipt"));
  }
  Ok(publication)
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::sync::atomic::{AtomicBool, Ordering};

  use auv_tracing::{
    ArtifactBody, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, CommitResult, ErrorCode,
    IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunSnapshot,
    RunStore, RunSubscription, StoreArtifactRequest, configure, dispatcher,
  };
  use image::{Rgb, RgbImage};
  use serde::Serialize;

  use super::*;

  // ROOT CAUSE:
  //
  // If an authority-backed artifact write failed, projection still returned
  // success because every publication failure was collapsed into None.
  //
  // Before the fix, callers could claim evidence that the run never recorded.
  // The fix propagates required publication failures from the workflow.
  #[tokio::test]
  async fn project_capture_propagates_enabled_screenshot_publication_failure() {
    let store = Arc::new(RejectArtifactStore::new());
    let dispatch = configure().run_store(store).build().expect("rejecting dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let target = MinecraftBlockTarget::new(BlockPosition::new(0, 0, 0));
    let future =
      root.in_scope(|| project_capture(projection_test_frame(), RgbImage::from_pixel(64, 64, Rgb([0, 0, 0])), &target, Some(0), true));

    let error = match root.instrument(future).await {
      Ok(_) => panic!("enabled screenshot publication failure must fail projection"),
      Err(error) => error,
    };

    assert!(error.contains("auv.test.minecraft_artifact_rejected"), "unexpected publication error: {error}");
  }

  // ROOT CAUSE:
  //
  // If artifact recording was disabled, projection invented an unrecorded URI
  // even though no artifact existed.
  //
  // Before the fix, optional telemetry looked like durable evidence. The fix
  // keeps the reference absent whenever no publication authority exists.
  #[tokio::test]
  async fn project_capture_without_artifact_authority_leaves_screenshot_reference_absent() {
    let projected = project_capture(
      projection_test_frame(),
      RgbImage::from_pixel(64, 64, Rgb([0, 0, 0])),
      &MinecraftBlockTarget::new(BlockPosition::new(0, 0, 0)),
      Some(0),
      true,
    )
    .await
    .expect("disabled recording must not change direct projection behavior");

    assert_eq!(projected.bound_frame.screenshot_artifact_ref, None);
    assert_eq!(projected.evidence.artifact().screenshot_artifact_ref, None);
    assert_eq!(projected.publications.artifacts().count(), 0);
  }

  #[tokio::test]
  async fn disabled_json_publication_does_not_serialize_the_direct_value() {
    let serialized = AtomicBool::new(false);

    drop(publish_json_artifact("auv.minecraft.test_probe", &SerializationProbe(&serialized)).await);

    assert!(!serialized.load(Ordering::SeqCst), "disabled publication must not inspect or serialize direct output");
  }

  #[tokio::test]
  async fn enabled_json_publication_rejects_payload_over_minecraft_limit() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let oversized =
      "x".repeat(usize::try_from(auv_game_minecraft::MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1).expect("test limit fits usize"));
    let future = root.in_scope(|| publish_json_artifact("auv.minecraft.test_oversized", &oversized));

    let error = root.instrument(future).await.expect_err("oversized enabled JSON publication must fail");

    assert!(error.contains("exceeding"), "unexpected oversized publication error: {error}");
  }

  #[tokio::test]
  async fn enabled_json_publication_propagates_serialization_failure() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let future = root.in_scope(|| publish_json_artifact("auv.minecraft.test_serialization_failure", &FailingSerializationProbe));

    let error = root.instrument(future).await.expect_err("enabled serialization failure must fail publication");

    assert!(error.contains("intentional serialization failure"), "unexpected serialization error: {error}");
  }

  struct SerializationProbe<'a>(&'a AtomicBool);

  impl Serialize for SerializationProbe<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
      S: serde::Serializer,
    {
      self.0.store(true, Ordering::SeqCst);
      serializer.serialize_str("serialized")
    }
  }

  struct FailingSerializationProbe;

  impl Serialize for FailingSerializationProbe {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
      S: serde::Serializer,
    {
      Err(<S::Error as serde::ser::Error>::custom("intentional serialization failure"))
    }
  }

  fn projection_test_frame() -> MinecraftSpatialFrame {
    MinecraftSpatialFrame {
      spatial_frame_id: "projection-test-frame".to_string(),
      world_tick: 1,
      monotonic_timestamp_ms: 1_000,
      telemetry_session_id: None,
      viewport: auv_game_minecraft::Viewport::new(64, 64),
      view_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      projection_matrix: [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0,
      ],
      player_pose: auv_game_minecraft::PlayerPose {
        eye_position: auv_game_minecraft::Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: None,
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: None,
      mc_capture_skew_ms: None,
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: vec!["file/test-pack".to_string()],
    }
  }

  struct RejectArtifactStore {
    inner: MemoryRunStore,
  }

  impl RejectArtifactStore {
    fn new() -> Self {
      Self {
        inner: MemoryRunStore::new(AuthorityId::new()),
      }
    }
  }

  impl RunStore for RejectArtifactStore {
    fn authority_id(&self) -> AuthorityId {
      self.inner.authority_id()
    }

    fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
      self.inner.commit(request)
    }

    fn write_artifact(
      &self,
      _request: StoreArtifactRequest,
      _body: ArtifactBody,
    ) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
      Box::pin(async {
        Err(ArtifactWriteError::Rejected(ErrorCode::parse("auv.test.minecraft_artifact_rejected").expect("test error code")))
      })
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
      self.inner.open_artifact(uri)
    }
  }
}
