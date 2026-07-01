use crate::view_parsers::sidebar::parse::parse_sidebar_viewport;
use crate::view_parsers::sidebar::region::{
  DefaultScreenRestoreReason, broad_sidebar_probe_bounds, detect_blocking_modal,
  detect_default_screen_restore, detect_sidebar_region, fallback_playlist_sidebar_region,
  sidebar_scroll_anchor,
};
use crate::view_parsers::sidebar::test_support::fake_recognition;
use crate::{RatioRect, SidebarCandidateKind, ViewBounds};

#[test]
fn sidebar_scroll_anchor_matches_live_ratio() {
  let bounds = ViewBounds::new(0.0, 469.8, 320.16, 338.2);
  let anchor = sidebar_scroll_anchor(bounds);

  assert_eq!(anchor.0.x, bounds.x + bounds.width * 0.5);
  assert_eq!(anchor.0.y, bounds.y + bounds.height * 0.75);
}

#[test]
fn detect_sidebar_region_uses_manual_region_when_provided() {
  let region = detect_sidebar_region(
    Some(RatioRect::new(0.0, 0.1, 0.25, 0.8)),
    auv_driver::Size::new(1000.0, 800.0),
    &fake_recognition(Vec::new()),
  )
  .expect("manual sidebar region should be accepted");

  assert_eq!(region.name, Some("playlist_sidebar".to_string()));
  assert_eq!(
    region.bounds,
    Some(ViewBounds::new(0.0, 80.0, 250.0, 640.0))
  );
  assert_eq!(region.coordinate_space, Some("window".to_string()));
}

#[test]
fn detect_sidebar_region_starts_at_playlist_marker() {
  let region = detect_sidebar_region(
    None,
    auv_driver::Size::new(1646.0, 1053.0),
    &fake_recognition(vec![
      ("推荐", 8.0, 20.0, 40.0, 20.0),
      ("创建的歌单", 8.0, 443.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 485.0, 120.0, 20.0),
      ("Reverberation", 98.0, 994.0, 160.0, 20.0),
    ]),
  )
  .expect("playlist marker should define the scroll body");

  assert_eq!(
    region.bounds,
    Some(ViewBounds::new(0.0, 443.0, 344.28, 528.0))
  );
}

#[test]
fn detect_sidebar_region_falls_back_to_full_sidebar_without_playlist_marker() {
  let region = detect_sidebar_region(
    None,
    auv_driver::Size::new(1000.0, 800.0),
    &fake_recognition(vec![("推荐", 8.0, 20.0, 40.0, 20.0)]),
  )
  .expect("navigation marker should preserve full sidebar fallback");

  assert_eq!(region.bounds, Some(ViewBounds::new(0.0, 0.0, 228.0, 718.0)));
}

#[test]
fn detect_sidebar_region_handles_negative_window_height_without_panic() {
  let region = detect_sidebar_region(
    None,
    auv_driver::Size::new(1646.0, -1.0),
    &fake_recognition(vec![
      ("推荐", 8.0, 20.0, 40.0, 20.0),
      ("创建的歌单", 8.0, 443.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 485.0, 120.0, 20.0),
    ]),
  )
  .expect("negative window height should not crash sidebar detection");

  let bounds = region.bounds.expect("bounds should still be produced");
  assert!(bounds.y >= 0.0, "y must be floored to 0, got {}", bounds.y);
}

#[test]
fn detect_sidebar_region_rejects_unanchored_playlist_like_rows() {
  let error = detect_sidebar_region(
    None,
    auv_driver::Size::new(1000.0, 800.0),
    &fake_recognition(vec![
      ("Future Garage", 72.0, 320.0, 140.0, 20.0),
      ("Progressive House", 72.0, 366.0, 170.0, 20.0),
      ("Trance", 72.0, 412.0, 80.0, 20.0),
    ]),
  )
  .expect_err("playlist-like rows without a sidebar marker should not anchor the sidebar");

  assert_eq!(error.code, "sidebar_region_not_found");
}

#[test]
fn detect_sidebar_region_ignores_main_content_without_sidebar_marker() {
  let error = detect_sidebar_region(
    None,
    auv_driver::Size::new(1000.0, 800.0),
    &fake_recognition(vec![
      ("网易云音乐", 52.0, 40.0, 100.0, 20.0),
      ("Future Garage", 72.0, 320.0, 140.0, 20.0),
      ("Progressive House", 72.0, 366.0, 170.0, 20.0),
      ("Trance", 72.0, 412.0, 80.0, 20.0),
      ("每日推荐", 430.0, 300.0, 120.0, 30.0),
      ("推荐歌单", 520.0, 520.0, 150.0, 30.0),
    ]),
  )
  .expect_err("main content rows should not anchor the sidebar");

  assert_eq!(error.code, "sidebar_region_not_found");
}

#[test]
fn fallback_playlist_sidebar_region_starts_below_library_rows() {
  let region = fallback_playlist_sidebar_region(auv_driver::Size::new(1418.0, 1002.0));
  let bounds = region.bounds.expect("fallback should carry bounds");

  assert_eq!(region.name, Some("playlist_sidebar".to_string()));
  assert!(bounds.y >= 220.0);
  assert!(bounds.y > 0.0);
  assert!(bounds.height > 0.0);
  assert!(bounds.width >= 280.0);
}

#[test]
fn fallback_playlist_sidebar_region_handles_negative_window_without_silent_negative() {
  let region = fallback_playlist_sidebar_region(auv_driver::Size::new(-10.0, -10.0));
  let bounds = region.bounds.expect("bounds should still be produced");

  assert!(bounds.x >= 0.0, "x must be ≥ 0, got {}", bounds.x);
  assert!(bounds.y >= 0.0, "y must be ≥ 0, got {}", bounds.y);
  assert!(
    bounds.width >= 0.0,
    "width must be ≥ 0, got {}",
    bounds.width
  );
  assert!(
    bounds.height >= 0.0,
    "height must be ≥ 0, got {}",
    bounds.height
  );
}

#[test]
fn broad_sidebar_probe_bounds_handles_negative_window_width_without_silent_negative() {
  let bounds = broad_sidebar_probe_bounds(auv_driver::Size::new(-50.0, 800.0));

  assert!(
    bounds.width >= 0.0,
    "probe width must be ≥ 0, got {}",
    bounds.width
  );
  assert!(
    bounds.height >= 0.0,
    "probe height must be ≥ 0, got {}",
    bounds.height
  );
}

#[test]
fn detect_default_screen_restore_targets_song_detail_back_affordance() {
  let restore = detect_default_screen_restore(
    &fake_recognition(vec![
      ("私藏推荐", 90.0, 86.0, 120.0, 28.0),
      ("评论", 760.0, 182.0, 80.0, 28.0),
      ("收藏", 880.0, 182.0, 80.0, 28.0),
    ]),
    auv_driver::Size::new(1646.0, 1053.0),
  )
  .expect("song detail screen should expose a restore click");

  assert_eq!(restore.reason, DefaultScreenRestoreReason::SongDetailScreen);
  assert_eq!(restore.point, auv_driver::Point::new(82.602, 16.336));
}

#[test]
fn detect_default_screen_restore_ignores_normal_sidebar_screen() {
  let restore = detect_default_screen_restore(
    &fake_recognition(vec![
      ("推荐", 8.0, 20.0, 40.0, 20.0),
      ("评论", 760.0, 182.0, 80.0, 28.0),
      ("收藏", 880.0, 182.0, 80.0, 28.0),
    ]),
    auv_driver::Size::new(1646.0, 1053.0),
  );

  assert_eq!(restore, None);
}

#[test]
fn detect_default_screen_restore_ignores_blocking_modal() {
  let restore = detect_default_screen_restore(
    &fake_recognition(vec![
      ("评论", 760.0, 182.0, 80.0, 28.0),
      ("收藏", 880.0, 182.0, 80.0, 28.0),
      ("打开", 760.0, 720.0, 80.0, 32.0),
      ("取消", 860.0, 720.0, 80.0, 32.0),
    ]),
    auv_driver::Size::new(1646.0, 1053.0),
  );

  assert_eq!(restore, None);
}

#[test]
fn detect_blocking_modal_reports_cancel_or_open_dialog_markers() {
  let diagnostic = detect_blocking_modal(&fake_recognition(vec![
    ("打开", 760.0, 720.0, 80.0, 32.0),
    ("取消", 860.0, 720.0, 80.0, 32.0),
  ]))
  .expect("open dialog markers should be reported as blocking modal");

  assert_eq!(diagnostic.code, "blocking_modal_dialog");
}

#[test]
fn detect_sidebar_region_expands_short_body_at_default_window() {
  let region = detect_sidebar_region(
    None,
    auv_driver::Size::new(1057.0, 752.0),
    &fake_recognition(vec![
      ("推荐", 8.0, 20.0, 40.0, 20.0),
      ("创建的歌单", 8.0, 534.0, 110.0, 20.0),
    ]),
  )
  .expect("default-window playlist marker should expand the sidebar body");

  let bounds = region.bounds.expect("expanded region should carry bounds");
  assert!(bounds.y.is_finite());
  assert!(bounds.height.is_finite());
  assert!(
    bounds.y < 534.0,
    "expanded top should sit above marker, got y={}",
    bounds.y
  );
  assert!(
    bounds.height >= 285.0,
    "expanded body should meet SIGNOFF min height, got {}",
    bounds.height
  );
}

#[test]
fn detect_sidebar_region_preserves_tall_body_when_marker_high() {
  let region = detect_sidebar_region(
    None,
    auv_driver::Size::new(1646.0, 1053.0),
    &fake_recognition(vec![
      ("推荐", 8.0, 20.0, 40.0, 20.0),
      ("创建的歌单", 8.0, 443.0, 110.0, 20.0),
      ("Coding BGM", 32.0, 485.0, 120.0, 20.0),
    ]),
  )
  .expect("tall playlist body should keep marker-aligned top");

  assert_eq!(
    region.bounds,
    Some(ViewBounds::new(0.0, 443.0, 344.28, 528.0))
  );
}

#[test]
fn detect_sidebar_region_short_body_respects_fallback_floor() {
  let floor_region = detect_sidebar_region(
    None,
    auv_driver::Size::new(600.0, 500.0),
    &fake_recognition(vec![
      ("推荐", 8.0, 20.0, 40.0, 20.0),
      ("创建的歌单", 8.0, 400.0, 110.0, 20.0),
    ]),
  )
  .expect("short body should clamp to fallback floor when bottom allows");
  let floor_bounds = floor_region
    .bounds
    .expect("floor branch should carry bounds");
  let fallback_y = fallback_playlist_sidebar_region(auv_driver::Size::new(600.0, 500.0))
    .bounds
    .expect("fallback region should carry bounds")
    .y;
  assert_eq!(floor_bounds.y, fallback_y);

  let tiny_region = detect_sidebar_region(
    None,
    auv_driver::Size::new(400.0, 100.0),
    &fake_recognition(vec![
      ("推荐", 8.0, 10.0, 40.0, 20.0),
      ("创建的歌单", 8.0, 12.0, 110.0, 20.0),
    ]),
  )
  .expect("tiny window should still return finite sidebar bounds");
  let tiny_bounds = tiny_region.bounds.expect("tiny branch should carry bounds");
  assert!(tiny_bounds.y.is_finite());
  assert!(tiny_bounds.height.is_finite());
  assert!(tiny_bounds.y >= 0.0);
  assert!(tiny_bounds.height >= 0.0);
  assert_ne!(
    tiny_bounds.y,
    fallback_playlist_sidebar_region(auv_driver::Size::new(400.0, 100.0))
      .bounds
      .expect("fallback region should carry bounds")
      .y,
    "bottom below fallback_y must not force y onto the fallback floor"
  );
}

#[test]
fn detect_sidebar_region_resized_window_item_stays_in_viewport() {
  let region = detect_sidebar_region(
    None,
    auv_driver::Size::new(1200.0, 820.0),
    &fake_recognition(vec![
      ("推荐", 8.0, 20.0, 40.0, 20.0),
      ("创建的歌单", 8.0, 536.0, 110.0, 20.0),
      ("VIP黑胶专属歌单", 32.0, 676.0, 150.0, 20.0),
    ]),
  )
  .expect("resized window should expand short playlist body");

  let bounds = region.bounds.expect("resized region should carry bounds");
  let item_center_y = 676.0 + 10.0;
  assert!(
    item_center_y >= bounds.y && item_center_y <= bounds.y + bounds.height,
    "playlist item center {:?} should stay inside {:?}",
    item_center_y,
    bounds
  );
}

#[test]
fn detect_sidebar_region_expanded_bounds_parse_playlist_item() {
  let window_size = auv_driver::Size::new(1057.0, 752.0);
  let recognition = fake_recognition(vec![
    ("推荐", 8.0, 20.0, 40.0, 20.0),
    ("创建的歌单", 8.0, 534.0, 110.0, 20.0),
    ("VIP黑胶专属歌单", 32.0, 580.0, 150.0, 20.0),
  ]);
  let region = detect_sidebar_region(None, window_size, &recognition)
    .expect("expanded default window should detect sidebar region");
  let bounds = region.bounds.expect("expanded region should carry bounds");
  let observation = parse_sidebar_viewport(0, bounds, &recognition);

  assert!(
    observation
      .candidates
      .iter()
      .any(|candidate| candidate.kind == SidebarCandidateKind::PlaylistItem),
    "expanded viewport should still classify playlist rows as PlaylistItem"
  );
}

#[test]
fn detect_sidebar_region_expanded_bounds_rejects_library_nav_rows() {
  let window_size = auv_driver::Size::new(1057.0, 752.0);
  let recognition = fake_recognition(vec![
    ("推荐", 8.0, 400.0, 40.0, 20.0),
    ("发现音乐", 8.0, 430.0, 60.0, 20.0),
    ("创建的歌单", 8.0, 534.0, 110.0, 20.0),
    ("VIP黑胶专属歌单", 32.0, 580.0, 150.0, 20.0),
  ]);
  let region = detect_sidebar_region(None, window_size, &recognition)
    .expect("expanded default window should detect sidebar region");
  let bounds = region.bounds.expect("expanded region should carry bounds");
  let observation = parse_sidebar_viewport(0, bounds, &recognition);

  assert!(
    observation
      .candidates
      .iter()
      .any(|candidate| candidate.kind == SidebarCandidateKind::PlaylistItem),
    "playlist row should still parse after expansion"
  );
  assert!(
    observation.candidates.iter().all(|candidate| candidate.kind
      != SidebarCandidateKind::PlaylistItem
      || candidate.label.as_deref() == Some("VIP黑胶专属歌单")),
    "library/nav rows inside expanded viewport must not become PlaylistItem candidates"
  );
  assert!(
    observation.candidates.iter().any(|candidate| {
      matches!(
        candidate.kind,
        SidebarCandidateKind::NavigationItem | SidebarCandidateKind::SectionHeader
      ) && matches!(candidate.label.as_deref(), Some("推荐") | Some("发现音乐"))
    }),
    "library/nav rows should remain navigation or section headers"
  );
}
