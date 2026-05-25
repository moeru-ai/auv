use std::collections::BTreeMap;

use super::{AppAnalysis, AppDistillation, AppValidation};

pub(crate) fn render_app_analysis_report(analysis: &AppAnalysis) -> String {
  let mut lines = vec![
    format!(
      "# App Analyze Report: {}",
      if analysis.app_identity.app_name.is_empty() {
        &analysis.app_identity.bundle_id
      } else {
        &analysis.app_identity.app_name
      }
    ),
    String::new(),
    format!("- bundle id: `{}`", analysis.app_identity.bundle_id),
    format!("- analysis version: `{}`", analysis.analysis_version),
    format!("- probe path: `{}`", analysis.probe_path.display()),
    String::new(),
    "## 1. App Basic Information".to_string(),
    String::new(),
    format!("- app name: `{}`", analysis.app_identity.app_name),
    format!(
      "- app path: `{}`",
      analysis
        .app_identity
        .app_path
        .as_ref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "unknown".to_string())
    ),
    format!(
      "- version: `{}` (`build {}`)",
      analysis.app_identity.version, analysis.app_identity.build_version
    ),
    format!(
      "- launch-services resolved: `{}`",
      if analysis.app_identity.launch_services_resolved {
        "true"
      } else {
        "false"
      }
    ),
  ];
  if let Some(executable) = analysis.app_identity.main_executable_path.as_ref() {
    lines.push(format!("- main executable: `{}`", executable.display()));
  }
  lines.push(format!(
    "- current window count: `{}`",
    analysis.window_context.observed_window_count
  ));
  if let Some(bounds) = analysis.window_context.primary_window_bounds.as_ref() {
    lines.push(format!(
      "- primary window bounds: `{}`",
      bounds.render_compact()
    ));
  }
  if let Some(scale) = analysis.window_context.primary_window_display_scale {
    lines.push(format!("- primary window display scale: `{scale:.3}`"));
  }
  lines.push(format!(
    "- permission status: screenRecording=`{}`, accessibility=`{}`, automationToSystemEvents=`{}`",
    analysis.permissions.screen_recording,
    analysis.permissions.accessibility,
    analysis.permissions.automation_to_system_events
  ));
  lines.push(String::new());
  lines.push("## 2. Available Surfaces".to_string());
  lines.push(String::new());
  lines.push(format!(
    "- Accessibility Tree quality: `{}`",
    analysis.available_surfaces.accessibility_tree.as_str()
  ));
  lines.push(format!(
    "- menu surface: `{}`",
    analysis.available_surfaces.menu_surface.as_str()
  ));
  lines.push(format!(
    "- shortcut surface: `{}`",
    analysis.available_surfaces.shortcut_surface.as_str()
  ));
  lines.push(format!(
    "- AppleScript surface: `{}`",
    analysis.available_surfaces.apple_script_surface.as_str()
  ));
  lines.push(format!(
    "- URL scheme surface: `{}`",
    analysis.available_surfaces.url_scheme_surface.as_str()
  ));
  if !analysis.app_identity.url_schemes.is_empty() {
    lines.push(format!(
      "- url schemes: {}",
      analysis
        .app_identity
        .url_schemes
        .iter()
        .map(|value| format!("`{value}`"))
        .collect::<Vec<_>>()
        .join(", ")
    ));
  }
  lines.push(format!(
    "- keyboard-first surface: `{}`",
    analysis.available_surfaces.keyboard_first_surface.as_str()
  ));
  lines.push(format!(
    "- pointer fallback surface: `{}`",
    analysis
      .available_surfaces
      .pointer_fallback_surface
      .as_str()
  ));
  lines.push(String::new());
  lines.push("## 3. Grounding Assessment".to_string());
  lines.push(String::new());
  lines.push(format!(
    "- OCR sample query `{}`: `{}` (matchCount={})",
    analysis.grounding_assessment.ocr_sample_query,
    analysis.grounding_assessment.ocr_sample_status.as_str(),
    analysis.grounding_assessment.ocr_sample_match_count
  ));
  lines.push(format!(
    "- overlay/debug artifacts recommended: `{}`",
    analysis
      .grounding_assessment
      .overlay_debug_artifacts_recommended
  ));
  if !analysis
    .grounding_assessment
    .stable_anchor_candidates
    .is_empty()
  {
    lines.push("- stable anchor candidates:".to_string());
    for item in &analysis.grounding_assessment.stable_anchor_candidates {
      lines.push(format!("  - {item}"));
    }
  }
  if !analysis
    .grounding_assessment
    .stable_region_candidates
    .is_empty()
  {
    lines.push("- stable region candidates:".to_string());
    for item in &analysis.grounding_assessment.stable_region_candidates {
      lines.push(format!("  - {item}"));
    }
  }
  lines.push(String::new());
  lines.push("## 4. Candidate / Annotation Layer".to_string());
  lines.push(String::new());
  lines.push(format!(
    "- structured candidate count: `{}`",
    analysis.annotation_candidates.len()
  ));
  if analysis.annotation_candidates.is_empty() {
    lines.push("- none synthesized from the current probe artifacts".to_string());
  } else {
    for candidate in &analysis.annotation_candidates {
      lines.push(format!(
        "- `{}`: area=`{}`, kind=`{}`, source=`{}`, status=`{}`",
        candidate.candidate_id,
        candidate.area,
        candidate.kind,
        candidate.source,
        candidate.status.as_str()
      ));
      if !candidate.primary_text.trim().is_empty() {
        lines.push(format!("  - primaryText: {}", candidate.primary_text));
      }
      if !candidate.secondary_text.trim().is_empty() {
        lines.push(format!("  - secondaryText: {}", candidate.secondary_text));
      }
      if !candidate.query_value.trim().is_empty() {
        lines.push(format!("  - queryValue: `{}`", candidate.query_value));
      }
      if !candidate.coordinate_space.trim().is_empty() {
        lines.push(format!(
          "  - coordinateSpace: `{}`",
          candidate.coordinate_space
        ));
      }
      if let Some(bounds) = &candidate.bounds {
        lines.push(format!("  - bounds: `{}`", bounds.render_compact()));
      }
      if let Some(point) = &candidate.click_point {
        lines.push(format!("  - clickPoint: `{}, {}`", point.x, point.y));
      }
      if let Some(confidence) = candidate.confidence {
        lines.push(format!("  - confidence: `{confidence:.3}`"));
      }
      lines.push(format!(
        "  - evidenceStep: `{}`",
        candidate.evidence_step_id
      ));
      if !candidate.input_bindings.is_empty() {
        lines.push("  - inputBindings:".to_string());
        for (key, value) in &candidate.input_bindings {
          lines.push(format!("    - `{key}` = `{value}`"));
        }
      }
      if !candidate.compatibility.direct_taxonomy_ids.is_empty() {
        lines.push("  - directTaxonomyIds:".to_string());
        for taxonomy_id in &candidate.compatibility.direct_taxonomy_ids {
          lines.push(format!("    - `{taxonomy_id}`"));
        }
      }
      if !candidate.compatibility.context_taxonomy_ids.is_empty() {
        lines.push("  - contextTaxonomyIds:".to_string());
        for taxonomy_id in &candidate.compatibility.context_taxonomy_ids {
          lines.push(format!("    - `{taxonomy_id}`"));
        }
      }
      for note in &candidate.notes {
        lines.push(format!("  - note: {note}"));
      }
    }
  }
  lines.push(String::new());
  lines.push("## 5. Control Strategy".to_string());
  lines.push(String::new());
  lines.push(format!(
    "- preferred path: {}",
    analysis.control_assessment.preferred_path
  ));
  lines.push(format!(
    "- non-pointer path: `{}`",
    analysis.control_assessment.non_pointer_path.as_str()
  ));
  lines.push(format!(
    "- keyboard path: `{}`",
    analysis.control_assessment.keyboard_path.as_str()
  ));
  lines.push(format!(
    "- pointer fallback: `{}`",
    analysis.control_assessment.pointer_fallback.as_str()
  ));
  for note in &analysis.control_assessment.notes {
    lines.push(format!("- note: {note}"));
  }
  lines.push(format!(
    "- disturbance profile: observation=`{}`, non-pointer=`{}`, pointer=`{}`",
    analysis.disturbance_profile.observation.join(", "),
    analysis.disturbance_profile.non_pointer_control.join(", "),
    analysis.disturbance_profile.pointer_fallback.join(", ")
  ));
  lines.push(String::new());
  lines.push("## 6. Verification Assessment".to_string());
  lines.push(String::new());
  lines.push(format!(
    "- AX verify: `{}`",
    analysis.verification_assessment.ax_verify.as_str()
  ));
  lines.push(format!(
    "- OCR/image verify: `{}`",
    analysis.verification_assessment.image_verify.as_str()
  ));
  lines.push(format!(
    "- UI-state verify: `{}`",
    analysis.verification_assessment.ui_state_verify.as_str()
  ));
  lines.push(format!(
    "- semantic success: `{}`",
    analysis.verification_assessment.semantic_success.as_str()
  ));
  for note in &analysis.verification_assessment.notes {
    lines.push(format!("- note: {note}"));
  }
  lines.push(String::new());
  lines.push("## 7. Known Boundaries".to_string());
  lines.push(String::new());
  if analysis.known_boundaries.is_empty() {
    lines.push("- none recorded".to_string());
  } else {
    for note in &analysis.known_boundaries {
      lines.push(format!("- {note}"));
    }
  }
  lines.push(String::new());
  lines.push("## 8. Recommended Candidate Strategies".to_string());
  lines.push(String::new());
  if analysis.recommended_strategies.is_empty() {
    lines.push("- none generated".to_string());
  } else {
    for strategy in &analysis.recommended_strategies {
      lines.push(format!(
        "- `{}` (`{}`): {}",
        strategy.taxonomy_id,
        strategy.status.as_str(),
        strategy.rationale
      ));
    }
  }
  lines.join("\n") + "\n"
}

pub(crate) fn render_app_distillation_report(
  analysis: &AppAnalysis,
  distillation: &AppDistillation,
) -> String {
  let mut lines = vec![
    format!(
      "# App Distillation Report: {}",
      if distillation.app_identity.app_name.is_empty() {
        &distillation.app_identity.bundle_id
      } else {
        &distillation.app_identity.app_name
      }
    ),
    String::new(),
    format!(
      "- source analysis: `{}`",
      distillation.source_analysis_path.display()
    ),
    format!("- distill version: `{}`", distillation.distill_version),
    format!(
      "- generated candidate count: `{}`",
      distillation.candidates.len()
    ),
    format!(
      "- available analysis annotations: `{}`",
      analysis.annotation_candidates.len()
    ),
    String::new(),
    "## Candidate Outputs".to_string(),
    String::new(),
  ];
  if distillation.candidates.is_empty() {
    lines.push("- none generated".to_string());
  } else {
    for candidate in &distillation.candidates {
      lines.push(format!(
        "- `{}` (`{}`)",
        candidate.recipe_id, candidate.taxonomy_id
      ));
      lines.push(format!("  - status: `{}`", candidate.status.as_str()));
      lines.push(format!("  - recipe: `{}`", candidate.recipe_path.display()));
      lines.push(format!(
        "  - cases: `{}`",
        candidate.case_matrix_path.display()
      ));
      lines.push(format!("  - rationale: {}", candidate.rationale));
      if !candidate.suggested_annotation_ids.is_empty() {
        lines.push("  - suggested annotations:".to_string());
        for candidate_id in &candidate.suggested_annotation_ids {
          lines.push(format!("    - `{candidate_id}`"));
        }
      }
      if !candidate.candidate_shape.direct_candidate_ids.is_empty() {
        lines.push("  - direct candidate ids:".to_string());
        for candidate_id in &candidate.candidate_shape.direct_candidate_ids {
          lines.push(format!("    - `{candidate_id}`"));
        }
      }
      if !candidate.candidate_shape.context_candidate_ids.is_empty() {
        lines.push("  - context candidate ids:".to_string());
        for candidate_id in &candidate.candidate_shape.context_candidate_ids {
          lines.push(format!("    - `{candidate_id}`"));
        }
      }
      if !candidate.candidate_shape.provided_inputs.is_empty() {
        lines.push("  - candidate shape inputs:".to_string());
        for (key, value) in &candidate.candidate_shape.provided_inputs {
          lines.push(format!("    - `{key}` = `{value}`"));
        }
      }
      for note in &candidate.candidate_shape.notes {
        lines.push(format!("  - shape note: {note}"));
      }
    }
  }
  lines.push(String::new());
  lines.push("## Boundaries Carried Forward".to_string());
  lines.push(String::new());
  if distillation.known_boundaries.is_empty() {
    lines.push("- none recorded".to_string());
  } else {
    for note in &distillation.known_boundaries {
      lines.push(format!("- {note}"));
    }
  }
  lines.push(String::new());
  lines.push("## Analysis Reminder".to_string());
  lines.push(String::new());
  lines.push(format!(
    "- available surfaces: AX=`{}`, keyboard-first=`{}`, pointer-fallback=`{}`",
    analysis.available_surfaces.accessibility_tree.as_str(),
    analysis.available_surfaces.keyboard_first_surface.as_str(),
    analysis
      .available_surfaces
      .pointer_fallback_surface
      .as_str()
  ));
  lines.push(format!(
    "- grounding: OCR sample `{}` produced `{}` with matchCount={}",
    analysis.grounding_assessment.ocr_sample_query,
    analysis.grounding_assessment.ocr_sample_status.as_str(),
    analysis.grounding_assessment.ocr_sample_match_count
  ));
  lines.push(
    "- candidate outputs are scaffolds only; they are not validated skills until a later validate/promote step says so."
      .to_string(),
  );
  lines.join("\n") + "\n"
}

pub(crate) fn render_app_validation_report(validation: &AppValidation) -> String {
  let mut lines = vec![
    format!(
      "# App Validation Report: {}",
      if validation.app_identity.app_name.is_empty() {
        &validation.app_identity.bundle_id
      } else {
        &validation.app_identity.app_name
      }
    ),
    String::new(),
    format!(
      "- source distillation: `{}`",
      validation.source_distillation_path.display()
    ),
    format!(
      "- source analysis: `{}`",
      validation.source_analysis_path.display()
    ),
    format!("- validate version: `{}`", validation.validate_version),
    String::new(),
  ];
  let mut by_status = BTreeMap::<&str, usize>::new();
  let mut by_verification_mode = BTreeMap::<&str, usize>::new();
  for candidate in &validation.candidates {
    *by_status.entry(candidate.status.as_str()).or_insert(0) += 1;
    *by_verification_mode
      .entry(candidate.verification_mode.as_str())
      .or_insert(0) += 1;
  }
  lines.push("## Status Counts".to_string());
  lines.push(String::new());
  if by_status.is_empty() {
    lines.push("- none recorded".to_string());
  } else {
    for (status, count) in by_status {
      lines.push(format!("- `{status}`: `{count}`"));
    }
  }
  lines.push(String::new());
  lines.push("## Verification Semantics".to_string());
  lines.push(String::new());
  lines.push(
    "- `validated` means the selected live cases executed successfully through the shared runtime."
      .to_string(),
  );
  lines.push(
    "- `machine-asserted` means the verification contract includes a machine-readable assertion step such as AX or OCR verification."
      .to_string(),
  );
  lines.push(
    "- `evidence-only` means the recipe captured evidence but did not machine-assert the user-visible outcome; human review is still required."
      .to_string(),
  );
  if !by_verification_mode.is_empty() {
    lines.push(String::new());
    for (mode, count) in by_verification_mode {
      lines.push(format!("- `{mode}`: `{count}`"));
    }
  }
  lines.push(String::new());
  lines.push("## Candidate Results".to_string());
  lines.push(String::new());
  if validation.candidates.is_empty() {
    lines.push("- none validated".to_string());
  } else {
    for candidate in &validation.candidates {
      lines.push(format!(
        "### {} [{}]",
        candidate.recipe_id,
        candidate.status.as_str()
      ));
      lines.push(String::new());
      lines.push(format!("- taxonomy: `{}`", candidate.taxonomy_id));
      lines.push(format!(
        "- selected cases: `{}`",
        candidate.selected_case_count
      ));
      lines.push(format!("- recipe: `{}`", candidate.recipe_path.display()));
      lines.push(format!(
        "- cases: `{}`",
        candidate.case_matrix_path.display()
      ));
      lines.push(format!(
        "- verification mode: `{}`",
        candidate.verification_mode.as_str()
      ));
      lines.push(format!(
        "- manual review required: `{}`",
        if candidate.verification_mode.manual_review_required() {
          "yes"
        } else {
          "no"
        }
      ));
      lines.push(format!("- rationale: {}", candidate.rationale));
      if !candidate.used_annotation_ids.is_empty() {
        lines.push("- used annotations:".to_string());
        for candidate_id in &candidate.used_annotation_ids {
          lines.push(format!("  - `{candidate_id}`"));
        }
      }
      if !candidate.resolved_inputs.is_empty() {
        lines.push("- resolved inputs:".to_string());
        for (key, value) in &candidate.resolved_inputs {
          lines.push(format!("  - `{key}` = `{value}`"));
        }
      }
      if !candidate.unresolved_inputs.is_empty() {
        lines.push("- unresolved inputs:".to_string());
        for key in &candidate.unresolved_inputs {
          lines.push(format!("  - `{key}`"));
        }
      }
      if let Some(error) = &candidate.failure_message {
        lines.push("- failure:".to_string());
        lines.push(format!("  - {}", error.replace('\n', "\n  - ")));
      }
      lines.push(String::new());
    }
  }
  lines.push("## Boundaries Carried Forward".to_string());
  lines.push(String::new());
  if validation.known_boundaries.is_empty() {
    lines.push("- none recorded".to_string());
  } else {
    for note in &validation.known_boundaries {
      lines.push(format!("- {note}"));
    }
  }
  lines.join("\n") + "\n"
}
