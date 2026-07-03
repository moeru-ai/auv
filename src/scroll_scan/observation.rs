// File: src/scroll_scan/observation.rs
use std::collections::BTreeMap;
use std::path::Path;

use serde_json::Value;

use crate::contract::{
  ArtifactRef, NodeRef, OBSERVATION_SNAPSHOT_API_VERSION, ObservationSnapshot, ObservationSource,
  RatioRegion, RecognitionBox, RecognitionResult, RecognitionScope, RecognitionSource,
  RecognitionSurface, RecognizedItem, SurfaceNode,
};
use crate::model::{AuvResult, now_millis};
use auv_tracing_driver::trace::{ArtifactRecordV1Alpha1, RunId, SpanId};

use super::{CollectionObservation, ObservationCluster, ScanRect, ScanTarget};

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
        let decision = merge_decision(observation, candidate)
          .expect("merge decision should exist when adjacent observations merge");
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
pub(crate) fn should_merge_adjacent_observations(
  left: &CollectionObservation,
  right: &CollectionObservation,
) -> bool {
  merge_decision(left, right).is_some()
}

struct MergeDecision {
  reason: &'static str,
  confidence: f64,
}

fn merge_decision(
  left: &CollectionObservation,
  right: &CollectionObservation,
) -> Option<MergeDecision> {
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
  if left_slot_identity.is_some()
    && right_slot_identity.is_some()
    && left_slot_identity != right_slot_identity
  {
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

fn shared_attribute<'a>(
  left: &'a CollectionObservation,
  right: &'a CollectionObservation,
  key: &str,
) -> Option<&'a str> {
  let left_value = left.attributes.get(key)?;
  let right_value = right.attributes.get(key)?;
  if left_value == right_value {
    Some(left_value.as_str())
  } else {
    None
  }
}

fn attribute_values_conflict(
  left: &CollectionObservation,
  right: &CollectionObservation,
  key: &str,
) -> bool {
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

/// Build a v0 `ObservationSnapshot` record for one scanned page. The
/// `nodes` field projects this page's observations into the unified UI layer;
/// the snapshot context captures who, what, when, and where.
///
/// Provenance limitations: scroll scan now threads observe-artifact
/// `ArtifactRef`s into the snapshot record, but it still does not emit a
/// separate capture-contract artifact. Per-node `source_artifacts` remain path
/// strings for now because `SurfaceNode` has not yet grown structured artifact
/// refs.
pub(crate) fn build_page_observation_snapshot(
  run_id: &RunId,
  span_id: &SpanId,
  page_index: usize,
  target: &ScanTarget,
  page_observations: &[CollectionObservation],
  screenshot_artifact: Option<&Path>,
  screenshot_artifact_record: Option<&ArtifactRecordV1Alpha1>,
  evidence_artifacts: &[ArtifactRecordV1Alpha1],
  new_observation_count: usize,
) -> ObservationSnapshot {
  let nodes = surface_nodes_from_observations(run_id, span_id, page_observations);
  let screenshot_path = screenshot_artifact.map(|path| path.display().to_string());
  let capture_artifact = screenshot_artifact_record.map(|artifact| artifact_ref(run_id, artifact));
  let evidence = evidence_artifacts
    .iter()
    .map(|artifact| artifact_ref(run_id, artifact))
    .collect::<Vec<_>>();
  let observation_count = page_observations.len();
  let mut detail = serde_json::Map::new();
  detail.insert("page_index".to_string(), Value::from(page_index));
  detail.insert(
    "observation_count".to_string(),
    Value::from(observation_count),
  );
  detail.insert(
    "new_observation_count".to_string(),
    Value::from(new_observation_count),
  );
  if let Some(path) = &screenshot_path {
    detail.insert("screenshot_artifact".to_string(), Value::from(path.clone()));
  }

  let mut known_limits = Vec::new();
  if screenshot_artifact.is_none() {
    known_limits.push(
      "scroll_scan: observe response did not include a png artifact for this page".to_string(),
    );
  }
  if evidence.is_empty() {
    known_limits.push(
      "scroll_scan: observe response did not expose any evidence artifacts for this page"
        .to_string(),
    );
  }

  ObservationSnapshot {
    api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
    snapshot_id: format!("snapshot_{}_{:04}", run_id, page_index + 1),
    run_id: run_id.clone(),
    span_id: span_id.clone(),
    captured_at_millis: now_millis(),
    source: infer_observation_source(page_observations),
    scope: RecognitionScope {
      surface: RecognitionSurface::Region,
      display_ref: None,
      native_display_id: None,
      app_bundle_id: target.application_id.clone(),
      window_title: target.window_title.clone(),
      window_number: None,
      region_hint: Some(RatioRegion {
        left: target.region.left_ratio,
        top: target.region.top_ratio,
        right: target.region.right_ratio,
        bottom: target.region.bottom_ratio,
      }),
      capture_artifact,
      capture_contract_artifact: None,
    },
    capture_contract_ref: None,
    evidence,
    nodes,
    detail: Value::Object(detail),
    known_limits,
  }
}

fn artifact_ref(run_id: &RunId, artifact: &ArtifactRecordV1Alpha1) -> ArtifactRef {
  ArtifactRef {
    run_id: run_id.clone(),
    artifact_id: artifact.artifact_id.clone(),
    span_id: artifact.span_id.clone(),
    captured_event_id: artifact.event_id.clone(),
  }
}

fn infer_observation_source(page_observations: &[CollectionObservation]) -> ObservationSource {
  let mut saw_ax = false;
  let mut saw_ocr = false;
  let mut saw_visual = false;

  for observation in page_observations {
    match observation_source_hint(observation) {
      Some(ObservationSource::Ax) => saw_ax = true,
      Some(ObservationSource::Ocr) => saw_ocr = true,
      Some(ObservationSource::Visual) => saw_visual = true,
      Some(ObservationSource::Merged) => {
        saw_ocr = true;
        saw_visual = true;
      }
      None => {}
    }
  }

  let source_count = usize::from(saw_ax) + usize::from(saw_ocr) + usize::from(saw_visual);
  if source_count > 1 {
    ObservationSource::Merged
  } else if saw_visual {
    ObservationSource::Visual
  } else if saw_ax {
    ObservationSource::Ax
  } else {
    ObservationSource::Ocr
  }
}

fn observation_source_hint(observation: &CollectionObservation) -> Option<ObservationSource> {
  observation
    .attributes
    .get("recognition_source")
    .and_then(|value| classify_source_tag(value))
    .or_else(|| {
      observation
        .attributes
        .get("source")
        .and_then(|value| classify_source_tag(value))
    })
}

fn classify_source_tag(value: &str) -> Option<ObservationSource> {
  let normalized = value.trim().to_ascii_lowercase();
  if normalized.contains("ax") {
    Some(ObservationSource::Ax)
  } else if normalized.contains("ocr") {
    Some(ObservationSource::Ocr)
  } else if normalized.contains("visual")
    || normalized.contains("segment")
    || normalized.contains("icon")
  {
    Some(ObservationSource::Visual)
  } else {
    None
  }
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

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::path::PathBuf;

  use super::{
    build_page_observation_snapshot, infer_observation_source, normalize_observation_text,
  };
  use crate::contract::ObservationSource;
  use crate::scroll_scan::{CollectionObservation, ScanRect, ScanRegion, ScanTarget};
  use auv_tracing_driver::trace::{RunId, SpanId};

  fn sample_target() -> ScanTarget {
    ScanTarget {
      application_id: Some("com.example.App".to_string()),
      window_title: Some("Fixture".to_string()),
      region: ScanRegion {
        left_ratio: 0.2,
        top_ratio: 0.3,
        right_ratio: 0.8,
        bottom_ratio: 0.9,
      },
    }
  }

  fn sample_observation(id: &str, source_key: &str, source_value: &str) -> CollectionObservation {
    CollectionObservation {
      observation_id: id.to_string(),
      page_index: 0,
      raw_text: format!("{source_value} item"),
      normalized_text_key: normalize_observation_text(&format!("{source_value} item")),
      bounds: ScanRect {
        x: 10,
        y: 20,
        width: 100,
        height: 24,
      },
      section_context: None,
      source_artifacts: vec![PathBuf::from("/tmp/fixture.json")],
      attributes: BTreeMap::from([(source_key.to_string(), source_value.to_string())]),
    }
  }

  #[test]
  fn infer_observation_source_prefers_visual_when_rows_are_visual() {
    let observations = vec![sample_observation(
      "obs_visual",
      "recognition_source",
      "visual_row",
    )];
    assert_eq!(
      infer_observation_source(&observations),
      ObservationSource::Visual
    );
  }

  #[test]
  fn infer_observation_source_marks_mixed_visual_and_ocr_pages_as_merged() {
    let observations = vec![
      sample_observation("obs_ocr", "recognition_source", "ocr_row"),
      sample_observation("obs_visual", "recognition_source", "visual_row"),
    ];
    assert_eq!(
      infer_observation_source(&observations),
      ObservationSource::Merged
    );
  }

  #[test]
  fn build_page_observation_snapshot_uses_supplied_producer_span() {
    let run_id = RunId::new("run_snapshot_test");
    let span_id = SpanId::new("0000000000000007");
    let snapshot = build_page_observation_snapshot(
      &run_id,
      &span_id,
      0,
      &sample_target(),
      &[sample_observation(
        "obs_ocr",
        "recognition_source",
        "ocr_row",
      )],
      None,
      None,
      &[],
      1,
    );

    assert_eq!(snapshot.span_id, span_id);
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
