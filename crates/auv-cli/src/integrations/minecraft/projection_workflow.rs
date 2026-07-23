//! Direct Minecraft projection workflows used by CLI and library frontends.

use std::path::{Path, PathBuf};

use auv_game_minecraft::evidence::{ProjectionEvidence, ScreenshotCapture, build_projection_evidence};
use auv_game_minecraft::{
  BlockPosition, MinecraftBlockTarget, MinecraftProjectionArtifact, MinecraftSpatialFrame, MinecraftTargetSemantics, MismatchRefusalReason,
  TailFrameWaitConfig, bind_capture_to_frame, mc6_projection_target_for_frame,
};
use auv_runtime::contract::VerificationResult;
use auv_runtime::model::AuvResult;
use auv_tracing::{
  ArtifactMetadata, ArtifactPurpose, Attributes, ByteLength, ContentType, Context, EventPayload, NewArtifact, Sha256Digest,
};
use futures_util::io::Cursor as AsyncCursor;
use image::{DynamicImage, ImageFormat, ImageReader, RgbImage};
use sha2::{Digest, Sha256};

use super::query_live_action::DirectWindowPointClickExecutor;

pub const MINECRAFT_SCREENSHOT_PURPOSE: &str = "auv.minecraft.screenshot";
pub const MINECRAFT_SPATIAL_FRAME_PURPOSE: &str = "auv.minecraft.spatial_frame";
pub const MINECRAFT_OVERLAY_PURPOSE: &str = "auv.minecraft.projection_overlay";
// TODO(minecraft-projection-calibration-reader-v1): add a typed canonical
// reader when calibration inspection becomes an owner-approved active slice.
pub const MINECRAFT_PROJECTION_CALIBRATION_PURPOSE: &str = "auv.minecraft.projection_calibration";

const LIVE_CLICK_POST_FRAME_WAIT: TailFrameWaitConfig = TailFrameWaitConfig::new(750, 25);

#[derive(Clone, Debug)]
/// Best-effort canonical publications produced without changing direct results.
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
/// Direct projection evidence plus any canonical publications that succeeded.
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
  publications.calibration = publish_json_artifact(MINECRAFT_PROJECTION_CALIBRATION_PURPOSE, &calibration).await;
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
  let _ = auv_runtime::run_read::publish_input_action_result(Some(&context), &input_action).await;

  let post_sample_path = inputs.post_telemetry_sample.as_deref().unwrap_or(&inputs.telemetry_sample);
  let post_frame = auv_game_minecraft::read_latest_spatial_frame_newer_than(
    post_sample_path,
    pre_frame.monotonic_timestamp_ms,
    LIVE_CLICK_POST_FRAME_WAIT,
  )?
  .ok_or_else(|| format!("no valid minecraft post frame found in {}", post_sample_path.display()))?;
  let post_frame_artifact = publish_json_artifact(MINECRAFT_SPATIAL_FRAME_PURPOSE, &post_frame).await;
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
  let screenshot_artifact = publish_png(MINECRAFT_SCREENSHOT_PURPOSE, &screenshot).await;
  let screenshot_ref =
    screenshot_artifact.as_ref().map(|artifact| artifact.uri().to_string()).unwrap_or_else(|| "unrecorded:minecraft-screenshot".to_string());
  let capture_timestamp_ms = capture_timestamp(frame.monotonic_timestamp_ms, capture_skew_ms);
  let bound = bind_capture_to_frame(frame, screenshot_ref.clone(), capture_timestamp_ms);
  let evidence = build_projection_evidence(
    bound.frame.clone(),
    ScreenshotCapture {
      screenshot_dimensions: Some((screenshot.width(), screenshot.height())),
      image: screenshot,
      artifact_ref: screenshot_ref,
      capture_monotonic_timestamp_ms: capture_timestamp_ms,
      is_minecraft_window: screenshot_is_minecraft_window,
    },
    target,
    Some(250),
  )?;
  let spatial_frame = publish_json_artifact(MINECRAFT_SPATIAL_FRAME_PURPOSE, &bound.frame).await;
  let context = Context::current();
  let projection = auv_game_minecraft::artifact::publish_minecraft_projection(Some(&context), evidence.artifact()).await.ok().flatten();
  let overlay = match &evidence {
    ProjectionEvidence::Bound { overlay, .. } => publish_png(MINECRAFT_OVERLAY_PURPOSE, overlay).await,
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
  ImageReader::open(path)
    .map_err(|error| format!("failed to open screenshot {}: {error}", path.display()))?
    .decode()
    .map(|image| image.to_rgb8())
    .map_err(|error| format!("failed to decode screenshot {}: {error}", path.display()))
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
  session.window().capture(&window).map(|capture| DynamicImage::ImageRgba8(capture.image).to_rgb8()).map_err(|error| error.to_string())
}

pub(crate) async fn publish_json_artifact<T: serde::Serialize>(purpose: &'static str, value: &T) -> Option<ArtifactMetadata> {
  let bytes = serde_json::to_vec_pretty(value).ok()?;
  publish_bytes(purpose, "application/json", bytes).await
}

async fn publish_png(purpose: &'static str, image: &RgbImage) -> Option<ArtifactMetadata> {
  let mut cursor = std::io::Cursor::new(Vec::new());
  DynamicImage::ImageRgb8(image.clone()).write_to(&mut cursor, ImageFormat::Png).ok()?;
  publish_bytes(purpose, "image/png", cursor.into_inner()).await
}

async fn publish_bytes(purpose: &'static str, content_type: &'static str, bytes: Vec<u8>) -> Option<ArtifactMetadata> {
  let context = Context::current();
  if !context.can_publish_artifacts() {
    return None;
  }
  let byte_length = u64::try_from(bytes.len()).ok()?;
  let artifact = NewArtifact::new(
    ArtifactPurpose::parse(purpose).ok()?,
    ContentType::parse(content_type).ok()?,
    ByteLength::new(byte_length).ok()?,
    Sha256Digest::new(Sha256::digest(&bytes).into()),
    Attributes::empty(),
    AsyncCursor::new(bytes),
  );
  context.in_scope(|| auv_tracing::emit_artifact!(artifact)).await.ok().flatten()
}
