use super::ViewMemory;
use super::*;
use crate::ViewBounds;
use crate::memory::{VIEW_MEMORY_SCHEMA_VERSION, ViewMemoryScopeSnapshot};
use auv_tracing::{ArtifactId, ArtifactUri, RunId};

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
    source_scan_uri: ArtifactUri::from_ids(RunId::new(), ArtifactId::new()),
    memory_id: "app:scope".into(),
    app_bundle_id: "app".into(),
    scope_id: "scope".into(),
    last_reconstructed_at_millis: 0,
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

  let outcome = reacquire(&memory, ReacquireTarget::NodeId("item.coding-bgm-synth".into()), &mut adapter, &ReacquireConfig::default());
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
      assert!(attempted_strategies.iter().any(|strategy| *strategy == ReacquireStrategy::LabelCurrentViewport));
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
