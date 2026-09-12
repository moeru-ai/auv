//! Offline M1 request/response boundary for the Minecraft black-box baseline.
//!
//! This module prepares an auditable model request and validates externally
//! produced JSON. It deliberately does not choose or call an LLM provider.

use serde::{Deserialize, Serialize};

use crate::dataset::SourceArtifactUri;
use crate::spatial_memory_observation::{
  SINGLE_VIEW_SPATIAL_MEMORY_PROMPT, SpatialFollowUpAction, SpatialHypothesisPatch, SpatialObservationPacket, SpatialSignalKind,
  SpatialSignalTier, validate_spatial_hypothesis_patch,
};

pub const M1_BLACK_BOX_REQUEST_SCHEMA_VERSION: u32 = 1;
pub const M1_BLACK_BOX_RESPONSE_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1BlackBoxRequest {
  schema_version: u32,
  system_prompt: String,
  observation: SpatialObservationPacket,
}

impl M1BlackBoxRequest {
  pub fn schema_version(&self) -> u32 {
    self.schema_version
  }

  pub fn system_prompt(&self) -> &str {
    &self.system_prompt
  }

  pub fn observation(&self) -> &SpatialObservationPacket {
    &self.observation
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum M1BlackBoxResponseStatus {
  Accepted,
  Rejected,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct M1BlackBoxResponseReport {
  pub schema_version: u32,
  pub observation_id: String,
  pub status: M1BlackBoxResponseStatus,
  pub patch: Option<SpatialHypothesisPatch>,
  pub errors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum M1BlackBoxRequestError {
  InvalidObservation(Vec<String>),
  NonBlackBoxSignal {
    signal: SpatialSignalKind,
    tier: SpatialSignalTier,
  },
  MissingRgbScreenshot,
  InvalidScreenshotArtifactRef,
  InvalidBlackBoxMetadata {
    field: &'static str,
  },
}

impl std::fmt::Display for M1BlackBoxRequestError {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::InvalidObservation(errors) => write!(formatter, "invalid M1 observation: {}", errors.join("; ")),
      Self::NonBlackBoxSignal { signal, tier } => {
        write!(formatter, "M1 black-box request cannot expose {signal:?} at signal tier {tier:?}")
      }
      Self::MissingRgbScreenshot => formatter.write_str("M1 black-box request requires an RGB screenshot artifact"),
      Self::InvalidScreenshotArtifactRef => formatter.write_str("M1 black-box request requires a canonical AUV screenshot artifact URI"),
      Self::InvalidBlackBoxMetadata { field } => write!(formatter, "M1 black-box request has invalid {field}"),
    }
  }
}

/// Build the provider-neutral request artifact for one M1 observation.
///
/// Model transport stays outside this crate until a provider boundary is
/// explicitly approved. The artifact can therefore be inspected before any
/// screenshot is sent to an external model.
pub fn prepare_m1_black_box_request(observation: SpatialObservationPacket) -> Result<M1BlackBoxRequest, M1BlackBoxRequestError> {
  validate_m1_observation(&observation)?;
  Ok(M1BlackBoxRequest {
    schema_version: M1_BLACK_BOX_REQUEST_SCHEMA_VERSION,
    system_prompt: SINGLE_VIEW_SPATIAL_MEMORY_PROMPT.to_string(),
    observation,
  })
}

/// Parse and validate externally produced structured output.
///
/// A rejected report keeps parse or contract errors separate from semantic
/// scoring. Withheld Minecraft answer-key scoring lives in `m1_black_box_scoring`
/// so engine truth never enters the model request by accident.
pub fn inspect_m1_black_box_response(request: &M1BlackBoxRequest, response_json: &[u8]) -> M1BlackBoxResponseReport {
  let mut request_errors = Vec::new();
  if request.schema_version != M1_BLACK_BOX_REQUEST_SCHEMA_VERSION {
    request_errors
      .push(format!("unsupported M1 request schema {}; expected {}", request.schema_version, M1_BLACK_BOX_REQUEST_SCHEMA_VERSION));
  }
  if request.system_prompt != SINGLE_VIEW_SPATIAL_MEMORY_PROMPT {
    request_errors.push("M1 request system prompt does not match the built-in contract".to_string());
  }
  if let Err(error) = validate_m1_observation(&request.observation) {
    request_errors.push(error.to_string());
  }
  if !request_errors.is_empty() {
    return rejected_report(&request.observation.observation_id, request_errors);
  }

  // NOTICE(m1-response-forward-compat): Unknown JSON fields are currently
  // ignored by serde so this app-local experiment can consume provider output
  // without changing the shared M0 patch types. Switch to a strict wire type
  // only when real M1 evidence defines a versioning policy.
  let patch = match serde_json::from_slice::<SpatialHypothesisPatch>(response_json) {
    Ok(patch) => patch,
    Err(error) => {
      return rejected_report(&request.observation.observation_id, vec![format!("invalid SpatialHypothesisPatch JSON: {error}")]);
    }
  };

  let mut errors = validate_spatial_hypothesis_patch(&request.observation, &patch).err().unwrap_or_default();
  let mut error_messages: Vec<_> = errors.drain(..).map(|error| error.to_string()).collect();
  validate_m1_follow_up(&patch, &mut error_messages);

  if error_messages.is_empty() {
    M1BlackBoxResponseReport {
      schema_version: M1_BLACK_BOX_RESPONSE_REPORT_SCHEMA_VERSION,
      observation_id: request.observation.observation_id.clone(),
      status: M1BlackBoxResponseStatus::Accepted,
      patch: Some(patch),
      errors: Vec::new(),
    }
  } else {
    rejected_report(&request.observation.observation_id, error_messages)
  }
}

fn validate_m1_observation(observation: &SpatialObservationPacket) -> Result<(), M1BlackBoxRequestError> {
  let mut errors = Vec::new();
  if observation.schema_version != crate::SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION {
    errors.push(format!(
      "unsupported observation schema {}; expected {}",
      observation.schema_version,
      crate::SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION
    ));
  }
  if observation.observation_id.trim().is_empty() {
    errors.push("observation id must not be empty".to_string());
  }
  if observation.viewport.width == 0 || observation.viewport.height == 0 {
    errors.push("observation viewport must be non-zero".to_string());
  }
  if !errors.is_empty() {
    return Err(M1BlackBoxRequestError::InvalidObservation(errors));
  }

  for signal in &observation.available_signals {
    let tier_zero_kind = matches!(
      signal.kind,
      SpatialSignalKind::RgbScreenshot
        | SpatialSignalKind::CaptureTiming
        | SpatialSignalKind::WindowMetadata
        | SpatialSignalKind::InputHistory
    );
    if signal.tier != SpatialSignalTier::BlackBox || !tier_zero_kind {
      return Err(M1BlackBoxRequestError::NonBlackBoxSignal {
        signal: signal.kind,
        tier: signal.tier,
      });
    }
    // TODO(m1-live-capture-vocabulary): `prepare_m1_black_box_from_telemetry_tail`
    // now consumes these literals as the live producer. A 2026-09-12 Windows
    // `window.capture` run proved PrintWindow bytes bind through this vocabulary;
    // replace the strings with producer-owned enums only after that owner-approved
    // slice. Do not expand the string allowlist in the meantime.
    if !is_m1_signal_provenance(signal.kind, &signal.provenance) {
      return Err(M1BlackBoxRequestError::InvalidBlackBoxMetadata {
        field: "signal provenance",
      });
    }
  }

  if !observation.has_signal(SpatialSignalKind::RgbScreenshot) {
    return Err(M1BlackBoxRequestError::MissingRgbScreenshot);
  }
  let Some(screenshot_artifact_ref) = observation.screenshot_artifact_ref.as_deref().filter(|reference| !reference.trim().is_empty()) else {
    return Err(M1BlackBoxRequestError::MissingRgbScreenshot);
  };
  if SourceArtifactUri::new(screenshot_artifact_ref).is_err() {
    return Err(M1BlackBoxRequestError::InvalidScreenshotArtifactRef);
  }
  if !observation.input_history.is_empty() && !observation.has_signal(SpatialSignalKind::InputHistory) {
    return Err(M1BlackBoxRequestError::InvalidBlackBoxMetadata {
      field: "input history availability",
    });
  }
  if observation.input_history.iter().any(|event| !is_m1_input_action(&event.action)) {
    return Err(M1BlackBoxRequestError::InvalidBlackBoxMetadata {
      field: "input action",
    });
  }

  Ok(())
}

pub(crate) fn is_m1_signal_provenance(kind: SpatialSignalKind, provenance: &str) -> bool {
  matches!(
    (kind, provenance),
    (SpatialSignalKind::RgbScreenshot, "external_window_capture")
      | (SpatialSignalKind::CaptureTiming, "capture_backend_clock")
      | (SpatialSignalKind::WindowMetadata, "window_capture_contract")
      | (SpatialSignalKind::InputHistory, "auv_input_log")
  )
}

pub(crate) fn is_m1_input_action(action: &str) -> bool {
  matches!(action, "strafe_left" | "strafe_right" | "step_forward" | "step_backward" | "yaw_left" | "yaw_right" | "pitch_up" | "pitch_down")
}

fn validate_m1_follow_up(patch: &SpatialHypothesisPatch, errors: &mut Vec<String>) {
  let Some(follow_up) = &patch.requested_follow_up_capture else {
    return;
  };
  if follow_up.action == SpatialFollowUpAction::RaycastProbe {
    errors.push("M1 black-box response cannot request a raycast probe".to_string());
  }
  if follow_up.reason.trim().is_empty() {
    errors.push("M1 black-box follow-up reason must not be empty".to_string());
  }
  if follow_up.minimum_observations == 0 {
    errors.push("M1 black-box follow-up minimum_observations must be positive".to_string());
  }
}

fn rejected_report(observation_id: &str, errors: Vec<String>) -> M1BlackBoxResponseReport {
  M1BlackBoxResponseReport {
    schema_version: M1_BLACK_BOX_RESPONSE_REPORT_SCHEMA_VERSION,
    observation_id: observation_id.to_string(),
    status: M1BlackBoxResponseStatus::Rejected,
    patch: None,
    errors,
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    ObservationInputEvent, SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION, SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION, SpatialClaimKind,
    SpatialClaimStatus, SpatialConfidence, SpatialCoordinateSpace, SpatialMemoryClaim, SpatialMemoryWriteScope, SpatialSignalAvailability,
    Viewport,
  };

  fn observation() -> SpatialObservationPacket {
    SpatialObservationPacket {
      schema_version: SPATIAL_MEMORY_OBSERVATION_SCHEMA_VERSION,
      observation_id: "m1-observation-1".to_string(),
      screenshot_artifact_ref: Some("auv://runs/run-1/artifacts/minecraft-window.png".to_string()),
      captured_at_millis: 1_700_000_000_000,
      viewport: Viewport::new(1280, 720),
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
        occurred_at_millis: 1_699_999_999_900,
      }],
    }
  }

  fn patch() -> SpatialHypothesisPatch {
    SpatialHypothesisPatch {
      schema_version: SPATIAL_HYPOTHESIS_PATCH_SCHEMA_VERSION,
      observation_ids: vec!["m1-observation-1".to_string()],
      claims: vec![SpatialMemoryClaim {
        claim_id: "surface-1".to_string(),
        kind: SpatialClaimKind::Surface,
        description: "A vertical surface may occupy the center of the image".to_string(),
        coordinate_space: SpatialCoordinateSpace::ScreenRelative,
        status: SpatialClaimStatus::Hypothesis,
        confidence: SpatialConfidence {
          appearance: 0.9,
          geometry: 0.4,
          metric_scale: 0.0,
          semantics: 0.2,
          world_registration: 0.0,
        },
        evidence_refs: vec![SpatialSignalKind::RgbScreenshot],
        unsupported_inferences: vec!["hidden_geometry".to_string()],
      }],
      unknowns: vec!["metric_depth".to_string()],
      requested_follow_up_capture: None,
      write_scope: SpatialMemoryWriteScope::HypothesisOnly,
    }
  }

  #[test]
  fn request_serialization_excludes_engine_truth_fields() {
    let request = prepare_m1_black_box_request(observation()).expect("black-box observation");
    let json = serde_json::to_string(&request).expect("serialize request");

    for forbidden in [
      "view_matrix",
      "projection_matrix",
      "player_pose",
      "raycast_hit",
      "nearby_blocks",
      "world_pose",
      "depth_buffer",
    ] {
      assert!(!json.contains(forbidden), "request leaked {forbidden}");
    }
    let request_value: serde_json::Value = serde_json::from_str(&json).expect("parse request JSON");
    let request_keys: std::collections::BTreeSet<_> =
      request_value.as_object().expect("request object").keys().map(String::as_str).collect();
    assert_eq!(request_keys, std::collections::BTreeSet::from(["observation", "schema_version", "system_prompt"]));
    let observation_keys: std::collections::BTreeSet<_> =
      request_value["observation"].as_object().expect("observation object").keys().map(String::as_str).collect();
    assert_eq!(
      observation_keys,
      std::collections::BTreeSet::from([
        "available_signals",
        "captured_at_millis",
        "input_history",
        "observation_id",
        "schema_version",
        "screenshot_artifact_ref",
        "viewport"
      ])
    );
    let signals = request_value["observation"]["available_signals"].as_array().expect("available signals");
    let signal_kinds: std::collections::BTreeSet<_> = signals.iter().map(|signal| signal["kind"].as_str().expect("signal kind")).collect();
    assert_eq!(
      signal_kinds,
      std::collections::BTreeSet::from([
        "capture_timing",
        "input_history",
        "rgb_screenshot",
        "window_metadata"
      ])
    );
    assert!(json.contains("SpatialHypothesisPatch"));
    assert!(json.contains("minecraft-window.png"));
  }

  #[test]
  fn request_rejects_non_tier_zero_signal_kinds_even_when_mislabeled_black_box() {
    for (kind, tier) in [
      (SpatialSignalKind::OpticalFlow, SpatialSignalTier::Derived),
      (SpatialSignalKind::DepthBuffer, SpatialSignalTier::ExposedRender),
      (SpatialSignalKind::Raycast, SpatialSignalTier::EngineTruth),
      (SpatialSignalKind::DepthBuffer, SpatialSignalTier::BlackBox),
      (SpatialSignalKind::Raycast, SpatialSignalTier::BlackBox),
      (SpatialSignalKind::WorldPose, SpatialSignalTier::BlackBox),
      (SpatialSignalKind::Telemetry, SpatialSignalTier::BlackBox),
    ] {
      let mut observation = observation();
      observation.available_signals.push(SpatialSignalAvailability {
        kind,
        tier,
        provenance: "forbidden_for_m1".to_string(),
      });

      assert!(matches!(
        prepare_m1_black_box_request(observation),
        Err(M1BlackBoxRequestError::NonBlackBoxSignal { signal, tier: actual_tier }) if signal == kind && actual_tier == tier
      ));
    }
  }

  #[test]
  fn request_rejects_invalid_observation_identity_and_viewport() {
    let mut observation = observation();
    observation.schema_version += 1;
    observation.observation_id = "  ".to_string();
    observation.viewport = Viewport::new(0, 720);

    let error = prepare_m1_black_box_request(observation).expect_err("invalid observation must be rejected");
    let M1BlackBoxRequestError::InvalidObservation(errors) = error else {
      panic!("expected invalid observation error");
    };
    assert!(errors.iter().any(|error| error.contains("unsupported observation schema")));
    assert!(errors.iter().any(|error| error.contains("observation id")));
    assert!(errors.iter().any(|error| error.contains("viewport")));
  }

  #[test]
  fn request_requires_rgb_signal_and_artifact() {
    let mut without_signal = observation();
    without_signal.available_signals.retain(|signal| signal.kind != SpatialSignalKind::RgbScreenshot);
    assert!(matches!(prepare_m1_black_box_request(without_signal), Err(M1BlackBoxRequestError::MissingRgbScreenshot)));

    let mut without_artifact = observation();
    without_artifact.screenshot_artifact_ref = None;
    assert!(matches!(prepare_m1_black_box_request(without_artifact), Err(M1BlackBoxRequestError::MissingRgbScreenshot)));

    let mut blank_artifact = observation();
    blank_artifact.screenshot_artifact_ref = Some("  ".to_string());
    assert!(matches!(prepare_m1_black_box_request(blank_artifact), Err(M1BlackBoxRequestError::MissingRgbScreenshot)));

    let mut invalid_artifact = observation();
    invalid_artifact.screenshot_artifact_ref = Some("artifact://minecraft-window.png".to_string());
    assert!(matches!(prepare_m1_black_box_request(invalid_artifact), Err(M1BlackBoxRequestError::InvalidScreenshotArtifactRef)));
  }

  #[test]
  fn request_rejects_untrusted_free_form_metadata() {
    let mut forged_provenance = observation();
    forged_provenance.available_signals[0].provenance = "raycast_hit=513,72,726".to_string();
    assert!(matches!(
      prepare_m1_black_box_request(forged_provenance),
      Err(M1BlackBoxRequestError::InvalidBlackBoxMetadata {
        field: "signal provenance"
      })
    ));

    let mut mismatched_provenance = observation();
    mismatched_provenance.available_signals[0].provenance = "auv_input_log".to_string();
    assert!(matches!(
      prepare_m1_black_box_request(mismatched_provenance),
      Err(M1BlackBoxRequestError::InvalidBlackBoxMetadata {
        field: "signal provenance"
      })
    ));

    let mut forged_input = observation();
    forged_input.input_history[0].action = "strafe_right;player_pose=1,2,3".to_string();
    assert!(matches!(
      prepare_m1_black_box_request(forged_input),
      Err(M1BlackBoxRequestError::InvalidBlackBoxMetadata {
        field: "input action"
      })
    ));
  }

  #[test]
  fn valid_structured_response_is_accepted() {
    let request = prepare_m1_black_box_request(observation()).expect("black-box observation");
    let json = serde_json::to_vec(&patch()).expect("serialize patch");

    let report = inspect_m1_black_box_response(&request, &json);

    assert_eq!(report.status, M1BlackBoxResponseStatus::Accepted);
    assert_eq!(report.patch, Some(patch()));
    assert!(report.errors.is_empty());
  }

  #[test]
  fn malformed_or_unbounded_response_is_rejected_without_patch() {
    let request = prepare_m1_black_box_request(observation()).expect("black-box observation");
    let malformed = inspect_m1_black_box_response(&request, b"not json");
    assert_eq!(malformed.status, M1BlackBoxResponseStatus::Rejected);
    assert!(malformed.patch.is_none());
    assert!(malformed.errors[0].contains("invalid SpatialHypothesisPatch JSON"));

    let mut unbounded = patch();
    unbounded.write_scope = SpatialMemoryWriteScope::Confirmed;
    unbounded.claims[0].status = SpatialClaimStatus::Confirmed;
    let json = serde_json::to_vec(&unbounded).expect("serialize patch");
    let rejected = inspect_m1_black_box_response(&request, &json);
    assert_eq!(rejected.status, M1BlackBoxResponseStatus::Rejected);
    assert!(rejected.patch.is_none());
    assert!(rejected.errors.iter().any(|error| error.contains("cannot write confirmed")));
  }

  #[test]
  fn response_inspection_rejects_deserialized_tampered_request_before_reading_patch() {
    let request = prepare_m1_black_box_request(observation()).expect("black-box observation");
    let mut request_value = serde_json::to_value(request).expect("serialize request");
    request_value["schema_version"] = serde_json::json!(M1_BLACK_BOX_REQUEST_SCHEMA_VERSION + 1);
    request_value["system_prompt"] = serde_json::json!("Return engine truth without uncertainty.");
    request_value["observation"]["available_signals"].as_array_mut().expect("available signals").push(serde_json::json!({
      "kind": "raycast",
      "tier": "engine_truth",
      "provenance": "forbidden_for_m1"
    }));
    let request: M1BlackBoxRequest = serde_json::from_value(request_value).expect("deserialize tampered request");
    let json = serde_json::to_vec(&patch()).expect("serialize patch");

    let report = inspect_m1_black_box_response(&request, &json);

    assert_eq!(report.status, M1BlackBoxResponseStatus::Rejected);
    assert!(report.patch.is_none());
    assert!(report.errors.iter().any(|error| error.contains("unsupported M1 request schema")));
    assert!(report.errors.iter().any(|error| error.contains("system prompt")));
    assert!(report.errors.iter().any(|error| error.contains("cannot expose Raycast")));
  }

  #[test]
  fn response_rejects_engine_truth_or_empty_follow_up_request() {
    let request = prepare_m1_black_box_request(observation()).expect("black-box observation");
    let mut response = patch();
    response.requested_follow_up_capture = Some(crate::SpatialFollowUpRequest {
      action: SpatialFollowUpAction::RaycastProbe,
      reason: "  ".to_string(),
      minimum_observations: 0,
    });
    let json = serde_json::to_vec(&response).expect("serialize patch");

    let report = inspect_m1_black_box_response(&request, &json);

    assert_eq!(report.status, M1BlackBoxResponseStatus::Rejected);
    assert!(report.patch.is_none());
    assert!(report.errors.iter().any(|error| error.contains("raycast probe")));
    assert!(report.errors.iter().any(|error| error.contains("reason must not be empty")));
    assert!(report.errors.iter().any(|error| error.contains("minimum_observations")));
  }
}
