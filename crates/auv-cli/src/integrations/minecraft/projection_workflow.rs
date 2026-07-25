//! Direct Minecraft projection workflows used by CLI and library frontends.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use auv_game_minecraft::evidence::{ProjectionEvidence, ScreenshotCapture, build_projection_evidence};
use auv_game_minecraft::{
  BlockPosition, MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, MinecraftBlockTarget, MinecraftProjectionArtifact, MinecraftSpatialFrame,
  MinecraftTargetSemantics, MismatchRefusalReason, TailFrameWaitConfig, bind_capture_to_frame, mc6_projection_target_for_frame,
};
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
  validate_minecraft_image_buffer,
};

pub const MINECRAFT_SCREENSHOT_PURPOSE: &str = "auv.minecraft.screenshot";
pub const MINECRAFT_SPATIAL_FRAME_PURPOSE: &str = "auv.minecraft.spatial_frame";
pub const MINECRAFT_OVERLAY_PURPOSE: &str = "auv.minecraft.projection_overlay";
// TODO(minecraft-projection-calibration-reader-v1): add a typed canonical
// reader when calibration inspection becomes an owner-approved active slice.
pub const MINECRAFT_PROJECTION_CALIBRATION_PURPOSE: &str = "auv.minecraft.projection_calibration";

const LIVE_CLICK_POST_FRAME_WAIT: TailFrameWaitConfig = TailFrameWaitConfig::new(750, 25);

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
/// Direct projection evidence.
pub struct MinecraftProjectionBridgeOutput {
  pub evidence: ProjectionEvidence,
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
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug)]
/// Direct calibration evidence and summary.
pub struct MinecraftProjectionCalibrationOutput {
  pub evidence: ProjectionEvidence,
  pub calibration: MinecraftProjectionCalibrationArtifact,
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
  pub verification: auv_game_minecraft::WorldDiffVerdict,
}

#[derive(serde::Serialize)]
struct MinecraftLiveClickVerificationEvent {
  verdict: auv_game_minecraft::WorldDiffVerdict,
  evidence: Vec<auv_tracing::ArtifactUri>,
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
    known_limits: vec![
      "geometry gate is visual-review driven; this artifact does not assert numeric pass/fail".to_string(),
      "MC-6 hit-face-center applies only when raycast_hit.block_pos matches target_block".to_string(),
    ],
  };
  drop(publish_json_artifact(MINECRAFT_PROJECTION_CALIBRATION_PURPOSE, &calibration).await?);
  Ok(MinecraftProjectionCalibrationOutput {
    evidence: projected.evidence,
    calibration,
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
  let input_action = executor.click(window_point)?;
  let context = Context::current();
  super::keep_artifact_receipt(
    auv_runtime::run_read::INPUT_ACTION_RESULT_PURPOSE,
    auv_runtime::run_read::publish_input_action_result(Some(&context), &input_action).await,
  );

  let post_sample_path = inputs.post_telemetry_sample.as_deref().unwrap_or(&inputs.telemetry_sample);
  let post_frame = auv_game_minecraft::read_latest_spatial_frame_newer_than(
    post_sample_path,
    pre_frame.monotonic_timestamp_ms,
    LIVE_CLICK_POST_FRAME_WAIT,
  )?
  .ok_or_else(|| format!("no valid minecraft post frame found in {}", post_sample_path.display()))?;
  let post_frame_artifact = publish_json_artifact(MINECRAFT_SPATIAL_FRAME_PURPOSE, &post_frame).await?;
  let evidence = projected.recorded_spatial_frame.iter().chain(post_frame_artifact.iter()).map(|artifact| artifact.uri().clone()).collect();
  let world_diff_request =
    auv_game_minecraft::verify::WorldDiffRequest::new(MinecraftBlockTarget::new(inputs.target_block)).allow_same_block_state_change();
  let verification = auv_game_minecraft::verify::evaluate_world_diff(&pre_frame, &post_frame, &world_diff_request);
  context.in_scope(|| {
    auv_tracing::emit_event!(MinecraftLiveClickVerificationEvent {
      verdict: verification.clone(),
      evidence,
    });
  });

  Ok(MinecraftLiveClickOutput {
    projection,
    input_action,
    verification,
  })
}

struct ProjectedCapture {
  bound_frame: MinecraftSpatialFrame,
  evidence: ProjectionEvidence,
  recorded_spatial_frame: Option<ArtifactMetadata>,
}

async fn project_capture(
  frame: MinecraftSpatialFrame,
  screenshot: RgbImage,
  target: &MinecraftBlockTarget,
  capture_skew_ms: Option<i64>,
  screenshot_is_minecraft_window: bool,
) -> AuvResult<ProjectedCapture> {
  let capture_timestamp_ms = capture_timestamp(frame.monotonic_timestamp_ms, capture_skew_ms);
  let bound = bind_capture_to_frame(frame.clone(), None, capture_timestamp_ms);
  let evidence = build_projection_evidence(
    frame,
    ScreenshotCapture {
      screenshot_dimensions: Some((screenshot.width(), screenshot.height())),
      image: screenshot.clone(),
      artifact_ref: None,
      capture_monotonic_timestamp_ms: capture_timestamp_ms,
      is_minecraft_window: screenshot_is_minecraft_window,
    },
    target,
    Some(250),
  )?;

  let screenshot_artifact = publish_png(MINECRAFT_SCREENSHOT_PURPOSE, &screenshot).await?;
  let mut recorded_frame = bound.frame.clone();
  let mut recorded_evidence = evidence.clone();
  if let Some(screenshot_uri) = screenshot_artifact.as_ref().map(|artifact| artifact.uri().to_string()) {
    recorded_frame.screenshot_artifact_ref = Some(screenshot_uri.clone());
    match &mut recorded_evidence {
      ProjectionEvidence::Bound { artifact, .. } | ProjectionEvidence::Refused { artifact, .. } => {
        artifact.screenshot_artifact_ref = Some(screenshot_uri);
      }
    }
  }
  let recorded_spatial_frame = publish_json_artifact(MINECRAFT_SPATIAL_FRAME_PURPOSE, &recorded_frame).await?;
  if screenshot_artifact.is_some() {
    let context = Context::current();
    super::keep_artifact_receipt(
      auv_game_minecraft::artifact::MINECRAFT_PROJECTION_PURPOSE,
      auv_game_minecraft::artifact::publish_minecraft_projection(Some(&context), recorded_evidence.artifact()).await,
    );
  }
  if let ProjectionEvidence::Bound { overlay, .. } = &evidence {
    drop(publish_png(MINECRAFT_OVERLAY_PURPOSE, overlay).await?);
  }
  Ok(ProjectedCapture {
    bound_frame: bound.frame,
    evidence,
    recorded_spatial_frame,
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
  let purpose_value = match ArtifactPurpose::parse(purpose) {
    Ok(purpose) => purpose,
    Err(error) => {
      return Ok(super::keep_artifact_receipt::<String>(purpose, Err(format!("invalid artifact purpose: {error}"))));
    }
  };
  let emission = match auv_tracing::emit_json_artifact(
    purpose_value,
    Attributes::empty(),
    ByteLength::new(MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT).expect("static Minecraft JSON limit is valid"),
    value,
  ) {
    Ok(artifact) => artifact,
    Err(error) => {
      return Ok(super::keep_artifact_receipt::<String>(purpose, Err(format!("failed to construct artifact: {error}"))));
    }
  };
  Ok(super::keep_artifact_receipt(purpose, emission.await))
}

async fn publish_png(purpose: &'static str, image: &RgbImage) -> AuvResult<Option<ArtifactMetadata>> {
  let context = Context::current();
  if !context.can_publish_artifacts() {
    return Ok(None);
  }
  if let Err(error) = validate_minecraft_image_buffer(image.width(), image.height(), image.as_raw().len(), purpose) {
    return Ok(super::keep_artifact_receipt::<String>(purpose, Err(error)));
  }
  let mut output = BoundedBytes::new(purpose, MINECRAFT_IMAGE_ARTIFACT_BYTE_LIMIT);
  if let Err(error) = PngEncoder::new(&mut output).write_image(image.as_raw(), image.width(), image.height(), ExtendedColorType::Rgb8) {
    return Ok(super::keep_artifact_receipt::<String>(purpose, Err(format!("failed to encode artifact: {error}"))));
  }
  Ok(publish_bytes(&context, purpose, "image/png", output.into_inner()).await)
}

async fn publish_bytes(context: &Context, purpose: &'static str, content_type: &'static str, bytes: Vec<u8>) -> Option<ArtifactMetadata> {
  let byte_length = match u64::try_from(bytes.len()) {
    Ok(byte_length) => byte_length,
    Err(_) => return super::keep_artifact_receipt::<String>(purpose, Err("artifact length does not fit u64".to_string())),
  };
  let purpose_value = match ArtifactPurpose::parse(purpose) {
    Ok(purpose) => purpose,
    Err(error) => return super::keep_artifact_receipt::<String>(purpose, Err(format!("invalid artifact purpose: {error}"))),
  };
  let content_type_value = match ContentType::parse(content_type) {
    Ok(content_type) => content_type,
    Err(error) => {
      return super::keep_artifact_receipt::<String>(purpose, Err(format!("invalid artifact content type {content_type}: {error}")));
    }
  };
  let byte_length = match ByteLength::new(byte_length) {
    Ok(byte_length) => byte_length,
    Err(error) => return super::keep_artifact_receipt::<String>(purpose, Err(format!("invalid artifact byte length: {error}"))),
  };
  let artifact = NewArtifact::new(
    purpose_value,
    content_type_value,
    byte_length,
    Sha256Digest::new(Sha256::digest(&bytes).into()),
    Attributes::empty(),
    AsyncCursor::new(bytes),
  );
  super::keep_artifact_receipt(purpose, context.in_scope(|| auv_tracing::emit_artifact!(artifact)).await)
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

  #[tokio::test]
  async fn artifact_authority_does_not_change_direct_projection_evidence() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let target = MinecraftBlockTarget::new(BlockPosition::new(0, 0, 0));
    let future =
      root.in_scope(|| project_capture(projection_test_frame(), RgbImage::from_pixel(64, 64, Rgb([0, 0, 0])), &target, Some(0), true));

    let projected = root.instrument(future).await.expect("projection");

    assert_eq!(projected.bound_frame.screenshot_artifact_ref, None);
    assert_eq!(projected.evidence.artifact().screenshot_artifact_ref, None);
    let snapshot = store.load_snapshot(run_id).await.expect("load run").expect("recorded run");
    assert!(snapshot.artifacts().len() >= 3, "recording should remain available independently from the direct evidence");
  }

  // ROOT CAUSE:
  //
  // An authority-backed artifact write failed after projection had already
  // produced its direct domain value.
  //
  // Before the fix, the recording failure replaced that direct result. The
  // projection now survives with every unavailable artifact reference absent.
  #[tokio::test]
  async fn project_capture_preserves_direct_projection_when_store_rejects_recording() {
    let store = Arc::new(RejectArtifactStore::new());
    let dispatch = configure().run_store(store).build().expect("rejecting dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let target = MinecraftBlockTarget::new(BlockPosition::new(0, 0, 0));
    let future =
      root.in_scope(|| project_capture(projection_test_frame(), RgbImage::from_pixel(64, 64, Rgb([0, 0, 0])), &target, Some(0), true));

    let projected = root.instrument(future).await.expect("projection result must survive recording failure");

    assert_eq!(projected.bound_frame.screenshot_artifact_ref, None);
    assert_eq!(projected.evidence.artifact().screenshot_artifact_ref, None);
    assert!(projected.recorded_spatial_frame.is_none());
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
    assert!(projected.recorded_spatial_frame.is_none());
  }

  #[tokio::test]
  async fn disabled_json_publication_does_not_serialize_the_direct_value() {
    let serialized = AtomicBool::new(false);

    drop(publish_json_artifact("auv.minecraft.test_probe", &SerializationProbe(&serialized)).await);

    assert!(!serialized.load(Ordering::SeqCst), "disabled publication must not inspect or serialize direct output");
  }

  #[tokio::test]
  async fn enabled_json_publication_drops_oversized_recording_without_failing_the_caller() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let oversized =
      "x".repeat(usize::try_from(auv_game_minecraft::MINECRAFT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT + 1).expect("test limit fits usize"));
    let future = root.in_scope(|| publish_json_artifact("auv.minecraft.test_oversized", &oversized));

    assert!(root.instrument(future).await.expect("recording preparation failure is not a domain failure").is_none());
  }

  #[tokio::test]
  async fn enabled_json_publication_drops_serialization_failure_without_failing_the_caller() {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store).build().expect("memory dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let future = root.in_scope(|| publish_json_artifact("auv.minecraft.test_serialization_failure", &FailingSerializationProbe));

    assert!(root.instrument(future).await.expect("recording serialization failure is not a domain failure").is_none());
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
