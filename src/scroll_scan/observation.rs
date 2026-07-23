// File: src/scroll_scan/observation.rs
use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::contract::{RecognitionResult, RecognitionSource, RecognitionSurface, RecognizedItem};
use crate::model::AuvResult;

use super::{CollectionObservation, ObservationCluster, ScanRect};

pub fn normalize_observation_text(raw: &str) -> String {
  raw.split_whitespace().collect::<Vec<_>>().join(" ").trim().to_lowercase()
}

pub fn conservative_merge_observations(observations: &[CollectionObservation]) -> Vec<ObservationCluster> {
  let mut clusters: Vec<ObservationCluster> = Vec::new();
  let mut assigned = vec![false; observations.len()];

  for (index, observation) in observations.iter().enumerate() {
    if assigned[index] {
      continue;
    }

    let mut ids = vec![observation.observation_id.clone()];
    assigned[index] = true;
    let mut merge_reason = "single_observation".to_string();
    let mut confidence = 1.0;

    for (candidate_index, candidate) in observations.iter().enumerate().skip(index + 1) {
      if assigned[candidate_index] {
        continue;
      }
      if should_merge_adjacent_observations(observation, candidate) {
        ids.push(candidate.observation_id.clone());
        assigned[candidate_index] = true;
        let decision = merge_decision(observation, candidate).expect("merge decision should exist when adjacent observations merge");
        merge_reason = decision.reason.to_string();
        confidence = decision.confidence;
      }
    }

    clusters.push(ObservationCluster {
      cluster_id: format!("cluster_{:04}", clusters.len() + 1),
      observation_ids: ids,
      representative_text: observation.raw_text.clone(),
      merge_reason,
      confidence,
    });
  }

  clusters
}

// TODO: Revisit merge identity after scroll-boundary evidence and row-local
// image hashes exist. This first rule is intentionally conservative and only
// merges adjacent-page overlap with nearly identical y positions.
pub(crate) fn should_merge_adjacent_observations(left: &CollectionObservation, right: &CollectionObservation) -> bool {
  merge_decision(left, right).is_some()
}

struct MergeDecision {
  reason: &'static str,
  confidence: f64,
}

fn merge_decision(left: &CollectionObservation, right: &CollectionObservation) -> Option<MergeDecision> {
  if left.section_context != right.section_context {
    return None;
  }
  if left.page_index.abs_diff(right.page_index) != 1 {
    return None;
  }

  if attribute_values_conflict(left, right, "recognized_item_id") {
    return None;
  }
  if let Some(recognized_item_id) = shared_attribute(left, right, "recognized_item_id") {
    if !recognized_item_id.is_empty() {
      return Some(MergeDecision {
        reason: "same_recognized_item_adjacent_page",
        confidence: 0.94,
      });
    }
  }

  let left_slot_identity = recognition_slot_identity(left);
  let right_slot_identity = recognition_slot_identity(right);
  if left_slot_identity.is_some() && right_slot_identity.is_some() && left_slot_identity != right_slot_identity {
    return None;
  }
  if left_slot_identity == right_slot_identity && left_slot_identity.is_some() {
    return Some(MergeDecision {
      reason: "same_recognition_slot_adjacent_page",
      confidence: 0.84,
    });
  }

  if left.normalized_text_key.is_empty() || left.normalized_text_key != right.normalized_text_key {
    return None;
  }
  if (left.bounds.y - right.bounds.y).abs() > 8 {
    return None;
  }

  Some(MergeDecision {
    reason: "same_text_adjacent_page_near_y",
    confidence: 0.72,
  })
}

fn shared_attribute<'a>(left: &'a CollectionObservation, right: &'a CollectionObservation, key: &str) -> Option<&'a str> {
  let left_value = left.attributes.get(key)?;
  let right_value = right.attributes.get(key)?;
  if left_value == right_value {
    Some(left_value.as_str())
  } else {
    None
  }
}

fn attribute_values_conflict(left: &CollectionObservation, right: &CollectionObservation, key: &str) -> bool {
  match (left.attributes.get(key), right.attributes.get(key)) {
    (Some(left_value), Some(right_value)) => left_value != right_value,
    _ => false,
  }
}

fn recognition_slot_identity(observation: &CollectionObservation) -> Option<(&str, &str, &str)> {
  let recognition_id = observation.attributes.get("recognition_id")?.as_str();
  let slot_key = if observation.attributes.contains_key("row_candidate_index") {
    "row_candidate_index"
  } else if observation.attributes.contains_key("row_index") {
    "row_index"
  } else {
    return None;
  };
  let slot_value = observation.attributes.get(slot_key)?.as_str();
  Some((recognition_id, slot_key, slot_value))
}

pub(crate) fn observation_from_row(
  page_index: usize,
  row_index: usize,
  row: &Value,
  source_artifact: &Path,
) -> AuvResult<CollectionObservation> {
  let raw_text = row
    .get("text")
    .and_then(Value::as_str)
    .ok_or_else(|| format!("malformed observe JSON: row {row_index} missing text string"))?
    .to_string();
  let bounds = row.get("bounds").ok_or_else(|| format!("malformed observe JSON: row {row_index} missing bounds object"))?;

  Ok(CollectionObservation {
    observation_id: format!("obs_{:04}_{:04}", page_index + 1, row_index + 1),
    page_index,
    raw_text: raw_text.clone(),
    normalized_text_key: normalize_observation_text(&raw_text),
    bounds: ScanRect {
      x: json_i64(bounds, "x", row_index)?,
      y: json_i64(bounds, "y", row_index)?,
      width: json_i64(bounds, "width", row_index)?,
      height: json_i64(bounds, "height", row_index)?,
    },
    section_context: None,
    source_artifacts: vec![source_artifact.to_path_buf()],
    attributes: observation_attributes_from_row(row),
  })
}

pub(crate) fn observation_from_recognized_item(
  page_index: usize,
  item_index: usize,
  item: &RecognizedItem,
  recognition: &RecognitionResult,
  source_artifact: &Path,
) -> CollectionObservation {
  let raw_text = recognized_item_text(item);
  CollectionObservation {
    observation_id: format!("obs_{:04}_{:04}", page_index + 1, item_index + 1),
    page_index,
    raw_text: raw_text.clone(),
    normalized_text_key: normalize_observation_text(&raw_text),
    bounds: ScanRect {
      x: item.box_.x,
      y: item.box_.y,
      width: item.box_.width,
      height: item.box_.height,
    },
    section_context: None,
    source_artifacts: vec![source_artifact.to_path_buf()],
    attributes: observation_attributes_from_recognized_item(item_index, item, recognition),
  }
}

fn observation_attributes_from_row(row: &Value) -> BTreeMap<String, String> {
  let mut attributes = BTreeMap::new();
  if let Some(source) = row.get("source").and_then(Value::as_str) {
    attributes.insert("source".to_string(), source.to_string());
  }
  if let Some(row_index) = row.get("row_index").and_then(Value::as_u64) {
    attributes.insert("row_index".to_string(), row_index.to_string());
  }
  if let Some(item_index) = row.get("item_index").and_then(Value::as_u64) {
    attributes.insert("item_index".to_string(), item_index.to_string());
  }
  if let Some(row_candidate_index) = row.get("row_candidate_index").and_then(Value::as_u64) {
    attributes.insert("row_candidate_index".to_string(), row_candidate_index.to_string());
  }
  if let Some(role) = row.get("segmented_region_role").and_then(Value::as_str) {
    attributes.insert("segmented_region_role".to_string(), role.to_string());
  }
  if let Some(reason) = row.get("filter_reason").and_then(Value::as_str) {
    attributes.insert("filter_reason".to_string(), reason.to_string());
  }
  if let Some(fragments) = row.get("text_fragments").and_then(Value::as_array) {
    let text = fragments.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(" | ");
    if !text.is_empty() {
      attributes.insert("text_fragments".to_string(), text);
    }
  }
  attributes
}

fn observation_attributes_from_recognized_item(
  item_index: usize,
  item: &RecognizedItem,
  recognition: &RecognitionResult,
) -> BTreeMap<String, String> {
  let mut attributes = BTreeMap::new();
  attributes.insert("item_index".to_string(), item_index.to_string());
  attributes.insert("recognition_id".to_string(), recognition.recognition_id.clone());
  attributes.insert("recognized_item_id".to_string(), item.item_id.clone());
  attributes.insert("recognition_source".to_string(), recognition_source_name(recognition.source).to_string());
  attributes.insert("recognition_surface".to_string(), recognition_surface_name(recognition.scope.surface).to_string());
  attributes.insert("recognized_item_kind".to_string(), item.kind.clone());
  if let Some(provider_score) = item.provider_score {
    attributes.insert("provider_score".to_string(), provider_score.to_string());
  }
  if let Some(source) = item.detail.get("source").and_then(Value::as_str) {
    attributes.insert("source".to_string(), source.to_string());
  } else {
    attributes.insert("source".to_string(), format!("recognition:{}", recognition_source_name(recognition.source)));
  }
  if let Some(row_index) = item.detail.get("row_index").and_then(Value::as_u64) {
    attributes.insert("row_index".to_string(), row_index.to_string());
    attributes.insert("row_candidate_index".to_string(), row_index.to_string());
  }
  if let Some(text_fragments) = recognized_item_text_fragments(item) {
    if !text_fragments.is_empty() {
      attributes.insert("text_fragments".to_string(), text_fragments.join(" | "));
    }
  }
  attributes
}

fn recognized_item_text(item: &RecognizedItem) -> String {
  item
    .text
    .as_deref()
    .filter(|text| !text.trim().is_empty())
    .map(str::to_string)
    .or_else(|| recognized_item_text_fragments(item).map(|fragments| fragments.join(" | ")))
    .unwrap_or_default()
}

fn recognized_item_text_fragments(item: &RecognizedItem) -> Option<Vec<String>> {
  let fragments = item
    .detail
    .get("text_fragments")
    .and_then(Value::as_array)?
    .iter()
    .filter_map(Value::as_str)
    .map(str::trim)
    .filter(|fragment| !fragment.is_empty())
    .map(str::to_string)
    .collect::<Vec<_>>();
  if fragments.is_empty() {
    None
  } else {
    Some(fragments)
  }
}

fn recognition_source_name(source: RecognitionSource) -> &'static str {
  match source {
    RecognitionSource::OcrText => "ocr_text",
    RecognitionSource::OcrRow => "ocr_row",
    RecognitionSource::VisualRow => "visual_row",
    RecognitionSource::SegmentedRegion => "segmented_region",
    RecognitionSource::IconMatch => "icon_match",
    RecognitionSource::Custom => "custom",
  }
}

fn recognition_surface_name(surface: RecognitionSurface) -> &'static str {
  match surface {
    RecognitionSurface::Screen => "screen",
    RecognitionSurface::Display => "display",
    RecognitionSurface::Window => "window",
    RecognitionSurface::Region => "region",
  }
}

fn json_i64(bounds: &Value, key: &str, row_index: usize) -> AuvResult<i64> {
  bounds.get(key).and_then(Value::as_i64).ok_or_else(|| format!("malformed observe JSON: row {row_index} bounds.{key} must be an integer"))
}
