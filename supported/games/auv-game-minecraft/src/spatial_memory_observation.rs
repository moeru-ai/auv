//! M0 observation and single-view spatial-memory patch contract.
//!
//! This is intentionally crate-local. The Minecraft lane is the first consumer,
//! and this slice proves the write boundary before a cross-game observation
//! contract or an LLM transport is approved.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::types::Viewport;

pub const SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION: u32 = 1;
pub const SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION: u32 = 1;

/// Built-in role prompt for turning bounded observations into append-only
/// hypothesis patches. It does not grant the model a memory-confirmation path.
pub const SINGLE_VIEW_SPATIAL_MEMORY_PROMPT: &str = r#"你是单视角空间记忆解释器，不是世界真值生成器。

你只能使用输入中明确存在的截图、时间、视口信息、输入历史，以及被标记为 available 的 depth、normal、motion、raycast、world pose 或 telemetry。禁止假设不存在的信号存在。禁止把模型常识、游戏常识或语言补全当作观测。

请把结果分成：
1. 直接观察到的内容；
2. 基于观测提出的空间假设；
3. 当前无法确认或互相矛盾的内容。

每个空间假设必须包含证据引用、坐标空间、几何/尺度/语义/配准置信度，以及明确的 hypothesis 状态。单张普通 RGB 截图不能确认隐藏表面、精确深度、碰撞边界、世界坐标或可交互性。

如果证据不足，请提出最小的后续采集动作。优先请求小幅横向或前后移动来制造视差；原地转头只能补充外观，不能替代平移基线。

输出 SpatialHypothesisPatch。你只能写入 hypothesis 或 candidate 范围，不能直接覆盖 confirmed spatial memory。不要删除冲突证据，不要把 unknown 改写成 false，也不要把 blocked 改写成 ready。"#;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialSignalTier {
  BlackBox,
  Derived,
  ExposedRender,
  EngineTruth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialSignalKind {
  RgbScreenshot,
  CaptureTiming,
  WindowMetadata,
  InputHistory,
  OpticalFlow,
  FeatureTracks,
  MonocularDepthEstimate,
  SurfaceNormalEstimate,
  DepthBuffer,
  NormalBuffer,
  MotionVectors,
  GBuffer,
  Raycast,
  WorldPose,
  Telemetry,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpatialSignalAvailability {
  pub kind: SpatialSignalKind,
  pub tier: SpatialSignalTier,
  pub provenance: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservationInputEvent {
  pub action: String,
  pub occurred_at_millis: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialObservationPacket {
  pub schema_version: u32,
  pub observation_id: String,
  pub screenshot_artifact_ref: Option<String>,
  pub captured_at_millis: u64,
  pub viewport: Viewport,
  pub available_signals: Vec<SpatialSignalAvailability>,
  pub input_history: Vec<ObservationInputEvent>,
}

impl SpatialObservationPacket {
  pub fn has_signal(&self, kind: SpatialSignalKind) -> bool {
    self.available_signals.iter().any(|signal| signal.kind == kind)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialClaimKind {
  Surface,
  Object,
  Relation,
  Projection,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialCoordinateSpace {
  ScreenRelative,
  CameraRelative,
  World,
  Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialClaimStatus {
  Hypothesis,
  Candidate,
  Confirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialMemoryWriteScope {
  HypothesisOnly,
  Candidate,
  Confirmed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialConfidence {
  pub appearance: f64,
  pub geometry: f64,
  pub metric_scale: f64,
  pub semantics: f64,
  pub world_registration: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialMemoryClaim {
  pub claim_id: String,
  pub kind: SpatialClaimKind,
  pub description: String,
  pub coordinate_space: SpatialCoordinateSpace,
  pub status: SpatialClaimStatus,
  pub confidence: SpatialConfidence,
  pub evidence_refs: Vec<SpatialSignalKind>,
  pub unsupported_inferences: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialFollowUpAction {
  StrafeLeft,
  StrafeRight,
  StepForward,
  StepBackward,
  OrbitTarget,
  Revisit,
  YawSweep,
  PitchSweep,
  RaycastProbe,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpatialFollowUpRequest {
  pub action: SpatialFollowUpAction,
  pub reason: String,
  pub minimum_observations: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialHypothesisPatch {
  pub schema_version: u32,
  pub observation_ids: Vec<String>,
  pub claims: Vec<SpatialMemoryClaim>,
  pub unknowns: Vec<String>,
  pub requested_follow_up_capture: Option<SpatialFollowUpRequest>,
  pub write_scope: SpatialMemoryWriteScope,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpatialMemoryPatchValidationError {
  UnsupportedObservationSchema {
    expected: u32,
    actual: u32,
  },
  UnsupportedPatchSchema {
    expected: u32,
    actual: u32,
  },
  EmptyObservationId,
  UnknownObservationId(String),
  ScreenshotSignalWithoutArtifact,
  EmptyClaims,
  ConfirmedWriteScope,
  ClaimStatusNotAllowed {
    claim_id: String,
    write_scope: SpatialMemoryWriteScope,
    status: SpatialClaimStatus,
  },
  EmptyClaimId,
  EmptyClaimDescription {
    claim_id: String,
  },
  ConfirmedClaim {
    claim_id: String,
  },
  MissingSignal {
    claim_id: String,
    signal: SpatialSignalKind,
  },
  WorldClaimWithoutWorldPose {
    claim_id: String,
  },
  InvalidConfidence {
    claim_id: String,
    field: &'static str,
  },
  ZeroViewport,
}

impl fmt::Display for SpatialMemoryPatchValidationError {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::UnsupportedObservationSchema { expected, actual } => {
        write!(formatter, "unsupported observation schema {actual}; expected {expected}")
      }
      Self::UnsupportedPatchSchema { expected, actual } => {
        write!(formatter, "unsupported hypothesis patch schema {actual}; expected {expected}")
      }
      Self::EmptyObservationId => formatter.write_str("observation id must not be empty"),
      Self::UnknownObservationId(observation_id) => write!(formatter, "patch references unknown observation {observation_id:?}"),
      Self::ScreenshotSignalWithoutArtifact => formatter.write_str("RGB screenshot signal requires a screenshot artifact reference"),
      Self::EmptyClaims => formatter.write_str("spatial hypothesis patch must contain at least one claim"),
      Self::ConfirmedWriteScope => formatter.write_str("prompt patch cannot write confirmed spatial memory"),
      Self::ClaimStatusNotAllowed {
        claim_id,
        write_scope,
        status,
      } => write!(formatter, "claim {claim_id:?} status {status:?} is not allowed for write scope {write_scope:?}"),
      Self::EmptyClaimId => formatter.write_str("claim id must not be empty"),
      Self::EmptyClaimDescription { claim_id } => write!(formatter, "claim {claim_id:?} description must not be empty"),
      Self::ConfirmedClaim { claim_id } => write!(formatter, "claim {claim_id:?} cannot be confirmed by the prompt"),
      Self::MissingSignal { claim_id, signal } => write!(formatter, "claim {claim_id:?} references unavailable signal {signal:?}"),
      Self::WorldClaimWithoutWorldPose { claim_id } => write!(formatter, "world claim {claim_id:?} requires an available world pose signal"),
      Self::InvalidConfidence { claim_id, field } => write!(formatter, "claim {claim_id:?} has invalid {field} confidence"),
      Self::ZeroViewport => formatter.write_str("observation viewport must be non-zero"),
    }
  }
}

pub fn validate_spatial_hypothesis_patch(
  packet: &SpatialObservationPacket,
  patch: &SpatialHypothesisPatch,
) -> Result<(), Vec<SpatialMemoryPatchValidationError>> {
  let mut errors = Vec::new();

  if packet.schema_version != SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION {
    errors.push(SpatialMemoryPatchValidationError::UnsupportedObservationSchema {
      expected: SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION,
      actual: packet.schema_version,
    });
  }
  if patch.schema_version != SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION {
    errors.push(SpatialMemoryPatchValidationError::UnsupportedPatchSchema {
      expected: SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION,
      actual: patch.schema_version,
    });
  }
  if packet.observation_id.is_empty() {
    errors.push(SpatialMemoryPatchValidationError::EmptyObservationId);
  }
  if packet.viewport.width == 0 || packet.viewport.height == 0 {
    errors.push(SpatialMemoryPatchValidationError::ZeroViewport);
  }
  if packet.has_signal(SpatialSignalKind::RgbScreenshot) && packet.screenshot_artifact_ref.is_none() {
    errors.push(SpatialMemoryPatchValidationError::ScreenshotSignalWithoutArtifact);
  }
  if patch.observation_ids.is_empty() {
    errors.push(SpatialMemoryPatchValidationError::EmptyObservationId);
  }
  for observation_id in &patch.observation_ids {
    if observation_id != &packet.observation_id {
      errors.push(SpatialMemoryPatchValidationError::UnknownObservationId(observation_id.clone()));
    }
  }
  if patch.claims.is_empty() {
    errors.push(SpatialMemoryPatchValidationError::EmptyClaims);
  }
  if patch.write_scope == SpatialMemoryWriteScope::Confirmed {
    errors.push(SpatialMemoryPatchValidationError::ConfirmedWriteScope);
  }

  for claim in &patch.claims {
    if claim.claim_id.is_empty() {
      errors.push(SpatialMemoryPatchValidationError::EmptyClaimId);
    }
    if claim.description.trim().is_empty() {
      errors.push(SpatialMemoryPatchValidationError::EmptyClaimDescription {
        claim_id: claim.claim_id.clone(),
      });
    }
    if claim.status == SpatialClaimStatus::Confirmed {
      errors.push(SpatialMemoryPatchValidationError::ConfirmedClaim {
        claim_id: claim.claim_id.clone(),
      });
    }
    let status_allowed = match patch.write_scope {
      SpatialMemoryWriteScope::HypothesisOnly => claim.status == SpatialClaimStatus::Hypothesis,
      SpatialMemoryWriteScope::Candidate => matches!(claim.status, SpatialClaimStatus::Hypothesis | SpatialClaimStatus::Candidate),
      SpatialMemoryWriteScope::Confirmed => false,
    };
    if !status_allowed {
      errors.push(SpatialMemoryPatchValidationError::ClaimStatusNotAllowed {
        claim_id: claim.claim_id.clone(),
        write_scope: patch.write_scope,
        status: claim.status,
      });
    }
    for signal in &claim.evidence_refs {
      if !packet.has_signal(*signal) {
        errors.push(SpatialMemoryPatchValidationError::MissingSignal {
          claim_id: claim.claim_id.clone(),
          signal: *signal,
        });
      }
    }
    if claim.coordinate_space == SpatialCoordinateSpace::World && !packet.has_signal(SpatialSignalKind::WorldPose) {
      errors.push(SpatialMemoryPatchValidationError::WorldClaimWithoutWorldPose {
        claim_id: claim.claim_id.clone(),
      });
    }
    for (field, confidence) in [
      ("appearance", claim.confidence.appearance),
      ("geometry", claim.confidence.geometry),
      ("metric_scale", claim.confidence.metric_scale),
      ("semantics", claim.confidence.semantics),
      ("world_registration", claim.confidence.world_registration),
    ] {
      if !confidence.is_finite() || !(0.0..=1.0).contains(&confidence) {
        errors.push(SpatialMemoryPatchValidationError::InvalidConfidence {
          claim_id: claim.claim_id.clone(),
          field,
        });
      }
    }
  }

  if errors.is_empty() {
    Ok(())
  } else {
    Err(errors)
  }
}

/// Append-only candidate memory for the M0 contract. Promotion to confirmed
/// memory is intentionally outside this type until an independent validator is
/// approved.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SpatialHypothesisMemory {
  pub patches: Vec<SpatialHypothesisPatch>,
}

impl SpatialHypothesisMemory {
  pub fn append(
    &mut self,
    packet: &SpatialObservationPacket,
    patch: SpatialHypothesisPatch,
  ) -> Result<(), Vec<SpatialMemoryPatchValidationError>> {
    validate_spatial_hypothesis_patch(packet, &patch)?;
    self.patches.push(patch);
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  fn black_box_packet() -> SpatialObservationPacket {
    SpatialObservationPacket {
      schema_version: SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION,
      observation_id: "obs-1".to_string(),
      screenshot_artifact_ref: Some("auv://runs/run-1/artifacts/screenshot-1".to_string()),
      captured_at_millis: 1_700_000_000_000,
      viewport: Viewport::new(854, 480),
      available_signals: vec![
        SpatialSignalAvailability {
          kind: SpatialSignalKind::RgbScreenshot,
          tier: SpatialSignalTier::BlackBox,
          provenance: "external_window_capture".to_string(),
        },
        SpatialSignalAvailability {
          kind: SpatialSignalKind::CaptureTiming,
          tier: SpatialSignalTier::BlackBox,
          provenance: "capture_backend_clock".to_string(),
        },
        SpatialSignalAvailability {
          kind: SpatialSignalKind::WindowMetadata,
          tier: SpatialSignalTier::BlackBox,
          provenance: "window_capture_contract".to_string(),
        },
        SpatialSignalAvailability {
          kind: SpatialSignalKind::InputHistory,
          tier: SpatialSignalTier::BlackBox,
          provenance: "auv_input_log".to_string(),
        },
      ],
      input_history: vec![ObservationInputEvent {
        action: "strafe_right".to_string(),
        occurred_at_millis: 1_699_999_999_950,
      }],
    }
  }

  fn hypothesis_patch() -> SpatialHypothesisPatch {
    SpatialHypothesisPatch {
      schema_version: SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION,
      observation_ids: vec!["obs-1".to_string()],
      claims: vec![SpatialMemoryClaim {
        claim_id: "claim-1".to_string(),
        kind: SpatialClaimKind::Surface,
        description: "前方可能存在一面垂直表面".to_string(),
        coordinate_space: SpatialCoordinateSpace::ScreenRelative,
        status: SpatialClaimStatus::Hypothesis,
        confidence: SpatialConfidence {
          appearance: 0.9,
          geometry: 0.45,
          metric_scale: 0.05,
          semantics: 0.35,
          world_registration: 0.0,
        },
        evidence_refs: vec![
          SpatialSignalKind::RgbScreenshot,
          SpatialSignalKind::InputHistory,
        ],
        unsupported_inferences: vec!["collision_boundary".to_string()],
      }],
      unknowns: vec!["world_coordinate".to_string()],
      requested_follow_up_capture: Some(SpatialFollowUpRequest {
        action: SpatialFollowUpAction::StrafeRight,
        reason: "需要横向视差确认平面关系".to_string(),
        minimum_observations: 1,
      }),
      write_scope: SpatialMemoryWriteScope::HypothesisOnly,
    }
  }

  #[test]
  fn built_in_prompt_requires_bounded_hypothesis_output() {
    assert!(SINGLE_VIEW_SPATIAL_MEMORY_PROMPT.contains("SpatialHypothesisPatch"));
    assert!(SINGLE_VIEW_SPATIAL_MEMORY_PROMPT.contains("confirmed spatial memory"));
    assert!(SINGLE_VIEW_SPATIAL_MEMORY_PROMPT.contains("视差"));
  }

  #[test]
  fn black_box_packet_does_not_advertise_engine_truth() {
    let packet = black_box_packet();
    assert!(!packet.has_signal(SpatialSignalKind::Raycast));
    assert!(!packet.has_signal(SpatialSignalKind::WorldPose));
    assert!(!packet.has_signal(SpatialSignalKind::DepthBuffer));

    let json = serde_json::to_string(&packet).expect("serialize black-box packet");
    assert!(!json.contains("raycast"));
    assert!(!json.contains("world_pose"));
    assert!(!json.contains("depth_buffer"));
  }

  #[test]
  fn accepts_black_box_hypothesis_and_appends_only_candidate_memory() {
    let packet = black_box_packet();
    let patch = hypothesis_patch();
    let mut memory = SpatialHypothesisMemory::default();

    memory.append(&packet, patch).expect("hypothesis patch should be accepted");
    assert_eq!(memory.patches.len(), 1);
    assert_eq!(memory.patches[0].write_scope, SpatialMemoryWriteScope::HypothesisOnly);
  }

  #[test]
  fn rejects_prompt_confirmation_attempt() {
    let packet = black_box_packet();
    let mut patch = hypothesis_patch();
    patch.write_scope = SpatialMemoryWriteScope::Confirmed;
    patch.claims[0].status = SpatialClaimStatus::Confirmed;

    let errors = validate_spatial_hypothesis_patch(&packet, &patch).expect_err("confirmation must be rejected");
    assert!(errors.iter().any(|error| matches!(error, SpatialMemoryPatchValidationError::ConfirmedWriteScope)));
    assert!(errors.iter().any(|error| matches!(error, SpatialMemoryPatchValidationError::ConfirmedClaim { .. })));
  }

  #[test]
  fn rejects_patch_schema_mismatch() {
    let packet = black_box_packet();
    let mut patch = hypothesis_patch();
    patch.schema_version = SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION + 1;

    let errors = validate_spatial_hypothesis_patch(&packet, &patch).expect_err("unknown patch schema must be rejected");
    assert!(errors.iter().any(|error| matches!(
      error,
      SpatialMemoryPatchValidationError::UnsupportedPatchSchema {
        expected: SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION,
        actual: 2,
      }
    )));
  }

  #[test]
  fn rejects_candidate_claim_in_hypothesis_only_patch() {
    let packet = black_box_packet();
    let mut patch = hypothesis_patch();
    patch.claims[0].status = SpatialClaimStatus::Candidate;

    let errors = validate_spatial_hypothesis_patch(&packet, &patch).expect_err("candidate must not enter hypothesis-only scope");
    assert!(errors.iter().any(|error| matches!(
      error,
      SpatialMemoryPatchValidationError::ClaimStatusNotAllowed {
        write_scope: SpatialMemoryWriteScope::HypothesisOnly,
        status: SpatialClaimStatus::Candidate,
        ..
      }
    )));
  }

  #[test]
  fn rejects_rgb_signal_without_capture_artifact() {
    let mut packet = black_box_packet();
    packet.screenshot_artifact_ref = None;

    let errors = validate_spatial_hypothesis_patch(&packet, &hypothesis_patch()).expect_err("RGB observation needs an artifact");
    assert!(errors.iter().any(|error| matches!(error, SpatialMemoryPatchValidationError::ScreenshotSignalWithoutArtifact)));
  }

  #[test]
  fn rejects_world_claim_without_world_pose() {
    let packet = black_box_packet();
    let mut patch = hypothesis_patch();
    patch.claims[0].coordinate_space = SpatialCoordinateSpace::World;

    let errors = validate_spatial_hypothesis_patch(&packet, &patch).expect_err("world claim needs a pose signal");
    assert!(errors.iter().any(|error| matches!(error, SpatialMemoryPatchValidationError::WorldClaimWithoutWorldPose { .. })));
  }

  #[test]
  fn rejects_evidence_signal_not_available_to_agent() {
    let packet = black_box_packet();
    let mut patch = hypothesis_patch();
    patch.claims[0].evidence_refs.push(SpatialSignalKind::Raycast);

    let errors = validate_spatial_hypothesis_patch(&packet, &patch).expect_err("unavailable raycast must be rejected");
    assert!(errors.iter().any(|error| matches!(
      error,
      SpatialMemoryPatchValidationError::MissingSignal {
        signal: SpatialSignalKind::Raycast,
        ..
      }
    )));
  }

  #[test]
  fn rejects_confidence_outside_unit_interval() {
    let packet = black_box_packet();
    let mut patch = hypothesis_patch();
    patch.claims[0].confidence.geometry = 1.1;

    let errors = validate_spatial_hypothesis_patch(&packet, &patch).expect_err("invalid confidence must be rejected");
    assert!(errors.iter().any(|error| matches!(
      error,
      SpatialMemoryPatchValidationError::InvalidConfidence {
        field: "geometry",
        ..
      }
    )));
  }
}
