//! v0.1 Scalar Parameter Lifting & Anti-Unification across isomorphic trajectories.
//!
//! Scope & Design Constraints:
//! - v0.1 specifically lifts scalar quantities (currently process volume) when literal
//!   arguments vary across structurally isomorphic trajectory skeletons.
//! - General AST anti-unification for arbitrary domain verbs is deferred (TODO: anti-unify-v1).
//! - Parameter sources are strictly restricted to:
//!   1. Anti-unification of >= 2 isomorphic trajectories with varying scalar literals.
//!   2. Explicit API whitelist (e.g. SetVolume(float)).
//! - FORBIDDEN: Actions that must remain parameter-less (such as Next(), SkipNext(), Play())
//!   are strictly barred from parameterization.

use crate::models::{ParameterDef, TrajectoryRecord, TrajectoryStep};

#[derive(Debug, Clone)]
pub struct ParameterLiftingOutcome {
  pub parameters: Vec<ParameterDef>,
  pub parameterized_steps: Vec<TrajectoryStep>,
  pub forbidden_parameter_detected: Option<String>,
}

/// Checks if an action type is explicitly forbidden from parameterization.
pub fn is_forbidden_for_parameterization(action_name: &str) -> bool {
  let lower = action_name.to_lowercase();
  lower.contains("next") || lower.contains("skip_next") || lower.contains("skip_previous") || lower.contains("toggle")
}

/// Performs anti-unification on two or more isomorphic trajectories.
/// When literal arguments differ at the same step across identical action skeletons,
/// anti-unification lifts the argument into a typed parameter.
pub fn anti_unify_trajectories(records: &[TrajectoryRecord]) -> ParameterLiftingOutcome {
  if records.len() < 2 {
    return ParameterLiftingOutcome {
      parameters: Vec::new(),
      parameterized_steps: records.first().map(|r| r.structured_trajectory.clone()).unwrap_or_default(),
      forbidden_parameter_detected: None,
    };
  }

  let base = &records[0].structured_trajectory;
  let mut parameters = Vec::new();
  let mut parameterized_steps = base.clone();

  // Check structural isomorphism: step count must match
  for other in &records[1..] {
    if other.structured_trajectory.len() != base.len() {
      // Non-isomorphic trajectories cannot be anti-unified
      return ParameterLiftingOutcome {
        parameters: Vec::new(),
        parameterized_steps: base.clone(),
        forbidden_parameter_detected: None,
      };
    }
  }

  for (i, base_step) in base.iter().enumerate() {
    let action_str = &base_step.action;

    // Check forbidden parameterization guard
    if is_forbidden_for_parameterization(action_str) {
      // Check if any other trajectory tried to vary arguments on Next()
      for other in &records[1..] {
        if other.structured_trajectory[i].action != *action_str {
          return ParameterLiftingOutcome {
            parameters: Vec::new(),
            parameterized_steps: base.clone(),
            forbidden_parameter_detected: Some(format!("illegal parameterization proposed for atomic action '{}'", action_str)),
          };
        }
      }
      continue;
    }

    // Check volume parameterization across trajectories: e.g. "volume 0.40" vs "volume 0.60"
    if action_str.contains("volume") {
      let mut values = Vec::new();
      for rec in records {
        let step = &rec.structured_trajectory[i];
        if let Some(val) = extract_volume_number(&step.action) {
          values.push(val);
        }
      }

      if values.len() == records.len() && has_variation(&values) {
        let param_name = "volume".to_string();
        parameters.push(ParameterDef {
          name: param_name.clone(),
          param_type: "float".to_string(),
          default_value: Some(serde_json::json!(values[0])),
          min_value: Some(0.0),
          max_value: Some(1.0),
        });

        // Replace literal with {{volume}} template
        parameterized_steps[i].action = replace_volume_in_action(&base_step.action, &format!("{{{{{}}}}}", param_name));
      }
    }
  }

  ParameterLiftingOutcome {
    parameters,
    parameterized_steps,
    forbidden_parameter_detected: None,
  }
}

fn extract_volume_number(action: &str) -> Option<f64> {
  for part in action.split_whitespace() {
    if let Ok(v) = part.parse::<f64>() {
      return Some(v);
    }
  }
  None
}

fn replace_volume_in_action(action: &str, replacement: &str) -> String {
  let parts: Vec<&str> = action.split_whitespace().collect();
  let mut new_parts = Vec::new();
  for p in parts {
    if p.parse::<f64>().is_ok() {
      new_parts.push(replacement.to_string());
    } else {
      new_parts.push(p.to_string());
    }
  }
  new_parts.join(" ")
}

fn has_variation(vals: &[f64]) -> bool {
  if vals.is_empty() {
    return false;
  }
  let first = vals[0];
  vals.iter().any(|&v| (v - first).abs() > 1e-4)
}
