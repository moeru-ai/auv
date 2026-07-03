use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use serde_json::json;
#[cfg(target_os = "macos")]
use std::fs;
#[cfg(target_os = "macos")]
use std::path::Path;

use crate::candidate_promotion::PromotionProjection;
use crate::contract::RecognitionResult;
#[cfg(target_os = "macos")]
use crate::contract::{
  ArtifactRef, RecognitionBox, RecognitionScope, RecognitionSource, RecognitionSurface,
  RecognizedItem,
};
#[cfg(target_os = "macos")]
use crate::model::{AuvResult, now_millis};
#[cfg(target_os = "macos")]
use auv_tracing_driver::recorded_operation::RecordedOperationContext;

#[cfg(target_os = "macos")]
use auv_driver_macos::types::{ObservedAxNode, ObservedAxTreeSnapshot, ObservedRect};

const AX_RECOGNITION_ARTIFACT_ROLE: &str = "ax-recognition";
const AX_RECOGNITION_BRIDGE_VERSION: &str = "ax-tree-recognitionresult.v0";

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AxRecognitionPolicy {
  pub query: Option<String>,
  pub role: Option<String>,
  pub require_bounds: bool,
  pub best_selection: AxBestSelectionStrategy,
}

impl Default for AxRecognitionPolicy {
  fn default() -> Self {
    Self {
      query: None,
      role: None,
      require_bounds: true,
      best_selection: AxBestSelectionStrategy::SingleFilteredItem,
    }
  }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AxBestSelectionStrategy {
  None,
  SingleFilteredItem,
  HighestScore,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxRecognitionRuntimeContext {
  pub recognition_id: String,
  pub source_artifact: ArtifactRef,
  pub window_number: Option<i64>,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AxRecognitionArtifactRequest {
  pub recognition_id: String,
  pub policy: AxRecognitionPolicy,
  pub artifact_role: String,
  pub artifact_label: String,
  pub artifact_note: String,
}

#[cfg(target_os = "macos")]
impl AxRecognitionArtifactRequest {
  pub fn new(recognition_id: impl Into<String>) -> Self {
    let recognition_id = recognition_id.into();
    Self {
      artifact_label: recognition_id.clone(),
      recognition_id,
      policy: AxRecognitionPolicy::default(),
      artifact_role: AX_RECOGNITION_ARTIFACT_ROLE.to_string(),
      artifact_note:
        "AX tree-backed RecognitionResult runtime artifact for window-addressable evidence."
          .to_string(),
    }
  }
}

#[cfg(target_os = "macos")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AxRecognitionError {
  NoUsableNodes,
}

#[cfg(target_os = "macos")]
impl std::fmt::Display for AxRecognitionError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::NoUsableNodes => write!(f, "AX recognition requires at least one usable AX node"),
    }
  }
}

#[cfg(target_os = "macos")]
impl std::error::Error for AxRecognitionError {}

#[cfg(target_os = "macos")]
pub fn map_ax_tree_to_recognition_result(
  snapshot: &ObservedAxTreeSnapshot,
  context: &AxRecognitionRuntimeContext,
  policy: &AxRecognitionPolicy,
) -> Result<RecognitionResult, AxRecognitionError> {
  let scored_nodes = snapshot
    .nodes
    .iter()
    .enumerate()
    .filter(|(_, node)| !policy.require_bounds || has_bounds(node))
    .filter_map(|(index, node)| score_node(index, node, policy).map(|score| (index, node, score)))
    .collect::<Vec<_>>();

  if scored_nodes.is_empty() {
    return Err(AxRecognitionError::NoUsableNodes);
  }

  let all = scored_nodes
    .iter()
    .map(|(index, node, score)| {
      recognized_ax_item(*index, node, *score, window_frame_from_snapshot(snapshot))
    })
    .collect::<Vec<_>>();
  let filtered = all.clone();
  let filtered_node_count = filtered.len();
  let best = select_best(&filtered, policy);

  Ok(RecognitionResult {
    recognition_id: context.recognition_id.clone(),
    source: RecognitionSource::Custom,
    scope: RecognitionScope {
      surface: RecognitionSurface::Window,
      display_ref: None,
      native_display_id: None,
      app_bundle_id: non_empty_string(&snapshot.bundle_id),
      window_title: non_empty_string(&snapshot.window_title),
      window_number: context.window_number,
      region_hint: None,
      capture_artifact: Some(context.source_artifact.clone()),
      capture_contract_artifact: None,
    },
    best,
    filtered,
    all,
    detail: json!({
      "bridge_policy_version": AX_RECOGNITION_BRIDGE_VERSION,
      "provider": "macos.ax_tree",
      "app_name": snapshot.app_name,
      "bundle_id": snapshot.bundle_id,
      "pid": snapshot.pid,
      "window_title": snapshot.window_title,
      "query": policy.query,
      "role": policy.role,
      "require_bounds": policy.require_bounds,
      "best_selection": policy.best_selection,
      "projection_candidate": "identity_window_addressable",
      "coordinate_basis": "ax_frame_bounds",
      "node_count": snapshot.nodes.len(),
      "filtered_node_count": filtered_node_count,
    }),
    evidence: vec![context.source_artifact.clone()],
    known_limits: vec![
      "AX RecognitionResult is addressability evidence only; it does not imply action permission"
        .to_string(),
      "AX node bounds are AX frame coordinates for re-addressing, not detector source-image pixels"
        .to_string(),
    ],
  })
}

#[cfg(target_os = "macos")]
pub fn record_ax_tree_recognition_artifact(
  context: &mut RecordedOperationContext<'_>,
  snapshot: &ObservedAxTreeSnapshot,
  ax_tree_report_path: &Path,
  ax_tree_artifact_role: &str,
  ax_tree_artifact_name: &str,
  ax_tree_artifact_summary: Option<String>,
  request: &AxRecognitionArtifactRequest,
) -> AuvResult<(ArtifactRef, ArtifactRef, RecognitionResult)> {
  let (_, ax_tree_artifact_ref) = context.stage_artifact_file_with_ref(
    ax_tree_artifact_role,
    ax_tree_report_path,
    ax_tree_artifact_name,
    ax_tree_artifact_summary,
  )?;

  let runtime_context = AxRecognitionRuntimeContext {
    recognition_id: request.recognition_id.clone(),
    source_artifact: ax_tree_artifact_ref.clone(),
    window_number: None,
  };
  let recognition = map_ax_tree_to_recognition_result(snapshot, &runtime_context, &request.policy)
    .map_err(|error| format!("failed to map AX tree into recognition result: {error}"))?;

  let recognition_json = serde_json::to_string_pretty(&recognition)
    .map(|mut rendered| {
      rendered.push('\n');
      rendered
    })
    .map_err(|error| format!("failed to encode AX recognition result JSON: {error}"))?;
  let recognition_source_path = ax_recognition_temp_json_path(&request.artifact_label);
  fs::write(&recognition_source_path, recognition_json).map_err(|error| {
    format!(
      "failed to write AX recognition temp artifact {}: {error}",
      recognition_source_path.display()
    )
  })?;

  let (_, recognition_artifact_ref) = context.stage_artifact_file_with_ref(
    &request.artifact_role,
    &recognition_source_path,
    format!("{}.json", sanitize_artifact_label(&request.artifact_label)),
    Some(request.artifact_note.clone()),
  )?;
  let _ = fs::remove_file(&recognition_source_path);

  context.record_event(
    "ax.recognition.artifact_recorded",
    Some(format!(
      "recorded {} from AX tree {}",
      recognition_artifact_ref.artifact_id, ax_tree_artifact_ref.artifact_id
    )),
  );

  Ok((ax_tree_artifact_ref, recognition_artifact_ref, recognition))
}

pub fn promotion_projection_for_recognition(
  recognition: &RecognitionResult,
) -> PromotionProjection {
  let provider = recognition
    .detail
    .get("provider")
    .and_then(serde_json::Value::as_str);
  let projection_candidate = recognition
    .detail
    .get("projection_candidate")
    .and_then(serde_json::Value::as_str);

  if provider == Some("macos.ax_tree")
    && projection_candidate == Some("identity_window_addressable")
    && recognition.scope.capture_artifact.is_some()
    && recognition.best.is_some()
  {
    return PromotionProjection::IdentityWindowAddressable;
  }

  PromotionProjection::Unavailable {
    reason: "recognition is not AX window-addressable evidence".to_string(),
  }
}

#[cfg(target_os = "macos")]
fn score_node(index: usize, node: &ObservedAxNode, policy: &AxRecognitionPolicy) -> Option<i64> {
  if let Some(role) = policy.role.as_deref()
    && node.role != role
  {
    return None;
  }

  let mut score = 1000_i64 - index as i64;
  if let Some(query) = policy.query.as_deref() {
    let query = normalize(query);
    if query.is_empty() {
      return Some(score);
    }
    let searchable = searchable_text(node);
    if !searchable.contains(&query) {
      return None;
    }
    if normalize(&node.title) == query {
      score += 200;
    } else if normalize(&node.title).contains(&query) {
      score += 120;
    }
    if normalize(&node.description).contains(&query) {
      score += 80;
    }
    if normalize(&node.value).contains(&query) {
      score += 60;
    }
    if normalize(&node.placeholder).contains(&query) {
      score += 40;
    }
  }
  score -= node.depth as i64;
  Some(score)
}

#[cfg(target_os = "macos")]
fn recognized_ax_item(
  index: usize,
  node: &ObservedAxNode,
  score: i64,
  window_frame: Option<&ObservedRect>,
) -> RecognizedItem {
  RecognizedItem {
    item_id: format!("ax:{}:{}", node.path, index),
    kind: ax_item_kind(node),
    box_: RecognitionBox {
      x: node.bounds.x,
      y: node.bounds.y,
      width: node.bounds.width,
      height: node.bounds.height,
    },
    text: preferred_text(node),
    provider_score: Some(score as f64),
    detail: json!({
      "provider": "macos.ax_tree",
      "path": node.path,
      "role": node.role,
      "subrole": node.subrole,
      "title": node.title,
      "description": node.description,
      "identifier": node.identifier,
      "placeholder": node.placeholder,
      "value": node.value,
      "focused": node.focused,
      "depth": node.depth,
      "coordinate_basis": "ax_frame_bounds",
      "projection_candidate": "identity_window_addressable",
      "window_frame": window_frame.map(|bounds| {
        json!({
          "x": bounds.x,
          "y": bounds.y,
          "width": bounds.width,
          "height": bounds.height,
        })
      }),
    }),
  }
}

#[cfg(target_os = "macos")]
fn window_frame_from_snapshot(snapshot: &ObservedAxTreeSnapshot) -> Option<&ObservedRect> {
  snapshot
    .nodes
    .iter()
    .find(|node| node.role == "AXWindow" && has_bounds(node))
    .map(|node| &node.bounds)
}

#[cfg(target_os = "macos")]
fn select_best(
  filtered: &[RecognizedItem],
  policy: &AxRecognitionPolicy,
) -> Option<RecognizedItem> {
  match policy.best_selection {
    AxBestSelectionStrategy::None => None,
    AxBestSelectionStrategy::SingleFilteredItem if filtered.len() == 1 => filtered.first().cloned(),
    AxBestSelectionStrategy::SingleFilteredItem => None,
    AxBestSelectionStrategy::HighestScore => filtered
      .iter()
      .max_by(|left, right| {
        left
          .provider_score
          .partial_cmp(&right.provider_score)
          .unwrap_or(std::cmp::Ordering::Equal)
      })
      .cloned(),
  }
}

#[cfg(target_os = "macos")]
fn has_bounds(node: &ObservedAxNode) -> bool {
  node.bounds.width > 0 && node.bounds.height > 0
}

#[cfg(target_os = "macos")]
fn searchable_text(node: &ObservedAxNode) -> String {
  normalize(
    &[
      node.title.as_str(),
      node.description.as_str(),
      node.help.as_str(),
      node.identifier.as_str(),
      node.placeholder.as_str(),
      node.value.as_str(),
    ]
    .join(" "),
  )
}

#[cfg(target_os = "macos")]
fn normalize(value: &str) -> String {
  value
    .chars()
    .filter(|character| !character.is_whitespace())
    .collect::<String>()
    .to_lowercase()
}

#[cfg(target_os = "macos")]
fn ax_item_kind(node: &ObservedAxNode) -> String {
  if node.subrole.trim().is_empty() {
    node.role.clone()
  } else {
    format!("{}:{}", node.role, node.subrole)
  }
}

#[cfg(target_os = "macos")]
fn preferred_text(node: &ObservedAxNode) -> Option<String> {
  for value in [
    &node.value,
    &node.title,
    &node.description,
    &node.help,
    &node.placeholder,
    &node.identifier,
  ] {
    let trimmed = value.trim();
    if !trimmed.is_empty() {
      return Some(trimmed.to_string());
    }
  }
  None
}

fn non_empty_string(value: &str) -> Option<String> {
  let trimmed = value.trim();
  if trimmed.is_empty() {
    None
  } else {
    Some(trimmed.to_string())
  }
}

#[cfg(target_os = "macos")]
#[cfg(target_os = "macos")]
fn ax_recognition_temp_json_path(label: &str) -> std::path::PathBuf {
  std::env::temp_dir().join(format!(
    "auv-ax-recognition-{}-{}-{}.json",
    sanitize_artifact_label(label),
    now_millis(),
    std::process::id()
  ))
}

fn sanitize_artifact_label(raw: &str) -> String {
  let sanitized = raw
    .chars()
    .map(|character| match character {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
      _ => '-',
    })
    .collect::<String>()
    .trim_matches('-')
    .to_string();
  if sanitized.is_empty() {
    "artifact".to_string()
  } else {
    sanitized
  }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
  use std::fs;
  use std::path::PathBuf;

  use serde_json::json;

  use super::{
    AxBestSelectionStrategy, AxRecognitionArtifactRequest, AxRecognitionPolicy,
    AxRecognitionRuntimeContext, map_ax_tree_to_recognition_result,
    promotion_projection_for_recognition, record_ax_tree_recognition_artifact,
  };
  use crate::build_runtime_with_store_root;
  use crate::candidate_promotion::PromotionProjection;
  use crate::contract::ArtifactRef;
  use auv_driver_macos::types::{ObservedAxNode, ObservedAxTreeSnapshot, ObservedRect};
  use auv_tracing_driver::run_builder::RunSpec;
  use auv_tracing_driver::trace::{ArtifactId, EventId, RunId, RunType, SpanId, TraceStatusCode};

  fn artifact_ref() -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new("run_ax_recognition"),
      artifact_id: ArtifactId::new("artifact_ax_tree"),
      span_id: SpanId::new("span_ax_recognition"),
      captured_event_id: Some(EventId::new("event_ax_tree")),
    }
  }

  fn node(path: &str, role: &str, title: &str, x: i64, y: i64) -> ObservedAxNode {
    ObservedAxNode {
      depth: path.matches('.').count(),
      path: path.to_string(),
      role: role.to_string(),
      subrole: String::new(),
      title: title.to_string(),
      description: String::new(),
      help: String::new(),
      identifier: String::new(),
      placeholder: String::new(),
      value: String::new(),
      focused: false,
      bounds: ObservedRect {
        x,
        y,
        width: 120,
        height: 40,
      },
    }
  }

  fn snapshot() -> ObservedAxTreeSnapshot {
    ObservedAxTreeSnapshot {
      observed_at: "2026-06-07T10:00:00Z".to_string(),
      app_name: "Notes".to_string(),
      bundle_id: "com.apple.Notes".to_string(),
      pid: 42,
      window_title: "Notes".to_string(),
      nodes: vec![
        node("0", "AXWindow", "Notes", 0, 0),
        node("0.0", "AXButton", "Share", 100, 20),
        node("0.1", "AXButton", "Done", 240, 20),
      ],
    }
  }

  #[test]
  fn ax_tree_maps_to_window_addressable_recognition_result() {
    let result = map_ax_tree_to_recognition_result(
      &snapshot(),
      &AxRecognitionRuntimeContext {
        recognition_id: "recognition_ax_done".to_string(),
        source_artifact: artifact_ref(),
        window_number: None,
      },
      &AxRecognitionPolicy {
        query: Some("Done".to_string()),
        role: Some("AXButton".to_string()),
        require_bounds: true,
        best_selection: AxBestSelectionStrategy::SingleFilteredItem,
      },
    )
    .expect("AX tree should map into RecognitionResult");

    assert_eq!(result.recognition_id, "recognition_ax_done");
    assert_eq!(
      result.scope.app_bundle_id.as_deref(),
      Some("com.apple.Notes")
    );
    assert_eq!(result.scope.window_title.as_deref(), Some("Notes"));
    assert_eq!(result.evidence.len(), 1);
    assert_eq!(result.all.len(), 1);
    assert_eq!(
      result.best.as_ref().map(|item| item.text.as_deref()),
      Some(Some("Done"))
    );
    assert_eq!(
      result.detail["projection_candidate"],
      json!("identity_window_addressable")
    );
    assert_eq!(
      promotion_projection_for_recognition(&result),
      PromotionProjection::IdentityWindowAddressable
    );
    assert_eq!(
      result
        .best
        .as_ref()
        .and_then(|item| item.detail.get("window_frame")),
      Some(&json!({
        "x": 0,
        "y": 0,
        "width": 120,
        "height": 40
      }))
    );
  }

  #[test]
  fn non_ax_recognition_keeps_projection_unavailable() {
    let mut result = map_ax_tree_to_recognition_result(
      &snapshot(),
      &AxRecognitionRuntimeContext {
        recognition_id: "recognition_ax_done".to_string(),
        source_artifact: artifact_ref(),
        window_number: None,
      },
      &AxRecognitionPolicy::default(),
    )
    .expect("AX tree should map");
    result.detail = json!({"provider": "manual-fixture"});

    assert!(matches!(
      promotion_projection_for_recognition(&result),
      PromotionProjection::Unavailable { .. }
    ));
  }

  #[test]
  fn recorded_operation_persists_ax_recognition_artifact() {
    let project_root = temp_dir("ax-recognition-record-project");
    let store_root = temp_dir("ax-recognition-record-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let ax_report_path = project_root.join("ax-tree.txt");
    fs::write(&ax_report_path, "synthetic ax tree").expect("AX report should write");
    let runtime = build_runtime_with_store_root(project_root.clone(), store_root.clone())
      .expect("runtime should build");
    let snapshot = snapshot();
    let request = AxRecognitionArtifactRequest {
      recognition_id: "recognition_ax_done_recorded".to_string(),
      policy: AxRecognitionPolicy {
        query: Some("Done".to_string()),
        role: Some("AXButton".to_string()),
        require_bounds: true,
        best_selection: AxBestSelectionStrategy::SingleFilteredItem,
      },
      artifact_role: "ax-recognition".to_string(),
      artifact_label: "notes-done-ax-recognition".to_string(),
      artifact_note: "Recorded AX-backed RecognitionResult artifact.".to_string(),
    };

    let output = runtime
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.ax.recognition"),
        "AX recognition artifact recording",
        |context| {
          record_ax_tree_recognition_artifact(
            context,
            &snapshot,
            &ax_report_path,
            "ax-tree",
            "ax-tree.txt",
            Some("Source AX tree artifact.".to_string()),
            &request,
          )
        },
      )
      .expect("recorded AX recognition operation should succeed");

    let run = runtime
      .recording()
      .read_run(output.run_id.as_str())
      .expect("recorded run should persist");
    assert_eq!(run.run.status_code, TraceStatusCode::Ok);
    assert_eq!(run.artifacts.len(), 2);
    assert_eq!(run.artifacts[0].role, "ax-tree");
    assert_eq!(run.artifacts[1].role, "ax-recognition");

    let (ax_tree_ref, recognition_ref, recognition) = output.value;
    assert_eq!(ax_tree_ref.artifact_id.as_str(), "artifact_0001");
    assert_eq!(recognition_ref.artifact_id.as_str(), "artifact_0002");
    assert_eq!(
      recognition
        .scope
        .capture_artifact
        .as_ref()
        .map(|reference| reference.artifact_id.as_str()),
      Some("artifact_0001")
    );
    assert_eq!(
      promotion_projection_for_recognition(&recognition),
      PromotionProjection::IdentityWindowAddressable
    );
    assert!(
      run
        .events
        .iter()
        .any(|event| event.name == "ax.recognition.artifact_recorded")
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()))
  }
}
