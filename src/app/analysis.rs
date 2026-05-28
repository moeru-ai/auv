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

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::contract::{
  ArtifactRef, CandidateQuery, SelectorScope, SurfaceSelector, SurfaceSelectorClause,
};
use crate::driver::{
  ObservedAxTreeSnapshot, ObservedDisplay, ObservedDisplaySnapshot, ObservedOcrRow, ObservedRect,
  ObservedWindow, OcrTextSnapshot, compute_combined_bounds, group_ocr_matches_into_rows,
  parse_observed_ax_tree, parse_ocr_text_snapshot, parse_window_line, report_value,
};
use crate::model::{AuvResult, RunStatus, now_millis};
use crate::skill::{SkillCaseMatrix, SkillStrategy};
use crate::trace::{ArtifactRecordV1Alpha1, RunId};

use super::infra::first_non_empty_string;
use super::recipe::recipe_app_slug;
use super::{
  APP_ANALYSIS_VERSION, AppAnalysis, AppAvailableSurfaces, AppCandidateCompatibility,
  AppCandidateGroundingTaxonomy, AppControlAssessment, AppDistilledCandidateShape,
  AppDisturbanceProfile, AppGroundingAssessment, AppIdentity, AppPermissionState, AppPoint,
  AppProbe, AppProbeStep, AppRecommendedStrategy, AppRect, AppSurfaceCandidate,
  AppVerificationAssessment, AppVerificationMode, AppWindowContext, AssessmentStatus,
};

#[derive(Clone, Debug)]
struct CoordinateReadinessReport {
  ready_for_logical_input: bool,
  reason: String,
}

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
    known_boundaries.push(format!(
      "Permission probe data was incomplete: {error}. Treat permission-dependent conclusions as provisional."
    ));
    default_permission_state()
  });
  let display_snapshot = parse_display_step(probe).unwrap_or_else(|error| {
    known_boundaries.push(format!(
      "Display probe data was incomplete: {error}. Display-relative projection remains provisional."
    ));
    default_display_snapshot()
  });
  let coordinate_readiness = parse_coordinate_readiness(probe).unwrap_or_else(|error| {
    known_boundaries.push(format!(
      "Coordinate-readiness probe data was incomplete: {error}. Logical-input alignment remains provisional."
    ));
    default_coordinate_readiness(error)
  });
  let window_snapshot = parse_window_snapshot(probe).unwrap_or_else(|error| {
    known_boundaries.push(format!(
      "Window observation data was incomplete: {error}. Window-targeted control should be treated as candidate-only."
    ));
    default_window_snapshot()
  });
  let ax_snapshot = parse_ax_snapshot(probe).unwrap_or_else(|error| {
    known_boundaries.push(format!(
      "AX snapshot was unavailable or partial: {error}. AX-first strategies remain candidate-only."
    ));
    default_ax_snapshot(&probe.app)
  });
  let ocr_snapshot = parse_ocr_snapshot(probe).unwrap_or_else(|error| {
    known_boundaries.push(format!(
      "OCR sample was unavailable or partial: {error}. OCR-anchor strategies remain candidate-only."
    ));
    default_ocr_snapshot(&probe.app)
  });

  let primary_window = choose_primary_window(&window_snapshot.windows);
  let primary_window_bounds = primary_window
    .map(|window| AppRect::from_observed(&window.bounds))
    .or_else(|| primary_window_bounds_from_ax_snapshot(&ax_snapshot));
  let primary_window_display_scale = primary_window_bounds
    .as_ref()
    .and_then(|bounds| display_scale_for_rect_center(&display_snapshot, bounds));

  let text_input_count = count_text_inputs(&ax_snapshot);
  let button_like_count = count_button_like_nodes(&ax_snapshot);
  let text_bearing_count = count_text_bearing_nodes(&ax_snapshot);
  let ax_quality = classify_ax_quality(
    ax_snapshot.nodes.len(),
    text_input_count,
    button_like_count,
    text_bearing_count,
  );
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
  let pointer_fallback_surface = if ax_quality == AssessmentStatus::Partial
    || ax_quality == AssessmentStatus::Unknown
    || ocr_snapshot.matches.is_empty()
  {
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
  if let Some(first_text_node) = first_text_bearing_node(&ax_snapshot) {
    stable_anchor_candidates.push(format!(
      "axText: {}",
      summarize_ax_node_text(first_text_node)
    ));
  }
  let mut stable_region_candidates = Vec::new();
  if let Some(bounds) = primary_window_bounds.as_ref() {
    stable_region_candidates.push(format!("primaryWindow={}", bounds.render_compact()));
  }
  stable_region_candidates.push("fullWindowCapture".to_string());

  let grounding_assessment = AppGroundingAssessment {
    ocr_sample_query: ocr_snapshot.query.clone(),
    ocr_sample_status,
    ocr_sample_match_count: ocr_snapshot.matches.len(),
    stable_anchor_candidates,
    stable_region_candidates,
    overlay_debug_artifacts_recommended: ocr_snapshot.matches.is_empty()
      || ax_quality != AssessmentStatus::Available,
  };
  let evidence_refs_by_step = build_probe_evidence_refs(probe);
  let annotation_candidates = build_annotation_candidates(
    &probe.app,
    primary_window,
    primary_window_bounds.as_ref(),
    &ax_snapshot,
    &ocr_snapshot,
    has_collection_like_surface(&ax_snapshot),
    &evidence_refs_by_step,
  );

  let mut control_notes = Vec::new();
  control_notes.push(format!(
    "coordinateReadiness={} ({})",
    if coordinate_readiness.ready_for_logical_input {
      "ready"
    } else {
      "not-ready"
    },
    coordinate_readiness.reason
  ));
  if keyboard_first_surface == AssessmentStatus::Candidate {
    control_notes.push("AX snapshot exposed at least one text-input-like node; keyboard-first entry is a plausible candidate but still unvalidated.".to_string());
  }
  if pointer_fallback_surface == AssessmentStatus::Likely {
    control_notes.push("Semantic controls remain partially opaque in the current surface snapshot; pointer fallback is likely for at least one interaction layer.".to_string());
  }
  let control_assessment = AppControlAssessment {
    preferred_path: if keyboard_first_surface == AssessmentStatus::Candidate {
      "non-pointer path first; escalate to pointer fallback only for opaque semantic targets"
        .to_string()
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
  let ax_verify = if text_bearing_count > 0 {
    verification_notes.push(
      "AX tree contains text-bearing nodes; verifyAxText is a viable candidate contract."
        .to_string(),
    );
    AssessmentStatus::Candidate
  } else {
    AssessmentStatus::Unavailable
  };
  let image_verify = if ocr_snapshot.matches.is_empty() {
    verification_notes.push("Sample OCR over the captured screenshot returned zero filtered matches; image-text verification is possible but currently weak for this sample.".to_string());
    AssessmentStatus::Partial
  } else {
    verification_notes.push("Sample OCR over the captured screenshot returned filtered matches; image-text verification is a candidate surface.".to_string());
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
  if !coordinate_readiness.ready_for_logical_input {
    known_boundaries.push(format!(
      "Coordinate readiness is not yet aligned for logical input: {}",
      coordinate_readiness.reason
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

  let mut recommended_strategies = Vec::new();
  if text_input_count > 0 {
    recommended_strategies.push(recommended_strategy(
      "search-entry",
      "ax-text-input",
      "clipboard-submit",
      "captureEvidence",
      AssessmentStatus::Candidate,
      "The sampled AX surface exposed text-input-like nodes, so a keyboard/clipboard search-entry path is worth validating before escalating to pointer control.",
    )?);
  }
  if text_bearing_count > 0 {
    recommended_strategies.push(recommended_strategy(
      "native-text",
      "ax-text",
      "pointer-focus-clipboard-paste",
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
  if primary_window_bounds.is_some()
    && pointer_fallback_surface == AssessmentStatus::Likely
    && recommended_strategies.is_empty()
  {
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
      let failure = step
        .failure_message
        .clone()
        .unwrap_or_else(|| step.output_summary.clone());
      format!(
        "Probe step `{}` (`{}`) did not complete successfully: {}",
        step.id, step.command_id, failure
      )
    })
    .collect()
}

pub(crate) fn apply_distilled_candidate_shape_inputs(
  candidate_shape: &AppDistilledCandidateShape,
  matrix: &mut SkillCaseMatrix,
  resolved_inputs: &mut BTreeMap<String, String>,
) {
  for case in &mut matrix.cases {
    for (key, value) in &mut case.inputs {
      if !looks_like_placeholder(value) {
        resolved_inputs
          .entry(key.clone())
          .or_insert_with(|| value.clone());
        continue;
      }
      if let Some(shape_value) = candidate_shape.provided_inputs.get(key)
        && !shape_value.trim().is_empty()
      {
        *value = shape_value.clone();
        resolved_inputs
          .entry(key.clone())
          .or_insert_with(|| shape_value.clone());
      }
    }
  }
}

pub(crate) fn apply_candidate_grounding(
  analysis: &AppAnalysis,
  ax_snapshot: Option<&ObservedAxTreeSnapshot>,
  taxonomy_id: &str,
  matrix: &mut SkillCaseMatrix,
  resolved_inputs: &mut BTreeMap<String, String>,
) -> AuvResult<(Vec<String>, Vec<String>)> {
  let taxonomy = AppCandidateGroundingTaxonomy::parse(taxonomy_id)?;
  let mut unresolved = BTreeSet::new();
  let mut used_annotation_ids = BTreeSet::new();
  let search_entry_annotation = find_annotation_candidate(analysis, "search-entry", "focus-query");
  let native_text_annotation = find_annotation_candidate(analysis, "native-text", "focus-query");
  let result_selection_annotation =
    find_annotation_candidate(analysis, "result-selection", "anchor-text");
  let window_primary_region_annotation = analysis
    .annotation_candidates
    .iter()
    .find(|candidate| candidate.candidate_id == "window-primary-region");

  for case in &mut matrix.cases {
    for (key, value) in &mut case.inputs {
      if !looks_like_placeholder(value) {
        resolved_inputs
          .entry(key.clone())
          .or_insert_with(|| value.clone());
        continue;
      }

      let replacement = match (taxonomy, key.as_str()) {
        (
          AppCandidateGroundingTaxonomy::SearchEntryAxTextInputClipboardSubmitCaptureEvidence,
          "focus_query",
        ) => {
          if let Some(candidate) = search_entry_annotation {
            used_annotation_ids.insert(candidate.candidate_id.clone());
            Some(candidate.query_value.clone())
          } else {
            choose_search_entry_query(ax_snapshot)
          }
        }
        (
          AppCandidateGroundingTaxonomy::SearchEntryAxTextInputClipboardSubmitCaptureEvidence,
          "query",
        ) => Some(format!(
          "AUV_{}",
          recipe_app_slug(&analysis.app_identity).to_ascii_uppercase()
        )),
        (
          AppCandidateGroundingTaxonomy::NativeTextAxTextPointerFocusClipboardPasteVerifyAxText,
          "focus_query",
        ) => {
          if let Some(candidate) = native_text_annotation {
            used_annotation_ids.insert(candidate.candidate_id.clone());
            Some(candidate.query_value.clone())
          } else {
            choose_native_text_focus_query(ax_snapshot)
          }
        }
        (
          AppCandidateGroundingTaxonomy::ResultSelectionOcrAnchorPointerClickCaptureEvidence,
          "anchor_text",
        ) => {
          if let Some(candidate) = result_selection_annotation {
            used_annotation_ids.insert(candidate.candidate_id.clone());
            Some(candidate.query_value.clone())
          } else {
            first_stable_anchor_value(&analysis.grounding_assessment.stable_anchor_candidates)
          }
        }
        (
          AppCandidateGroundingTaxonomy::WindowActionWindowPointPointerClickCaptureEvidence,
          "relative_x",
        ) => {
          if let Some(candidate) = window_primary_region_annotation {
            candidate.input_bindings.get("relative_x").map(|value| {
              used_annotation_ids.insert(candidate.candidate_id.clone());
              value.clone()
            })
          } else {
            None
          }
        }
        (
          AppCandidateGroundingTaxonomy::WindowActionWindowPointPointerClickCaptureEvidence,
          "relative_y",
        ) => {
          if let Some(candidate) = window_primary_region_annotation {
            candidate.input_bindings.get("relative_y").map(|value| {
              used_annotation_ids.insert(candidate.candidate_id.clone());
              value.clone()
            })
          } else {
            None
          }
        }
        _ => None,
      };

      if let Some(replacement) = replacement.filter(|value| !value.trim().is_empty()) {
        *value = replacement.clone();
        resolved_inputs.entry(key.clone()).or_insert(replacement);
      } else {
        unresolved.insert(key.clone());
      }
    }
  }

  Ok((
    unresolved.into_iter().collect(),
    used_annotation_ids.into_iter().collect(),
  ))
}

pub(crate) fn build_annotation_candidates(
  app: &AppIdentity,
  primary_window: Option<&ObservedWindow>,
  primary_window_bounds: Option<&AppRect>,
  ax_snapshot: &ObservedAxTreeSnapshot,
  ocr_snapshot: &OcrTextSnapshot,
  has_collection_surface: bool,
  evidence_refs_by_step: &BTreeMap<String, Vec<ArtifactRef>>,
) -> Vec<AppSurfaceCandidate> {
  let mut candidates = Vec::new();

  if let Some(bounds) = primary_window_bounds.cloned() {
    let compact_bounds = bounds.render_compact();
    let click_point = bounds.center_point();
    let input_bindings = window_region_input_bindings(&compact_bounds, &click_point, &bounds);
    let evidence_step_id = if primary_window.is_some() {
      "list-windows"
    } else {
      "capture-ax-tree"
    };
    let note = if primary_window.is_some() {
      "Primary visible window bounds from the window snapshot."
    } else {
      "Primary window bounds inferred from the AX root window because the window snapshot did not expose a visible window."
    };
    candidates.push(AppSurfaceCandidate {
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
      evidence_refs: evidence_refs_for_step(evidence_refs_by_step, evidence_step_id),
      input_bindings,
      compatibility: candidate_compatibility(
        &["window-action.window-point.pointer-click.capture-evidence"],
        &[],
      ),
      notes: vec![note.to_string()],
    });
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
      candidate_compatibility(
        &["search-entry.ax-text-input.clipboard-submit.capture-evidence"],
        &[],
      ),
      "AX-exposed search-entry or search-like input candidate.",
      evidence_refs_for_step(evidence_refs_by_step, "capture-ax-tree"),
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
      candidate_compatibility(
        &["native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text"],
        &[],
      ),
      "AX-exposed editable text-surface candidate.",
      evidence_refs_for_step(evidence_refs_by_step, "capture-ax-tree"),
    ));
  }

  for matched in ocr_snapshot.matches.iter().take(8) {
    let bounds = AppRect::from_observed(&matched.bounds);
    let area = "ocr-visible-text";
    let mut notes =
      vec!["Visible OCR text candidate from the sampled screenshot artifact.".to_string()];
    if matched.text == ocr_snapshot.query {
      notes.push("This match equals the sampled OCR query.".to_string());
    }
    if has_collection_surface && ocr_snapshot.matches.len() >= 2 {
      notes.push(
        "Collection-like surface was present, but OCR text alone is title-level evidence until grouped rows or semantic result evidence corroborate it."
          .to_string(),
      );
    }
    candidates.push(AppSurfaceCandidate {
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
      click_point: Some(bounds.center_point()),
      bounds: Some(bounds),
      confidence: Some(matched.confidence),
      evidence_step_id: "ocr-sample".to_string(),
      candidate_query: Some(ocr_candidate_query(
        &format!("ocr-anchor-{}", matched.match_index),
        &matched.text,
        Some(matched.confidence),
      )),
      evidence_refs: evidence_refs_for_step(evidence_refs_by_step, "ocr-sample"),
      input_bindings: BTreeMap::from([("anchor_text".to_string(), matched.text.clone())]),
      compatibility: AppCandidateCompatibility::default(),
      notes,
    });
  }

  if has_collection_surface && ocr_snapshot.matches.len() >= 2 {
    let rows = group_ocr_rows_from_ocr_snapshot(ocr_snapshot);
    for row in rows.into_iter().take(8) {
      candidates.push(row_candidate(
        row,
        evidence_refs_for_step(evidence_refs_by_step, "ocr-sample"),
      ));
    }
  }

  candidates
}

fn window_region_input_bindings(
  compact_bounds: &str,
  click_point: &AppPoint,
  bounds: &AppRect,
) -> BTreeMap<String, String> {
  let mut bindings = BTreeMap::from([("window_bounds".to_string(), compact_bounds.to_string())]);
  if let Some((relative_x, relative_y)) = bounds.relative_point(click_point) {
    bindings.insert("relative_x".to_string(), format!("{relative_x:.6}"));
    bindings.insert("relative_y".to_string(), format!("{relative_y:.6}"));
  }
  bindings
}

fn ax_focus_candidate(
  candidate_id: &str,
  area: &str,
  kind: &str,
  node: &crate::driver::ObservedAxNode,
  query_value: String,
  evidence_step_id: &str,
  compatibility: AppCandidateCompatibility,
  note: &str,
  evidence_refs: Vec<ArtifactRef>,
) -> AppSurfaceCandidate {
  let bounds = AppRect::from_observed(&node.bounds);
  let focus_query = query_value.clone();
  AppSurfaceCandidate {
    candidate_id: candidate_id.to_string(),
    area: area.to_string(),
    kind: kind.to_string(),
    source: "ax".to_string(),
    status: AssessmentStatus::Candidate,
    primary_text: summarize_ax_node_text(node),
    secondary_text: format!("role={} path={}", node.role, node.path),
    query_value: query_value.clone(),
    coordinate_space: "global-logical".to_string(),
    click_point: Some(bounds.center_point()),
    bounds: Some(bounds),
    confidence: None,
    evidence_step_id: evidence_step_id.to_string(),
    candidate_query: Some(ax_candidate_query(candidate_id, node, &query_value, kind)),
    evidence_refs,
    input_bindings: BTreeMap::from([("focus_query".to_string(), focus_query)]),
    compatibility,
    notes: vec![note.to_string()],
  }
}

fn group_ocr_rows_from_ocr_snapshot(snapshot: &OcrTextSnapshot) -> Vec<ObservedOcrRow> {
  let matches = snapshot.matches.iter().collect::<Vec<_>>();
  group_ocr_matches_into_rows(&matches)
}

fn row_candidate(row: ObservedOcrRow, evidence_refs: Vec<ArtifactRef>) -> AppSurfaceCandidate {
  let bounds = AppRect::from_observed(&row.bounds);
  AppSurfaceCandidate {
    candidate_id: format!("visible-row-{}", row.row_index + 1),
    area: "result-selection".to_string(),
    kind: "row".to_string(),
    source: row.source,
    status: AssessmentStatus::Candidate,
    primary_text: row.text_fragments.join(" | "),
    secondary_text: format!("rowIndex={}", row.row_index + 1),
    query_value: format!("{}", row.row_index + 1),
    coordinate_space: "global-logical".to_string(),
    click_point: Some(bounds.center_point()),
    bounds: Some(bounds),
    confidence: None,
    evidence_step_id: "ocr-sample".to_string(),
    candidate_query: Some(row_candidate_query(
      &format!("visible-row-{}", row.row_index + 1),
      row.row_index + 1,
      &row.text_fragments.join(" "),
    )),
    evidence_refs,
    input_bindings: BTreeMap::from([("row_index".to_string(), format!("{}", row.row_index + 1))]),
    compatibility: candidate_compatibility(
      &[],
      &["result-selection.ocr-anchor.pointer-click.capture-evidence"],
    ),
    notes: vec![
      "Visible row candidate grouped from OCR observations; useful for list-like UI targets."
        .to_string(),
    ],
  }
}

pub(crate) fn candidate_compatibility(
  direct_taxonomy_ids: &[&str],
  context_taxonomy_ids: &[&str],
) -> AppCandidateCompatibility {
  AppCandidateCompatibility {
    direct_taxonomy_ids: direct_taxonomy_ids
      .iter()
      .map(|value| value.to_string())
      .collect(),
    context_taxonomy_ids: context_taxonomy_ids
      .iter()
      .map(|value| value.to_string())
      .collect(),
  }
}

fn evidence_refs_for_step(
  evidence_refs_by_step: &BTreeMap<String, Vec<ArtifactRef>>,
  step_id: &str,
) -> Vec<ArtifactRef> {
  evidence_refs_by_step
    .get(step_id)
    .cloned()
    .unwrap_or_default()
}

pub(crate) fn build_probe_evidence_refs(probe: &AppProbe) -> BTreeMap<String, Vec<ArtifactRef>> {
  let mut refs_by_step = BTreeMap::new();
  for step in &probe.steps {
    let refs = artifact_refs_for_probe_step(probe, step);
    if !refs.is_empty() {
      refs_by_step.insert(step.id.clone(), refs);
    }
  }
  refs_by_step
}

fn artifact_refs_for_probe_step(probe: &AppProbe, step: &AppProbeStep) -> Vec<ArtifactRef> {
  if step.run_id.trim().is_empty() || step.artifact_paths.is_empty() {
    return Vec::new();
  }
  let run_dir = probe
    .project_root
    .join(".auv")
    .join("runs")
    .join(step.run_id.trim());
  let Some(records) = read_artifact_records_jsonl(&run_dir.join("artifacts.jsonl")) else {
    return Vec::new();
  };

  let mut refs = Vec::new();
  for artifact_path in &step.artifact_paths {
    if let Some(record) = records
      .iter()
      .find(|record| artifact_path_matches(&run_dir, &record.path, artifact_path))
      && !refs
        .iter()
        .any(|existing: &ArtifactRef| existing.artifact_id == record.artifact_id)
    {
      refs.push(ArtifactRef {
        run_id: RunId::new(step.run_id.clone()),
        artifact_id: record.artifact_id.clone(),
        span_id: record.span_id.clone(),
        captured_event_id: record.event_id.clone(),
      });
    }
  }
  refs
}

fn read_artifact_records_jsonl(path: &Path) -> Option<Vec<ArtifactRecordV1Alpha1>> {
  let raw = fs::read_to_string(path).ok()?;
  let records = raw
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .filter_map(|line| serde_json::from_str::<ArtifactRecordV1Alpha1>(line).ok())
    .collect::<Vec<_>>();
  Some(records)
}

fn artifact_path_matches(run_dir: &Path, record_path: &str, artifact_path: &Path) -> bool {
  let record_path = Path::new(record_path);
  if let Ok(relative_path) = artifact_path.strip_prefix(run_dir)
    && paths_equal(relative_path, record_path)
  {
    return true;
  }
  artifact_path.ends_with(record_path) || artifact_path.file_name() == record_path.file_name()
}

fn paths_equal(left: &Path, right: &Path) -> bool {
  left.to_string_lossy() == right.to_string_lossy()
}

fn grouped_result_row_count(candidates: &[AppSurfaceCandidate]) -> usize {
  candidates
    .iter()
    .filter(|candidate| candidate.area == "result-selection" && candidate.kind == "row")
    .count()
}

fn ax_candidate_query(
  candidate_id: &str,
  node: &crate::driver::ObservedAxNode,
  label: &str,
  output_kind: &str,
) -> CandidateQuery {
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
    known_limits: vec![
      "Generated from app analyze AX snapshot; validate liveness before action.".to_string(),
    ],
  }
}

fn ocr_candidate_query(
  candidate_id: &str,
  text: &str,
  provider_score: Option<f64>,
) -> CandidateQuery {
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
    known_limits: vec![
      "OCR text from app analyze is visible-text evidence, not semantic result evidence."
        .to_string(),
    ],
  }
}

fn row_candidate_query(
  candidate_id: &str,
  row_index: usize,
  contains_text: &str,
) -> CandidateQuery {
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
    known_limits: vec![
      "Grouped visible row is structural evidence; semantic identity requires later verification."
        .to_string(),
    ],
  }
}

pub(crate) fn build_distilled_candidate_shape(
  analysis: &AppAnalysis,
  taxonomy_id: &str,
) -> AppDistilledCandidateShape {
  let mut direct_candidate_ids = Vec::new();
  let mut context_candidate_ids = Vec::new();
  let mut provided_inputs = BTreeMap::new();
  let mut notes = Vec::new();

  for candidate in &analysis.annotation_candidates {
    if candidate
      .compatibility
      .direct_taxonomy_ids
      .iter()
      .any(|value| value == taxonomy_id)
    {
      direct_candidate_ids.push(candidate.candidate_id.clone());
      for (key, value) in &candidate.input_bindings {
        if !value.trim().is_empty() {
          provided_inputs
            .entry(key.clone())
            .or_insert_with(|| value.clone());
        }
      }
    } else if candidate
      .compatibility
      .context_taxonomy_ids
      .iter()
      .any(|value| value == taxonomy_id)
    {
      context_candidate_ids.push(candidate.candidate_id.clone());
    }
  }

  if direct_candidate_ids.is_empty() {
    notes.push(format!(
      "No direct candidate shape was available for taxonomy {} during distill.",
      taxonomy_id
    ));
  }
  if !context_candidate_ids.is_empty() {
    notes.push(
      "Context-only candidates were recorded for later review, but they did not project directly into recipe inputs."
        .to_string(),
    );
  }

  AppDistilledCandidateShape {
    direct_candidate_ids,
    context_candidate_ids,
    provided_inputs,
    notes,
  }
}

pub(crate) fn verification_mode_for_strategy(
  strategy: &SkillStrategy,
) -> AuvResult<AppVerificationMode> {
  match strategy.verification_contract.trim() {
    "captureEvidence" => Ok(AppVerificationMode::EvidenceOnly),
    "verifyImageText" | "verifyNowPlayingTitle" | "verifyAxText" => {
      Ok(AppVerificationMode::MachineAsserted)
    }
    other => Err(format!(
      "strategy.verificationContract {} is unsupported; expected one of captureEvidence, verifyImageText, verifyNowPlayingTitle, verifyAxText",
      other
    )),
  }
}

pub(crate) fn validated_candidate_rationale(
  selected_case_count: usize,
  verification_mode: AppVerificationMode,
) -> String {
  match verification_mode {
    AppVerificationMode::MachineAsserted => format!(
      "All {} candidate case(s) executed successfully through the shared runtime with machine-asserted verification.",
      selected_case_count
    ),
    AppVerificationMode::EvidenceOnly => format!(
      "All {} candidate case(s) executed successfully through the shared runtime, but the verification contract is evidence-only and still requires human review.",
      selected_case_count
    ),
  }
}

pub(crate) fn suggested_annotation_ids_for_candidate_shape(
  candidate_shape: &AppDistilledCandidateShape,
) -> Vec<String> {
  candidate_shape
    .direct_candidate_ids
    .iter()
    .chain(candidate_shape.context_candidate_ids.iter())
    .take(8)
    .cloned()
    .collect()
}

fn find_annotation_candidate<'a>(
  analysis: &'a AppAnalysis,
  area: &str,
  kind: &str,
) -> Option<&'a AppSurfaceCandidate> {
  analysis.annotation_candidates.iter().find(|candidate| {
    candidate.area == area && candidate.kind == kind && !candidate.query_value.trim().is_empty()
  })
}

fn find_search_entry_node(
  snapshot: &ObservedAxTreeSnapshot,
) -> Option<&crate::driver::ObservedAxNode> {
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

fn find_native_text_focus_node(
  snapshot: &ObservedAxTreeSnapshot,
) -> Option<&crate::driver::ObservedAxNode> {
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

fn choose_search_entry_query(snapshot: Option<&ObservedAxTreeSnapshot>) -> Option<String> {
  let snapshot = snapshot?;
  find_search_entry_node(snapshot).and_then(preferred_ax_query_text)
}

fn choose_native_text_focus_query(snapshot: Option<&ObservedAxTreeSnapshot>) -> Option<String> {
  let snapshot = snapshot?;
  find_native_text_focus_node(snapshot).and_then(preferred_ax_query_text)
}

fn preferred_ax_query_text(node: &crate::driver::ObservedAxNode) -> Option<String> {
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

fn first_stable_anchor_value(candidates: &[String]) -> Option<String> {
  candidates.iter().find_map(|value| {
    let trimmed = value.trim();
    if trimmed.is_empty() {
      return None;
    }
    if let Some((_, suffix)) = trimmed.split_once(':') {
      let suffix = suffix.trim();
      if !suffix.is_empty() {
        return Some(suffix.to_string());
      }
    }
    Some(trimmed.to_string())
  })
}

fn looks_like_placeholder(value: &str) -> bool {
  let trimmed = value.trim();
  trimmed.starts_with("TODO_") || trimmed == "TODO"
}

fn parse_permission_state(probe: &AppProbe) -> AuvResult<AppPermissionState> {
  let report = read_named_text_artifact(probe, "probe-permissions", None)?;
  Ok(AppPermissionState {
    screen_recording: report_value(&report, "screenRecording=")
      .unwrap_or("unknown")
      .to_string(),
    accessibility: report_value(&report, "accessibility=")
      .unwrap_or("unknown")
      .to_string(),
    automation_to_system_events: report_value(&report, "automationToSystemEvents=")
      .unwrap_or("unknown")
      .to_string(),
    launch_host_process: report_value(&report, "launchHostProcess=")
      .unwrap_or("unknown")
      .to_string(),
  })
}

fn parse_display_step(probe: &AppProbe) -> AuvResult<ObservedDisplaySnapshot> {
  let raw = read_named_artifact(probe, "list-displays", Some("display-list"), "json")?;
  let displays_json: Vec<Value> = serde_json::from_str(&raw)
    .map_err(|error| format!("failed to parse list-displays JSON artifact: {error}"))?;
  let displays = displays_json
    .iter()
    .map(parse_display_descriptor_value)
    .collect::<AuvResult<Vec<_>>>()?;
  if displays.is_empty() {
    return Err("list-displays artifact did not contain any displays".to_string());
  }
  Ok(ObservedDisplaySnapshot {
    combined_bounds: compute_combined_bounds(&displays),
    displays,
    captured_at: "".to_string(),
  })
}

fn parse_coordinate_readiness(probe: &AppProbe) -> AuvResult<CoordinateReadinessReport> {
  let report = read_named_text_artifact(
    probe,
    "probe-coordinate-readiness",
    Some("coordinate-readiness-report"),
  )?;
  Ok(CoordinateReadinessReport {
    ready_for_logical_input: report_value(&report, "readyForLogicalInput=")
      .unwrap_or("false")
      .trim()
      == "true",
    reason: report_value(&report, "reason=")
      .unwrap_or("unknown")
      .to_string(),
  })
}

fn parse_window_snapshot(probe: &AppProbe) -> AuvResult<WindowSnapshotAnalysis> {
  let report = read_named_text_artifact(probe, "list-windows", Some("window-list"))?;
  let windows = report
    .lines()
    .filter(|line| line.starts_with("window\t"))
    .map(parse_window_line)
    .collect::<AuvResult<Vec<_>>>()?;
  Ok(WindowSnapshotAnalysis {
    observed_at: report_value(&report, "observedAt=")
      .unwrap_or("")
      .to_string(),
    frontmost_app_name: report_value(&report, "frontmostAppName=")
      .unwrap_or("")
      .to_string(),
    frontmost_window_title: report_value(&report, "frontmostWindowTitle=")
      .unwrap_or("")
      .to_string(),
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

fn default_coordinate_readiness(reason: String) -> CoordinateReadinessReport {
  CoordinateReadinessReport {
    ready_for_logical_input: false,
    reason,
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
    window_report
      .as_deref()
      .and_then(|report| report_value(report, "frontmostWindowTitle="))
      .map(str::to_string),
    window_report
      .as_deref()
      .and_then(|report| report_value(report, "frontmostAppName="))
      .map(str::to_string),
    ax_report
      .as_deref()
      .and_then(|report| report_value(report, "windowTitle="))
      .map(str::to_string),
    ax_report
      .as_deref()
      .and_then(|report| report_value(report, "appName="))
      .map(str::to_string),
    non_empty_trimmed(&app.app_name),
    non_empty_trimmed(&app.bundle_id),
  ])
  .unwrap_or_else(|| app.bundle_id.clone())
}

fn read_probe_step_artifact_text(
  steps: &[AppProbeStep],
  step_id: &str,
  file_name_hint: Option<&str>,
) -> Option<String> {
  let step = steps.iter().find(|step| step.id == step_id)?;
  let artifact_path = step.artifact_paths.iter().find(|path| {
    path
      .extension()
      .and_then(|value| value.to_str())
      .is_some_and(|value| value.eq_ignore_ascii_case("txt"))
      && file_name_hint.is_none_or(|hint| {
        path
          .file_name()
          .and_then(|value| value.to_str())
          .is_some_and(|name| name.contains(hint))
      })
  })?;
  fs::read_to_string(artifact_path).ok()
}

fn read_named_text_artifact(
  probe: &AppProbe,
  step_id: &str,
  file_name_hint: Option<&str>,
) -> AuvResult<String> {
  read_named_artifact(probe, step_id, file_name_hint, "txt")
}

fn read_named_artifact(
  probe: &AppProbe,
  step_id: &str,
  file_name_hint: Option<&str>,
  extension: &str,
) -> AuvResult<String> {
  let step = probe
    .steps
    .iter()
    .find(|step| step.id == step_id)
    .ok_or_else(|| format!("probe is missing required step {}", step_id))?;
  let artifact_path = step
    .artifact_paths
    .iter()
    .find(|path| {
      path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case(extension))
        && file_name_hint.is_none_or(|hint| {
          path
            .file_name()
            .and_then(|value| value.to_str())
            .is_some_and(|name| name.contains(hint))
        })
    })
    .cloned()
    .ok_or_else(|| {
      format!(
        "probe step {} did not produce the expected .{} artifact",
        step_id, extension
      )
    })?;
  fs::read_to_string(&artifact_path).map_err(|error| {
    format!(
      "failed to read probe artifact {}: {error}",
      artifact_path.display()
    )
  })
}

fn parse_display_descriptor_value(value: &Value) -> AuvResult<ObservedDisplay> {
  let display_ref = value
    .get("display_ref")
    .and_then(Value::as_str)
    .unwrap_or("display");
  let native_display_id = value
    .get("native_display_id")
    .and_then(Value::as_str)
    .unwrap_or("0")
    .parse::<u32>()
    .map_err(|error| {
      format!("invalid native_display_id for {display_ref} in display-list artifact: {error}")
    })?;
  let bounds = parse_json_rect(value, "global_logical_bounds", display_ref)?;
  let visible_bounds = parse_json_rect(value, "visible_logical_bounds", display_ref)?;
  let physical_pixel_size = value
    .get("physical_pixel_size")
    .ok_or_else(|| format!("display {display_ref} is missing physical_pixel_size"))?;
  Ok(ObservedDisplay {
    display_id: native_display_id,
    is_main: value
      .get("is_main")
      .and_then(Value::as_bool)
      .unwrap_or(false),
    is_built_in: value
      .get("is_builtin")
      .and_then(Value::as_bool)
      .unwrap_or(false),
    bounds,
    visible_bounds,
    scale_factor: value
      .get("scale_factor")
      .and_then(Value::as_f64)
      .unwrap_or(1.0),
    pixel_width: json_number_to_i64(physical_pixel_size, "width", display_ref)?,
    pixel_height: json_number_to_i64(physical_pixel_size, "height", display_ref)?,
  })
}

fn parse_json_rect(value: &Value, field: &str, display_ref: &str) -> AuvResult<ObservedRect> {
  let rect = value
    .get(field)
    .ok_or_else(|| format!("display {display_ref} is missing {field}"))?;
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

fn primary_window_bounds_from_ax_snapshot(snapshot: &ObservedAxTreeSnapshot) -> Option<AppRect> {
  snapshot
    .nodes
    .iter()
    .find(|node| {
      (node.role == "AXWindow"
        || node.subrole == "AXStandardWindow"
        || node.subrole.ends_with("Window"))
        && node.bounds.width > 0
        && node.bounds.height > 0
    })
    .map(|node| AppRect::from_observed(&node.bounds))
}

fn display_scale_for_rect_center(
  snapshot: &ObservedDisplaySnapshot,
  rect: &AppRect,
) -> Option<f64> {
  let (x, y) = rect.center();
  snapshot
    .displays
    .iter()
    .find(|display| contains_point(&display.bounds, x, y))
    .map(|display| display.scale_factor)
}

fn contains_point(rect: &ObservedRect, x: i64, y: i64) -> bool {
  x >= rect.x && y >= rect.y && x < rect.x + rect.width && y < rect.y + rect.height
}

fn classify_ax_quality(
  node_count: usize,
  text_input_count: usize,
  button_like_count: usize,
  text_bearing_count: usize,
) -> AssessmentStatus {
  if node_count == 0 {
    AssessmentStatus::Unavailable
  } else if node_count >= 20 && (text_input_count + button_like_count + text_bearing_count) >= 8 {
    AssessmentStatus::Available
  } else if node_count >= 6
    && (text_input_count > 0 || button_like_count > 0 || text_bearing_count > 0)
  {
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
  snapshot
    .nodes
    .iter()
    .filter(|node| node.role == "AXButton" || node.subrole == "AXButton" || node.role == "AXLink")
    .count()
}

fn count_text_bearing_nodes(snapshot: &ObservedAxTreeSnapshot) -> usize {
  snapshot
    .nodes
    .iter()
    .filter(|node| !summarize_ax_node_text(node).is_empty())
    .count()
}

fn first_text_bearing_node(
  snapshot: &ObservedAxTreeSnapshot,
) -> Option<&crate::driver::ObservedAxNode> {
  snapshot
    .nodes
    .iter()
    .find(|node| !summarize_ax_node_text(node).is_empty())
}

fn summarize_ax_node_text(node: &crate::driver::ObservedAxNode) -> String {
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
  snapshot
    .nodes
    .iter()
    .any(|node| node.role.contains("Menu") || node.subrole.contains("Menu"))
}

fn has_collection_like_surface(snapshot: &ObservedAxTreeSnapshot) -> bool {
  snapshot
    .nodes
    .iter()
    .filter(|node| {
      matches!(
        node.role.as_str(),
        "AXRow" | "AXCell" | "AXTable" | "AXOutline" | "AXList" | "AXBrowser"
      ) || matches!(
        node.subrole.as_str(),
        "AXRow" | "AXCell" | "AXTable" | "AXOutline" | "AXList"
      )
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
  let strategy = SkillStrategy {
    family: family.to_string(),
    grounding: grounding.to_string(),
    activation: activation.to_string(),
    verification_contract: verification_contract.to_string(),
  };
  Ok(AppRecommendedStrategy {
    taxonomy_id: strategy.taxonomy_id()?,
    status,
    rationale: rationale.to_string(),
  })
}
