// File: src/app/analysis.rs
//! App probe analysis.
//!
//! Turns a collected `AppProbe` (probe runs + artifacts) into a structured
//! `AppAnalysis`: capability/permission assessments, surface candidates,
//! strategy recommendations, and explicit "known boundaries" when probe data is
//! partial.
//!
//! Boundary: analysis is inference + reporting over evidence; it does not run
//! automation itself (drivers/runtime do).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::contract::{CandidateQuery, SelectorScope, SurfaceSelector, SurfaceSelectorClause};
use crate::model::{AuvResult, RunStatus, now_millis};
use auv_driver_macos::support::{
  group_ocr_matches_into_rows, parse_observed_ax_tree, parse_ocr_text_snapshot, parse_window_line, report_value,
};
use auv_driver_macos::types::{
  ObservedAxNode, ObservedAxTreeSnapshot, ObservedDisplay, ObservedDisplaySnapshot, ObservedOcrRow, ObservedRect, ObservedWindow,
  OcrTextSnapshot, compute_combined_bounds,
};
use auv_tracing_driver::trace::{ArtifactId, EventId, RunId, SpanId};
use serde_json::Value;

use super::infra::first_non_empty_string;
use super::{
  APP_ANALYSIS_VERSION, AppAnalysis, AppAvailableSurfaces, AppCandidateCompatibility, AppCandidateGroundingTaxonomy,
  AppCandidatePromotionGate, AppCandidatePromotionStatus, AppControlAssessment, AppDisturbanceProfile, AppGroundingAssessment, AppIdentity,
  AppPermissionState, AppPoint, AppProbe, AppProbeArtifact, AppProbeStep, AppRecommendedStrategy, AppSurfaceCandidate,
  AppVerificationAssessment, AppWindowContext, AssessmentStatus, rect_center, rect_center_point, rect_relative_point, render_compact_rect,
};

#[derive(Clone, Debug)]
struct WindowSnapshotAnalysis {
  observed_at: String,
  frontmost_app_name: String,
  frontmost_window_title: String,
  windows: Vec<ObservedWindow>,
}

pub(crate) fn build_app_analysis(probe_path: &Path, probe: &AppProbe) -> AuvResult<AppAnalysis> {
  let mut known_boundaries = probe.app.resolution_notes.clone();
  known_boundaries.extend(summarize_failed_probe_steps(probe));
  let permission_state = parse_permission_state(probe).unwrap_or_else(|error| {
    known_boundaries.push(format!("Permission probe data was incomplete: {error}. Treat permission-dependent conclusions as provisional."));
    default_permission_state()
  });
  let display_snapshot = parse_display_step(probe).unwrap_or_else(|error| {
    known_boundaries.push(format!("Display probe data was incomplete: {error}. Display-relative projection remains provisional."));
    default_display_snapshot()
  });
  let window_snapshot = parse_window_snapshot(probe).unwrap_or_else(|error| {
    known_boundaries
      .push(format!("Window observation data was incomplete: {error}. Window-targeted control should be treated as candidate-only."));
    default_window_snapshot()
  });
  let ax_snapshot = parse_ax_snapshot(probe).unwrap_or_else(|error| {
    known_boundaries.push(format!("AX snapshot was unavailable or partial: {error}. AX-first strategies remain candidate-only."));
    default_ax_snapshot(&probe.app)
  });
  let ocr_snapshot = parse_ocr_snapshot(probe).unwrap_or_else(|error| {
    known_boundaries.push(format!("OCR sample was unavailable or partial: {error}. OCR-anchor strategies remain candidate-only."));
    default_ocr_snapshot(&probe.app)
  });

  let primary_window = choose_primary_window(&window_snapshot.windows);
  let primary_window_bounds =
    primary_window.map(|window| window.bounds.clone()).or_else(|| primary_window_bounds_from_ax_snapshot(&ax_snapshot));
  let primary_window_display_scale =
    primary_window_bounds.as_ref().and_then(|bounds| display_scale_for_rect_center(&display_snapshot, bounds));

  let text_input_count = count_text_inputs(&ax_snapshot);
  let button_like_count = count_button_like_nodes(&ax_snapshot);
  let content_text_bearing_count = count_content_text_bearing_nodes(&ax_snapshot);
  let ax_quality = classify_ax_quality(ax_snapshot.nodes.len(), text_input_count, button_like_count, content_text_bearing_count);
  let menu_surface = if has_menu_surface(&ax_snapshot) {
    AssessmentStatus::Available
  } else {
    AssessmentStatus::Unknown
  };
  let shortcut_surface = if text_input_count > 0 || button_like_count > 0 {
    AssessmentStatus::Candidate
  } else {
    AssessmentStatus::Unknown
  };
  let keyboard_first_surface = if text_input_count > 0 {
    AssessmentStatus::Candidate
  } else {
    AssessmentStatus::Unknown
  };
  let pointer_fallback_surface =
    if ax_quality == AssessmentStatus::Partial || ax_quality == AssessmentStatus::Unknown || ocr_snapshot.matches.is_empty() {
      AssessmentStatus::Likely
    } else {
      AssessmentStatus::Candidate
    };
  let apple_script_surface = if probe.app.apple_script_addressable {
    AssessmentStatus::Available
  } else {
    AssessmentStatus::Unavailable
  };
  let url_scheme_surface = if probe.app.url_schemes.is_empty() {
    AssessmentStatus::Unavailable
  } else {
    AssessmentStatus::Available
  };
  let available_surfaces = AppAvailableSurfaces {
    accessibility_tree: ax_quality,
    menu_surface,
    shortcut_surface,
    apple_script_surface,
    url_scheme_surface,
    keyboard_first_surface,
    pointer_fallback_surface,
  };

  let ocr_sample_status = if ocr_snapshot.matches.is_empty() {
    AssessmentStatus::Unavailable
  } else if ocr_snapshot.matches.len() >= 2 {
    AssessmentStatus::Candidate
  } else {
    AssessmentStatus::Partial
  };

  let mut stable_anchor_candidates = Vec::new();
  if !probe.app.app_name.trim().is_empty() {
    stable_anchor_candidates.push(format!("appName: {}", probe.app.app_name));
  }
  if !ax_snapshot.window_title.trim().is_empty() {
    stable_anchor_candidates.push(format!("windowTitle: {}", ax_snapshot.window_title));
  }
  if let Some(first_text_node) = first_content_text_bearing_node(&ax_snapshot) {
    stable_anchor_candidates.push(format!("axText: {}", summarize_ax_node_text(first_text_node)));
  }
  let mut stable_region_candidates = Vec::new();
  if let Some(bounds) = primary_window_bounds.as_ref() {
    stable_region_candidates.push(format!("primaryWindow={}", render_compact_rect(bounds)));
  }
  stable_region_candidates.push("fullWindowCapture".to_string());

  let grounding_assessment = AppGroundingAssessment {
    ocr_sample_query: ocr_snapshot.query.clone(),
    ocr_sample_status,
    ocr_sample_match_count: ocr_snapshot.matches.len(),
    stable_anchor_candidates,
    stable_region_candidates,
    overlay_debug_artifacts_recommended: ocr_snapshot.matches.is_empty() || ax_quality != AssessmentStatus::Available,
  };
  let annotation_candidates = build_annotation_candidates(
    &probe.app,
    primary_window,
    primary_window_bounds.as_ref(),
    &ax_snapshot,
    &ocr_snapshot,
    &probe.steps,
    has_collection_like_surface(&ax_snapshot),
  );

  let mut control_notes = Vec::new();
  if keyboard_first_surface == AssessmentStatus::Candidate {
    control_notes.push(
      "AX snapshot exposed at least one text-input-like node; keyboard-first entry is a plausible candidate but still unvalidated."
        .to_string(),
    );
  }
  if pointer_fallback_surface == AssessmentStatus::Likely {
    control_notes.push("Semantic controls remain partially opaque in the current surface snapshot; pointer fallback is likely for at least one interaction layer.".to_string());
  }
  let control_assessment = AppControlAssessment {
    preferred_path: if keyboard_first_surface == AssessmentStatus::Candidate {
      "non-pointer path first; escalate to pointer fallback only for opaque semantic targets".to_string()
    } else {
      "start from explicit observation and expect pointer fallback for primary control".to_string()
    },
    non_pointer_path: if text_input_count > 0 {
      AssessmentStatus::Candidate
    } else {
      AssessmentStatus::Unknown
    },
    keyboard_path: keyboard_first_surface,
    pointer_fallback: pointer_fallback_surface,
    notes: control_notes,
  };

  let mut verification_notes = Vec::new();
  let ax_verify = if content_text_bearing_count > 0 {
    verification_notes.push("AX tree contains text-bearing nodes; verifyAxText is a viable candidate contract.".to_string());
    AssessmentStatus::Candidate
  } else {
    AssessmentStatus::Unavailable
  };
  let image_verify = if ocr_snapshot.matches.is_empty() {
    verification_notes.push("Sample OCR over the captured screenshot returned zero filtered matches; image-text verification is possible but currently weak for this sample.".to_string());
    AssessmentStatus::Partial
  } else {
    verification_notes.push(
      "Sample OCR over the captured screenshot returned filtered matches; image-text verification is a candidate surface.".to_string(),
    );
    AssessmentStatus::Candidate
  };
  let ui_state_verify = if primary_window_bounds.is_some() {
    AssessmentStatus::Candidate
  } else {
    AssessmentStatus::Unknown
  };
  let semantic_success = if ax_verify == AssessmentStatus::Candidate {
    AssessmentStatus::Candidate
  } else {
    AssessmentStatus::Unknown
  };
  let verification_assessment = AppVerificationAssessment {
    ax_verify,
    image_verify,
    ui_state_verify,
    semantic_success,
    notes: verification_notes,
  };

  let disturbance_profile = AppDisturbanceProfile {
    observation: vec!["none".to_string()],
    non_pointer_control: vec![
      "focus".to_string(),
      "foreground_app".to_string(),
      "keyboard".to_string(),
      "clipboard".to_string(),
    ],
    pointer_fallback: vec!["pointer".to_string()],
  };

  if permission_state.accessibility != "granted" {
    known_boundaries.push(format!(
      "Accessibility permission is {} instead of granted; AX-first strategies will remain degraded until this is fixed.",
      permission_state.accessibility
    ));
  }
  if permission_state.screen_recording != "granted" {
    known_boundaries.push(format!(
      "Screen Recording permission is {} instead of granted; screenshot/OCR evidence may be blocked or partial.",
      permission_state.screen_recording
    ));
  }
  if ocr_snapshot.matches.is_empty() {
    known_boundaries.push(
      "The sample OCR query did not produce a filtered match on the captured image; treat OCR text anchors as candidate-only until a validated chain proves otherwise."
        .to_string(),
    );
  }
  if ax_quality == AssessmentStatus::Partial || ax_quality == AssessmentStatus::Unknown {
    known_boundaries.push(
      "The sampled AX tree is partial rather than fully semantic; treat pointer fallback as a likely requirement for at least one control layer."
        .to_string(),
    );
  }

  let has_search_entry_surface = find_search_entry_node(&ax_snapshot).is_some();

  let mut recommended_strategies = Vec::new();
  if has_search_entry_surface {
    recommended_strategies.push(recommended_strategy(
      "search-entry",
      "ax-text-input",
      "clipboard-submit",
      "captureEvidence",
      AssessmentStatus::Candidate,
      "The sampled AX surface exposed a search-like text input, so a keyboard/clipboard search-entry path is worth validating before escalating to pointer control.",
    )?);
  }
  if content_text_bearing_count > 0 {
    recommended_strategies.push(recommended_strategy(
      "native-text",
      "ax-text",
      "ax-perform-action-clipboard-paste",
      "verifyAxText",
      AssessmentStatus::Candidate,
      "The sampled AX tree contains visible text-bearing nodes, which makes AX-based verification and native-text flows plausible candidates.",
    )?);
  }
  let grouped_result_rows = grouped_result_row_count(&annotation_candidates);
  if has_collection_like_surface(&ax_snapshot) && grouped_result_rows >= 2 {
    known_boundaries.push(format!(
      "The sample surface produced {grouped_result_rows} grouped visible-row candidates. Analyze records them as surface candidates, but does not promote them to a recipe strategy until a row-selection action and verification contract are validated."
    ));
  }
  if primary_window_bounds.is_some() && pointer_fallback_surface == AssessmentStatus::Likely && recommended_strategies.is_empty() {
    recommended_strategies.push(recommended_strategy(
      "window-action",
      "window-point",
      "pointer-click",
      "captureEvidence",
      AssessmentStatus::Candidate,
      "The sampled app surface exposed a stable primary window region but not a reliable semantic target. Distill a window-relative pointer candidate instead of inventing AX or OCR grounding that the probe did not prove.",
    )?);
  }

  Ok(AppAnalysis {
    analysis_version: APP_ANALYSIS_VERSION.to_string(),
    created_at_millis: now_millis(),
    probe_path: probe_path.to_path_buf(),
    app_identity: probe.app.clone(),
    window_context: AppWindowContext {
      observed_window_count: window_snapshot.windows.len(),
      observed_at: window_snapshot.observed_at,
      frontmost_app_name: window_snapshot.frontmost_app_name,
      frontmost_window_title: window_snapshot.frontmost_window_title,
      primary_window_title: primary_window
        .map(|window| window.title.clone())
        .filter(|title| !title.trim().is_empty())
        .or_else(|| non_empty_trimmed(&ax_snapshot.window_title))
        .unwrap_or_default(),
      primary_window_bounds,
      primary_window_display_scale,
    },
    permissions: permission_state,
    available_surfaces,
    grounding_assessment,
    control_assessment,
    verification_assessment,
    disturbance_profile,
    annotation_candidates,
    known_boundaries,
    recommended_strategies,
  })
}

pub(crate) fn summarize_failed_probe_steps(probe: &AppProbe) -> Vec<String> {
  probe
    .steps
    .iter()
    .filter(|step| step.status != RunStatus::Completed.as_str())
    .map(|step| {
      let failure = step.failure_message.clone().unwrap_or_else(|| step.output_summary.clone());
      format!("Probe step `{}` (`{}`) did not complete successfully: {}", step.id, step.command_id, failure)
    })
    .collect()
}

pub(crate) fn build_annotation_candidates(
  app: &AppIdentity,
  primary_window: Option<&ObservedWindow>,
  primary_window_bounds: Option<&ObservedRect>,
  ax_snapshot: &ObservedAxTreeSnapshot,
  ocr_snapshot: &OcrTextSnapshot,
  probe_steps: &[AppProbeStep],
  has_collection_surface: bool,
) -> Vec<AppSurfaceCandidate> {
  let mut candidates = Vec::new();

  if let Some(bounds) = primary_window_bounds.cloned() {
    let compact_bounds = render_compact_rect(&bounds);
    let click_point = rect_center_point(&bounds);
    let input_bindings = window_region_input_bindings(&compact_bounds, &click_point, &bounds);
    let evidence_step_id = if primary_window.is_some() {
      "list-windows"
    } else {
      "capture-ax-tree"
    };
    let evidence_refs = artifact_refs_for_probe_step(probe_steps, evidence_step_id);
    let note = if primary_window.is_some() {
      "Primary visible window bounds from the window snapshot."
    } else {
      "Primary window bounds inferred from the AX root window because the window snapshot did not expose a visible window."
    };
    let mut candidate = AppSurfaceCandidate {
      candidate_id: "window-primary-region".to_string(),
      area: "window.primary".to_string(),
      kind: "region".to_string(),
      source: if primary_window.is_some() {
        "window".to_string()
      } else {
        "ax".to_string()
      },
      status: AssessmentStatus::Candidate,
      primary_text: primary_window
        .map(|window| window.title.clone())
        .filter(|title| !title.trim().is_empty())
        .unwrap_or_else(|| app.app_name.clone()),
      secondary_text: app.bundle_id.clone(),
      query_value: compact_bounds.clone(),
      coordinate_space: "global-logical".to_string(),
      click_point: Some(click_point.clone()),
      bounds: Some(bounds),
      confidence: None,
      evidence_step_id: evidence_step_id.to_string(),
      candidate_query: None,
      evidence_refs,
      promotion_gate: None,
      input_bindings,
      compatibility: candidate_compatibility(&["window-action.window-point.pointer-click.capture-evidence"], &[]),
      notes: vec![note.to_string()],
    };
    candidate.promotion_gate = Some(promotion_gate_for_candidate(&candidate));
    candidates.push(candidate);
  }

  if let Some(node) = find_search_entry_node(ax_snapshot)
    && let Some(query_value) = preferred_ax_query_text(node)
  {
    candidates.push(ax_focus_candidate(
      "search-entry-focus-ax",
      "search-entry",
      "focus-query",
      node,
      query_value,
      "capture-ax-tree",
      artifact_refs_for_probe_step(probe_steps, "capture-ax-tree"),
      candidate_compatibility(&["search-entry.ax-text-input.clipboard-submit.capture-evidence"], &[]),
      "AX-exposed search-entry or search-like input candidate.",
    ));
  }

  if let Some(node) = find_native_text_focus_node(ax_snapshot)
    && let Some(query_value) = preferred_ax_query_text(node)
  {
    candidates.push(ax_focus_candidate(
      "native-text-focus-ax",
      "native-text",
      "focus-query",
      node,
      query_value,
      "capture-ax-tree",
      artifact_refs_for_probe_step(probe_steps, "capture-ax-tree"),
      candidate_compatibility(&["native-text.ax-text.ax-perform-action-clipboard-paste.verify-ax-text"], &[]),
      "AX-exposed editable text-surface candidate.",
    ));
  }

  for matched in ocr_snapshot.matches.iter().take(8) {
    let bounds = matched.bounds.clone();
    let area = "ocr-visible-text";
    let mut notes = vec!["Visible OCR text candidate from the sampled screenshot artifact.".to_string()];
    if matched.text == ocr_snapshot.query {
      notes.push("This match equals the sampled OCR query.".to_string());
    }
    if has_collection_surface && ocr_snapshot.matches.len() >= 2 {
      notes.push(
        "Collection-like surface was present, but OCR text alone is title-level evidence until grouped rows or semantic result evidence corroborate it."
          .to_string(),
      );
    }
    let mut candidate = AppSurfaceCandidate {
      candidate_id: format!("ocr-anchor-{}", matched.match_index),
      area: area.to_string(),
      kind: "anchor-text".to_string(),
      source: "ocr".to_string(),
      status: if has_collection_surface {
        AssessmentStatus::Candidate
      } else {
        AssessmentStatus::Partial
      },
      primary_text: matched.text.clone(),
      secondary_text: String::new(),
      query_value: matched.text.clone(),
      coordinate_space: "global-logical".to_string(),
      click_point: Some(rect_center_point(&bounds)),
      bounds: Some(bounds),
      confidence: Some(matched.confidence),
      evidence_step_id: "ocr-sample".to_string(),
      candidate_query: Some(ocr_candidate_query(&format!("ocr-anchor-{}", matched.match_index), &matched.text, Some(matched.confidence))),
      evidence_refs: artifact_refs_for_probe_step(probe_steps, "ocr-sample"),
      promotion_gate: None,
      input_bindings: BTreeMap::from([("anchor_text".to_string(), matched.text.clone())]),
      compatibility: AppCandidateCompatibility::default(),
      notes,
    };
    candidate.promotion_gate = Some(promotion_gate_for_candidate(&candidate));
    candidates.push(candidate);
  }

  if has_collection_surface && ocr_snapshot.matches.len() >= 2 {
    let rows = group_ocr_rows_from_ocr_snapshot(ocr_snapshot);
    for row in rows.into_iter().take(8) {
      candidates.push(row_candidate(row, artifact_refs_for_probe_step(probe_steps, "ocr-sample")));
    }
  }

  candidates
}

fn window_region_input_bindings(compact_bounds: &str, click_point: &AppPoint, bounds: &ObservedRect) -> BTreeMap<String, String> {
  let mut bindings = BTreeMap::from([("window_bounds".to_string(), compact_bounds.to_string())]);
  if let Some((relative_x, relative_y)) = rect_relative_point(bounds, click_point) {
    bindings.insert("relative_x".to_string(), format!("{relative_x:.6}"));
    bindings.insert("relative_y".to_string(), format!("{relative_y:.6}"));
  }
  bindings
}

fn ax_focus_candidate(
  candidate_id: &str,
  area: &str,
  kind: &str,
  node: &ObservedAxNode,
  query_value: String,
  evidence_step_id: &str,
  evidence_refs: Vec<crate::contract::ArtifactRef>,
  compatibility: AppCandidateCompatibility,
  note: &str,
) -> AppSurfaceCandidate {
  let bounds = node.bounds.clone();
  let focus_query = query_value.clone();
  let mut candidate = AppSurfaceCandidate {
    candidate_id: candidate_id.to_string(),
    area: area.to_string(),
    kind: kind.to_string(),
    source: "ax".to_string(),
    status: AssessmentStatus::Candidate,
    primary_text: summarize_ax_node_text(node),
    secondary_text: format!("role={} path={}", node.role, node.path),
    query_value: query_value.clone(),
    coordinate_space: "global-logical".to_string(),
    click_point: Some(rect_center_point(&bounds)),
    bounds: Some(bounds),
    confidence: None,
    evidence_step_id: evidence_step_id.to_string(),
    candidate_query: Some(ax_candidate_query(candidate_id, node, &query_value, kind)),
    evidence_refs,
    promotion_gate: None,
    input_bindings: BTreeMap::from([("focus_query".to_string(), focus_query)]),
    compatibility,
    notes: vec![note.to_string()],
  };
  candidate.promotion_gate = Some(promotion_gate_for_candidate(&candidate));
  candidate
}

fn group_ocr_rows_from_ocr_snapshot(snapshot: &OcrTextSnapshot) -> Vec<ObservedOcrRow> {
  let matches = snapshot.matches.iter().collect::<Vec<_>>();
  group_ocr_matches_into_rows(&matches)
}

fn row_candidate(row: ObservedOcrRow, evidence_refs: Vec<crate::contract::ArtifactRef>) -> AppSurfaceCandidate {
  let bounds = row.bounds.clone();
  let mut candidate = AppSurfaceCandidate {
    candidate_id: format!("visible-row-{}", row.row_index + 1),
    area: "result-selection".to_string(),
    kind: "row".to_string(),
    source: row.source,
    status: AssessmentStatus::Candidate,
    primary_text: row.text_fragments.join(" | "),
    secondary_text: format!("rowIndex={}", row.row_index + 1),
    query_value: format!("{}", row.row_index + 1),
    coordinate_space: "global-logical".to_string(),
    click_point: Some(rect_center_point(&bounds)),
    bounds: Some(bounds),
    confidence: None,
    evidence_step_id: "ocr-sample".to_string(),
    evidence_refs,
    candidate_query: Some(row_candidate_query(
      &format!("visible-row-{}", row.row_index + 1),
      row.row_index + 1,
      &row.text_fragments.join(" "),
    )),
    promotion_gate: None,
    input_bindings: BTreeMap::from([("row_index".to_string(), format!("{}", row.row_index + 1))]),
    compatibility: candidate_compatibility(&[], &["result-selection.ocr-anchor.pointer-click.capture-evidence"]),
    notes: vec!["Visible row candidate grouped from OCR observations; useful for list-like UI targets.".to_string()],
  };
  candidate.promotion_gate = Some(promotion_gate_for_candidate(&candidate));
  candidate
}

fn promotion_gate_for_candidate(candidate: &AppSurfaceCandidate) -> AppCandidatePromotionGate {
  let mut missing_gates = Vec::new();
  let mut notes = Vec::new();

  if candidate.evidence_refs.is_empty() {
    missing_gates.push("artifact_ref".to_string());
    notes.push("Candidate has no ArtifactRef; action consumers cannot reconstruct the source evidence chain.".to_string());
  }

  if candidate.candidate_query.is_none() && candidate.input_bindings.is_empty() {
    missing_gates.push("relocation_query_or_inputs".to_string());
    notes.push("Candidate has neither a surface selector query nor recipe input bindings for re-grounding.".to_string());
  }

  let status = match (candidate.area.as_str(), candidate.kind.as_str()) {
    ("search-entry", "focus-query") if search_entry_candidate_is_action_grade(candidate) => {
      notes.push("Candidate satisfies the v0 search-entry promotion seam and can project into contract::Candidate.".to_string());
      AppCandidatePromotionStatus::ActionGradeCandidate
    }
    ("native-text", "focus-query") if native_text_candidate_is_action_grade(candidate) => {
      notes.push("Candidate satisfies the v0 native-text promotion seam and can project into contract::Candidate.".to_string());
      AppCandidatePromotionStatus::ActionGradeCandidate
    }
    ("window.primary", "region") if window_action_candidate_is_action_grade(candidate) => {
      notes.push("Candidate satisfies the v0 window-action promotion seam and can project into contract::Candidate.".to_string());
      AppCandidatePromotionStatus::ActionGradeCandidate
    }
    ("ocr-visible-text", _) => {
      push_unique(&mut missing_gates, "semantic_verification_contract");
      push_unique(&mut missing_gates, "action_contract");
      notes.push("OCR visible text remains evidence only; it is not a validated semantic target.".to_string());
      AppCandidatePromotionStatus::Blocked
    }
    ("result-selection", "row") => {
      push_unique(&mut missing_gates, "row_action_contract");
      push_unique(&mut missing_gates, "semantic_verification_contract");
      notes.push(
        "Row/list grouping is a surface candidate; it needs a row action and semantic verifier before action-grade promotion.".to_string(),
      );
      AppCandidatePromotionStatus::Blocked
    }
    _ if !candidate.compatibility.direct_taxonomy_ids.is_empty() => {
      notes.push(
        "Candidate can seed a known distillation strategy, but this slice does not promote this surface family into contract::Candidate."
          .to_string(),
      );
      AppCandidatePromotionStatus::DistillStrategyOnly
    }
    _ => {
      push_unique(&mut missing_gates, "promotion_path");
      notes.push("No direct taxonomy or action contract currently consumes this candidate.".to_string());
      AppCandidatePromotionStatus::Blocked
    }
  };

  AppCandidatePromotionGate {
    status,
    missing_gates,
    notes,
  }
}

fn search_entry_candidate_is_action_grade(candidate: &AppSurfaceCandidate) -> bool {
  candidate.area == "search-entry"
    && candidate.kind == "focus-query"
    && !candidate.evidence_refs.is_empty()
    && candidate.candidate_query.is_some()
    && !candidate.query_value.trim().is_empty()
    && candidate
      .compatibility
      .direct_taxonomy_ids
      .iter()
      .any(|value| value == "search-entry.ax-text-input.clipboard-submit.capture-evidence")
}

fn native_text_candidate_is_action_grade(candidate: &AppSurfaceCandidate) -> bool {
  candidate.area == "native-text"
    && candidate.kind == "focus-query"
    && !candidate.evidence_refs.is_empty()
    && candidate.candidate_query.is_some()
    && !candidate.query_value.trim().is_empty()
    && candidate.compatibility.direct_taxonomy_ids.iter().any(|value| {
      AppCandidateGroundingTaxonomy::parse(value)
        .is_ok_and(|taxonomy| taxonomy == AppCandidateGroundingTaxonomy::NativeTextAxTextAxPerformActionClipboardPasteVerifyAxText)
    })
}

fn window_action_candidate_is_action_grade(candidate: &AppSurfaceCandidate) -> bool {
  candidate.area == "window.primary"
    && candidate.kind == "region"
    && !candidate.evidence_refs.is_empty()
    && candidate.bounds.is_some()
    && candidate.click_point.is_some()
    && candidate.input_bindings.contains_key("relative_x")
    && candidate.input_bindings.contains_key("relative_y")
    && candidate.compatibility.direct_taxonomy_ids.iter().any(|value| value == "window-action.window-point.pointer-click.capture-evidence")
}

fn push_unique(values: &mut Vec<String>, value: &str) {
  if !values.iter().any(|existing| existing == value) {
    values.push(value.to_string());
  }
}

fn artifact_refs_for_probe_step(steps: &[AppProbeStep], step_id: &str) -> Vec<crate::contract::ArtifactRef> {
  steps
    .iter()
    .find(|step| step.id == step_id)
    .map(|step| step.artifacts.iter().map(|artifact| app_probe_artifact_ref(step, artifact)).collect())
    .unwrap_or_default()
}

fn app_probe_artifact_ref(step: &AppProbeStep, artifact: &AppProbeArtifact) -> crate::contract::ArtifactRef {
  crate::contract::ArtifactRef {
    run_id: RunId::new(step.run_id.clone()),
    artifact_id: ArtifactId::new(artifact.artifact_id.clone()),
    span_id: SpanId::new(artifact.span_id.clone()),
    captured_event_id: artifact.captured_event_id.as_ref().map(|event_id| EventId::new(event_id.clone())),
  }
}

pub(crate) fn candidate_compatibility(direct_taxonomy_ids: &[&str], context_taxonomy_ids: &[&str]) -> AppCandidateCompatibility {
  AppCandidateCompatibility {
    direct_taxonomy_ids: direct_taxonomy_ids.iter().map(|value| value.to_string()).collect(),
    context_taxonomy_ids: context_taxonomy_ids.iter().map(|value| value.to_string()).collect(),
  }
}

fn grouped_result_row_count(candidates: &[AppSurfaceCandidate]) -> usize {
  candidates.iter().filter(|candidate| candidate.area == "result-selection" && candidate.kind == "row").count()
}

fn ax_candidate_query(candidate_id: &str, node: &ObservedAxNode, label: &str, output_kind: &str) -> CandidateQuery {
  CandidateQuery {
    query_id: candidate_id.to_string(),
    selector: SurfaceSelector {
      any_of: vec![SurfaceSelectorClause::Ax {
        role: non_empty_trimmed(&node.role),
        label: non_empty_trimmed(label),
        path: non_empty_trimmed(&node.path),
        enabled: None,
        visible: Some(true),
      }],
      within: SelectorScope::TargetWindow,
      require_visible: true,
    },
    output_kind: Some(output_kind.to_string()),
    known_limits: vec!["Generated from app analyze AX snapshot; validate liveness before action.".to_string()],
  }
}

fn ocr_candidate_query(candidate_id: &str, text: &str, provider_score: Option<f64>) -> CandidateQuery {
  CandidateQuery {
    query_id: candidate_id.to_string(),
    selector: SurfaceSelector {
      any_of: vec![SurfaceSelectorClause::Ocr {
        text: text.to_string(),
        region_hint: None,
        min_provider_score: provider_score,
      }],
      within: SelectorScope::TargetWindow,
      require_visible: true,
    },
    output_kind: Some("anchor-text".to_string()),
    known_limits: vec!["OCR text from app analyze is visible-text evidence, not semantic result evidence.".to_string()],
  }
}

fn row_candidate_query(candidate_id: &str, row_index: usize, contains_text: &str) -> CandidateQuery {
  CandidateQuery {
    query_id: candidate_id.to_string(),
    selector: SurfaceSelector {
      any_of: vec![SurfaceSelectorClause::Row {
        row_index: Some(row_index),
        contains_text: non_empty_trimmed(contains_text),
        region_hint: None,
      }],
      within: SelectorScope::TargetWindow,
      require_visible: true,
    },
    output_kind: Some("row".to_string()),
    known_limits: vec!["Grouped visible row is structural evidence; semantic identity requires later verification.".to_string()],
  }
}

fn find_search_entry_node(snapshot: &ObservedAxTreeSnapshot) -> Option<&ObservedAxNode> {
  snapshot.nodes.iter().find(|node| {
    node.subrole == "AXSearchField"
      || node.role == "AXSearchField"
      || node.placeholder.to_lowercase().contains("search")
      || node.title.to_lowercase().contains("search")
      || node.description.to_lowercase().contains("search")
      || node.placeholder.contains("搜索")
      || node.title.contains("搜索")
      || node.description.contains("搜索")
  })
}

fn find_native_text_focus_node(snapshot: &ObservedAxTreeSnapshot) -> Option<&ObservedAxNode> {
  snapshot.nodes.iter().find(|node| {
    let role = node.role.as_str();
    let subrole = node.subrole.as_str();
    role == "AXTextField"
      || role == "AXTextArea"
      || role == "AXComboBox"
      || subrole == "AXSearchField"
      || !node.placeholder.trim().is_empty()
  })
}

fn preferred_ax_query_text(node: &ObservedAxNode) -> Option<String> {
  first_non_empty_string(&[
    non_empty_trimmed(&node.placeholder),
    non_empty_trimmed(&node.title),
    non_empty_trimmed(&node.description),
    non_empty_trimmed(&node.help),
    non_empty_trimmed(&node.identifier),
    short_non_empty_value(&node.value),
  ])
}

fn short_non_empty_value(value: &str) -> Option<String> {
  let trimmed = value.trim();
  if trimmed.is_empty() || trimmed.len() > 64 || trimmed.contains('\n') {
    None
  } else {
    Some(trimmed.to_string())
  }
}

fn non_empty_trimmed(value: &str) -> Option<String> {
  let trimmed = value.trim();
  if trimmed.is_empty() {
    None
  } else {
    Some(trimmed.to_string())
  }
}

fn parse_permission_state(probe: &AppProbe) -> AuvResult<AppPermissionState> {
  let report = read_named_text_artifact(probe, "probe-permissions", None)?;
  Ok(AppPermissionState {
    screen_recording: report_value(&report, "screenRecording=").unwrap_or("unknown").to_string(),
    accessibility: report_value(&report, "accessibility=").unwrap_or("unknown").to_string(),
    automation_to_system_events: report_value(&report, "automationToSystemEvents=").unwrap_or("unknown").to_string(),
    launch_host_process: report_value(&report, "launchHostProcess=").unwrap_or("unknown").to_string(),
  })
}

fn parse_display_step(probe: &AppProbe) -> AuvResult<ObservedDisplaySnapshot> {
  let raw = read_named_artifact(probe, "list-displays", Some("display-list"), "json")?;
  let displays_json: Vec<Value> =
    serde_json::from_str(&raw).map_err(|error| format!("failed to parse list-displays JSON artifact: {error}"))?;
  let displays = displays_json.iter().map(parse_display_descriptor_value).collect::<AuvResult<Vec<_>>>()?;
  if displays.is_empty() {
    return Err("list-displays artifact did not contain any displays".to_string());
  }
  Ok(ObservedDisplaySnapshot {
    combined_bounds: compute_combined_bounds(&displays),
    displays,
    captured_at: "".to_string(),
  })
}

fn parse_window_snapshot(probe: &AppProbe) -> AuvResult<WindowSnapshotAnalysis> {
  let report = read_named_text_artifact(probe, "list-windows", Some("window-list"))?;
  let windows = report.lines().filter(|line| line.starts_with("window\t")).map(parse_window_line).collect::<AuvResult<Vec<_>>>()?;
  Ok(WindowSnapshotAnalysis {
    observed_at: report_value(&report, "observedAt=").unwrap_or("").to_string(),
    frontmost_app_name: report_value(&report, "frontmostAppName=").unwrap_or("").to_string(),
    frontmost_window_title: report_value(&report, "frontmostWindowTitle=").unwrap_or("").to_string(),
    windows,
  })
}

pub(crate) fn parse_ax_snapshot(probe: &AppProbe) -> AuvResult<ObservedAxTreeSnapshot> {
  let report = read_named_text_artifact(probe, "capture-ax-tree", None)?;
  parse_observed_ax_tree(&report)
}

fn parse_ocr_snapshot(probe: &AppProbe) -> AuvResult<OcrTextSnapshot> {
  let report = read_named_text_artifact(probe, "ocr-sample", None)?;
  parse_ocr_text_snapshot(&report)
}

fn default_permission_state() -> AppPermissionState {
  AppPermissionState {
    screen_recording: "unknown".to_string(),
    accessibility: "unknown".to_string(),
    automation_to_system_events: "unknown".to_string(),
    launch_host_process: "unknown".to_string(),
  }
}

fn default_display_snapshot() -> ObservedDisplaySnapshot {
  ObservedDisplaySnapshot {
    combined_bounds: ObservedRect {
      x: 0,
      y: 0,
      width: 0,
      height: 0,
    },
    displays: Vec::new(),
    captured_at: String::new(),
  }
}

fn default_window_snapshot() -> WindowSnapshotAnalysis {
  WindowSnapshotAnalysis {
    observed_at: String::new(),
    frontmost_app_name: String::new(),
    frontmost_window_title: String::new(),
    windows: Vec::new(),
  }
}

fn default_ax_snapshot(app: &AppIdentity) -> ObservedAxTreeSnapshot {
  ObservedAxTreeSnapshot {
    observed_at: String::new(),
    app_name: app.app_name.clone(),
    bundle_id: app.bundle_id.clone(),
    pid: 0,
    window_title: String::new(),
    nodes: Vec::new(),
  }
}

fn default_ocr_snapshot(app: &AppIdentity) -> OcrTextSnapshot {
  OcrTextSnapshot {
    recognized_at: String::new(),
    image_path: PathBuf::new(),
    image_width: 0,
    image_height: 0,
    query: if app.app_name.trim().is_empty() {
      app.bundle_id.clone()
    } else {
      app.app_name.clone()
    },
    exact: false,
    case_sensitive: false,
    matches: Vec::new(),
  }
}

pub(crate) fn resolve_probe_ocr_sample_query(app: &AppIdentity, steps: &[AppProbeStep]) -> String {
  let window_report = read_probe_step_artifact_text(steps, "observe-windows", None);
  let ax_report = read_probe_step_artifact_text(steps, "observe-window-tree", None);
  first_non_empty_string(&[
    window_report.as_deref().and_then(|report| report_value(report, "frontmostWindowTitle=")).map(str::to_string),
    window_report.as_deref().and_then(|report| report_value(report, "frontmostAppName=")).map(str::to_string),
    ax_report.as_deref().and_then(|report| report_value(report, "windowTitle=")).map(str::to_string),
    ax_report.as_deref().and_then(|report| report_value(report, "appName=")).map(str::to_string),
    non_empty_trimmed(&app.app_name),
    non_empty_trimmed(&app.bundle_id),
  ])
  .unwrap_or_else(|| app.bundle_id.clone())
}

fn read_probe_step_artifact_text(steps: &[AppProbeStep], step_id: &str, file_name_hint: Option<&str>) -> Option<String> {
  let step = steps.iter().find(|step| step.id == step_id)?;
  let artifact_path = step.artifact_paths.iter().find(|path| {
    path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("txt"))
      && file_name_hint.is_none_or(|hint| path.file_name().and_then(|value| value.to_str()).is_some_and(|name| name.contains(hint)))
  })?;
  fs::read_to_string(artifact_path).ok()
}

fn read_named_text_artifact(probe: &AppProbe, step_id: &str, file_name_hint: Option<&str>) -> AuvResult<String> {
  read_named_artifact(probe, step_id, file_name_hint, "txt")
}

fn read_named_artifact(probe: &AppProbe, step_id: &str, file_name_hint: Option<&str>, extension: &str) -> AuvResult<String> {
  let step = probe.steps.iter().find(|step| step.id == step_id).ok_or_else(|| format!("probe is missing required step {}", step_id))?;
  let artifact_path = step
    .artifact_paths
    .iter()
    .find(|path| {
      path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case(extension))
        && file_name_hint.is_none_or(|hint| path.file_name().and_then(|value| value.to_str()).is_some_and(|name| name.contains(hint)))
    })
    .cloned()
    .ok_or_else(|| format!("probe step {} did not produce the expected .{} artifact", step_id, extension))?;
  fs::read_to_string(&artifact_path).map_err(|error| format!("failed to read probe artifact {}: {error}", artifact_path.display()))
}

fn parse_display_descriptor_value(value: &Value) -> AuvResult<ObservedDisplay> {
  let display_ref = value.get("display_ref").and_then(Value::as_str).unwrap_or("display");
  let native_display_id = value
    .get("native_display_id")
    .and_then(Value::as_str)
    .unwrap_or("0")
    .parse::<u32>()
    .map_err(|error| format!("invalid native_display_id for {display_ref} in display-list artifact: {error}"))?;
  let bounds = parse_json_rect(value, "global_logical_bounds", display_ref)?;
  let visible_bounds = parse_json_rect(value, "visible_logical_bounds", display_ref)?;
  let physical_pixel_size =
    value.get("physical_pixel_size").ok_or_else(|| format!("display {display_ref} is missing physical_pixel_size"))?;
  Ok(ObservedDisplay {
    display_id: native_display_id,
    is_main: value.get("is_main").and_then(Value::as_bool).unwrap_or(false),
    is_built_in: value.get("is_builtin").and_then(Value::as_bool).unwrap_or(false),
    bounds,
    visible_bounds,
    scale_factor: value.get("scale_factor").and_then(Value::as_f64).unwrap_or(1.0),
    pixel_width: json_number_to_i64(physical_pixel_size, "width", display_ref)?,
    pixel_height: json_number_to_i64(physical_pixel_size, "height", display_ref)?,
  })
}

fn parse_json_rect(value: &Value, field: &str, display_ref: &str) -> AuvResult<ObservedRect> {
  let rect = value.get(field).ok_or_else(|| format!("display {display_ref} is missing {field}"))?;
  Ok(ObservedRect {
    x: json_number_to_i64(rect, "x", display_ref)?,
    y: json_number_to_i64(rect, "y", display_ref)?,
    width: json_number_to_i64(rect, "width", display_ref)?,
    height: json_number_to_i64(rect, "height", display_ref)?,
  })
}

fn json_number_to_i64(value: &Value, field: &str, display_ref: &str) -> AuvResult<i64> {
  value
    .get(field)
    .and_then(Value::as_f64)
    .map(|number| number.round() as i64)
    .ok_or_else(|| format!("display {display_ref} has invalid numeric field {field}"))
}

fn choose_primary_window(windows: &[ObservedWindow]) -> Option<&ObservedWindow> {
  windows.iter().max_by_key(|window| {
    let area = window.bounds.width.max(0) * window.bounds.height.max(0);
    let titled_bonus = if window.title.trim().is_empty() { 0 } else { 1 };
    (area, titled_bonus)
  })
}

fn primary_window_bounds_from_ax_snapshot(snapshot: &ObservedAxTreeSnapshot) -> Option<ObservedRect> {
  snapshot
    .nodes
    .iter()
    .find(|node| {
      (node.role == "AXWindow" || node.subrole == "AXStandardWindow" || node.subrole.ends_with("Window"))
        && node.bounds.width > 0
        && node.bounds.height > 0
    })
    .map(|node| node.bounds.clone())
}

fn display_scale_for_rect_center(snapshot: &ObservedDisplaySnapshot, rect: &ObservedRect) -> Option<f64> {
  let (x, y) = rect_center(rect);
  snapshot.displays.iter().find(|display| contains_point(&display.bounds, x, y)).map(|display| display.scale_factor)
}

fn contains_point(rect: &ObservedRect, x: i64, y: i64) -> bool {
  x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height
}

fn classify_ax_quality(node_count: usize, text_input_count: usize, button_like_count: usize, text_bearing_count: usize) -> AssessmentStatus {
  if node_count == 0 {
    AssessmentStatus::Unavailable
  } else if node_count >= 20 && (text_input_count + button_like_count + text_bearing_count) >= 8 {
    AssessmentStatus::Available
  } else if node_count >= 6 && (text_input_count > 0 || button_like_count > 0 || text_bearing_count > 0) {
    AssessmentStatus::Partial
  } else {
    AssessmentStatus::Unknown
  }
}

fn count_text_inputs(snapshot: &ObservedAxTreeSnapshot) -> usize {
  snapshot
    .nodes
    .iter()
    .filter(|node| {
      let role = node.role.as_str();
      let subrole = node.subrole.as_str();
      role == "AXTextField"
        || role == "AXTextArea"
        || role == "AXComboBox"
        || subrole == "AXSearchField"
        || !node.placeholder.trim().is_empty()
    })
    .count()
}

fn count_button_like_nodes(snapshot: &ObservedAxTreeSnapshot) -> usize {
  snapshot.nodes.iter().filter(|node| node.role == "AXButton" || node.subrole == "AXButton" || node.role == "AXLink").count()
}

fn is_window_chrome_node(node: &ObservedAxNode) -> bool {
  node.role == "AXWindow"
    || node.role == "AXApplication"
    || node.subrole == "AXStandardWindow"
    || (node.role == "AXScrollArea" && summarize_ax_node_text(node).is_empty())
}

fn count_content_text_bearing_nodes(snapshot: &ObservedAxTreeSnapshot) -> usize {
  snapshot.nodes.iter().filter(|node| !is_window_chrome_node(node) && !summarize_ax_node_text(node).is_empty()).count()
}

fn first_content_text_bearing_node(snapshot: &ObservedAxTreeSnapshot) -> Option<&ObservedAxNode> {
  snapshot.nodes.iter().find(|node| !is_window_chrome_node(node) && !summarize_ax_node_text(node).is_empty())
}

fn summarize_ax_node_text(node: &ObservedAxNode) -> String {
  [
    node.title.as_str(),
    node.description.as_str(),
    node.placeholder.as_str(),
    node.value.as_str(),
  ]
  .into_iter()
  .map(str::trim)
  .find(|value| !value.is_empty())
  .unwrap_or("")
  .to_string()
}

fn has_menu_surface(snapshot: &ObservedAxTreeSnapshot) -> bool {
  snapshot.nodes.iter().any(|node| node.role.contains("Menu") || node.subrole.contains("Menu"))
}

fn has_collection_like_surface(snapshot: &ObservedAxTreeSnapshot) -> bool {
  snapshot
    .nodes
    .iter()
    .filter(|node| {
      matches!(node.role.as_str(), "AXRow" | "AXCell" | "AXTable" | "AXOutline" | "AXList" | "AXBrowser")
        || matches!(node.subrole.as_str(), "AXRow" | "AXCell" | "AXTable" | "AXOutline" | "AXList")
    })
    .count()
    >= 2
}

pub(crate) fn recommended_strategy(
  family: &str,
  grounding: &str,
  activation: &str,
  verification_contract: &str,
  status: AssessmentStatus,
  rationale: &str,
) -> AuvResult<AppRecommendedStrategy> {
  let verification_contract = app_verification_contract_taxonomy_id(verification_contract)?;
  Ok(AppRecommendedStrategy {
    taxonomy_id: format!("{family}.{grounding}.{activation}.{verification_contract}"),
    status,
    rationale: rationale.to_string(),
  })
}

fn app_verification_contract_taxonomy_id(raw: &str) -> AuvResult<&'static str> {
  match raw.trim() {
    "captureEvidence" => Ok("capture-evidence"),
    "verifyImageText" => Ok("verify-image-text"),
    "verifyNowPlayingTitle" => Ok("verify-now-playing-title"),
    "verifyAxText" => Ok("verify-ax-text"),
    other => Err(format!("verification contract {other} is unsupported for app analysis recommendations")),
  }
}
