//! Trajectory cleaner implementing deterministic forward state diff filtering
//! and backward slicing along the tool-call dependency graph.
//!
//! Rule: ZERO LLM-as-judge. Deterministic drop vs. ambiguous drop.
//!
//! Method:
//! 1. Forward State Diff:
//!    Compares `pre_state` and `post_state` for each step to extract concrete mutated keys.
//! 2. Backward Dependency Slicing:
//!    Checks whether state mutations and produced outputs align with the target domain goals
//!    (e.g. media playback: volume, status, title, window aliveness).
//! 3. Ambiguous Drop Classification:
//!    - Safe drop: Step produces zero state diff (pure observation or identical pre/post state)
//!      and its output is not consumed downstream.
//!    - Ambiguous drop: Step performs a state mutation (pre_state != post_state) that is NOT
//!      part of the goal slicing path, or lacks verifiable perception feedback.
//!      Dropping it would cause silent replay divergence. Trajectory is rejected.

use crate::models::{DroppedStep, TrajectoryRecord, TrajectoryStep};
use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct CleaningOutcome {
  pub kept_steps: Vec<TrajectoryStep>,
  pub dropped_steps: Vec<DroppedStep>,
  pub has_ambiguous_drops: bool,
}

/// Known terminal goal state keys for media playback operations.
const MEDIA_GOAL_KEYS: &[&str] = &[
  "volume",
  "target",
  "readback",
  "status",
  "playback_status",
  "title",
  "title_changed",
  "alive",
  "non_black_ratio",
  "app_id",
];

/// Computes the keys mutated between pre_state and post_state.
pub fn compute_state_diff(pre: &Option<serde_json::Value>, post: &Option<serde_json::Value>) -> Vec<String> {
  let mut changed = Vec::new();
  match (pre, post) {
    (Some(serde_json::Value::Object(pre_map)), Some(serde_json::Value::Object(post_map))) => {
      let mut all_keys: HashSet<&String> = pre_map.keys().collect();
      all_keys.extend(post_map.keys());

      for k in all_keys {
        let v_pre = pre_map.get(k);
        let v_post = post_map.get(k);
        if v_pre != v_post {
          changed.push(k.clone());
        }
      }
    }
    (None, Some(serde_json::Value::Object(post_map))) => {
      changed.extend(post_map.keys().cloned());
    }
    (Some(serde_json::Value::Object(pre_map)), None) => {
      changed.extend(pre_map.keys().cloned());
    }
    (Some(v1), Some(v2)) if v1 != v2 => {
      changed.push("root_state".to_string());
    }
    _ => {}
  }
  changed.sort();
  changed
}

/// Cleans a recorded VLM trajectory using forward state-diff and backward dependency slicing.
pub fn clean_trajectory(record: &TrajectoryRecord) -> CleaningOutcome {
  let steps = &record.structured_trajectory;
  let mut kept_steps = Vec::new();
  let mut dropped_steps = Vec::new();
  let mut has_ambiguous_drops = false;

  for (idx, step) in steps.iter().enumerate() {
    let action_str = step.action.to_lowercase();
    let mutated_keys = compute_state_diff(&step.pre_state, &step.post_state);
    let has_concrete_state_diff = !mutated_keys.is_empty();

    // 1. Backward Slicing: Check if state diff is on the goal path
    if has_concrete_state_diff {
      let is_on_goal_slice = mutated_keys.iter().any(|k| {
        let k_lower = k.to_lowercase();
        MEDIA_GOAL_KEYS.iter().any(|goal| k_lower.contains(goal))
      });

      if !is_on_goal_slice {
        // Concrete state mutation exists, but is NOT on the goal slicing path!
        // Dropping this step silently would omit state mutations during replay.
        dropped_steps.push(DroppedStep {
          step: step.step,
          intent: step.intent.clone(),
          reason: format!(
            "ambiguous side-effect: state mutation on keys [{}] is outside the verifiable goal slice",
            mutated_keys.join(", ")
          ),
          is_ambiguous: true,
        });
        has_ambiguous_drops = true;
        continue;
      }
    }

    // 2. Check for actions with unverified mutations or missing deterministic feedback
    let lacks_verifiable_feedback = step.perception.to_lowercase().contains("no visible")
      || step.perception.to_lowercase().contains("no direct perception")
      || step.result.get("mutation").and_then(|v| v.as_bool()).unwrap_or(false);

    let is_non_goal_action = !action_str.contains("volume")
      && !action_str.contains("query")
      && !action_str.contains("next")
      && !action_str.contains("skip")
      && !action_str.contains("play")
      && !action_str.contains("capture")
      && !action_str.contains("wgc");

    if is_non_goal_action && lacks_verifiable_feedback {
      dropped_steps.push(DroppedStep {
        step: step.step,
        intent: step.intent.clone(),
        reason: "ambiguous side-effect: action is outside goal slice and lacks deterministic feedback".to_string(),
        is_ambiguous: true,
      });
      has_ambiguous_drops = true;
      continue;
    }

    // 3. Check for redundant queries that produce no state mutation and whose values are overwritten
    let is_query = action_str.contains("query") || action_str.contains("read");
    let is_redundant_query = if is_query && !has_concrete_state_diff {
      idx + 1 < steps.len() && (steps[idx + 1].action.contains("query") || (steps[idx + 1].action.contains("volume") && idx > 0))
    } else {
      false
    };

    if is_redundant_query && idx > 0 {
      dropped_steps.push(DroppedStep {
        step: step.step,
        intent: step.intent.clone(),
        reason: "redundant observation query with zero state diff and zero subsequent dependency".to_string(),
        is_ambiguous: false,
      });
      continue;
    }

    // 4. Check for pure no-op steps (producing zero state diff)
    let is_noop = (action_str.contains("noop") || action_str.contains("echo")) && !has_concrete_state_diff;
    if is_noop {
      dropped_steps.push(DroppedStep {
        step: step.step,
        intent: step.intent.clone(),
        reason: "no-op step producing zero state diff".to_string(),
        is_ambiguous: false,
      });
      continue;
    }

    // Step is on the verified dependency slice
    kept_steps.push(step.clone());
  }

  CleaningOutcome {
    kept_steps,
    dropped_steps,
    has_ambiguous_drops,
  }
}
