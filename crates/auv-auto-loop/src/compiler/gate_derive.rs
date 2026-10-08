//! Template-based gate derivation for AUV compiler.
//!
//! Maps known perception patterns to rigorous verification gates:
//! - SMTC -> Strict equality / inequality templates (session present, title change, status).
//! - CoreAudio float -> Mandatory ±0.05 tolerance (never exact float equality).
//! - WGC -> SSIM / histogram / non-black pixel ratio (strictly forbids pixel hash).
//! - Unknown/unsupported -> Marked as `unverified-step` with `UnverifiedFallback`,
//!   which triggers strict runtime mode (single failure -> instant escalation + isolation).
//!
//! Gate template to execution mode mapping:
//! - `TitleChangeGate(require_title_change=true)` -> Verified
//! - `StatusAndVolumeGate` / `SmtcSessionPresent` / `WgcAliveGate` -> Verified
//! - Pure action dispatch without effect gate -> Fast candidate (subject to blast-radius approval)
//! - Blast-radius high risk actions (delete/send/sys-config) -> constructively barred from Fast
//! - `unverified-step` -> Verified only (rejected with Fast at compilation gate)

use crate::models::{ExecutionMode, OperationStepDef, TrajectoryStep, VerificationGateDef};

/// Derives an operation step definition and its verification gate from a trajectory step.
pub fn derive_step_gate(step: &TrajectoryStep, app_id: &str) -> OperationStepDef {
  let action_lower = step.action.to_lowercase();
  let intent_lower = step.intent.to_lowercase();

  let step_id = format!("step_{}", step.step);
  let name = step.intent.clone();
  let description = format!("Auto-derived step {} from VLM trajectory: {}", step.step, step.intent);

  // 1. SMTC session / query template
  if (action_lower.contains("query") || intent_lower.contains("query")) && !action_lower.contains("volume") {
    let action = serde_json::json!({
      "type": "smtc_query",
      "app_id": app_id,
    });
    let gate = VerificationGateDef::SmtcSessionPresent {
      timeout_ms: 1000,
      escalate_on_mismatch: "escalate_to_vlm".to_string(),
    };
    return OperationStepDef {
      id: step_id,
      name,
      description,
      action,
      verification_gate: gate,
      is_unverified: false,
    };
  }

  // 2. Volume / CoreAudio template (with mandatory 0.05 float tolerance)
  if action_lower.contains("volume") || intent_lower.contains("volume") {
    let vol = extract_volume_from_step(step).unwrap_or(0.40);
    let action = serde_json::json!({
      "type": "ensure_playing_and_volume",
      "app_id": app_id,
      "volume": vol,
    });
    let gate = VerificationGateDef::StatusAndVolumeGate {
      expected_status: "Playing".to_string(),
      expected_volume: vol as f32,
      // Mandatory float tolerance: ±0.05
      volume_tolerance: 0.05,
      timeout_ms: 2000,
      escalate_on_mismatch: "escalate_to_vlm".to_string(),
    };
    return OperationStepDef {
      id: step_id,
      name,
      description,
      action,
      verification_gate: gate,
      is_unverified: false,
    };
  }

  // 3. Skip next / Title change template
  if action_lower.contains("next") || intent_lower.contains("next") || action_lower.contains("skip") {
    let action = serde_json::json!({
      "type": "skip_next",
      "app_id": app_id,
    });
    let gate = VerificationGateDef::TitleChangeGate {
      require_title_change: true,
      timeout_ms: 3000,
      escalate_on_mismatch: "escalate_to_vlm".to_string(),
    };
    return OperationStepDef {
      id: step_id,
      name,
      description,
      action,
      verification_gate: gate,
      is_unverified: false,
    };
  }

  // 4. Play / Ensure playing template
  let is_play_action = action_lower.split_whitespace().any(|w| w == "play")
    || (intent_lower.contains("play") && !intent_lower.contains("player") && !intent_lower.contains("display"));
  if is_play_action {
    let action = serde_json::json!({
      "type": "play",
      "app_id": app_id,
    });
    let gate = VerificationGateDef::StatusAndVolumeGate {
      expected_status: "Playing".to_string(),
      expected_volume: 0.40,
      volume_tolerance: 0.05,
      timeout_ms: 2000,
      escalate_on_mismatch: "escalate_to_vlm".to_string(),
    };
    return OperationStepDef {
      id: step_id,
      name,
      description,
      action,
      verification_gate: gate,
      is_unverified: false,
    };
  }

  // 5. WGC window capture / health template (SSIM / histogram / non-black ratio, forbids pixel hash)
  if action_lower.contains("capture") || action_lower.contains("wgc") || intent_lower.contains("capture") || intent_lower.contains("alive") {
    let action = serde_json::json!({
      "type": "wgc_capture",
      "app_id": app_id,
    });
    let gate = VerificationGateDef::WgcAliveGate {
      min_non_black_ratio: 0.50,
      timeout_ms: 2000,
      escalate_on_mismatch: "escalate_to_vlm".to_string(),
    };
    return OperationStepDef {
      id: step_id,
      name,
      description,
      action,
      verification_gate: gate,
      is_unverified: false,
    };
  }

  // 6. Unknown / Uncovered template -> Fallback marked as `unverified-step`
  let action = serde_json::json!({
    "type": "custom_action",
    "raw_action": step.action,
    "app_id": app_id,
  });
  let gate = VerificationGateDef::UnverifiedFallback {
    description: format!("No verification template for step {}: {}", step.step, step.intent),
    timeout_ms: 1000,
    escalate_on_mismatch: "instant_isolate_strict".to_string(),
  };

  OperationStepDef {
    id: step_id,
    name,
    description,
    action,
    verification_gate: gate,
    is_unverified: true, // triggers strict runtime mode
  }
}

fn extract_volume_from_step(step: &TrajectoryStep) -> Option<f64> {
  // Check in result
  if let Some(target) = step.result.get("target").and_then(|v| v.as_f64()) {
    return Some(target);
  }
  if let Some(vol) = step.result.get("volume").and_then(|v| v.as_f64()) {
    return Some(vol);
  }
  // Check in action string
  for part in step.action.split_whitespace() {
    if let Ok(v) = part.parse::<f64>() {
      return Some(v);
    }
  }
  None
}

/// Derives the execution mode required by a slice of compiled steps.
///
/// Rules:
/// - If any step requires effect verification (`TitleChangeGate(require_title_change=true)`,
///   `StatusAndVolumeGate`, `WgcAliveGate`, `SmtcSessionPresent`, etc.) or is unverified,
///   the operation requires `ExecutionMode::Verified`.
/// - Pure action dispatch with zero effect verification gates is a candidate for `ExecutionMode::Fast`.
pub fn derive_mode_from_steps(steps: &[OperationStepDef]) -> ExecutionMode {
  for step in steps {
    if step.is_unverified || step.verification_gate.requires_verified_mode().is_some() {
      return ExecutionMode::Verified;
    }
  }
  ExecutionMode::Fast
}
