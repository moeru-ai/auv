use super::reacquire_adapter::ReacquireDriverAdapter;
use super::{
  ViewMemory,
  read::{MemoryReadConfig, MemoryReadOutcome, StaleReason, read_memory},
};
use crate::{ParserDiagnostic, ViewBounds, normalize_identity};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReacquireTarget {
  NodeId(String),
  Anchor(String),
  LabelWithSection {
    label: String,
    section_hint: Option<String>,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    if let Some(observation) = observe(
      adapter,
      &mut observation_count,
      &mut observe_error_count,
      &mut observe_diagnostics,
      &mut saw_any_candidates,
    ) {
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
  if let Some(observation) = observe(
    adapter,
    &mut observation_count,
    &mut observe_error_count,
    &mut observe_diagnostics,
    &mut saw_any_candidates,
  ) {
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
    if let Some(observation) = observe(
      adapter,
      &mut observation_count,
      &mut observe_error_count,
      &mut observe_diagnostics,
      &mut saw_any_candidates,
    ) {
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
          format!(
            "no sidebar candidates observed across {observation_count} viewport(s) while reacquiring label={label:?}"
          )
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

fn target_label_and_section(
  memory: &ViewMemory,
  target: &ReacquireTarget,
) -> (String, Option<String>) {
  match target {
    ReacquireTarget::LabelWithSection {
      label,
      section_hint,
    } => (label.clone(), section_hint.clone()),
    ReacquireTarget::NodeId(node_id) => memory
      .node_snapshots
      .get(node_id)
      .map(|snap| {
        (
          snap.label.clone().unwrap_or_default(),
          snap.section_hint.clone(),
        )
      })
      .unwrap_or_default(),
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
  observation
    .candidates
    .iter()
    .find(|candidate| candidate.node_id.as_deref() == Some(node_id))
    .map(candidate_to_node)
}

fn match_label(
  label: &str,
  section_hint: Option<&str>,
  observation: &ReacquireObservation,
  require_section: bool,
) -> LabelMatch {
  let normalized = normalize_identity(label);
  let matches: Vec<_> = observation
    .candidates
    .iter()
    .filter(|candidate| normalize_identity(&candidate.label) == normalized)
    .filter(|candidate| {
      if require_section {
        section_hint.is_none_or(|hint| {
          candidate
            .section_hint
            .as_deref()
            .is_some_and(|value| value == hint)
        })
      } else if let Some(hint) = section_hint {
        candidate
          .section_hint
          .as_deref()
          .is_none_or(|value| value == hint)
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
    node_id: candidate
      .node_id
      .clone()
      .unwrap_or_else(|| normalize_identity(&candidate.label)),
    label: Some(candidate.label.clone()),
    bounds: candidate.bounds,
    section_hint: candidate.section_hint.clone(),
  }
}

#[cfg(test)]
mod tests {
  use super::ViewMemory;
  use super::*;
  use crate::ViewBounds;
  use crate::memory::{VIEW_MEMORY_SCHEMA_VERSION, ViewMemoryScopeSnapshot};

  struct FakeAdapter {
    observations: Vec<ReacquireObservation>,
    cursor: usize,
    scrolls: usize,
  }

  impl ReacquireDriverAdapter for FakeAdapter {
    fn observe_viewport(&mut self) -> Result<ReacquireObservation, ParserDiagnostic> {
      self
        .observations
        .get(self.cursor)
        .cloned()
        .map(|observation| {
          self.cursor += 1;
          observation
        })
        .ok_or_else(|| ParserDiagnostic {
          code: "no_observation".into(),
          message: "fake adapter exhausted".into(),
          node_id: None,
        })
    }

    fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
      self.scrolls += 1;
      Ok(())
    }

    fn scroll_up(&mut self) -> Result<(), ParserDiagnostic> {
      Ok(())
    }
  }

  fn empty_memory() -> ViewMemory {
    ViewMemory {
      schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
      memory_id: "app:scope".into(),
      app_bundle_id: "app".into(),
      scope_id: "scope".into(),
      last_reconstructed_at_millis: 0,
      source_run_id: String::new(),
      source_reconstruction_ref: String::new(),
      anchors: Vec::new(),
      landmarks: Vec::new(),
      node_snapshots: Default::default(),
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: "playlist_sidebar".into(),
        region_bounds_window_local: ViewBounds::default(),
        baseline_width: 240,
        schema_version_view_ir: "view-ir-v0".into(),
      },
      diagnostics: Vec::new(),
    }
  }

  #[test]
  fn reacquire_stage1_direct_id_on_screen() {
    let memory = empty_memory();
    let mut adapter = FakeAdapter {
      observations: vec![ReacquireObservation {
        fingerprint: "a".into(),
        candidates: vec![ReacquireCandidate {
          node_id: Some("item.coding-bgm-synth".into()),
          label: "Coding BGM".into(),
          section_hint: Some("my_playlists".into()),
          bounds: ViewBounds::new(32.0, 74.0, 120.0, 20.0),
        }],
      }],
      cursor: 0,
      scrolls: 0,
    };

    let outcome = reacquire(
      &memory,
      ReacquireTarget::NodeId("item.coding-bgm-synth".into()),
      &mut adapter,
      &ReacquireConfig::default(),
    );
    match outcome {
      ReacquireOutcome::Reacquired {
        strategy_used: ReacquireStrategy::DirectId,
        ..
      } => {}
      other => panic!("expected direct id match, got {other:?}"),
    }
  }

  #[test]
  fn reacquire_stage3_unique_label() {
    let memory = empty_memory();
    let mut adapter = FakeAdapter {
      observations: vec![ReacquireObservation {
        fingerprint: "b".into(),
        candidates: vec![ReacquireCandidate {
          node_id: Some("item.road-trip".into()),
          label: "Road Trip".into(),
          section_hint: Some("favorite_playlists".into()),
          bounds: ViewBounds::new(32.0, 106.0, 120.0, 20.0),
        }],
      }],
      cursor: 0,
      scrolls: 0,
    };

    let outcome = reacquire(
      &memory,
      ReacquireTarget::LabelWithSection {
        label: "Road Trip".into(),
        section_hint: Some("favorite_playlists".into()),
      },
      &mut adapter,
      &ReacquireConfig::default(),
    );
    match outcome {
      ReacquireOutcome::Reacquired {
        strategy_used: ReacquireStrategy::LabelCurrentViewport,
        ..
      } => {}
      other => panic!("expected label match, got {other:?}"),
    }
  }

  #[test]
  fn reacquire_stage3_ambiguous_falls_through() {
    let memory = empty_memory();
    let mut adapter = FakeAdapter {
      observations: vec![
        ReacquireObservation {
          fingerprint: "a".into(),
          candidates: vec![
            ReacquireCandidate {
              node_id: None,
              label: "Jazz".into(),
              section_hint: None,
              bounds: ViewBounds::default(),
            },
            ReacquireCandidate {
              node_id: None,
              label: "Jazz".into(),
              section_hint: None,
              bounds: ViewBounds::default(),
            },
          ],
        },
        ReacquireObservation {
          fingerprint: "b".into(),
          candidates: vec![],
        },
      ],
      cursor: 0,
      scrolls: 0,
    };

    let outcome = reacquire(
      &memory,
      ReacquireTarget::LabelWithSection {
        label: "Jazz".into(),
        section_hint: None,
      },
      &mut adapter,
      &ReacquireConfig {
        max_scroll_attempts: 1,
        ..Default::default()
      },
    );
    assert!(matches!(outcome, ReacquireOutcome::NotFound { .. }));
  }

  #[test]
  fn reacquire_stage5_label_section_after_scroll() {
    let memory = empty_memory();
    let mut adapter = FakeAdapter {
      observations: vec![
        ReacquireObservation {
          fingerprint: "page0".into(),
          candidates: vec![ReacquireCandidate {
            node_id: Some("item.coding-bgm".into()),
            label: "Coding BGM".into(),
            section_hint: Some("my_playlists".into()),
            bounds: ViewBounds::default(),
          }],
        },
        ReacquireObservation {
          fingerprint: "page1".into(),
          candidates: vec![ReacquireCandidate {
            node_id: Some("item.jazz".into()),
            label: "Jazz".into(),
            section_hint: Some("my_playlists".into()),
            bounds: ViewBounds::new(32.0, 42.0, 80.0, 20.0),
          }],
        },
      ],
      cursor: 0,
      scrolls: 0,
    };

    let outcome = reacquire(
      &memory,
      ReacquireTarget::LabelWithSection {
        label: "Jazz".into(),
        section_hint: Some("my_playlists".into()),
      },
      &mut adapter,
      &ReacquireConfig {
        max_scroll_attempts: 2,
        ..Default::default()
      },
    );
    match outcome {
      ReacquireOutcome::Reacquired {
        strategy_used: ReacquireStrategy::LabelPlusSection,
        observation_count,
        ..
      } => {
        assert!(observation_count >= 2);
      }
      other => panic!("expected scrolled label+section match, got {other:?}"),
    }
  }

  #[test]
  fn reacquire_not_found_lists_attempted_strategies() {
    let memory = empty_memory();
    let mut adapter = FakeAdapter {
      observations: vec![ReacquireObservation {
        fingerprint: "other".into(),
        candidates: vec![ReacquireCandidate {
          node_id: None,
          label: "Other Playlist".into(),
          section_hint: None,
          bounds: ViewBounds::default(),
        }],
      }],
      cursor: 0,
      scrolls: 0,
    };

    let outcome = reacquire(
      &memory,
      ReacquireTarget::LabelWithSection {
        label: "Missing".into(),
        section_hint: None,
      },
      &mut adapter,
      &ReacquireConfig {
        max_scroll_attempts: 0,
        ..Default::default()
      },
    );
    match outcome {
      ReacquireOutcome::NotFound {
        attempted_strategies,
        ..
      } => {
        assert!(attempted_strategies.contains(&ReacquireStrategy::LabelCurrentViewport));
      }
      other => panic!("expected not found, got {other:?}"),
    }
  }

  #[test]
  fn reacquire_stale_on_freshness_rejection() {
    let mut memory = empty_memory();
    memory.last_reconstructed_at_millis = 1_000;
    let mut adapter = FakeAdapter {
      observations: vec![],
      cursor: 0,
      scrolls: 0,
    };

    let outcome = reacquire(
      &memory,
      ReacquireTarget::LabelWithSection {
        label: "Road Trip".into(),
        section_hint: None,
      },
      &mut adapter,
      &ReacquireConfig {
        max_scroll_attempts: 0,
        memory_read: Some(MemoryReadConfig {
          now_millis: 1_000 + super::super::DEFAULT_MEMORY_TTL_MILLIS + 1,
          ..Default::default()
        }),
        current_baseline_width: None,
      },
    );
    match outcome {
      ReacquireOutcome::Stale {
        reason: StaleReason::MemoryRejectedAtFreshness,
        ..
      } => {}
      other => panic!("expected freshness stale, got {other:?}"),
    }
  }

  #[test]
  fn reacquire_not_found_when_viewport_observed_but_empty_candidates() {
    let memory = empty_memory();
    let mut adapter = FakeAdapter {
      observations: vec![ReacquireObservation {
        fingerprint: "empty".into(),
        candidates: vec![],
      }],
      cursor: 0,
      scrolls: 0,
    };

    let outcome = reacquire(
      &memory,
      ReacquireTarget::LabelWithSection {
        label: "Missing".into(),
        section_hint: None,
      },
      &mut adapter,
      &ReacquireConfig {
        max_scroll_attempts: 0,
        ..Default::default()
      },
    );
    match outcome {
      ReacquireOutcome::NotFound {
        attempted_strategies,
        observation_count,
        ..
      } => {
        assert_eq!(observation_count, 1);
        assert!(
          attempted_strategies
            .iter()
            .any(|strategy| *strategy == ReacquireStrategy::LabelCurrentViewport)
        );
      }
      other => panic!("expected not_found after successful observe, got {other:?}"),
    }
  }

  #[test]
  fn reacquire_not_found_when_candidates_exist_but_no_match() {
    let memory = empty_memory();
    let mut adapter = FakeAdapter {
      observations: vec![ReacquireObservation {
        fingerprint: "other".into(),
        candidates: vec![ReacquireCandidate {
          node_id: None,
          label: "Visible Row".into(),
          section_hint: None,
          bounds: ViewBounds::default(),
        }],
      }],
      cursor: 0,
      scrolls: 0,
    };

    let outcome = reacquire(
      &memory,
      ReacquireTarget::LabelWithSection {
        label: "Missing".into(),
        section_hint: None,
      },
      &mut adapter,
      &ReacquireConfig {
        max_scroll_attempts: 0,
        ..Default::default()
      },
    );
    assert!(matches!(outcome, ReacquireOutcome::NotFound { .. }));
  }

  struct AlwaysErrAdapter;

  impl ReacquireDriverAdapter for AlwaysErrAdapter {
    fn observe_viewport(&mut self) -> Result<ReacquireObservation, ParserDiagnostic> {
      Err(ParserDiagnostic {
        code: "capture_failed".into(),
        message: "simulated observe failure".into(),
        node_id: None,
      })
    }

    fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
      Ok(())
    }

    fn scroll_up(&mut self) -> Result<(), ParserDiagnostic> {
      Ok(())
    }
  }

  #[test]
  fn reacquire_stale_when_all_observes_fail() {
    let memory = empty_memory();
    let mut adapter = AlwaysErrAdapter;

    let outcome = reacquire(
      &memory,
      ReacquireTarget::LabelWithSection {
        label: "Missing".into(),
        section_hint: None,
      },
      &mut adapter,
      &ReacquireConfig {
        max_scroll_attempts: 0,
        ..Default::default()
      },
    );
    match outcome {
      ReacquireOutcome::Stale {
        reason: StaleReason::ObservationFailedAtReacquisition,
        observation_count: 0,
        diagnostics,
      } => {
        assert!(!diagnostics.is_empty());
      }
      other => panic!("expected observation-failed stale, got {other:?}"),
    }
  }
}
