use super::*;
use crate::scroll::policies::detection_motion::MotionEvidence;
use crate::view_parsers::sidebar::test_support::fake_recognition;
use crate::*;

#[test]
fn reconstruct_sidebar_groups_items_under_carried_section() {
  let page0 = parse_sidebar_viewport(
    0,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ]),
  );
  let page1 = parse_sidebar_viewport(
    1,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("Jazz", 32.0, 42.0, 80.0, 20.0),
      ("收藏的歌单", 8.0, 74.0, 110.0, 20.0),
      ("Road Trip", 32.0, 106.0, 120.0, 20.0),
    ]),
  );

  let scan =
    reconstruct_playlist_sidebar(ScanAppContext::default(), ScanWindowContext::default(), ViewRegionRecord::default(), vec![page0, page1]);

  assert_eq!(scan.projection.sections.len(), 2);
  assert_eq!(scan.projection.sections.iter().map(|section| section.items.len()).sum::<usize>(), 3);
  assert_eq!(scan.projection.sections[0].items[0].label, "Coding BGM");
  assert_eq!(scan.projection.sections[0].items[1].section_hint, Some(SidebarSectionKind::MyPlaylists));
  assert_eq!(scan.projection.sections[1].items[0].section_hint, Some(SidebarSectionKind::FavoritePlaylists));
  assert_eq!(scan.reconstruction.root.kind, ViewNodeKind::Collection);
  assert_eq!(scan.reconstruction.root.children.len(), 2);
}

#[test]
fn created_category_scan_stops_at_favorite_landmark_before_scrolling_again() {
  let observations = vec![
    parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    ),
    parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("收藏的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Road Trip", 32.0, 74.0, 120.0, 20.0),
      ]),
    ),
    parse_sidebar_viewport(
      2,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("Should Not Scan", 32.0, 42.0, 140.0, 20.0)]),
    ),
  ];
  let mut observer = FakeSidebarObserver::new(observations);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::Created,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.observations.len(), 2);
  assert_eq!(observer.cursor, 1);
  assert_eq!(scan.projection.sections.len(), 1);
  assert_eq!(scan.projection.sections[0].kind, SidebarSectionKind::MyPlaylists);
  assert_eq!(scan.projection.sections[0].items.len(), 1);
  assert_eq!(scan.projection.sections[0].items[0].label, "Coding BGM");
  assert!(
    scan
      .interaction_events
      .iter()
      .any(|event| { event.kind == InteractionEventKind::StopDecision && event.note.as_deref() == Some("reached_stop_landmark") })
  );
}

#[test]
fn favorite_category_starts_collecting_at_favorite_landmark() {
  let observations = vec![
    parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    ),
    parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("收藏的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Road Trip", 32.0, 74.0, 120.0, 20.0),
      ]),
    ),
  ];
  let mut observer = FakeSidebarObserver::new(observations);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::Favorite,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.projection.sections.len(), 1);
  assert_eq!(scan.projection.sections[0].kind, SidebarSectionKind::FavoritePlaylists);
  assert_eq!(scan.projection.sections[0].items.len(), 1);
  assert_eq!(scan.projection.sections[0].items[0].label, "Road Trip");
}

#[test]
fn reconstruct_sidebar_records_observe_and_scroll_interaction_events() {
  let mut first = parse_sidebar_viewport(
    0,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ]),
  );
  first.source_artifacts = vec!["obs-0000-window.png".to_string()];
  let mut second =
    parse_sidebar_viewport(1, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("Jazz", 32.0, 42.0, 80.0, 20.0)]));
  second.source_artifacts = vec!["obs-0001-window.png".to_string()];
  second.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
  let mut observer = FakeSidebarObserver::new(vec![first, second]);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 2,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    250,
  );

  assert!(scan.interaction_events.iter().any(|event| {
    event.kind == InteractionEventKind::Observe && event.observation_index == Some(0) && event.artifacts == vec!["obs-0000-window.png"]
  }));
  assert!(scan.interaction_events.iter().any(|event| {
    event.kind == InteractionEventKind::InputScroll
      && event.from_observation == Some(0)
      && event.to_observation == Some(1)
      && event.artifacts
        == vec![
          "obs-0000-window.png".to_string(),
          "obs-0001-window.png".to_string(),
        ]
      && event
        .scroll
        .as_ref()
        .is_some_and(|scroll| scroll.settle_ms == 250 && scroll.delivery_path.as_deref() == Some("window_targeted_wheel"))
  }));
}

#[test]
fn decode_playlist_sidebar_scan_json_accepts_current_schema() {
  let page0 = parse_sidebar_viewport(
    0,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ]),
  );
  let scan = reconstruct_playlist_sidebar(ScanAppContext::default(), ScanWindowContext::default(), ViewRegionRecord::default(), vec![page0]);

  let json = serde_json::to_string(&scan).expect("scan should serialize");
  let decoded = decode_playlist_sidebar_scan_json(&json).expect("current schema should decode");

  assert_eq!(decoded, scan);
}

#[test]
fn decode_playlist_sidebar_scan_json_rejects_missing_or_unknown_schema() {
  let missing = r#"{"projection":{"sections":[]}}"#;
  let missing_error = decode_playlist_sidebar_scan_json(missing).expect_err("missing schema version should be rejected");
  assert!(missing_error.contains("missing schema_version"));

  let unknown = r#"{"schema_version":"view-ir-v999","projection":{"sections":[]}}"#;
  let unknown_error = decode_playlist_sidebar_scan_json(unknown).expect_err("unknown schema version should be rejected");
  assert!(unknown_error.contains("unsupported playlist sidebar scan schema_version"));
}

#[test]
fn reconstruct_sidebar_deduplicates_repeated_item_labels_in_same_section() {
  let page0 = parse_sidebar_viewport(
    0,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ]),
  );
  let page1 =
    parse_sidebar_viewport(1, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("Coding BGM", 32.0, 42.0, 120.0, 20.0)]));

  let scan =
    reconstruct_playlist_sidebar(ScanAppContext::default(), ScanWindowContext::default(), ViewRegionRecord::default(), vec![page0, page1]);

  assert_eq!(scan.projection.sections[0].items.len(), 1);
  assert!(scan.diagnostics.iter().any(|diagnostic| diagnostic.code == "deduplicated_item"));
}

#[test]
fn reconstruct_sidebar_reports_ocr_evidence_without_reliable_candidates() {
  // ROOT CAUSE:
  //
  // If OCR produced evidence but every node was rejected as an unreliable
  // sidebar candidate, reconstruction returned a clean empty projection.
  //
  // Before the fix, JSON consumers could not distinguish an empty sidebar
  // from a parser rejection. The fix keeps that boundary explicit.
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("搜索框占位", 8.0, 42.0, 120.0, 20.0)]));

  let scan =
    reconstruct_playlist_sidebar(ScanAppContext::default(), ScanWindowContext::default(), ViewRegionRecord::default(), vec![observation]);

  assert!(scan.projection.sections.is_empty());
  assert!(scan.diagnostics.iter().any(|diagnostic| diagnostic.code == "parser_no_reliable_candidates"));
}

#[test]
fn reconstruct_sidebar_deduplicates_items_per_actual_section() {
  let page0 = parse_sidebar_viewport(
    0,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ]),
  );
  let page1 =
    parse_sidebar_viewport(1, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("Coding BGM", 32.0, 42.0, 120.0, 20.0)]));
  let page2 = parse_sidebar_viewport(
    2,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("我的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ]),
  );

  let scan = reconstruct_playlist_sidebar(
    ScanAppContext::default(),
    ScanWindowContext::default(),
    ViewRegionRecord::default(),
    vec![page0, page1, page2],
  );

  assert_eq!(scan.projection.sections.len(), 2);
  assert_eq!(scan.projection.sections[0].kind, SidebarSectionKind::MyPlaylists);
  assert_eq!(scan.projection.sections[0].items.len(), 1);
  assert_eq!(scan.projection.sections[0].items[0].label, "Coding BGM");
  assert_eq!(scan.projection.sections[1].kind, SidebarSectionKind::MyPlaylists);
  assert_eq!(scan.projection.sections[1].items.len(), 1);
  assert_eq!(scan.projection.sections[1].items[0].label, "Coding BGM");
  assert_eq!(scan.diagnostics.iter().filter(|diagnostic| diagnostic.code == "deduplicated_item").count(), 1);
}

#[test]
fn scan_loop_stops_on_repeated_viewport_fingerprint() {
  let observations = vec![
    parse_sidebar_viewport(
      0,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![
        ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
        ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
      ]),
    ),
    parse_sidebar_viewport(1, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("Jazz", 32.0, 42.0, 80.0, 20.0)])),
    parse_sidebar_viewport(2, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("Jazz", 32.0, 42.0, 80.0, 20.0)])),
  ];
  let mut observer = FakeSidebarObserver::new(observations);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.window.id, Some("fake".to_string()));
  assert_eq!(scan.observations.len(), 3);
  assert_eq!(scan.boundary.bottom, BoundaryConfidence::Likely);
  assert!(
    scan
      .interaction_events
      .iter()
      .any(|event| event.kind == InteractionEventKind::StopDecision && event.note.as_deref() == Some("repeated_viewport_fingerprint"))
  );
}

#[test]
fn scan_loop_stops_after_two_scrolls_with_no_motion_evidence() {
  let mut first = parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("A", 32.0, 42.0, 80.0, 20.0)]));
  first.viewport_fingerprint = "page-a".to_string();
  let mut second =
    parse_sidebar_viewport(1, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("B", 32.0, 42.0, 80.0, 20.0)]));
  second.viewport_fingerprint = "page-b".to_string();
  second.scroll_motion = Some(MotionEvidence {
    estimated_shift_y: 9,
    normalized_diff: 0.24,
    no_motion: false,
  });
  let mut third = parse_sidebar_viewport(2, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("C", 32.0, 42.0, 80.0, 20.0)]));
  third.viewport_fingerprint = "page-c".to_string();
  third.scroll_motion = Some(MotionEvidence {
    estimated_shift_y: 0,
    normalized_diff: 0.0,
    no_motion: true,
  });
  let mut fourth =
    parse_sidebar_viewport(3, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("D", 32.0, 42.0, 80.0, 20.0)]));
  fourth.viewport_fingerprint = "page-d".to_string();
  fourth.scroll_motion = Some(MotionEvidence {
    estimated_shift_y: 0,
    normalized_diff: 0.0,
    no_motion: true,
  });
  let mut observer = FakeSidebarObserver::new(vec![first, second, third, fourth]);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.observations.len(), 4);
  assert_eq!(scan.boundary.bottom, BoundaryConfidence::Likely);
  assert!(
    scan
      .interaction_events
      .iter()
      .any(|event| event.kind == InteractionEventKind::StopDecision && event.note.as_deref() == Some("scroll_no_motion_after_input"))
  );
  assert!(!scan.known_limits.iter().any(|limit| limit.contains("max_scrolls")));
}

#[test]
fn top_seek_budget_is_capped_separately_from_collection_budget() {
  assert_eq!(top_seek_scroll_budget(3), 3);
  assert_eq!(top_seek_scroll_budget(250), LIVE_TOP_SEEK_MAX_SCROLL_INPUTS);
}

#[test]
fn sidebar_target_seek_stops_on_first_match() {
  let budget = sidebar_rescan_target_seek_budget(12, 1);
  for attempt in 0..budget {
    let found = attempt == 2;
    match next_sidebar_target_seek_step(attempt, budget, found) {
      Some(SidebarTargetSeekStep::Found(hit)) => {
        assert_eq!(hit, 2);
        return;
      }
      Some(SidebarTargetSeekStep::ScrollNext(_)) => {}
      None => panic!("expected found before budget exhausted"),
    }
  }
  panic!("expected Found step");
}

#[test]
fn sidebar_target_seek_exhausts_budget_without_match() {
  let budget = 3usize;
  let mut scrolls = 0usize;
  for attempt in 0..budget {
    match next_sidebar_target_seek_step(attempt, budget, false) {
      Some(SidebarTargetSeekStep::Found(_)) => panic!("unexpected match"),
      Some(SidebarTargetSeekStep::ScrollNext(_)) => scrolls += 1,
      None => {
        assert_eq!(attempt, budget - 1);
        assert_eq!(scrolls, budget - 1);
        return;
      }
    }
  }
  panic!("expected None on final attempt");
}

#[test]
fn scan_loop_stops_after_one_no_motion_when_ax_scrollbar_is_bottom() {
  let mut first = parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("A", 32.0, 42.0, 80.0, 20.0)]));
  first.viewport_fingerprint = "page-a".to_string();
  let mut second =
    parse_sidebar_viewport(1, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("B", 32.0, 42.0, 80.0, 20.0)]));
  second.viewport_fingerprint = "page-b".to_string();
  second.scroll_motion = Some(MotionEvidence {
    estimated_shift_y: 9,
    normalized_diff: 0.24,
    no_motion: false,
  });
  let mut third = parse_sidebar_viewport(2, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("C", 32.0, 42.0, 80.0, 20.0)]));
  third.viewport_fingerprint = "page-c".to_string();
  third.scroll_motion = Some(MotionEvidence {
    estimated_shift_y: 0,
    normalized_diff: 0.0,
    no_motion: true,
  });
  third.ax_scrollbar_boundary = Some(SidebarScrollbarBoundary::Bottom);
  let mut fourth =
    parse_sidebar_viewport(3, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("D", 32.0, 42.0, 80.0, 20.0)]));
  fourth.viewport_fingerprint = "page-d".to_string();
  let mut observer = FakeSidebarObserver::new(vec![first, second, third, fourth]);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.observations.len(), 3);
  assert_eq!(scan.boundary.bottom, BoundaryConfidence::Likely);
  assert!(scan.interaction_events.iter().any(
    |event| event.kind == InteractionEventKind::StopDecision && event.note.as_deref() == Some("scroll_no_motion_with_ax_scrollbar_bottom")
  ));
}

#[test]
fn scan_loop_does_not_stop_scroll_on_no_motion_without_prior_motion() {
  let observations = (0..4)
    .map(|index| {
      let mut observation =
        parse_sidebar_viewport(index, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("A", 32.0, 42.0, 80.0, 20.0)]));
      observation.viewport_fingerprint = format!("page-{index}");
      if index > 0 {
        observation.scroll_motion = Some(MotionEvidence {
          estimated_shift_y: 0,
          normalized_diff: 0.0,
          no_motion: true,
        });
      }
      observation
    })
    .collect::<Vec<_>>();
  let mut observer = FakeSidebarObserver::new(observations);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 3,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.observations.len(), 4);
  assert_eq!(scan.boundary.bottom, BoundaryConfidence::Unknown);
  assert!(
    !scan
      .interaction_events
      .iter()
      .any(|event| event.kind == InteractionEventKind::StopDecision && event.note.as_deref() == Some("scroll_no_motion_after_input"))
  );
  assert!(scan.known_limits.iter().any(|limit| limit.contains("max_scrolls=3")));
}

#[test]
fn scan_loop_does_not_stop_scroll_on_no_motion_from_noop_delivery() {
  let observations = (0..4)
    .map(|index| {
      let mut observation =
        parse_sidebar_viewport(index, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("A", 32.0, 42.0, 80.0, 20.0)]));
      observation.viewport_fingerprint = format!("page-{index}");
      if index > 0 {
        observation.incoming_scroll_delivery_path = Some("noop".to_string());
        observation.scroll_motion = Some(MotionEvidence {
          estimated_shift_y: 0,
          normalized_diff: 0.0,
          no_motion: true,
        });
      }
      observation
    })
    .collect::<Vec<_>>();
  let mut observer = FakeSidebarObserver::new(observations);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 3,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.observations.len(), 4);
  assert_eq!(scan.boundary.bottom, BoundaryConfidence::Unknown);
  assert!(
    !scan
      .interaction_events
      .iter()
      .any(|event| event.kind == InteractionEventKind::StopDecision && event.note.as_deref() == Some("scroll_no_motion_after_input"))
  );
}

#[test]
fn scan_loop_stops_after_two_scrolls_with_no_new_semantic_candidates() {
  let mut first = parse_sidebar_viewport(
    0,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ]),
  );
  first.viewport_fingerprint = "page-a".to_string();
  let mut second = parse_sidebar_viewport(
    1,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 106.0, 120.0, 20.0),
    ]),
  );
  second.viewport_fingerprint = "page-b".to_string();
  second.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
  let mut third = parse_sidebar_viewport(
    2,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 138.0, 120.0, 20.0),
    ]),
  );
  third.viewport_fingerprint = "page-c".to_string();
  third.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
  let fourth = parse_sidebar_viewport(
    3,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Fresh Playlist", 32.0, 170.0, 120.0, 20.0),
    ]),
  );
  let mut observer = FakeSidebarObserver::new(vec![first, second, third, fourth]);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.observations.len(), 3);
  assert_eq!(scan.boundary.bottom, BoundaryConfidence::Likely);
  assert!(scan.interaction_events.iter().any(|event| event.kind == InteractionEventKind::StopDecision
    && event.note.as_deref() == Some("scroll_no_new_semantic_candidates_after_input")));
  assert!(!scan.known_limits.iter().any(|limit| limit.contains("max_scrolls")));
}

#[test]
fn query_scan_continues_past_no_new_candidates_until_query_is_visible() {
  let mut first = parse_sidebar_viewport(
    0,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ]),
  );
  first.viewport_fingerprint = "page-a".to_string();
  let mut second = parse_sidebar_viewport(
    1,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 106.0, 120.0, 20.0),
    ]),
  );
  second.viewport_fingerprint = "page-b".to_string();
  second.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
  let mut third = parse_sidebar_viewport(
    2,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 138.0, 120.0, 20.0),
    ]),
  );
  third.viewport_fingerprint = "page-c".to_string();
  third.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
  let mut fourth = parse_sidebar_viewport(
    3,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("我喜欢的风格 | Trance Vol.2", 32.0, 170.0, 168.0, 20.0),
    ]),
  );
  fourth.viewport_fingerprint = "page-d".to_string();
  fourth.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
  let mut observer = FakeSidebarObserver::new(vec![first, second, third, fourth]);

  let scan = scan_sidebar_with_observer_until_query(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
    "Trance Vol.2",
  );

  assert_eq!(scan.observations.len(), 4);
  assert_eq!(scan.boundary.bottom, BoundaryConfidence::Unknown);
  assert!(!scan.interaction_events.iter().any(|event| event.kind == InteractionEventKind::StopDecision
    && event.note.as_deref() == Some("scroll_no_new_semantic_candidates_after_input")));
  assert_eq!(
    scan
      .projection
      .sections
      .iter()
      .flat_map(|section| section.items.iter())
      .find(|item| item.label.contains("Trance Vol.2"))
      .map(|item| item.label.as_str()),
    Some("我喜欢的风格 | Trance Vol.2")
  );
}

#[test]
fn query_scan_skipped_top_rewind_when_query_unique_exact_in_viewport() {
  let first = parse_sidebar_viewport(
    0,
    ViewBounds::new(0.0, 469.8, 320.16, 338.2),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("3", 71.0, 500.0, 10.0, 19.0),
      ("43", 71.0, 530.0, 15.0, 12.0),
    ]),
  );
  let mut observer = FakeSidebarObserver::new(vec![first]);

  let scan = scan_sidebar_with_observer_until_query(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
    "3",
  );

  assert!(scan.known_limits.iter().any(|limit| limit == QUERY_SCAN_SKIPPED_TOP_REWIND_LIMIT));
  assert_eq!(scan.boundary.top, BoundaryConfidence::Unknown);
  assert_eq!(
    scan.projection.sections.iter().flat_map(|section| section.items.iter()).find(|item| item.label == "3").map(|item| item.label.as_str()),
    Some("3")
  );
}

#[test]
fn query_scan_applies_top_rewind_when_query_not_in_initial_viewport() {
  let mut first = parse_sidebar_viewport(
    0,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ]),
  );
  first.viewport_fingerprint = "page-a".to_string();
  let mut second = parse_sidebar_viewport(
    1,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("3", 32.0, 106.0, 10.0, 19.0),
    ]),
  );
  second.viewport_fingerprint = "page-b".to_string();
  second.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
  let mut observer = FakeSidebarObserver::new(vec![first, second]);

  let scan = scan_sidebar_with_observer_until_query(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
    "3",
  );

  assert!(scan.known_limits.iter().any(|limit| limit == QUERY_SCAN_TOP_REWIND_APPLIED_LIMIT));
  assert_eq!(scan.boundary.top, BoundaryConfidence::Likely);
  assert_eq!(
    scan.projection.sections.iter().flat_map(|section| section.items.iter()).find(|item| item.label == "3").map(|item| item.label.as_str()),
    Some("3")
  );
}

#[test]
fn scan_loop_ignores_scroll_no_new_semantic_candidates_from_noop_delivery() {
  let mut first = parse_sidebar_viewport(
    0,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ]),
  );
  first.viewport_fingerprint = "page-a".to_string();
  let mut second = parse_sidebar_viewport(
    1,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 106.0, 120.0, 20.0),
    ]),
  );
  second.viewport_fingerprint = "page-b".to_string();
  second.incoming_scroll_delivery_path = Some("noop".to_string());
  let mut third = parse_sidebar_viewport(
    2,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 138.0, 120.0, 20.0),
    ]),
  );
  third.viewport_fingerprint = "page-c".to_string();
  third.incoming_scroll_delivery_path = Some("noop".to_string());
  let mut fourth = parse_sidebar_viewport(
    3,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Fresh Playlist", 32.0, 170.0, 120.0, 20.0),
    ]),
  );
  fourth.viewport_fingerprint = "page-d".to_string();
  let mut observer = FakeSidebarObserver::new(vec![first, second, third, fourth]);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.observations.len(), 4);
  assert!(!scan.interaction_events.iter().any(|event| event.kind == InteractionEventKind::StopDecision
    && event.note.as_deref() == Some("scroll_no_new_semantic_candidates_after_input")));
}

#[test]
fn favorite_category_does_not_stop_on_no_new_candidates_before_start_landmark() {
  let mut first =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("创建的歌单", 8.0, 42.0, 110.0, 20.0)]));
  first.viewport_fingerprint = "page-a".to_string();
  let mut second =
    parse_sidebar_viewport(1, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("创建的歌单", 8.0, 42.0, 110.0, 20.0)]));
  second.viewport_fingerprint = "page-b".to_string();
  second.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
  let mut third = parse_sidebar_viewport(
    2,
    ViewBounds::new(0.0, 0.0, 240.0, 400.0),
    &fake_recognition(vec![
      ("收藏的歌单", 8.0, 42.0, 110.0, 20.0),
      ("Road Trip", 32.0, 74.0, 120.0, 20.0),
    ]),
  );
  third.viewport_fingerprint = "page-c".to_string();
  third.incoming_scroll_delivery_path = Some("window_targeted_wheel".to_string());
  let mut observer = FakeSidebarObserver::new(vec![first, second, third]);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 10,
      max_scrolls: 10,
    },
    PlaylistCategory::Favorite,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.projection.sections.len(), 1);
  assert_eq!(scan.projection.sections[0].kind, SidebarSectionKind::FavoritePlaylists);
  assert_eq!(scan.projection.sections[0].items.len(), 1);
  assert_eq!(scan.projection.sections[0].items[0].label, "Road Trip");
  assert!(!scan.interaction_events.iter().any(|event| event.note.as_deref() == Some("scroll_no_new_semantic_candidates_after_input")));
}

#[test]
fn crop_image_projects_logical_sidebar_bounds_into_capture_pixels() {
  let mut image = RgbaImage::new(16, 16);
  for y in 0..16 {
    for x in 0..16 {
      image.put_pixel(x, y, Rgba([x as u8, y as u8, 0, 255]));
    }
  }

  let cropped = crop_image(&image, ViewBounds::new(2.0, 3.0, 4.0, 5.0), 2.0);

  assert_eq!(cropped.width(), 8);
  assert_eq!(cropped.height(), 10);
  assert_eq!(cropped.get_pixel(0, 0), &Rgba([4, 6, 0, 255]));
  assert_eq!(cropped.get_pixel(7, 9), &Rgba([11, 15, 0, 255]));
}

#[test]
fn scan_loop_ignores_shared_page_budget_and_scans_until_boundary() {
  let observations = vec![
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("A", 32.0, 42.0, 80.0, 20.0)])),
    parse_sidebar_viewport(1, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("B", 32.0, 42.0, 80.0, 20.0)])),
  ];
  let mut observer = FakeSidebarObserver::new(observations);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 1,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.observations.len(), 2);
  assert!(!scan.known_limits.iter().any(|limit| limit.contains("max_pages")));
  assert!(scan.diagnostics.iter().any(|diagnostic| diagnostic.code == "no_more_fake_observations"));
}

#[test]
fn scan_loop_rewinds_to_top_before_collecting_pages() {
  let observations = vec![
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("创建的歌单", 8.0, 42.0, 110.0, 20.0)])),
    parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("Middle Playlist", 32.0, 42.0, 120.0, 20.0)]),
    ),
  ];
  let mut observer = FakeSidebarObserver::new_at(observations, 1);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 1,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.observations.len(), 2);
  assert_eq!(scan.observations[0].candidates[0].label.as_deref(), Some("创建的歌单"));
  assert_eq!(scan.observations[1].candidates[0].label.as_deref(), Some("Middle Playlist"));
  assert_eq!(scan.boundary.top, BoundaryConfidence::Likely);
}

#[test]
fn scan_loop_clears_top_seek_scroll_metadata_before_collection() {
  let observations = vec![
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &fake_recognition(vec![("创建的歌单", 8.0, 42.0, 110.0, 20.0)])),
    parse_sidebar_viewport(
      1,
      ViewBounds::new(0.0, 0.0, 240.0, 400.0),
      &fake_recognition(vec![("Middle Playlist", 32.0, 42.0, 120.0, 20.0)]),
    ),
  ];
  let mut observer = FakeSidebarObserver::new_at(observations, 1);

  let scan = scan_sidebar_with_observer(
    &mut observer,
    ScanOptions {
      max_pages: 1,
      max_scrolls: 10,
    },
    PlaylistCategory::All,
    300.0,
    DEFAULT_SCROLL_SETTLE_MS,
  );

  assert_eq!(scan.observations.len(), 2);
  assert_eq!(scan.boundary.top, BoundaryConfidence::Likely);
  assert_eq!(scan.observations[0].incoming_scroll_delivery_path, None);
}

struct FakeSidebarObserver {
  observations: Vec<SidebarViewportObservation>,
  cursor: usize,
  pending_scroll_delivery_path: Option<String>,
  last_scroll_seek_cursor: Option<usize>,
}

impl FakeSidebarObserver {
  fn new(observations: Vec<SidebarViewportObservation>) -> Self {
    Self {
      observations,
      cursor: 0,
      pending_scroll_delivery_path: None,
      last_scroll_seek_cursor: None,
    }
  }

  fn new_at(observations: Vec<SidebarViewportObservation>, cursor: usize) -> Self {
    Self {
      observations,
      cursor,
      pending_scroll_delivery_path: None,
      last_scroll_seek_cursor: None,
    }
  }
}

impl SidebarScanObserver for FakeSidebarObserver {
  fn reset_collection_phase(&mut self) {
    self.pending_scroll_delivery_path = None;
    self.last_scroll_seek_cursor = None;
  }

  fn observe_scroll_seek(&mut self, observation_index: usize) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    let cursor = self.cursor;
    let mut observation = self.observe(observation_index)?;
    if observation.scroll_motion.is_none() {
      if let Some(previous_cursor) = self.last_scroll_seek_cursor {
        if observation.incoming_scroll_delivery_path.is_some() {
          let no_motion = cursor == previous_cursor;
          observation.scroll_motion = Some(MotionEvidence {
            estimated_shift_y: if no_motion { 0 } else { 1 },
            normalized_diff: if no_motion { 0.0 } else { 0.2 },
            no_motion,
          });
        }
      }
    }
    self.last_scroll_seek_cursor = Some(cursor);
    Ok(observation)
  }
}

impl ViewObserver for FakeSidebarObserver {
  type Observation = SidebarViewportObservation;

  fn observe(&mut self, observation_index: usize) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    let mut observation = self.observations.get(self.cursor).cloned().ok_or_else(|| ParserDiagnostic {
      code: "no_more_fake_observations".to_string(),
      message: "fake sidebar observer has no more observations".to_string(),
      node_id: None,
    })?;
    let pending_scroll_delivery_path = self.pending_scroll_delivery_path.take();
    if observation.incoming_scroll_delivery_path.is_none() {
      observation.incoming_scroll_delivery_path = pending_scroll_delivery_path;
    }
    observation.observation_index = observation_index;
    observation.viewport.page_index = observation_index;
    Ok(observation)
  }

  fn observe_probe(&mut self) -> Result<SidebarViewportObservation, ParserDiagnostic> {
    self.observations.get(self.cursor).cloned().ok_or_else(|| ParserDiagnostic {
      code: "no_more_fake_observations".to_string(),
      message: "fake sidebar observer has no more observations".to_string(),
      node_id: None,
    })
  }

  fn scroll_up(&mut self) -> Result<(), ParserDiagnostic> {
    self.cursor = self.cursor.saturating_sub(1);
    self.pending_scroll_delivery_path = Some("fake_scroll_up".to_string());
    Ok(())
  }

  fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
    self.cursor += 1;
    self.pending_scroll_delivery_path = Some("fake_scroll_down".to_string());
    Ok(())
  }
}
