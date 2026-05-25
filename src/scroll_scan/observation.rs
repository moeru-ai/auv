// File: src/scroll_scan/observation.rs
use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::contract::{
  NodeRef, RecognitionBox, RecognitionResult, RecognitionSource, RecognitionSurface,
  RecognizedItem, SurfaceNode,
};
use crate::model::AuvResult;
use crate::trace::{RunId, SpanId};

use super::{CollectionObservation, ObservationCluster, ScanRect};

pub fn normalize_observation_text(raw: &str) -> String {
  raw
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .trim()
    .to_lowercase()
}

pub fn conservative_merge_observations(
  observations: &[CollectionObservation],
) -> Vec<ObservationCluster> {
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
        merge_reason = "same_text_adjacent_page_near_y".to_string();
        confidence = 0.72;
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
pub(crate) fn should_merge_adjacent_observations(
  left: &CollectionObservation,
  right: &CollectionObservation,
) -> bool {
  if left.normalized_text_key.is_empty() || left.normalized_text_key != right.normalized_text_key {
    return false;
  }
  if left.section_context != right.section_context {
    return false;
  }
  if left.page_index.abs_diff(right.page_index) != 1 {
    return false;
  }
  (left.bounds.y - right.bounds.y).abs() <= 8
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
  let bounds = row
    .get("bounds")
    .ok_or_else(|| format!("malformed observe JSON: row {row_index} missing bounds object"))?;

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
    attributes.insert(
      "row_candidate_index".to_string(),
      row_candidate_index.to_string(),
    );
  }
  if let Some(role) = row.get("segmented_region_role").and_then(Value::as_str) {
    attributes.insert("segmented_region_role".to_string(), role.to_string());
  }
  if let Some(reason) = row.get("filter_reason").and_then(Value::as_str) {
    attributes.insert("filter_reason".to_string(), reason.to_string());
  }
  if let Some(fragments) = row.get("text_fragments").and_then(Value::as_array) {
    let text = fragments
      .iter()
      .filter_map(Value::as_str)
      .collect::<Vec<_>>()
      .join(" | ");
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
  attributes.insert(
    "recognition_id".to_string(),
    recognition.recognition_id.clone(),
  );
  attributes.insert("recognized_item_id".to_string(), item.item_id.clone());
  attributes.insert(
    "recognition_source".to_string(),
    recognition_source_name(recognition.source).to_string(),
  );
  attributes.insert(
    "recognition_surface".to_string(),
    recognition_surface_name(recognition.scope.surface).to_string(),
  );
  attributes.insert("recognized_item_kind".to_string(), item.kind.clone());
  if let Some(provider_score) = item.provider_score {
    attributes.insert("provider_score".to_string(), provider_score.to_string());
  }
  if let Some(source) = item.detail.get("source").and_then(Value::as_str) {
    attributes.insert("source".to_string(), source.to_string());
  } else {
    attributes.insert(
      "source".to_string(),
      format!(
        "recognition:{}",
        recognition_source_name(recognition.source)
      ),
    );
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

pub(crate) fn surface_nodes_from_observations(
  run_id: &RunId,
  span_id: &SpanId,
  observations: &[CollectionObservation],
) -> Vec<SurfaceNode> {
  observations
    .iter()
    .map(|observation| surface_node_from_observation(run_id, span_id, observation))
    .collect()
}

fn surface_node_from_observation(
  run_id: &RunId,
  span_id: &SpanId,
  observation: &CollectionObservation,
) -> SurfaceNode {
  let source_artifacts = observation
    .source_artifacts
    .iter()
    .map(|path| path.display().to_string())
    .collect::<Vec<_>>();
  let recognition_id = observation.attributes.get("recognition_id").cloned();
  let recognition_source = observation
    .attributes
    .get("recognition_source")
    .and_then(|value| parse_recognition_source_name(value));
  let recognition_surface = observation
    .attributes
    .get("recognition_surface")
    .and_then(|value| parse_recognition_surface_name(value));
  let recognized_item_id = observation.attributes.get("recognized_item_id").cloned();
  let recognized_item_kind = observation.attributes.get("recognized_item_kind").cloned();
  let provider_score = observation
    .attributes
    .get("provider_score")
    .and_then(|value| value.parse::<f64>().ok());
  let kind = recognized_item_kind
    .clone()
    .or_else(|| observation.attributes.get("segmented_region_role").cloned())
    .or_else(|| observation.attributes.get("source").cloned())
    .unwrap_or_else(|| "observation".to_string());
  let label = if observation.raw_text.trim().is_empty() {
    None
  } else {
    Some(observation.raw_text.clone())
  };

  SurfaceNode {
    node_ref: NodeRef {
      run_id: run_id.clone(),
      span_id: span_id.clone(),
      node_id: observation.observation_id.clone(),
    },
    kind,
    label,
    box_: RecognitionBox {
      x: observation.bounds.x,
      y: observation.bounds.y,
      width: observation.bounds.width,
      height: observation.bounds.height,
    },
    source_artifacts: source_artifacts.clone(),
    recognition_id,
    recognition_source,
    recognition_surface,
    recognized_item_id,
    recognized_item_kind,
    provider_score,
    detail: serde_json::json!({
      "observation_id": observation.observation_id.clone(),
      "page_index": observation.page_index,
      "normalized_text_key": observation.normalized_text_key.clone(),
      "section_context": observation.section_context.clone(),
      "source_artifacts": source_artifacts,
      "attributes": observation.attributes.clone(),
    }),
  }
}

fn parse_recognition_source_name(value: &str) -> Option<RecognitionSource> {
  match value {
    "ocr_text" => Some(RecognitionSource::OcrText),
    "ocr_row" => Some(RecognitionSource::OcrRow),
    "visual_row" => Some(RecognitionSource::VisualRow),
    "segmented_region" => Some(RecognitionSource::SegmentedRegion),
    "icon_match" => Some(RecognitionSource::IconMatch),
    "custom" => Some(RecognitionSource::Custom),
    _ => None,
  }
}

fn parse_recognition_surface_name(value: &str) -> Option<RecognitionSurface> {
  match value {
    "screen" => Some(RecognitionSurface::Screen),
    "display" => Some(RecognitionSurface::Display),
    "window" => Some(RecognitionSurface::Window),
    "region" => Some(RecognitionSurface::Region),
    _ => None,
  }
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
  bounds.get(key).and_then(Value::as_i64).ok_or_else(|| {
    format!("malformed observe JSON: row {row_index} bounds.{key} must be an integer")
  })
}
