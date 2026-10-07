use super::NeteasePolicy;
use crate::recognition_test_data::fake_recognition;
use crate::view_parsers::sidebar::parse::parse_sidebar_viewport;
use crate::{SidebarSectionKind, ViewBounds};

// ROOT CAUSE:
//
// If OCR read the created-playlists header differently between frames (the
// disclosure chevron read as `～` in one frame and dropped in the other), the
// section key differed and reconstruction opened a second section.
//
// Before the fix, every row seen in both frames was listed twice.
// The fix keys playlist collection sections by kind alone.
#[test]
fn reconstruct_merges_a_playlist_section_whose_header_text_changes_between_frames() {
  let viewport = ViewBounds::new(0.0, 0.0, 330.0, 400.0);
  let first = parse_sidebar_viewport(
    0,
    viewport,
    &fake_recognition(vec![
      ("创建的歌单 215", 33.5, 42.0, 93.0, 14.0),
      ("Coding BGM", 71.5, 74.0, 120.0, 14.0),
    ]),
  );
  let second = parse_sidebar_viewport(
    1,
    viewport,
    &fake_recognition(vec![
      ("创建的歌单 215～", 33.5, 20.0, 104.5, 14.0),
      ("Coding BGM", 71.5, 52.0, 120.0, 14.0),
      ("Jazz", 71.5, 84.0, 60.0, 14.0),
    ]),
  );

  let output = auv_view::reconstruct(&NeteasePolicy, &[first, second], viewport);

  assert_eq!(output.sections.len(), 1);
  assert_eq!(output.sections[0].kind, SidebarSectionKind::MyPlaylists);
  let labels = output.sections[0].items.iter().map(|item| item.label.as_str()).collect::<Vec<_>>();
  assert_eq!(labels, vec!["Coding BGM", "Jazz"]);
}
