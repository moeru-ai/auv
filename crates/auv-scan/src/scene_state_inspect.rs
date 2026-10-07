//! L3 in-memory scene state consumption projection (structured text / read-model).
//!
//! NOTICE(scan-s6a): NOT a durable wire, read cache, or viewer surface. No `Serialize`.

use crate::scene_state::{
  SceneDraftAnswers, SceneStateError, SceneStateInput, SceneStateProduct, build_scene_state_product, items_match_frames,
};

/// L3 in-memory consumption surface. NOT a durable wire or read cache.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneStateInspect {
  /// Memory-only convenience wrapper around the L2 product. NOT a schema or read cache.
  pub product: SceneStateProduct,
  pub frame_count: usize,
  pub items_frame_count: usize,
  pub items_input_valid: bool,
}

/// List/badge projection (mirrors ViewParserListSummary intent).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneStateListSummary {
  pub action_ready: bool,
  pub blocking_codes: Vec<String>,
  pub track_count: usize,
  pub recommendation_codes: Vec<String>,
}

/// Build the L3 inspect read surface from scene state input.
pub fn build_scene_state_inspect(input: &SceneStateInput) -> Result<SceneStateInspect, SceneStateError> {
  let product = build_scene_state_product(input)?;
  let frame_count = input.frames.len();
  let items_frame_count = input.items_by_frame.len();
  let items_input_valid = items_match_frames(&input.frames, &input.items_by_frame);
  Ok(SceneStateInspect {
    product,
    frame_count,
    items_frame_count,
    items_input_valid,
  })
}

/// Summarize inspect for list/badge consumption.
pub fn summarize_scene_state_inspect(inspect: &SceneStateInspect) -> SceneStateListSummary {
  SceneStateListSummary {
    action_ready: inspect.product.action_readiness.ready,
    blocking_codes: inspect.product.action_readiness.blocking_codes.clone(),
    track_count: inspect.product.tracks.len(),
    recommendation_codes: inspect.product.recommendations.iter().map(|req| req.code.clone()).collect(),
  }
}

/// Structured text projection for scene state consumption (no IO).
pub fn format_scene_state_inspect_text(inspect: &SceneStateInspect) -> String {
  let product = &inspect.product;
  let mut lines = Vec::new();

  lines.push(format!(
    "[scene.input] as_of_frame_id={} frames={} item_frames={} items_valid={}",
    product.as_of_frame_id, inspect.frame_count, inspect.items_frame_count, inspect.items_input_valid,
  ));

  lines.push(format!("[scene.coverage] entry_count={}", product.coverage.entries.len(),));

  lines.push(format!(
    "[scene.readiness] ready={} reason={} blocking={:?}",
    product.action_readiness.ready, product.action_readiness.reason, product.action_readiness.blocking_codes,
  ));

  if product.tracks.is_empty() {
    lines.push("[scene.track] (none)".into());
  } else {
    for track in &product.tracks {
      lines.push(format!(
        "[scene.track] track_id={} last_seen={:?} latest_present={} identity={:?} visibility={:?} lifecycle={:?}",
        track.track_id,
        track.last_seen_frame_id,
        track.latest_item_present,
        track.identity_assessment,
        track.visibility_assessment,
        track.lifecycle_verdict,
      ));
    }
  }

  if product.recommendations.is_empty() {
    lines.push("[scene.recommended] (none)".into());
  } else {
    for req in &product.recommendations {
      lines.push(format!("[scene.recommended] code={} rationale={}", req.code, req.rationale,));
    }
  }

  if product.diagnostics.is_empty() {
    lines.push("[scene.diagnostics] (none)".into());
  } else {
    for diag in &product.diagnostics {
      lines.push(format!("[scene.diagnostics] code={} message={}", diag.code, diag.message,));
    }
  }

  lines.push(format_draft_answers_section(product.draft_answers()));
  lines.join("\n")
}

fn format_draft_answers_section(draft: SceneDraftAnswers<'_>) -> String {
  let recommended = draft.recommendations.iter().map(|req| req.code.as_str()).collect::<Vec<_>>().join(",");
  format!(
    "[scene.draft_answers] as_of={} tracks={} action_ready={} blocking={:?} recommended=[{recommended}]",
    draft.as_of_frame_id,
    draft.tracks.len(),
    draft.action_readiness.ready,
    draft.action_readiness.blocking_codes,
  )
}
