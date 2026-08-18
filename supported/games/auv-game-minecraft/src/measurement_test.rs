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

#[test]
fn thresholds_validate_rejects_blank_and_empty_fields() {
  let mut thresholds = TextureSweepThresholds::mc6_v0();
  thresholds.pose_error_p95_max_px = 0.0;
  assert_eq!(thresholds.validate(), Err("pose_error_p95_max_px must be positive finite, got 0".to_string()));

  let mut thresholds = TextureSweepThresholds::mc6_v0();
  thresholds.occlusion_iou_min = 1.5;
  assert_eq!(thresholds.validate(), Err("occlusion_iou_min must be between 0 and 1, got 1.5".to_string()));

  let mut thresholds = TextureSweepThresholds::mc6_v0();
  thresholds.resource_pack_count = 0;
  assert_eq!(thresholds.validate(), Err("resource_pack_count must be greater than 0".to_string()));

  let mut thresholds = TextureSweepThresholds::mc6_v0();
  thresholds.required_texture_profiles.clear();
  assert_eq!(thresholds.validate(), Err("required_texture_profiles must not be empty".to_string()));

  let mut thresholds = TextureSweepThresholds::mc6_v0();
  thresholds.per_pack_duration_seconds = f64::NAN;
  assert_eq!(thresholds.validate(), Err("per_pack_duration_seconds must be positive finite, got NaN".to_string()));

  let mut thresholds = TextureSweepThresholds::mc6_v0();
  thresholds.refuse_on_noise_rule = "   ".to_string();
  assert_eq!(thresholds.validate(), Err("refuse_on_noise_rule must be defined".to_string()));
}

#[test]
fn sample_statistics_validate_empty_and_invalid_values() {
  assert_eq!(percentile_95(vec![]), Ok(None));
  assert_eq!(min_finite(vec![]), Ok(None));
  assert_eq!(max_finite(vec![]), Ok(0.0));

  assert_eq!(percentile_95(vec![1.0, f64::NAN]), Err("pose_error_px samples must be finite and non-negative".to_string()));
  assert_eq!(min_finite(vec![1.0, f64::NAN]), Err("occlusion_iou samples must be finite".to_string()));
  assert_eq!(max_finite(vec![1.0, -1.0]), Err("duration_seconds samples must be finite and non-negative".to_string()));
}
