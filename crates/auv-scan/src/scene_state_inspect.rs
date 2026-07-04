//! L3 in-memory scene state consumption projection (structured text / read-model).
//!
//! NOTICE(scan-s6a): NOT a durable wire, read cache, or viewer surface. No `Serialize`.

use crate::scene_state::{
  SceneDraftAnswers, SceneStateError, SceneStateInput, SceneStateProduct,
  build_scene_state_product, observations_match_bundle,
};

/// L3 in-memory consumption surface. NOT a durable wire or read cache.
#[derive(Clone, Debug, PartialEq)]
pub struct SceneStateInspect {
  /// Memory-only convenience wrapper around the L2 product. NOT a schema or read cache.
  pub product: SceneStateProduct,
  pub bundle_frame_count: usize,
  pub observations_frame_count: usize,
  pub observations_input_valid: bool,
}

/// List/badge projection (mirrors ViewParserListSummary intent).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneStateListSummary {
  /// Always true when inspect build succeeded; reserved for future list aggregation over partial inputs.
  pub has_scene_state: bool,
  pub action_ready: bool,
  pub blocking_codes: Vec<String>,
  pub track_count: usize,
  pub recommended_observation_codes: Vec<String>,
}

/// Build the L3 inspect read surface from scene state input.
pub fn build_scene_state_inspect(
  input: &SceneStateInput,
) -> Result<SceneStateInspect, SceneStateError> {
  let product = build_scene_state_product(input)?;
  let bundle_frame_count = input.bundle.frames.len();
  let observations_frame_count = input.observations_by_frame.len();
  let observations_input_valid =
    observations_match_bundle(&input.bundle, &input.observations_by_frame);
  Ok(SceneStateInspect {
    product,
    bundle_frame_count,
    observations_frame_count,
    observations_input_valid,
  })
}

/// Summarize inspect for list/badge consumption.
pub fn summarize_scene_state_inspect(inspect: &SceneStateInspect) -> SceneStateListSummary {
  SceneStateListSummary {
    has_scene_state: inspect.bundle_frame_count > 0,
    action_ready: inspect.product.action_readiness.ready,
    blocking_codes: inspect.product.action_readiness.blocking_codes.clone(),
    track_count: inspect.product.tracks.len(),
    recommended_observation_codes: inspect
      .product
      .recommended_observations
      .iter()
      .map(|req| req.code.clone())
      .collect(),
  }
}

/// Structured text projection for scene state consumption (no IO).
pub fn format_scene_state_inspect_text(inspect: &SceneStateInspect) -> String {
  let product = &inspect.product;
  let mut lines = Vec::new();

  lines.push(format!(
    "[scene.input] as_of_frame_id={} bundle_frames={} observation_frames={} observations_valid={}",
    product.as_of_frame_id,
    inspect.bundle_frame_count,
    inspect.observations_frame_count,
    inspect.observations_input_valid,
  ));

  lines.push(format!(
    "[scene.readiness] ready={} reason={} blocking={:?}",
    product.action_readiness.ready,
    product.action_readiness.reason,
    product.action_readiness.blocking_codes,
  ));

  if product.tracks.is_empty() {
    lines.push("[scene.track] (none)".into());
  } else {
    for track in &product.tracks {
      lines.push(format!(
        "[scene.track] track_id={} last_seen={:?} latest_present={} identity={:?} visibility={:?} lifecycle={:?}",
        track.track_id,
        track.last_seen_frame_id,
        track.latest_observation_present,
        track.identity_assessment,
        track.visibility_assessment,
        track.lifecycle_verdict,
      ));
    }
  }

  if product.recommended_observations.is_empty() {
    lines.push("[scene.recommended] (none)".into());
  } else {
    for req in &product.recommended_observations {
      lines.push(format!(
        "[scene.recommended] code={} rationale={}",
        req.code, req.rationale,
      ));
    }
  }

  if product.diagnostics.is_empty() {
    lines.push("[scene.diagnostics] (none)".into());
  } else {
    for diag in &product.diagnostics {
      lines.push(format!(
        "[scene.diagnostics] code={} message={}",
        diag.code, diag.message,
      ));
    }
  }

  lines.push(format_draft_answers_section(&product.draft_answers));
  lines.join("\n")
}

fn format_draft_answers_section(draft: &SceneDraftAnswers) -> String {
  let recommended = draft
    .recommended_observations
    .iter()
    .map(|req| req.code.as_str())
    .collect::<Vec<_>>()
    .join(",");
  format!(
    "[scene.draft_answers] as_of={} tracks={} action_ready={} blocking={:?} recommended=[{recommended}]",
    draft.as_of_frame_id,
    draft.track_summaries.len(),
    draft.action_readiness.ready,
    draft.action_readiness.blocking_codes,
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::scene_fixture_support::scene_input_from_fixture;

  const SECTION_INPUT: &str = "[scene.input]";
  const SECTION_READINESS: &str = "[scene.readiness]";
  const SECTION_TRACK: &str = "[scene.track]";
  const SECTION_RECOMMENDED: &str = "[scene.recommended]";
  const SECTION_DIAGNOSTICS: &str = "[scene.diagnostics]";
  const SECTION_DRAFT: &str = "[scene.draft_answers]";

  fn build_inspect_from_fixture(scenario_dir: &str) -> SceneStateInspect {
    let input = scene_input_from_fixture(scenario_dir);
    build_scene_state_inspect(&input).expect("build inspect")
  }

  fn assert_all_section_markers(text: &str) {
    for marker in [
      SECTION_INPUT,
      SECTION_READINESS,
      SECTION_TRACK,
      SECTION_RECOMMENDED,
      SECTION_DIAGNOSTICS,
      SECTION_DRAFT,
    ] {
      assert!(text.contains(marker), "missing section marker: {marker}");
    }
  }

  #[test]
  fn inspect_durable_coverage_smoke() {
    let mut input = scene_input_from_fixture("scene_stable_v0");
    input.coverage_wire =
      crate::scene_fixture_support::coverage_wire_from_scene_fixture("scene_stable_v0");
    let inspect = build_scene_state_inspect(&input).expect("inspect");
    let product = build_scene_state_product(&input).expect("product");
    assert_eq!(inspect.product, product);
  }

  #[test]
  fn inspect_product_matches_direct_build() {
    let input = scene_input_from_fixture("scene_stable_v0");
    let inspect = build_scene_state_inspect(&input).expect("inspect");
    let product = build_scene_state_product(&input).expect("product");
    assert_eq!(inspect.product, product);
    assert_eq!(
      inspect.observations_input_valid,
      observations_match_bundle(&input.bundle, &input.observations_by_frame)
    );
  }

  #[test]
  fn summarize_scene_state_inspect_projection() {
    let inspect = build_inspect_from_fixture("scene_stale_v0");
    let summary = summarize_scene_state_inspect(&inspect);
    assert_eq!(summary.action_ready, inspect.product.action_readiness.ready);
    assert_eq!(
      summary.blocking_codes,
      inspect.product.action_readiness.blocking_codes
    );
    assert_eq!(summary.track_count, inspect.product.tracks.len());
    assert_eq!(
      summary.recommended_observation_codes,
      inspect
        .product
        .recommended_observations
        .iter()
        .map(|req| req.code.clone())
        .collect::<Vec<_>>()
    );
    assert!(
      summary
        .blocking_codes
        .iter()
        .any(|code| code == "no_new_observation")
    );
    assert!(
      summary
        .recommended_observation_codes
        .iter()
        .any(|code| code == "rescan_after_motion")
    );
  }

  #[test]
  fn scene_inspect_stable_fixture() {
    let inspect = build_inspect_from_fixture("scene_stable_v0");
    let summary = summarize_scene_state_inspect(&inspect);
    assert!(summary.action_ready);
    let text = format_scene_state_inspect_text(&inspect);
    assert_all_section_markers(&text);
    assert!(text.contains(SECTION_TRACK));
  }

  #[test]
  fn scene_inspect_stale_fixture() {
    let inspect = build_inspect_from_fixture("scene_stale_v0");
    let summary = summarize_scene_state_inspect(&inspect);
    assert!(
      summary
        .blocking_codes
        .iter()
        .any(|code| code == "no_new_observation")
    );
    let text = format_scene_state_inspect_text(&inspect);
    assert!(text.contains(SECTION_RECOMMENDED));
  }

  #[test]
  fn scene_inspect_ambiguous_fixture() {
    let inspect = build_inspect_from_fixture("scene_ambiguous_v0");
    let summary = summarize_scene_state_inspect(&inspect);
    assert!(
      summary
        .recommended_observation_codes
        .iter()
        .any(|code| code == "disambiguate_label")
    );
    let text = format_scene_state_inspect_text(&inspect);
    assert!(text.contains(SECTION_RECOMMENDED));
  }

  #[test]
  fn scene_inspect_lost_fixture() {
    let inspect = build_inspect_from_fixture("scene_lost_v0");
    let summary = summarize_scene_state_inspect(&inspect);
    assert!(
      summary
        .blocking_codes
        .iter()
        .any(|code| code == "lifecycle_lost")
    );
    let text = format_scene_state_inspect_text(&inspect);
    assert_all_section_markers(&text);
  }

  #[test]
  fn scene_inspect_missing_observations_fixture() {
    let inspect = build_inspect_from_fixture("scene_missing_observations_v0");
    assert!(!inspect.observations_input_valid);
    let text = format_scene_state_inspect_text(&inspect);
    assert!(text.contains("observations_valid=false"));
  }

  #[test]
  fn scene_inspect_lifecycle_bad_evidence_fixture() {
    let inspect = build_inspect_from_fixture("scene_lifecycle_bad_evidence_v0");
    let summary = summarize_scene_state_inspect(&inspect);
    assert!(
      summary
        .blocking_codes
        .iter()
        .any(|code| code == "lifecycle_missing_evidence")
    );
    let text = format_scene_state_inspect_text(&inspect);
    assert!(text.contains(SECTION_DIAGNOSTICS));
  }

  #[test]
  fn format_scene_state_inspect_text_has_fixed_section_order() {
    let inspect = build_inspect_from_fixture("scene_stable_v0");
    let text = format_scene_state_inspect_text(&inspect);
    let input_pos = text.find(SECTION_INPUT).expect("input");
    let readiness_pos = text.find(SECTION_READINESS).expect("readiness");
    let track_pos = text.find(SECTION_TRACK).expect("track");
    let recommended_pos = text.find(SECTION_RECOMMENDED).expect("recommended");
    let diagnostics_pos = text.find(SECTION_DIAGNOSTICS).expect("diagnostics");
    let draft_pos = text.find(SECTION_DRAFT).expect("draft");
    assert!(input_pos < readiness_pos);
    assert!(readiness_pos < track_pos);
    assert!(track_pos < recommended_pos);
    assert!(recommended_pos < diagnostics_pos);
    assert!(diagnostics_pos < draft_pos);
  }
}
