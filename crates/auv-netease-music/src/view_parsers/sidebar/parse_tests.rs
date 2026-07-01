use crate::view_parsers::sidebar::parse::parse_sidebar_viewport;
use crate::view_parsers::sidebar::target_probe::{
  SidebarTargetMissReason, analyze_sidebar_target_probe,
};
use crate::view_parsers::sidebar::test_support::fake_recognition;
use crate::{SidebarCandidateKind, ViewBounds, ViewEvidenceSource};

#[test]
fn analyze_sidebar_target_probe_finds_playlist_item() {
  let recognition = fake_recognition(vec![
    ("创建的歌单", 8.0, 480.0, 110.0, 20.0),
    ("16", 70.0, 512.0, 14.0, 11.0),
    ("21", 70.0, 544.0, 14.0, 11.0),
  ]);
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 469.8, 320.0, 338.2), &recognition);
  let probe = analyze_sidebar_target_probe(&observation, "16", "16");

  assert_eq!(probe.playlist_item_count, 2);
  assert_eq!(probe.result, Some(ViewBounds::new(70.0, 512.0, 14.0, 11.0)));
  assert!(probe.miss_reason.is_none());
}

#[test]
fn analyze_sidebar_target_probe_reports_misclassified_numeric() {
  let recognition = fake_recognition(vec![
    ("创建的歌单", 8.0, 480.0, 110.0, 20.0),
    ("16", 10.0, 512.0, 14.0, 11.0),
    ("21", 70.0, 544.0, 14.0, 11.0),
  ]);
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 469.8, 320.0, 338.2), &recognition);
  let probe = analyze_sidebar_target_probe(&observation, "16", "16");

  assert!(probe.result.is_none());
  match probe.miss_reason.as_ref() {
    Some(SidebarTargetMissReason::LabelNotMatched {
      ocr_contains_target,
      misclassified,
      ..
    }) => {
      assert_eq!(ocr_contains_target, &vec!["16".to_string()]);
      assert_eq!(misclassified.len(), 1);
      assert_eq!(misclassified[0].label, "16");
    }
    other => panic!("expected label-not-matched reason, got {other:?}"),
  }
}

#[test]
fn analyze_sidebar_target_probe_reports_label_not_matched() {
  let recognition = fake_recognition(vec![
    ("创建的歌单", 8.0, 480.0, 110.0, 20.0),
    ("21", 70.0, 512.0, 14.0, 11.0),
    ("34", 70.0, 544.0, 14.0, 11.0),
  ]);
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 469.8, 320.0, 338.2), &recognition);
  let probe = analyze_sidebar_target_probe(&observation, "16", "16");

  assert!(probe.result.is_none());
  match probe.miss_reason.as_ref() {
    Some(SidebarTargetMissReason::LabelNotMatched {
      playlist_labels, ..
    }) => {
      assert_eq!(playlist_labels, &vec!["21".to_string(), "34".to_string()]);
    }
    other => panic!("expected label-not-matched reason, got {other:?}"),
  }
}

#[test]
fn parse_viewport_classifies_sections_and_playlist_items() {
  let recognition = fake_recognition(vec![
    ("推荐", 8.0, 10.0, 40.0, 20.0),
    ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
    ("Coding BGM", 32.0, 74.0, 120.0, 20.0),
    ("Jazz", 32.0, 106.0, 80.0, 20.0),
  ]);
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &recognition);

  assert_eq!(observation.candidates.len(), 4);
  assert_eq!(
    observation.candidates[1].kind,
    SidebarCandidateKind::SectionHeader
  );
  assert_eq!(
    observation.candidates[1].label,
    Some("创建的歌单".to_string())
  );
  assert_eq!(
    observation.candidates[2].kind,
    SidebarCandidateKind::PlaylistItem
  );
  assert_eq!(
    observation.candidates[2].label,
    Some("Coding BGM".to_string())
  );
  assert_eq!(
    observation.evidence_nodes[2].source,
    ViewEvidenceSource::OcrText
  );
}

#[test]
fn parse_viewport_ignores_bottom_player_bar_outside_sidebar_bounds() {
  let recognition = fake_recognition(vec![
    ("创建的歌单", 8.0, 443.0, 110.0, 20.0),
    ("Coding BGM", 72.0, 485.0, 120.0, 20.0),
    ("Reverberation", 98.0, 994.0, 160.0, 20.0),
    ("1w+", 322.0, 1003.0, 19.0, 9.0),
    ("伊藤賢", 98.0, 1018.0, 45.0, 17.0),
  ]);

  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 443.0, 344.0, 528.0), &recognition);

  assert!(
    observation
      .candidates
      .iter()
      .any(|candidate| candidate.label.as_deref() == Some("Coding BGM"))
  );
  assert!(
    observation
      .candidates
      .iter()
      .all(|candidate| candidate.label.as_deref() != Some("Reverberation"))
  );
  assert!(
    observation
      .candidates
      .iter()
      .all(|candidate| candidate.label.as_deref() != Some("1w+"))
  );
}

#[test]
fn parse_viewport_assigns_unique_candidate_ids_for_duplicate_cjk_labels() {
  let recognition = fake_recognition(vec![
    ("创建的歌单", 8.0, 42.0, 110.0, 20.0),
    ("中文歌单", 32.0, 74.0, 120.0, 20.0),
    ("中文歌单", 32.0, 106.0, 120.0, 20.0),
  ]);
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 240.0, 400.0), &recognition);
  let candidate_ids = observation
    .candidates
    .iter()
    .map(|candidate| candidate.id.as_str())
    .collect::<Vec<_>>();
  let unique_candidate_ids = candidate_ids
    .iter()
    .copied()
    .collect::<std::collections::HashSet<_>>();

  assert_eq!(observation.candidates.len(), 3);
  assert_eq!(
    candidate_ids,
    vec![
      "obs0.candidate.ocr0._____",
      "obs0.candidate.ocr1.____",
      "obs0.candidate.ocr2.____"
    ]
  );
  assert_eq!(unique_candidate_ids.len(), observation.candidates.len());
}

#[test]
fn parse_single_digit_playlist_at_sidebar_x_becomes_candidate() {
  // obs-0005 live fixture (A6c-8): OCR recognized "1" at sidebar playlist row x=71.
  let recognition = fake_recognition(vec![("1", 71.0, 591.0, 10.0, 19.0)]);
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 469.8, 320.16, 338.2), &recognition);

  assert_eq!(observation.evidence_nodes.len(), 1);
  assert_eq!(observation.candidates.len(), 1);
  assert_eq!(
    observation.candidates[0].kind,
    SidebarCandidateKind::PlaylistItem
  );
  assert_eq!(observation.candidates[0].label.as_deref(), Some("1"));
}

#[test]
fn parse_single_digit_rejected_below_playlist_x_threshold() {
  let recognition = fake_recognition(vec![("1", 10.0, 591.0, 10.0, 19.0)]);
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 469.8, 320.16, 338.2), &recognition);

  assert!(observation.candidates.is_empty());
}

#[test]
fn parse_single_non_digit_char_still_rejected_at_playlist_x() {
  let recognition = fake_recognition(vec![("A", 71.0, 591.0, 10.0, 19.0)]);
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 469.8, 320.16, 338.2), &recognition);

  assert!(observation.candidates.is_empty());
}

#[test]
fn parse_two_digit_playlist_label_unchanged() {
  let recognition = fake_recognition(vec![("43", 71.0, 500.0, 15.0, 12.0)]);
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 469.8, 320.16, 338.2), &recognition);

  assert_eq!(observation.candidates.len(), 1);
  assert_eq!(observation.candidates[0].label.as_deref(), Some("43"));
}

#[test]
fn parse_viewport_treats_playlist_named_rows_as_items_not_sections() {
  let recognition = fake_recognition(vec![
    ("创建的歌单 215", 8.0, 42.0, 120.0, 20.0),
    ("年度精选歌单", 72.0, 74.0, 180.0, 20.0),
    ("猫音歌单", 72.0, 106.0, 120.0, 20.0),
  ]);
  let observation =
    parse_sidebar_viewport(0, ViewBounds::new(0.0, 0.0, 280.0, 400.0), &recognition);

  assert_eq!(
    observation.candidates[0].kind,
    SidebarCandidateKind::SectionHeader
  );
  assert_eq!(
    observation.candidates[1].kind,
    SidebarCandidateKind::PlaylistItem
  );
  assert_eq!(
    observation.candidates[2].kind,
    SidebarCandidateKind::PlaylistItem
  );
}
