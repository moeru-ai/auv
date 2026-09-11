use super::*;

impl PlaylistSidebarScan {
  fn from_projection_for_tests(projection: PlaylistSidebarProjection) -> Self {
    let mut scan = Self::empty(ScanAppContext::default(), ScanWindowContext::default(), ViewRegionRecord::default());
    scan.projection = projection;
    scan
  }
}

#[test]
fn playlist_select_target_resolves_candidate_bounds_from_scan_observation() {
  let candidate_id = "obs2.candidate.ocr1.human_machine";
  let bounds = ViewBounds::new(71.0, 166.0, 72.0, 15.0);
  let mut scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
    sections: vec![SidebarSection {
      id: "section-created".to_string(),
      kind: SidebarSectionKind::MyPlaylists,
      label: Some("创建的歌单".to_string()),
      items: vec![PlaylistSidebarItem {
        id: "item-human-machine".to_string(),
        label: "人造器械".to_string(),
        section_hint: Some(SidebarSectionKind::MyPlaylists),
        confidence: Confidence::High,
        candidate_id: Some(candidate_id.to_string()),
        anchor_id: Some("anchor-human-machine".to_string()),
      }],
    }],
  });
  scan.observations.push(SidebarViewportObservation {
    observation_index: 2,
    candidates: vec![SidebarViewportCandidate {
      id: candidate_id.to_string(),
      kind: SidebarCandidateKind::PlaylistItem,
      label: Some("人造器械".to_string()),
      bounds: Some(bounds),
      evidence_ids: Vec::new(),
      confidence: Confidence::High,
    }],
    ..SidebarViewportObservation::default()
  });

  let target = scan.select_target("人造").expect("single playlist match should resolve");

  assert_eq!(target.label, "人造器械");
  assert_eq!(target.item_id, "item-human-machine");
  assert_eq!(target.anchor_id.as_deref(), Some("anchor-human-machine"));
  assert_eq!(target.observation_index, Some(2));
  assert_eq!(target.bounds, Some(bounds));
}

#[test]
fn playlist_select_target_resolves_by_candidate_id() {
  let candidate_id = "obs6.candidate.ocr4.trance_vol_2";
  let bounds = ViewBounds::new(72.0, 492.0, 148.0, 16.0);
  let mut scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
    sections: vec![SidebarSection {
      id: "section-favorite".to_string(),
      kind: SidebarSectionKind::FavoritePlaylists,
      label: Some("收藏的歌单".to_string()),
      items: vec![PlaylistSidebarItem {
        id: "item-trance-vol-2".to_string(),
        label: "我喜欢的风格 | Trance Vol.2".to_string(),
        section_hint: Some(SidebarSectionKind::FavoritePlaylists),
        confidence: Confidence::High,
        candidate_id: Some(candidate_id.to_string()),
        anchor_id: Some("anchor-trance-vol-2".to_string()),
      }],
    }],
  });
  scan.observations.push(SidebarViewportObservation {
    observation_index: 6,
    candidates: vec![SidebarViewportCandidate {
      id: candidate_id.to_string(),
      kind: SidebarCandidateKind::PlaylistItem,
      label: Some("我喜欢的风格 | Trance Vol.2".to_string()),
      bounds: Some(bounds),
      evidence_ids: Vec::new(),
      confidence: Confidence::High,
    }],
    ..SidebarViewportObservation::default()
  });

  let target = scan.select_target_by_candidate_id(candidate_id).expect("candidate id should resolve");

  assert_eq!(target.label, "我喜欢的风格 | Trance Vol.2");
  assert_eq!(target.candidate_id.as_deref(), Some(candidate_id));
  assert_eq!(target.observation_index, Some(6));
  assert_eq!(target.bounds, Some(bounds));
}

#[test]
fn playlist_select_target_prefers_exact_numeric_label() {
  let scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
    sections: vec![SidebarSection {
      id: "section-created".to_string(),
      kind: SidebarSectionKind::MyPlaylists,
      label: Some("创建的歌单".to_string()),
      items: vec![
        PlaylistSidebarItem {
          id: "item-43".to_string(),
          label: "43".to_string(),
          section_hint: Some(SidebarSectionKind::MyPlaylists),
          confidence: Confidence::High,
          candidate_id: None,
          anchor_id: None,
        },
        PlaylistSidebarItem {
          id: "item-3".to_string(),
          label: "3".to_string(),
          section_hint: Some(SidebarSectionKind::MyPlaylists),
          confidence: Confidence::High,
          candidate_id: None,
          anchor_id: None,
        },
      ],
    }],
  });

  let target = scan.select_target("3").expect("exact numeric match");
  assert_eq!(target.label, "3");
  assert_eq!(target.item_id, "item-3");
}

#[test]
fn playlist_select_target_reports_ambiguous_contains_numeric_query() {
  let scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
    sections: vec![SidebarSection {
      id: "section-created".to_string(),
      kind: SidebarSectionKind::MyPlaylists,
      label: Some("创建的歌单".to_string()),
      items: vec![
        PlaylistSidebarItem {
          id: "item-43".to_string(),
          label: "43".to_string(),
          section_hint: Some(SidebarSectionKind::MyPlaylists),
          confidence: Confidence::High,
          candidate_id: None,
          anchor_id: None,
        },
        PlaylistSidebarItem {
          id: "item-13".to_string(),
          label: "13".to_string(),
          section_hint: Some(SidebarSectionKind::MyPlaylists),
          confidence: Confidence::High,
          candidate_id: None,
          anchor_id: None,
        },
      ],
    }],
  });

  let error = scan.select_target("3").expect_err("ambiguous contains");
  assert!(error.contains("matched 2 items"));
}

#[test]
fn playlist_ref_resolves_duplicate_labels_by_collection() {
  let scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
    sections: vec![
      SidebarSection {
        id: "section-created".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("创建的歌单".to_string()),
        items: vec![PlaylistSidebarItem {
          id: "created-focus".to_string(),
          label: "Focus".to_string(),
          section_hint: Some(SidebarSectionKind::MyPlaylists),
          confidence: Confidence::High,
          candidate_id: None,
          anchor_id: None,
        }],
      },
      SidebarSection {
        id: "section-favorite".to_string(),
        kind: SidebarSectionKind::FavoritePlaylists,
        label: Some("收藏的歌单".to_string()),
        items: vec![PlaylistSidebarItem {
          id: "favorite-focus".to_string(),
          label: "Focus".to_string(),
          section_hint: Some(SidebarSectionKind::FavoritePlaylists),
          confidence: Confidence::High,
          candidate_id: None,
          anchor_id: None,
        }],
      },
    ],
  });
  let reference = PlaylistRef::new(PlaylistSection::Favorite, "Focus").expect("valid reference");

  let target = scan.select_target_ref(&reference).expect("section should disambiguate the label");

  assert_eq!(target.item_id, "favorite-focus");
  assert_eq!(target.section_kind, SidebarSectionKind::FavoritePlaylists);
}

#[test]
fn playlist_ref_rejects_duplicates_inside_one_collection() {
  let scan = PlaylistSidebarScan::from_projection_for_tests(PlaylistSidebarProjection {
    sections: vec![SidebarSection {
      id: "section-created".to_string(),
      kind: SidebarSectionKind::MyPlaylists,
      label: Some("创建的歌单".to_string()),
      items: vec![
        PlaylistSidebarItem {
          id: "created-focus-a".to_string(),
          label: "Focus".to_string(),
          section_hint: Some(SidebarSectionKind::MyPlaylists),
          confidence: Confidence::High,
          candidate_id: None,
          anchor_id: None,
        },
        PlaylistSidebarItem {
          id: "created-focus-b".to_string(),
          label: "Focus".to_string(),
          section_hint: Some(SidebarSectionKind::MyPlaylists),
          confidence: Confidence::High,
          candidate_id: None,
          anchor_id: None,
        },
      ],
    }],
  });
  let reference = PlaylistRef::new(PlaylistSection::Created, "Focus").expect("valid reference");

  let error = scan.select_target_ref(&reference).expect_err("same-section duplicates must remain ambiguous");

  assert!(error.contains("matched 2 items"));
}
