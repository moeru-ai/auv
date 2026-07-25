#[test]
fn view_evidence_source_defaults_to_the_typed_ocr_producer() {
  assert_eq!(serde_json::to_string(&auv_view::ViewEvidenceSource::default()).expect("serialize default evidence source"), "\"ocr_text\"");
}
