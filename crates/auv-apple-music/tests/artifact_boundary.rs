use auv_apple_music::{MetadataSource, PlaybackState, PlaybackStatus, ProbeResult, SearchVerification, SearchVerificationStatus};
use serde_json::Value;

fn assert_no_artifact_locator(value: Value) {
  let object = value.as_object().expect("public result serializes as an object");
  assert!(!object.contains_key("artifact"), "public result leaked an artifact locator: {object:?}");
  assert!(
    object.values().all(|value| value.as_str().is_none_or(|value| !value.contains(std::path::MAIN_SEPARATOR))),
    "public result leaked a platform path: {object:?}"
  );
}

#[test]
fn public_results_do_not_carry_artifact_locators() {
  assert_no_artifact_locator(
    serde_json::to_value(ProbeResult {
      command: "probe-macos".into(),
      bundle_id: "com.apple.Music".into(),
      activated: true,
      ax_snapshot_captured: true,
      node_count: 0,
      search_field_candidates: Vec::new(),
      toolbar_inspections: Vec::new(),
      diagnostics: Vec::new(),
    })
    .unwrap(),
  );
  assert_no_artifact_locator(
    serde_json::to_value(PlaybackStatus {
      command: "playback.status".into(),
      window_title: Some("Apple Music".into()),
      state: PlaybackState::Paused,
      track_title: None,
      artist: None,
      metadata_source: MetadataSource::NotFound,
      diagnostics: Vec::new(),
    })
    .unwrap(),
  );
  assert_no_artifact_locator(
    serde_json::to_value(SearchVerification {
      status: SearchVerificationStatus::Verified,
      method: "ui_automation".into(),
      observed_text: Some("query".into()),
    })
    .unwrap(),
  );
}
