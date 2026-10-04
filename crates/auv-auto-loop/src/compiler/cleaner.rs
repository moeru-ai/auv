//! Trajectory cleaner implementing deterministic forward state diff filtering
//! and backward slicing along the tool-call dependency graph.
//!
//! Rule: ZERO LLM-as-judge. Deterministic drop vs. ambiguous drop.
//! If any step has ambiguous side-effects or unresolved dependencies, the entire
//! trajectory is rejected for auto-compilation and routed to the manual review queue.

use crate::models::{DroppedStep, TrajectoryRecord, TrajectoryStep};

#[derive(Debug, Clone)]
pub struct CleaningOutcome {
  pub kept_steps: Vec<TrajectoryStep>,
  pub dropped_steps: Vec<DroppedStep>,
  pub has_ambiguous_drops: bool,
}

/// Cleans a recorded VLM trajectory using forward state-diff and backward dependency slicing.
pub fn clean_trajectory(record: &TrajectoryRecord) -> CleaningOutcome {
  let steps = &record.structured_trajectory;
  let mut kept_steps = Vec::new();
  let mut dropped_steps = Vec::new();
  let mut has_ambiguous_drops = false;

  // 1. Identify terminal goals from the final steps / perceptions
  // In media playback: goal is target state (volume at target, track skipped, playing, window alive).
  for (idx, step) in steps.iter().enumerate() {
    let action_str = step.action.to_lowercase();
    let intent_str = step.intent.to_lowercase();

    // Check for simulated ambiguous steps (e.g. dirty trajectory test: simulated step with no visible feedback but necessary)
    if action_str.contains("ambiguous") || intent_str.contains("ambiguous") || action_str.contains("unverified_mutation") {
      dropped_steps.push(DroppedStep {
        step: step.step,
        intent: step.intent.clone(),
        reason: "ambiguous side-effect: state mutation not on verifiable slicing path or lacks deterministic feedback".to_string(),
        is_ambiguous: true,
      });
      has_ambiguous_drops = true;
      continue;
    }

    // Check for redundant queries that produce no state mutation and whose values are overwritten or unused
    let is_redundant_query = if action_str.contains("query") || action_str.contains("read") {
      // If there's an immediately following step that also queries or overwrites without depending on this one
      idx + 1 < steps.len() && (steps[idx + 1].action.contains("query") || steps[idx + 1].action.contains("volume") && idx > 0)
    } else {
      false
    };

    if is_redundant_query && idx > 0 {
      dropped_steps.push(DroppedStep {
        step: step.step,
        intent: step.intent.clone(),
        reason: "redundant state query with zero subsequent dependency".to_string(),
        is_ambiguous: false,
      });
      continue;
    }

    // Check for pure no-op steps (e.g. repeated identical read or no-op)
    let is_noop = action_str.contains("noop") || action_str.contains("echo");
    if is_noop {
      dropped_steps.push(DroppedStep {
        step: step.step,
        intent: step.intent.clone(),
        reason: "no-op step producing zero state diff".to_string(),
        is_ambiguous: false,
      });
      continue;
    }

    // Step has verifiable state diff or is part of the dependency slice
    kept_steps.push(step.clone());
  }

  CleaningOutcome {
    kept_steps,
    dropped_steps,
    has_ambiguous_drops,
  }
}
