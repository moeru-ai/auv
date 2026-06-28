use std::path::PathBuf;

use auv_game_osu::evaluate_detection_fixture;

#[test]
fn detection_fixture_eval_writes_report_with_provenance() {
  let manifest_dir =
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/osu_eval_run_artifacts");
  let detections_path =
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/osu_eval_detection");
  let output_dir = tempfile::tempdir().expect("tempdir");

  let result = evaluate_detection_fixture(&auv_game_osu::DetectionEvalInputs {
    run_artifact_dir: manifest_dir,
    detections_path,
    output_dir: output_dir.path().join("report"),
  })
  .expect("fixture detection eval should succeed");

  assert_eq!(result.visual_eval_report.total_frames, 3);
  assert_eq!(result.visual_eval_report.label_matched_frames, 1);
  assert_eq!(result.visual_eval_report.label_missing_frames, 2);
  assert_eq!(result.visual_eval_report.spatial_matched_frames, 1);
  assert_eq!(result.visual_eval_report.spatial_missing_frames, 2);
  assert_eq!(result.visual_eval_report.spatial_unscored_frames, 0);
  assert_eq!(result.visual_eval_report.spurious_detection_count, 0);

  let provenance = result
    .visual_eval_report
    .detector_provenance
    .expect("detector provenance should be recorded");
  assert_eq!(provenance.model_id, "test-osu-fixture-detector");
  assert_eq!(provenance.label_map_source, "inline_fixture_dir");

  assert_eq!(
    result.detection_eval_manifest.detector_model_id,
    "test-osu-fixture-detector"
  );
  assert!(result.output_dir.join("visual_eval_report.json").exists());
  assert!(
    result
      .output_dir
      .join("detection_eval_manifest.json")
      .exists()
  );
}
