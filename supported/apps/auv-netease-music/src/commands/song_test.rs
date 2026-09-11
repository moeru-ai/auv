use super::*;

#[test]
fn song_list_bounds_include_the_live_title_column() {
  // ROOT CAUSE:
  //
  // If the current 1645x957 NetEase layout is used, visible row indexes start
  // near x=360 and titles near x=450, but the old fixed 30% left edge started
  // OCR at x=493.5 and returned an empty song list.
  //
  // Before the fix, the scan cropped away both the index and title anchors.
  // The fix keeps the full row-leading area inside the song-list viewport.
  let bounds = song_list_bounds(auv_driver::Size::new(1645.0, 957.0));

  assert!(bounds.x <= 360.0, "song-list left edge must include live row indexes, got {}", bounds.x);
  assert!(bounds.x + bounds.width >= 1600.0);
}

#[test]
fn parse_song_list_rows_reads_the_current_live_layout() {
  let bounds = ViewBounds::new(329.0, 220.0, 1292.0, 655.0);
  let recognition = TextRecognition {
    text: "08\nMagnolia\nM2U / Guriri\nDeemo 原创音乐集\n02:30".to_string(),
    regions: vec![
      recognized("08", 362.0, 246.0, 18.0, 20.0),
      recognized("Magnolia", 452.0, 236.0, 92.0, 22.0),
      recognized("M2U / Guriri", 452.0, 262.0, 120.0, 18.0),
      recognized("Deemo 原创音乐集", 1055.0, 246.0, 160.0, 20.0),
      recognized("02:30", 1528.0, 246.0, 52.0, 20.0),
    ],
  };

  let rows = parse_song_list_rows(0, bounds, &recognition);

  assert_eq!(rows.len(), 1);
  assert_eq!(rows[0].index, Some(8));
  assert_eq!(rows[0].title, "Magnolia");
  assert!(rows[0].row_text.contains("M2U / Guriri"));
}

fn recognized(text: &str, x: f64, y: f64, width: f64, height: f64) -> auv_driver::vision::RecognizedText {
  auv_driver::vision::RecognizedText {
    text: text.to_string(),
    bounds: auv_driver::Rect::new(x, y, width, height),
    confidence: Some(0.9),
  }
}
