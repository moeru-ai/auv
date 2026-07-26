use super::*;

#[test]
fn decode_ocr_text_rejects_mismatched_match_vectors() {
  let error = decode_ocr_text_response(DecodedOcrTextResponse {
    recognized_at: "2026-05-20T00:00:00Z".to_string(),
    image_path: "sample.png".to_string(),
    image_width: 100,
    image_height: 100,
    query: "play".to_string(),
    exact: false,
    case_sensitive: false,
    normalized_query: "play".to_string(),
    crop_enabled: false,
    crop_x: 0,
    crop_y: 0,
    crop_width: 0,
    crop_height: 0,
    ocr_scale_factor: 1.0,
    match_indices: vec![0, 1],
    texts: vec!["Play".to_string(), "Pause".to_string()],
    confidences: vec![0.99],
    x_values: vec![1, 2],
    y_values: vec![3, 4],
    width_values: vec![5, 6],
    height_values: vec![7, 8],
    error_message: None,
    recovery_hint: None,
  })
  .unwrap_err();

  assert!(error.contains("mismatched OCR match vector lengths"));
}

#[test]
fn decode_visual_rows_preserves_row_order() {
  let capture = decode_visual_rows_response(DecodedVisualRowsResponse {
    detected_at: "2026-05-20T00:00:00Z".to_string(),
    image_path: "sample.png".to_string(),
    image_width: 300,
    image_height: 300,
    crop_enabled: false,
    crop_x: 0,
    crop_y: 0,
    crop_width: 0,
    crop_height: 0,
    analysis_strip_x: 0,
    analysis_strip_y: 0,
    analysis_strip_width: 100,
    analysis_strip_height: 300,
    row_indices: vec![0, 1],
    x_values: vec![10, 10],
    y_values: vec![100, 200],
    width_values: vec![80, 80],
    height_values: vec![30, 30],
    peak_densities: vec![0.2, 0.3],
    error_message: None,
    recovery_hint: None,
  })
  .unwrap();

  assert_eq!(capture.rows.rows[0].bounds.y, 100);
  assert_eq!(capture.rows.rows[1].bounds.y, 200);
}
