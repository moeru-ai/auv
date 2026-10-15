use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::model::BeatmapModel;
use crate::parser::parse_beatmap_file;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GroundTruthStats {
  pub beatmap_id: u32,
  pub beatmapset_id: u32,
  pub source: String,
  pub title: String,
  pub artist: String,
  pub version: String,
  pub circle_size: f32,
  pub approach_rate: f32,
  pub overall_difficulty: f32,
  pub hp_drain_rate: f32,
  pub count_circles: usize,
  pub count_sliders: usize,
  pub count_spinners: usize,
  pub total_objects: usize,
  pub first_object_time_ms: f64,
  pub last_object_time_ms: f64,
}

use crate::model::EvidenceLevel;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetadataReconciliationReport {
  pub beatmap_name: String,
  pub reference_source: String,
  pub independent_source: String,
  pub evidence_level: EvidenceLevel,
  pub matched: bool,
  pub expected_circles: usize,
  pub actual_circles: usize,
  pub expected_sliders: usize,
  pub actual_sliders: usize,
  pub expected_spinners: usize,
  pub actual_spinners: usize,
  pub expected_total: usize,
  pub actual_total: usize,
  pub expected_first_ms: f64,
  pub actual_first_ms: f64,
  pub expected_last_ms: f64,
  pub actual_last_ms: f64,
  pub diff_notes: Vec<String>,
  pub disclosure: String,
}

pub type ReconciliationResult = MetadataReconciliationReport;

pub fn official_tear_rain_normal_ground_truth() -> GroundTruthStats {
  GroundTruthStats {
    beatmap_id: 351190,
    beatmapset_id: 140662,
    source: "https://osu.ppy.sh/beatmapsets/140662#osu/351190 (Official Ranked Beatmap Page)".to_string(),
    title: "Tear Rain".to_string(),
    artist: "cYsmix feat. Emmy".to_string(),
    version: "Normal".to_string(),
    circle_size: 3.0,
    approach_rate: 4.0,
    overall_difficulty: 3.0,
    hp_drain_rate: 3.0,
    count_circles: 120,
    count_sliders: 173,
    count_spinners: 2,
    total_objects: 295,
    first_object_time_ms: 852.0,
    last_object_time_ms: 238977.0,
  }
}

pub fn reconcile_beatmap(model: &BeatmapModel, expected: &GroundTruthStats) -> MetadataReconciliationReport {
  let mut diff_notes = Vec::new();

  let actual_circles = model.circle_count();
  let actual_sliders = model.slider_count();
  let actual_spinners = model.spinner_count();
  let actual_total = model.total_objects();
  let actual_first_ms = model.first_object_time_ms().unwrap_or(0.0);
  let actual_last_ms = model.hit_objects.last().map(|o| o.start_time_ms()).unwrap_or(0.0);

  if actual_circles != expected.count_circles {
    diff_notes.push(format!("circle mismatch: expected {}, got {}", expected.count_circles, actual_circles));
  }
  if actual_sliders != expected.count_sliders {
    diff_notes.push(format!("slider mismatch: expected {}, got {}", expected.count_sliders, actual_sliders));
  }
  if actual_spinners != expected.count_spinners {
    diff_notes.push(format!("spinner mismatch: expected {}, got {}", expected.count_spinners, actual_spinners));
  }
  if actual_total != expected.total_objects {
    diff_notes.push(format!("total mismatch: expected {}, got {}", expected.total_objects, actual_total));
  }
  if (actual_first_ms - expected.first_object_time_ms).abs() > 1.0 {
    diff_notes.push(format!("first object time mismatch: expected {}, got {}", expected.first_object_time_ms, actual_first_ms));
  }
  if (actual_last_ms - expected.last_object_time_ms).abs() > 1.0 {
    diff_notes.push(format!("last object time mismatch: expected {}, got {}", expected.last_object_time_ms, actual_last_ms));
  }

  let matched = diff_notes.is_empty();

  MetadataReconciliationReport {
    beatmap_name: format!("{} - {} [{}]", model.artist, model.title, model.version),
    reference_source: expected.source.clone(),
    independent_source: expected.source.clone(),
    evidence_level: EvidenceLevel::StaticReferenceChecked,
    matched,
    expected_circles: expected.count_circles,
    actual_circles,
    expected_sliders: expected.count_sliders,
    actual_sliders,
    expected_spinners: expected.count_spinners,
    actual_spinners,
    expected_total: expected.total_objects,
    actual_total,
    expected_first_ms: expected.first_object_time_ms,
    actual_first_ms,
    expected_last_ms: expected.last_object_time_ms,
    actual_last_ms,
    diff_notes,
    disclosure: "Static metadata comparison against known map constants, NOT Bancho live run alignment.".to_string(),
  }
}

pub fn reconcile_tear_rain_fixture() -> Result<ReconciliationResult> {
  let fixture_path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/351190-cYsmix-Tear-Rain-Normal.osu");
  let model = parse_beatmap_file(fixture_path).with_context(|| "failed to parse real Tear Rain beatmap")?;
  let expected = official_tear_rain_normal_ground_truth();
  Ok(reconcile_beatmap(&model, &expected))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_reconcile_real_beatmap_against_official_web_source() {
    let result = reconcile_tear_rain_fixture().expect("must reconcile real beatmap fixture");
    assert!(result.matched, "Reconciliation failed with diffs: {:?}", result.diff_notes);
    assert_eq!(result.actual_circles, 120);
    assert_eq!(result.actual_sliders, 173);
    assert_eq!(result.actual_spinners, 2);
    assert_eq!(result.actual_total, 295);
    assert_eq!(result.actual_first_ms, 852.0);
    assert_eq!(result.actual_last_ms, 238977.0);
  }
}
