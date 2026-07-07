pub(crate) fn fake_recognition(rows: Vec<(&str, f64, f64, f64, f64)>) -> auv_driver::vision::TextRecognition {
  auv_driver::vision::TextRecognition {
    text: rows.iter().map(|(text, _, _, _, _)| *text).collect::<Vec<_>>().join("\n"),
    regions: rows
      .into_iter()
      .map(|(text, x, y, width, height)| auv_driver::vision::RecognizedText {
        text: text.to_string(),
        bounds: auv_driver::Rect::new(x, y, width, height),
        confidence: Some(0.92),
      })
      .collect(),
  }
}
