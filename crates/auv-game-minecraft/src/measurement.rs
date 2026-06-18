use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub type MeasurementResult<T> = Result<T, String>;

pub const TEXTURE_SWEEP_REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSweepInputs {
  pub samples_path: PathBuf,
  pub output_dir: PathBuf,
  pub thresholds: TextureSweepThresholds,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSweepThresholds {
  pub pose_error_p95_max_px: f64,
  pub occlusion_iou_min: f64,
  pub resource_pack_count: usize,
  pub required_texture_profiles: Vec<String>,
  pub per_pack_duration_seconds: f64,
  pub refuse_on_noise_rule: String,
}

impl TextureSweepThresholds {
  pub fn mc6_v0() -> Self {
    Self {
      pose_error_p95_max_px: 8.0,
      occlusion_iou_min: 0.85,
      resource_pack_count: 3,
      required_texture_profiles: vec![
        "rich".to_string(),
        "flat_color".to_string(),
        "repetitive".to_string(),
      ],
      per_pack_duration_seconds: 30.0,
      refuse_on_noise_rule:
        "exclude refused noisy frames from metrics, but require at least one exercised refusal"
          .to_string(),
    }
  }

  pub fn validate(&self) -> MeasurementResult<()> {
    if !self.pose_error_p95_max_px.is_finite() || self.pose_error_p95_max_px <= 0.0 {
      return Err(format!(
        "pose_error_p95_max_px must be positive finite, got {}",
        self.pose_error_p95_max_px
      ));
    }
    if !self.occlusion_iou_min.is_finite() || !(0.0..=1.0).contains(&self.occlusion_iou_min) {
      return Err(format!(
        "occlusion_iou_min must be between 0 and 1, got {}",
        self.occlusion_iou_min
      ));
    }
    if self.resource_pack_count == 0 {
      return Err("resource_pack_count must be greater than 0".to_string());
    }
    if self.required_texture_profiles.is_empty() {
      return Err("required_texture_profiles must not be empty".to_string());
    }
    if !self.per_pack_duration_seconds.is_finite() || self.per_pack_duration_seconds <= 0.0 {
      return Err(format!(
        "per_pack_duration_seconds must be positive finite, got {}",
        self.per_pack_duration_seconds
      ));
    }
    if self.refuse_on_noise_rule.trim().is_empty() {
      return Err("refuse_on_noise_rule must be defined".to_string());
    }
    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSweepSampleSet {
  pub samples: Vec<TextureSweepSample>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSweepSample {
  pub resource_pack: String,
  pub texture_profile: String,
  pub duration_seconds: f64,
  pub pose_error_px: f64,
  pub occlusion_iou: f64,
  #[serde(default)]
  pub refused_noise: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSweepReport {
  pub schema_version: u32,
  pub thresholds: TextureSweepThresholds,
  pub rows: Vec<TextureSweepReportRow>,
  pub covered_texture_profiles: Vec<String>,
  pub expected_resource_pack_count: usize,
  pub actual_resource_pack_count: usize,
  pub noise_refusal_exercised: bool,
  pub passed: bool,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextureSweepReportRow {
  pub resource_pack: String,
  pub texture_profile: String,
  pub sample_count: usize,
  pub refused_noise_count: usize,
  pub duration_seconds: f64,
  pub pose_error_p95_px: Option<f64>,
  pub min_occlusion_iou: Option<f64>,
  pub pose_passed: bool,
  pub occlusion_passed: bool,
  pub duration_passed: bool,
  pub passed: bool,
}

pub fn evaluate_texture_sweep(
  inputs: &TextureSweepInputs,
) -> MeasurementResult<TextureSweepReport> {
  inputs.thresholds.validate()?;
  let sample_set = read_sample_set(&inputs.samples_path)?;
  let report = build_texture_sweep_report(&sample_set.samples, inputs.thresholds.clone())?;
  fs::create_dir_all(&inputs.output_dir).map_err(|error| {
    format!(
      "failed to create minecraft texture sweep output directory {}: {error}",
      inputs.output_dir.display()
    )
  })?;
  write_report(
    &inputs.output_dir.join("texture_sweep_report.json"),
    &report,
  )?;
  Ok(report)
}

pub fn build_texture_sweep_report(
  samples: &[TextureSweepSample],
  thresholds: TextureSweepThresholds,
) -> MeasurementResult<TextureSweepReport> {
  thresholds.validate()?;
  let mut pack_names = samples
    .iter()
    .map(|sample| sample.resource_pack.clone())
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect::<Vec<_>>();
  pack_names.sort();

  let mut rows = Vec::new();
  for pack in &pack_names {
    let pack_samples = samples
      .iter()
      .filter(|sample| sample.resource_pack == *pack)
      .collect::<Vec<_>>();
    let texture_profile = pack_samples
      .first()
      .map(|sample| sample.texture_profile.clone())
      .unwrap_or_default();
    if pack_samples
      .iter()
      .any(|sample| sample.texture_profile != texture_profile)
    {
      return Err(format!(
        "resource pack {pack} has mixed texture profiles; split the samples before evaluation"
      ));
    }
    let refused_noise_count = pack_samples
      .iter()
      .filter(|sample| sample.refused_noise)
      .count();
    let accepted = pack_samples
      .iter()
      .filter(|sample| !sample.refused_noise)
      .collect::<Vec<_>>();
    let pose_error_p95_px = percentile_95(
      accepted
        .iter()
        .map(|sample| sample.pose_error_px)
        .collect::<Vec<_>>(),
    )?;
    let min_occlusion_iou = min_finite(
      accepted
        .iter()
        .map(|sample| sample.occlusion_iou)
        .collect::<Vec<_>>(),
    )?;
    let duration_seconds = max_finite(
      pack_samples
        .iter()
        .map(|sample| sample.duration_seconds)
        .collect::<Vec<_>>(),
    )?;
    let pose_passed = pose_error_p95_px
      .map(|value| value < thresholds.pose_error_p95_max_px)
      .unwrap_or(false);
    let occlusion_passed = min_occlusion_iou
      .map(|value| value > thresholds.occlusion_iou_min)
      .unwrap_or(false);
    let duration_passed = duration_seconds >= thresholds.per_pack_duration_seconds;
    rows.push(TextureSweepReportRow {
      resource_pack: pack.clone(),
      texture_profile,
      sample_count: accepted.len(),
      refused_noise_count,
      duration_seconds,
      pose_error_p95_px,
      min_occlusion_iou,
      pose_passed,
      occlusion_passed,
      duration_passed,
      passed: pose_passed && occlusion_passed && duration_passed,
    });
  }

  let covered_texture_profiles = rows
    .iter()
    .map(|row| row.texture_profile.clone())
    .collect::<BTreeSet<_>>()
    .into_iter()
    .collect::<Vec<_>>();
  let expected_profiles = thresholds
    .required_texture_profiles
    .iter()
    .cloned()
    .collect::<BTreeSet<_>>();
  let covered_profiles = covered_texture_profiles
    .iter()
    .cloned()
    .collect::<BTreeSet<_>>();
  let noise_refusal_exercised = rows.iter().any(|row| row.refused_noise_count > 0);
  let actual_resource_pack_count = rows.len();
  let expected_resource_pack_count = thresholds.resource_pack_count;
  let passed = actual_resource_pack_count == thresholds.resource_pack_count
    && expected_profiles.is_subset(&covered_profiles)
    && noise_refusal_exercised
    && rows.iter().all(|row| row.passed);

  Ok(TextureSweepReport {
    schema_version: TEXTURE_SWEEP_REPORT_SCHEMA_VERSION,
    thresholds,
    rows,
    covered_texture_profiles,
    expected_resource_pack_count,
    actual_resource_pack_count,
    noise_refusal_exercised,
    passed,
    known_limits: vec![
      "texture sweep report consumes precomputed MC-6 measurement samples; it does not collect live frames".to_string(),
      "refused noisy samples exercise the refusal rule and are excluded from p95/IoU metrics".to_string(),
    ],
  })
}

fn read_sample_set(path: &Path) -> MeasurementResult<TextureSweepSampleSet> {
  let bytes = fs::read(path).map_err(|error| {
    format!(
      "failed to read minecraft texture sweep samples {}: {error}",
      path.display()
    )
  })?;
  serde_json::from_slice::<TextureSweepSampleSet>(&bytes).map_err(|error| {
    format!(
      "failed to parse minecraft texture sweep samples {}: {error}",
      path.display()
    )
  })
}

fn write_report(path: &Path, report: &TextureSweepReport) -> MeasurementResult<()> {
  let json = serde_json::to_string_pretty(report)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize minecraft texture sweep report: {error}"))?;
  fs::write(path, json.as_bytes()).map_err(|error| {
    format!(
      "failed to write minecraft texture sweep report {}: {error}",
      path.display()
    )
  })
}

fn percentile_95(mut values: Vec<f64>) -> MeasurementResult<Option<f64>> {
  if values.is_empty() {
    return Ok(None);
  }
  if values
    .iter()
    .any(|value| !value.is_finite() || *value < 0.0)
  {
    return Err("pose_error_px samples must be finite and non-negative".to_string());
  }
  values.sort_by(|left, right| left.total_cmp(right));
  let index = ((values.len() as f64) * 0.95).ceil() as usize - 1;
  Ok(values.get(index).copied())
}

fn min_finite(values: Vec<f64>) -> MeasurementResult<Option<f64>> {
  if values.is_empty() {
    return Ok(None);
  }
  if values.iter().any(|value| !value.is_finite()) {
    return Err("occlusion_iou samples must be finite".to_string());
  }
  Ok(values.into_iter().reduce(f64::min))
}

fn max_finite(values: Vec<f64>) -> MeasurementResult<f64> {
  if values.is_empty() {
    return Ok(0.0);
  }
  if values
    .iter()
    .any(|value| !value.is_finite() || *value < 0.0)
  {
    return Err("duration_seconds samples must be finite and non-negative".to_string());
  }
  Ok(values.into_iter().fold(0.0, f64::max))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn sample(
    resource_pack: &str,
    texture_profile: &str,
    pose_error_px: f64,
    occlusion_iou: f64,
    refused_noise: bool,
  ) -> TextureSweepSample {
    TextureSweepSample {
      resource_pack: resource_pack.to_string(),
      texture_profile: texture_profile.to_string(),
      duration_seconds: 30.0,
      pose_error_px,
      occlusion_iou,
      refused_noise,
    }
  }

  #[test]
  fn default_thresholds_are_the_mc6_presets() {
    let thresholds = TextureSweepThresholds::mc6_v0();

    assert_eq!(thresholds.pose_error_p95_max_px, 8.0);
    assert_eq!(thresholds.occlusion_iou_min, 0.85);
    assert_eq!(thresholds.resource_pack_count, 3);
    assert_eq!(
      thresholds.required_texture_profiles,
      vec!["rich", "flat_color", "repetitive"]
    );
    assert_eq!(thresholds.per_pack_duration_seconds, 30.0);
    assert!(thresholds.refuse_on_noise_rule.contains("require"));
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

    let report = build_texture_sweep_report(&samples, TextureSweepThresholds::mc6_v0())
      .expect("report should build");

    assert_eq!(report.schema_version, 1);
    assert_eq!(report.actual_resource_pack_count, 3);
    assert!(report.noise_refusal_exercised);
    assert!(report.passed);
    let rich = report
      .rows
      .iter()
      .find(|row| row.resource_pack == "rich-pack")
      .expect("rich row");
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
      sample("repeat-pack", "repetitive", 3.0, 0.93, false),
    ];

    let report = build_texture_sweep_report(&samples, TextureSweepThresholds::mc6_v0())
      .expect("report should build");

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

    let report = build_texture_sweep_report(&samples, TextureSweepThresholds::mc6_v0())
      .expect("report should build");

    assert!(!report.passed);
    let rich = report
      .rows
      .iter()
      .find(|row| row.resource_pack == "rich-pack")
      .expect("rich row");
    assert!(!rich.pose_passed);
    let flat = report
      .rows
      .iter()
      .find(|row| row.resource_pack == "flat-pack")
      .expect("flat row");
    assert!(!flat.occlusion_passed);
  }

  #[test]
  fn writes_report_from_sample_file() {
    let temp = tempfile::tempdir().expect("temp dir");
    let samples_path = temp.path().join("samples.json");
    let output_dir = temp.path().join("output");
    fs::write(
      &samples_path,
      serde_json::to_vec_pretty(&TextureSweepSampleSet {
        samples: vec![
          sample("rich-pack", "rich", 2.0, 0.95, false),
          sample("flat-pack", "flat_color", 4.0, 0.92, false),
          sample("flat-pack", "flat_color", 20.0, 0.10, true),
          sample("repeat-pack", "repetitive", 3.0, 0.93, false),
        ],
      })
      .expect("samples json"),
    )
    .expect("samples write");

    let report = evaluate_texture_sweep(&TextureSweepInputs {
      samples_path,
      output_dir: output_dir.clone(),
      thresholds: TextureSweepThresholds::mc6_v0(),
    })
    .expect("sweep should evaluate");

    assert!(report.passed);
    assert!(output_dir.join("texture_sweep_report.json").is_file());
  }
}
