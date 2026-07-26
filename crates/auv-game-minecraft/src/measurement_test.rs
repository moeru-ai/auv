use super::*;

fn sample(resource_pack: &str, texture_profile: &str, pose_error_px: f64, occlusion_iou: f64, refused_noise: bool) -> TextureSweepSample {
  TextureSweepSample {
    resource_pack: resource_pack.to_string(),
    texture_profile: texture_profile.to_string(),
    duration_seconds: 30.0,
    pose_error_px,
    occlusion_iou,
    refused_noise,
    refusal_reason: refused_noise.then_some(MismatchRefusalReason::MenuLoadingScreen),
  }
}

fn refused_sample_with_reason(resource_pack: &str, texture_profile: &str, reason: MismatchRefusalReason) -> TextureSweepSample {
  TextureSweepSample {
    resource_pack: resource_pack.to_string(),
    texture_profile: texture_profile.to_string(),
    duration_seconds: 30.0,
    pose_error_px: 0.0,
    occlusion_iou: 0.0,
    refused_noise: true,
    refusal_reason: Some(reason),
  }
}

#[test]
fn evaluates_texture_sweep_table_against_fixed_thresholds() {
  let samples = vec![
    sample("rich-pack", "rich", 2.0, 0.95, false),
    sample("rich-pack", "rich", 7.0, 0.90, false),
    sample("rich-pack", "rich", 50.0, 0.10, true),
    sample("flat-pack", "flat_color", 4.0, 0.92, false),
    sample("flat-pack", "flat_color", 6.0, 0.88, false),
    sample("repeat-pack", "repetitive", 3.0, 0.93, false),
    sample("repeat-pack", "repetitive", 5.0, 0.89, false),
  ];

  let report = build_texture_sweep_report(&samples, TextureSweepThresholds::mc6_v0()).expect("report should build");

  assert_eq!(report.schema_version, 1);
  assert_eq!(report.actual_resource_pack_count, 3);
  assert!(report.noise_refusal_exercised);
  assert!(report.passed);
  let rich = report.rows.iter().find(|row| row.resource_pack == "rich-pack").expect("rich row");
  assert_eq!(rich.sample_count, 2);
  assert_eq!(rich.refused_noise_count, 1);
  assert_eq!(rich.pose_error_p95_px, Some(7.0));
  assert_eq!(rich.min_occlusion_iou, Some(0.90));
}

#[test]
fn fails_when_noise_refusal_rule_was_not_exercised() {
  let samples = vec![
    sample("rich-pack", "rich", 2.0, 0.95, false),
    sample("flat-pack", "flat_color", 4.0, 0.92, false),
    refused_sample_with_reason("flat-pack", "flat_color", MismatchRefusalReason::ScreenshotUnavailable),
    sample("repeat-pack", "repetitive", 3.0, 0.93, false),
  ];

  let report = build_texture_sweep_report(&samples, TextureSweepThresholds::mc6_v0()).expect("report should build");

  assert!(!report.noise_refusal_exercised);
  assert!(!report.passed);
}

#[test]
fn fails_when_pose_or_iou_threshold_is_missed() {
  let samples = vec![
    sample("rich-pack", "rich", 9.0, 0.95, false),
    sample("flat-pack", "flat_color", 4.0, 0.80, false),
    sample("flat-pack", "flat_color", 20.0, 0.10, true),
    sample("repeat-pack", "repetitive", 3.0, 0.93, false),
  ];

  let report = build_texture_sweep_report(&samples, TextureSweepThresholds::mc6_v0()).expect("report should build");

  assert!(!report.passed);
  let rich = report.rows.iter().find(|row| row.resource_pack == "rich-pack").expect("rich row");
  assert!(!rich.pose_passed);
  let flat = report.rows.iter().find(|row| row.resource_pack == "flat-pack").expect("flat row");
  assert!(!flat.occlusion_passed);
}
