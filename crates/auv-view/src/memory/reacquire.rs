use super::reacquire_adapter::ReacquireDriverAdapter;
use super::{
  ViewMemory,
  read::{MemoryReadConfig, MemoryReadOutcome, StaleReason, read_memory},
};
use crate::{ParserDiagnostic, ViewBounds, normalize_identity};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReacquireTarget {
  NodeId(String),
  Anchor(String),
  LabelWithSection {
    label: String,
    section_hint: Option<String>,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReacquireStrategy {
  DirectId,
  LabelCurrentViewport,
  LabelPlusSection,
  // TODO(view-memory-a4): ViewportFingerprint stage deferred per anchor-reacquisition-v0.
  ViewportFingerprint,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReacquiredNode {
  pub node_id: String,
  pub label: Option<String>,
  pub bounds: ViewBounds,
  pub section_hint: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReacquireCandidate {
  pub node_id: Option<String>,
  pub label: String,
  pub section_hint: Option<String>,
  pub bounds: ViewBounds,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ReacquireObservation {
  pub fingerprint: String,
  pub candidates: Vec<ReacquireCandidate>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ReacquireOutcome {
  Reacquired {
    node: ReacquiredNode,
    strategy_used: ReacquireStrategy,
    observation_count: usize,
    diagnostics: Vec<ParserDiagnostic>,
  },
  Stale {
    reason: StaleReason,
    observation_count: usize,
    diagnostics: Vec<ParserDiagnostic>,
  },
  NotFound {
    attempted_strategies: Vec<ReacquireStrategy>,
    observation_count: usize,
    diagnostics: Vec<ParserDiagnostic>,
  },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReacquireConfig {
  pub max_scroll_attempts: usize,
  pub memory_read: Option<MemoryReadConfig>,
  pub current_baseline_width: Option<u32>,
}

impl Default for ReacquireConfig {
  fn default() -> Self {
    Self {
      max_scroll_attempts: 5,
      memory_read: None,
      current_baseline_width: None,
    }
  }
}

pub fn reacquire(
  memory: &ViewMemory,
  target: ReacquireTarget,
  adapter: &mut dyn ReacquireDriverAdapter,
  config: &ReacquireConfig,
) -> ReacquireOutcome {
  let checked_memory = if let Some(read_config) = &config.memory_read {
    match read_memory(memory.clone(), read_config, config.current_baseline_width) {
      MemoryReadOutcome::Rejected { reason } => {
        return ReacquireOutcome::Stale {
          reason,
          observation_count: 0,
          diagnostics: vec![ParserDiagnostic {
            code: "reacquire_memory_stale".into(),
            message: format!("view memory rejected at reacquire entry: {reason:?}"),
            node_id: None,
          }],
        };
      }
      MemoryReadOutcome::Accepted(memory) => memory,
    }
  } else {
    memory.clone()
  };

  let resolved = resolve_target(&checked_memory, target);
  let mut attempted = Vec::new();
  let mut observation_count = 0usize;
  let mut observe_error_count = 0usize;
  let mut observe_diagnostics = Vec::new();
  let mut saw_any_candidates = false;

  if let ReacquireTarget::NodeId(node_id) = &resolved {
    attempted.push(ReacquireStrategy::DirectId);
    if let Some(observation) =
      observe(adapter, &mut observation_count, &mut observe_error_count, &mut observe_diagnostics, &mut saw_any_candidates)
    {
      if let Some(node) = match_direct_id(node_id, &observation) {
        return ReacquireOutcome::Reacquired {
          node,
          strategy_used: ReacquireStrategy::DirectId,
          observation_count,
          diagnostics: Vec::new(),
        };
      }
    }
  }

  let (label, section_hint) = target_label_and_section(&checked_memory, &resolved);
  attempted.push(ReacquireStrategy::LabelCurrentViewport);
  if let Some(observation) =
    observe(adapter, &mut observation_count, &mut observe_error_count, &mut observe_diagnostics, &mut saw_any_candidates)
  {
    match match_label(&label, section_hint.as_deref(), &observation, false) {
      LabelMatch::Unique(node) => {
        return ReacquireOutcome::Reacquired {
          node,
          strategy_used: ReacquireStrategy::LabelCurrentViewport,
          observation_count,
          diagnostics: Vec::new(),
        };
      }
      LabelMatch::Ambiguous | LabelMatch::None => {}
    }
  }

  attempted.push(ReacquireStrategy::LabelPlusSection);
  for _ in 0..config.max_scroll_attempts {
    if let Some(observation) =
      observe(adapter, &mut observation_count, &mut observe_error_count, &mut observe_diagnostics, &mut saw_any_candidates)
    {
      match match_label(&label, section_hint.as_deref(), &observation, true) {
        LabelMatch::Unique(node) => {
          return ReacquireOutcome::Reacquired {
            node,
            strategy_used: ReacquireStrategy::LabelPlusSection,
            observation_count,
            diagnostics: Vec::new(),
          };
        }
        LabelMatch::Ambiguous | LabelMatch::None => {}
      }
    }
    if adapter.scroll_down().is_err() {
      break;
    }
  }

  if !saw_any_candidates {
    if observe_error_count > 0 && observation_count == 0 {
      return ReacquireOutcome::Stale {
        reason: StaleReason::ObservationFailedAtReacquisition,
        observation_count,
        diagnostics: observe_diagnostics,
      };
    }
    // NOTICE(a6c-4): viewport observe succeeded but adapter returned zero
    // reacquire candidates (e.g. Case B target scrolled off-viewport while
    // section/nav OCR remains). Classify as miss, not region-gone stale.
    return ReacquireOutcome::NotFound {
      attempted_strategies: attempted,
      observation_count,
      diagnostics: vec![ParserDiagnostic {
        code: "reacquire_not_found".into(),
        message: if observation_count > 0 {
          format!("no sidebar candidates observed across {observation_count} viewport(s) while reacquiring label={label:?}")
        } else {
          format!("no sidebar candidates observed while reacquiring label={label:?}")
        },
        node_id: None,
      }],
    };
  }

  ReacquireOutcome::NotFound {
    attempted_strategies: attempted,
    observation_count,
    diagnostics: vec![ParserDiagnostic {
      code: "reacquire_not_found".into(),
      message: format!("could not reacquire target label={label:?}"),
      node_id: None,
    }],
  }
}

fn resolve_target(memory: &ViewMemory, target: ReacquireTarget) -> ReacquireTarget {
  match target {
    ReacquireTarget::Anchor(anchor_id) => memory
      .anchors
      .iter()
      .find(|anchor| anchor.id == anchor_id)
      .map(|anchor| ReacquireTarget::LabelWithSection {
        label: anchor.label.clone(),
        section_hint: memory
          .node_snapshots
          .values()
          .find(|snap| snap.label.as_deref() == Some(anchor.label.as_str()))
          .and_then(|snap| snap.section_hint.clone()),
      })
      .unwrap_or(ReacquireTarget::Anchor(anchor_id)),
    other => other,
  }
}

fn target_label_and_section(memory: &ViewMemory, target: &ReacquireTarget) -> (String, Option<String>) {
  match target {
    ReacquireTarget::LabelWithSection {
      label,
      section_hint,
    } => (label.clone(), section_hint.clone()),
    ReacquireTarget::NodeId(node_id) => {
      memory.node_snapshots.get(node_id).map(|snap| (snap.label.clone().unwrap_or_default(), snap.section_hint.clone())).unwrap_or_default()
    }
    ReacquireTarget::Anchor(id) => (id.clone(), None),
  }
}

enum LabelMatch {
  Unique(ReacquiredNode),
  Ambiguous,
  None,
}

fn observe(
  adapter: &mut dyn ReacquireDriverAdapter,
  observation_count: &mut usize,
  observe_error_count: &mut usize,
  observe_diagnostics: &mut Vec<ParserDiagnostic>,
  saw_any_candidates: &mut bool,
) -> Option<ReacquireObservation> {
  match adapter.observe_viewport() {
    Ok(observation) => {
      *observation_count += 1;
      if !observation.candidates.is_empty() {
        *saw_any_candidates = true;
      }
      Some(observation)
    }
    Err(diagnostic) => {
      *observe_error_count += 1;
      observe_diagnostics.push(diagnostic);
      None
    }
  }
}

fn match_direct_id(node_id: &str, observation: &ReacquireObservation) -> Option<ReacquiredNode> {
  observation.candidates.iter().find(|candidate| candidate.node_id.as_deref() == Some(node_id)).map(candidate_to_node)
}

fn match_label(label: &str, section_hint: Option<&str>, observation: &ReacquireObservation, require_section: bool) -> LabelMatch {
  let normalized = normalize_identity(label);
  let matches: Vec<_> = observation
    .candidates
    .iter()
    .filter(|candidate| normalize_identity(&candidate.label) == normalized)
    .filter(|candidate| {
      if require_section {
        section_hint.is_none_or(|hint| candidate.section_hint.as_deref().is_some_and(|value| value == hint))
      } else if let Some(hint) = section_hint {
        candidate.section_hint.as_deref().is_none_or(|value| value == hint)
      } else {
        true
      }
    })
    .collect();

  match matches.len() {
    0 => LabelMatch::None,
    1 => LabelMatch::Unique(candidate_to_node(matches[0])),
    _ => LabelMatch::Ambiguous,
  }
}

fn candidate_to_node(candidate: &ReacquireCandidate) -> ReacquiredNode {
  ReacquiredNode {
    node_id: candidate.node_id.clone().unwrap_or_else(|| normalize_identity(&candidate.label)),
    label: Some(candidate.label.clone()),
    bounds: candidate.bounds,
    section_hint: candidate.section_hint.clone(),
  }
}

#[cfg(test)]
#[path = "reacquire_test.rs"]
mod tests;
