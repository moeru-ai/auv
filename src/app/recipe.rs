use serde_json::{Value, json};

use crate::model::AuvResult;

use super::{
  AppAnalysis, AppCandidateGroundingTaxonomy, AppDistilledCandidateShape, AppIdentity,
  AppRecommendedStrategy,
};

pub(crate) fn recipe_app_slug(app: &AppIdentity) -> String {
  let source = if app.app_name.trim().is_empty() {
    &app.bundle_id
  } else {
    &app.app_name
  };
  let mut slug = String::new();
  let mut last_was_sep = false;
  for character in source.chars() {
    let lower = character.to_ascii_lowercase();
    if lower.is_ascii_alphanumeric() {
      slug.push(lower);
      last_was_sep = false;
    } else if !last_was_sep {
      slug.push('_');
      last_was_sep = true;
    }
  }
  slug.trim_matches('_').to_string()
}

pub(crate) fn candidate_slug(taxonomy_id: &str) -> String {
  taxonomy_id
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() {
        character.to_ascii_lowercase()
      } else {
        '_'
      }
    })
    .collect::<String>()
    .trim_matches('_')
    .to_string()
}

pub(crate) fn render_candidate_recipe(
  analysis: &AppAnalysis,
  strategy: &AppRecommendedStrategy,
  _candidate_shape: &AppDistilledCandidateShape,
) -> AuvResult<Value> {
  match AppCandidateGroundingTaxonomy::parse(&strategy.taxonomy_id)? {
    AppCandidateGroundingTaxonomy::SearchEntryAxTextInputClipboardSubmitCaptureEvidence => {
      Ok(render_search_entry_candidate_recipe(analysis))
    }
    AppCandidateGroundingTaxonomy::NativeTextAxTextPointerFocusClipboardPasteVerifyAxText => {
      Ok(render_native_text_candidate_recipe(analysis))
    }
    AppCandidateGroundingTaxonomy::ResultSelectionOcrAnchorPointerClickCaptureEvidence => {
      Ok(render_result_selection_candidate_recipe(analysis))
    }
    AppCandidateGroundingTaxonomy::WindowActionWindowPointPointerClickCaptureEvidence => {
      Ok(render_window_action_candidate_recipe(analysis))
    }
  }
}

pub(crate) fn render_candidate_case_matrix(
  analysis: &AppAnalysis,
  strategy: &AppRecommendedStrategy,
  candidate_shape: &AppDistilledCandidateShape,
) -> AuvResult<Value> {
  match AppCandidateGroundingTaxonomy::parse(&strategy.taxonomy_id)? {
    AppCandidateGroundingTaxonomy::SearchEntryAxTextInputClipboardSubmitCaptureEvidence => Ok(
      render_search_entry_candidate_cases(analysis, candidate_shape),
    ),
    AppCandidateGroundingTaxonomy::NativeTextAxTextPointerFocusClipboardPasteVerifyAxText => Ok(
      render_native_text_candidate_cases(analysis, candidate_shape),
    ),
    AppCandidateGroundingTaxonomy::ResultSelectionOcrAnchorPointerClickCaptureEvidence => Ok(
      render_result_selection_candidate_cases(analysis, candidate_shape),
    ),
    AppCandidateGroundingTaxonomy::WindowActionWindowPointPointerClickCaptureEvidence => Ok(
      render_window_action_candidate_cases(analysis, candidate_shape),
    ),
  }
}

pub(crate) fn render_search_entry_candidate_recipe(analysis: &AppAnalysis) -> Value {
  let app_slug = recipe_app_slug(&analysis.app_identity);
  json!({
    "recipe_id": format!("macos.{app_slug}.search_entry_candidate.v0"),
    "version": "0.1.0",
    "status": "candidate-recipe",
    "platform": "macOS",
    "target_app": {
      "name": analysis.app_identity.app_name,
      "bundle_id": "${app_id}",
      "display_mode": "live-desktop"
    },
    "strategy": {
      "family": "search-entry",
      "grounding": "ax-text-input",
      "activation": "clipboard-submit",
      "verificationContract": "captureEvidence"
    },
    "objective": format!("Candidate search-entry slice for {} distilled from app-surface analysis.", analysis.app_identity.app_name),
    "inputs": {
      "app_id": { "type": "string", "default": analysis.app_identity.bundle_id },
      "focus_query": { "type": "string", "note": "Replace with the best known search-field or entry-point AX query for this app." },
      "query": { "type": "string", "note": "Replace with a real query during validation." },
      "activate_settle_ms": { "type": "integer", "default": 250 },
      "submit_settle_ms": { "type": "integer", "default": 600 }
    },
    "preconditions": [
      "The host is macOS.",
      format!("{} is installed and can be addressed by ${{app_id}}.", analysis.app_identity.app_name),
      "Screen Recording and Accessibility permissions are already granted."
    ],
    "disturbance_policy": {
      "max_disturbance": "pointer",
      "declared_classes": ["none", "foreground_app", "keyboard", "clipboard", "pointer"],
      "notes": [
        "This is a candidate distilled from probe/analyze output, not a validated skill.",
        "The search-field focus query is still provisional and must be validated live."
      ]
    },
    "steps": [
      {
        "id": "activate-target-app",
        "command_id": "debug.activateApp",
        "disturbance": { "classes": ["foreground_app"], "max": "foreground_app" },
        "args": { "target": "${app_id}", "settle_ms": "${activate_settle_ms}" },
        "purpose": "Bring the app to the foreground before search-entry probing."
      },
      {
        "id": "focus-search-input",
        "command_id": "debug.focusTextInput",
        "disturbance": { "classes": ["foreground_app", "keyboard", "pointer"], "max": "pointer" },
        "args": { "target": "${app_id}", "query": "${focus_query}", "max_depth": 6, "max_children": 24 },
        "purpose": "Try to focus the search-entry surface through AX."
      },
      {
        "id": "paste-query",
        "command_id": "debug.pasteTextPreserveClipboard",
        "disturbance": { "classes": ["foreground_app", "keyboard", "clipboard"], "max": "clipboard" },
        "args": { "target": "${app_id}", "text": "${query}", "replace_existing": true, "submit_key": "return", "submit_settle_ms": "${submit_settle_ms}" },
        "purpose": "Paste and submit the candidate query while restoring the clipboard."
      },
      {
        "id": "capture-evidence",
        "command_id": "debug.captureDisplay",
        "disturbance": { "classes": ["foreground_app"], "max": "foreground_app" },
        "args": { "target": "${app_id}", "activate_target_before_capture": true, "label": format!("{app_slug}-search-entry-${{query}}") },
        "expect": { "artifact_count_at_least": 1 },
        "purpose": "Capture post-submit evidence for later validation."
      }
    ],
    "verification": {
      "expected_signals": [
        "The app can be foregrounded.",
        "A candidate entry field can be focused through AX.",
        "The query can be submitted with clipboard-backed input.",
        "A post-submit screenshot artifact exists."
      ],
      "success_criteria": [
        "The query is submitted through the shared runtime.",
        "A post-submit screenshot artifact exists."
      ],
      "non_goals": [
        "This candidate does not prove semantic result selection.",
        "This candidate does not prove playback or action success."
      ]
    },
    "known_limits": {
      "candidate_only": "This recipe was distilled from analysis output and has not been validated yet.",
      "focus_query": "The focus_query input must be grounded during validate.",
      "semantic_success": "This candidate only covers search-entry and evidence capture."
    }
  })
}

pub(crate) fn render_native_text_candidate_recipe(analysis: &AppAnalysis) -> Value {
  let app_slug = recipe_app_slug(&analysis.app_identity);
  let marker = format!("AUV_{}_MARKER", app_slug.to_ascii_uppercase());
  json!({
    "recipe_id": format!("macos.{app_slug}.native_text_candidate.v0"),
    "version": "0.1.0",
    "status": "candidate-recipe",
    "platform": "macOS",
    "target_app": {
      "name": analysis.app_identity.app_name,
      "bundle_id": "${app_id}",
      "display_mode": "live-desktop"
    },
    "strategy": {
      "family": "native-text",
      "grounding": "ax-text",
      "activation": "pointer-focus-clipboard-paste",
      "verificationContract": "verifyAxText"
    },
    "objective": format!("Candidate native-text slice for {} distilled from app-surface analysis.", analysis.app_identity.app_name),
    "inputs": {
      "app_id": { "type": "string", "default": analysis.app_identity.bundle_id },
      "focus_query": { "type": "string", "note": "Replace with the best known editable text-area AX query for this app." },
      "target_text": { "type": "string", "default": marker },
      "activate_settle_ms": { "type": "integer", "default": 250 },
      "type_settle_ms": { "type": "integer", "default": 250 }
    },
    "preconditions": [
      "The host is macOS.",
      format!("{} is installed and can be addressed by ${{app_id}}.", analysis.app_identity.app_name),
      "Screen Recording and Accessibility permissions are already granted."
    ],
    "disturbance_policy": {
      "max_disturbance": "pointer",
      "declared_classes": ["none", "foreground_app", "keyboard", "clipboard", "pointer"],
      "notes": [
        "This is a candidate distilled from probe/analyze output, not a validated skill.",
        "The focus_query input must be validated against a real editable text surface."
      ]
    },
    "steps": [
      {
        "id": "activate-target-app",
        "command_id": "debug.activateApp",
        "disturbance": { "classes": ["foreground_app"], "max": "foreground_app" },
        "args": { "target": "${app_id}", "settle_ms": "${activate_settle_ms}" },
        "purpose": "Bring the app to the foreground before text interaction."
      },
      {
        "id": "focus-text-surface",
        "command_id": "debug.focusTextInput",
        "disturbance": { "classes": ["foreground_app", "keyboard", "pointer"], "max": "pointer" },
        "args": { "target": "${app_id}", "query": "${focus_query}", "max_depth": 6, "max_children": 40 },
        "purpose": "Focus a text-bearing surface through AX."
      },
      {
        "id": "paste-text",
        "command_id": "debug.pasteTextPreserveClipboard",
        "disturbance": { "classes": ["foreground_app", "keyboard", "clipboard"], "max": "clipboard" },
        "args": { "target": "${app_id}", "text": "${target_text}", "replace_existing": true, "submit_settle_ms": "${type_settle_ms}" },
        "purpose": "Write the marker through clipboard-backed text input."
      },
      {
        "id": "verify-text",
        "command_id": "debug.verifyAxText",
        "disturbance": { "classes": ["none"], "max": "none" },
        "args": { "target": "${app_id}", "target_text": "${target_text}", "max_depth": 6, "max_children": 48 },
        "expect": {
          "signal_equals": { "ax.node_found": "true" },
          "signal_contains": { "ax.matched_text": "${target_text}" },
          "artifact_count_at_least": 1
        },
        "purpose": "Verify the marker through the AX tree."
      }
    ],
    "verification": {
      "expected_signals": [
        "The app can be foregrounded.",
        "A text-bearing surface can be focused.",
        "The target text can be written.",
        "The same marker can be matched through AX."
      ],
      "success_criteria": [
        "The marker is visible in the AX tree.",
        "The recipe completes without screenshot-only verification."
      ],
      "non_goals": [
        "This candidate does not prove rich editing coverage.",
        "This candidate does not prove cross-app semantic reuse by itself."
      ]
    },
    "known_limits": {
      "candidate_only": "This recipe was distilled from analysis output and has not been validated yet.",
      "focus_query": "The focus_query input must be grounded during validate."
    }
  })
}

pub(crate) fn render_result_selection_candidate_recipe(analysis: &AppAnalysis) -> Value {
  let app_slug = recipe_app_slug(&analysis.app_identity);
  json!({
    "recipe_id": format!("macos.{app_slug}.result_selection_candidate.v0"),
    "version": "0.1.0",
    "status": "candidate-recipe",
    "platform": "macOS",
    "target_app": {
      "name": analysis.app_identity.app_name,
      "bundle_id": "${app_id}",
      "display_mode": "live-desktop"
    },
    "strategy": {
      "family": "result-selection",
      "grounding": "ocr-anchor",
      "activation": "pointer-click",
      "verificationContract": "captureEvidence"
    },
    "objective": format!("Candidate OCR-anchor result-selection slice for {} distilled from app-surface analysis.", analysis.app_identity.app_name),
    "inputs": {
      "app_id": { "type": "string", "default": analysis.app_identity.bundle_id },
      "anchor_text": { "type": "string", "note": "Replace with a visible OCR anchor when validating this candidate." },
      "match_index": { "type": "integer", "default": 0 },
      "post_click_settle_ms": { "type": "integer", "default": 700 }
    },
    "preconditions": [
      "The host is macOS.",
      format!("{} is installed and can be addressed by ${{app_id}}.", analysis.app_identity.app_name),
      "Screen Recording and Accessibility permissions are already granted."
    ],
    "disturbance_policy": {
      "max_disturbance": "pointer",
      "declared_classes": ["none", "foreground_app", "pointer"],
      "notes": [
        "This is a candidate distilled from probe/analyze output, not a validated skill.",
        "The visible OCR anchor must be validated live."
      ]
    },
    "steps": [
      {
        "id": "activate-target-app",
        "command_id": "debug.activateApp",
        "disturbance": { "classes": ["foreground_app"], "max": "foreground_app" },
        "args": { "target": "${app_id}", "settle_ms": 250 },
        "purpose": "Bring the app to the foreground before result selection."
      },
      {
        "id": "click-anchor",
        "command_id": "debug.clickScreenText",
        "disturbance": { "classes": ["pointer"], "max": "pointer" },
        "args": { "target": "${app_id}", "query": "${anchor_text}", "match_index": "${match_index}" },
        "purpose": "Click a visible OCR anchor as the candidate result-selection path."
      },
      {
        "id": "capture-evidence",
        "command_id": "debug.captureDisplay",
        "disturbance": { "classes": ["none"], "max": "none" },
        "args": { "target": "${app_id}", "activate_target_before_capture": true, "label": format!("{app_slug}-result-selection-${{anchor_text}}") },
        "expect": { "artifact_count_at_least": 1 },
        "purpose": "Capture post-selection evidence for later validation."
      }
    ],
    "verification": {
      "expected_signals": [
        "A visible OCR anchor can be resolved.",
        "The pointer click succeeds.",
        "A post-click screenshot artifact exists."
      ],
      "success_criteria": [
        "The candidate anchor can be clicked through the runtime.",
        "A post-click screenshot artifact exists."
      ],
      "non_goals": [
        "This candidate does not prove semantic success.",
        "This candidate does not prove playback or action completion."
      ]
    },
    "known_limits": {
      "candidate_only": "This recipe was distilled from analysis output and has not been validated yet.",
      "anchor_text": "The anchor_text input must be grounded during validate.",
      "semantic_success": "This candidate proves activation-attempt shape only, not semantic success."
    }
  })
}

pub(crate) fn render_search_entry_candidate_cases(
  analysis: &AppAnalysis,
  candidate_shape: &AppDistilledCandidateShape,
) -> Value {
  let focus_query = candidate_shape
    .provided_inputs
    .get("focus_query")
    .cloned()
    .unwrap_or_else(|| "TODO_FOCUS_QUERY".to_string());
  json!({
    "skill_id": format!("macos.{}.search_entry_candidate.v0", recipe_app_slug(&analysis.app_identity)),
    "version": "0.1.0",
    "status": "candidate-case-matrix",
    "cases": [
      {
        "case_id": "default-candidate",
        "status": "candidate",
        "inputs": {
          "focus_query": focus_query,
          "query": "TODO_QUERY"
        },
        "disturbance": "pointer",
        "notes": [
          "Generated from app analyze output.",
          "Replace focus_query and query with a real validated baseline during the validate step."
        ]
      }
    ]
  })
}

pub(crate) fn render_native_text_candidate_cases(
  analysis: &AppAnalysis,
  candidate_shape: &AppDistilledCandidateShape,
) -> Value {
  let focus_query = candidate_shape
    .provided_inputs
    .get("focus_query")
    .cloned()
    .unwrap_or_else(|| "TODO_TEXT_SURFACE_QUERY".to_string());
  json!({
    "skill_id": format!("macos.{}.native_text_candidate.v0", recipe_app_slug(&analysis.app_identity)),
    "version": "0.1.0",
    "status": "candidate-case-matrix",
    "cases": [
      {
        "case_id": "default-candidate",
        "status": "candidate",
        "inputs": {
          "focus_query": focus_query
        },
        "disturbance": "pointer",
        "notes": [
          "Generated from app analyze output.",
          "Replace focus_query with a concrete editable text surface before validate."
        ]
      }
    ]
  })
}

pub(crate) fn render_result_selection_candidate_cases(
  analysis: &AppAnalysis,
  candidate_shape: &AppDistilledCandidateShape,
) -> Value {
  let anchor_text = candidate_shape
    .provided_inputs
    .get("anchor_text")
    .cloned()
    .unwrap_or_else(|| "TODO_VISIBLE_ANCHOR_TEXT".to_string());
  json!({
    "skill_id": format!("macos.{}.result_selection_candidate.v0", recipe_app_slug(&analysis.app_identity)),
    "version": "0.1.0",
    "status": "candidate-case-matrix",
    "cases": [
      {
        "case_id": "default-candidate",
        "status": "candidate",
        "inputs": {
          "anchor_text": anchor_text
        },
        "disturbance": "pointer",
        "notes": [
          "Generated from app analyze output.",
          "Replace anchor_text with a concrete visible OCR anchor during validate."
        ]
      }
    ]
  })
}

pub(crate) fn render_window_action_candidate_recipe(analysis: &AppAnalysis) -> Value {
  let app_slug = recipe_app_slug(&analysis.app_identity);
  json!({
    "recipe_id": format!("macos.{app_slug}.window_action_candidate.v0"),
    "version": "0.1.0",
    "status": "candidate-recipe",
    "platform": "macOS",
    "target_app": {
      "name": analysis.app_identity.app_name,
      "bundle_id": "${app_id}",
      "display_mode": "live-desktop"
    },
    "strategy": {
      "family": "window-action",
      "grounding": "window-point",
      "activation": "pointer-click",
      "verificationContract": "captureEvidence"
    },
    "objective": format!("Candidate window-relative pointer slice for {} distilled from app-surface analysis.", analysis.app_identity.app_name),
    "inputs": {
      "app_id": { "type": "string", "default": analysis.app_identity.bundle_id },
      "relative_x": { "type": "number", "note": "Replace with a window-relative x ratio in the range [0, 1]." },
      "relative_y": { "type": "number", "note": "Replace with a window-relative y ratio in the range [0, 1]." },
      "activate_settle_ms": { "type": "integer", "default": 250 },
      "post_click_settle_ms": { "type": "integer", "default": 500 }
    },
    "preconditions": [
      "The host is macOS.",
      format!("{} is installed and can be addressed by ${{app_id}}.", analysis.app_identity.app_name),
      "Screen Recording and Accessibility permissions are already granted.",
      "A stable target point can be expressed relative to the primary app window."
    ],
    "disturbance_policy": {
      "max_disturbance": "pointer",
      "declared_classes": ["none", "foreground_app", "pointer"],
      "notes": [
        "This is a candidate distilled from probe/analyze output, not a validated skill.",
        "The relative_x and relative_y inputs must be grounded against a real target point before validate can promote anything."
      ]
    },
    "steps": [
      {
        "id": "activate-target-app",
        "command_id": "debug.activateApp",
        "disturbance": { "classes": ["foreground_app"], "max": "foreground_app" },
        "args": { "target": "${app_id}", "settle_ms": "${activate_settle_ms}" },
        "purpose": "Bring the app to the foreground before the pointer action."
      },
      {
        "id": "click-window-point",
        "command_id": "debug.clickWindowPoint",
        "disturbance": { "classes": ["foreground_app", "pointer"], "max": "pointer" },
        "args": { "target": "${app_id}", "relative_x": "${relative_x}", "relative_y": "${relative_y}" },
        "purpose": "Click a window-relative target point without pretending semantic grounding exists."
      },
      {
        "id": "capture-evidence",
        "command_id": "debug.captureWindow",
        "disturbance": { "classes": ["foreground_app"], "max": "foreground_app" },
        "args": { "target": "${app_id}", "activate_target_before_capture": true, "label": format!("{app_slug}-window-action") },
        "expect": { "artifact_count_at_least": 1 },
        "purpose": "Capture post-click evidence for later inspection."
      }
    ],
    "verification": {
      "expected_signals": [
        "The app can be foregrounded.",
        "A stable target point can be clicked relative to the resolved window.",
        "A post-click window screenshot artifact exists."
      ],
      "success_criteria": [
        "The window-relative pointer click succeeds.",
        "A post-click evidence artifact exists."
      ],
      "non_goals": [
        "This candidate does not prove semantic selection.",
        "This candidate does not prove app-specific intent like search, playback, or result activation."
      ]
    },
    "known_limits": {
      "candidate_only": "This recipe was distilled from analysis output and has not been validated yet.",
      "relative_point": "The relative_x and relative_y inputs must be grounded against a real window target before validate.",
      "semantic_success": "This candidate only proves a window-relative pointer action shape."
    }
  })
}

pub(crate) fn render_window_action_candidate_cases(
  analysis: &AppAnalysis,
  candidate_shape: &AppDistilledCandidateShape,
) -> Value {
  let relative_x = candidate_shape
    .provided_inputs
    .get("relative_x")
    .cloned()
    .unwrap_or_else(|| "TODO_RELATIVE_X".to_string());
  let relative_y = candidate_shape
    .provided_inputs
    .get("relative_y")
    .cloned()
    .unwrap_or_else(|| "TODO_RELATIVE_Y".to_string());
  json!({
    "skill_id": format!("macos.{}.window_action_candidate.v0", recipe_app_slug(&analysis.app_identity)),
    "version": "0.1.0",
    "status": "candidate-case-matrix",
    "cases": [
      {
        "case_id": "default-candidate",
        "status": "candidate",
        "inputs": {
          "relative_x": relative_x,
          "relative_y": relative_y
        },
        "disturbance": "pointer",
        "notes": [
          "Generated from app analyze output.",
          "Replace relative_x and relative_y with a concrete window-relative target before validate."
        ]
      }
    ]
  })
}
