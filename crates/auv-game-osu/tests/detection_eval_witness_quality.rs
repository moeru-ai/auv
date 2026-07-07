use std::path::PathBuf;

use auv_game_osu::{
  DetectionEvalQualityInputs, DetectionEvalWitnessInputs, build_detection_eval_quality, build_detection_eval_witness,
  evaluate_detection_fixture,
};

#[test]
fn detection_eval_witness_quality_chain_from_fixture() {
  let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/osu_eval_run_artifacts");
  let detections_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/osu_eval_detection");
  let output_dir = tempfile::tempdir().expect("tempdir");
  let eval_output = output_dir.path().join("eval");

  let eval = evaluate_detection_fixture(&auv_game_osu::DetectionEvalInputs {
    run_artifact_dir: manifest_dir,
    detections_path,
    output_dir: eval_output.clone(),
  })
  .expect("eval");

  let witness = build_detection_eval_witness(&DetectionEvalWitnessInputs {
    detection_eval_output_dir: eval_output.clone(),
    output_dir: eval_output.join("witness"),
  })
  .expect("witness");

  let quality = build_detection_eval_quality(&DetectionEvalQualityInputs {
    witness_manifest_path: witness.manifest_path.clone(),
    output_dir: eval_output.join("quality"),
  })
  .expect("quality");

  assert_eq!(eval.visual_eval_report.total_frames, witness.manifest.total_frames);
  assert_eq!(witness.manifest.frame_witnesses.len(), 3);
  assert_eq!(quality.manifest.verdict, auv_game_osu::DetectionEvalQualityVerdict::MeasuredOnly);
  assert!(witness.manifest_path.exists());
  assert!(quality.manifest_path.exists());
}
