use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::driver::{
  ObservedAxTreeSnapshot, ObservedDisplaySnapshot, ObservedRect, ObservedWindow, OcrTextSnapshot,
  parse_display_snapshot, parse_observed_ax_tree, parse_ocr_text_snapshot, parse_window_line,
  report_value, sanitized_artifact_name,
};
use crate::model::{AuvResult, ExecutionTarget, InvokeRequest, RunStatus, now_millis};
use crate::runtime::Runtime;
use crate::skill::{
  SkillCaseMatrix, SkillCaseRunOptions, SkillManifest, SkillStrategy, run_skill_case_matrix_inline,
  validate_case_matrix_against_skill, validate_case_matrix_manifest, validate_skill_manifest,
};

const APP_PROBE_VERSION: &str = "v0";
const APP_ANALYSIS_VERSION: &str = "v0";
const APP_DISTILL_VERSION: &str = "v0";
const APP_VALIDATE_VERSION: &str = "v0";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppIdentity {
  pub bundle_id: String,
  pub app_name: String,
  pub app_path: PathBuf,
  pub main_executable_path: Option<PathBuf>,
  pub version: String,
  pub build_version: String,
  pub url_schemes: Vec<String>,
  pub apple_script_addressable: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppProbe {
  pub probe_version: String,
  pub created_at_millis: u128,
  pub project_root: PathBuf,
  pub output_dir: PathBuf,
  pub app: AppIdentity,
  pub steps: Vec<AppProbeStep>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppProbeStep {
  pub id: String,
  pub command_id: String,
  pub target_application_id: Option<String>,
  pub inputs: BTreeMap<String, String>,
  pub run_id: String,
  pub status: String,
  pub output_summary: String,
  pub artifact_paths: Vec<PathBuf>,
  pub inspect_path: PathBuf,
  pub failure_message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppAnalyzeOutput {
  pub analysis: AppAnalysis,
  pub analysis_path: PathBuf,
  pub report_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppDistillOutput {
  pub distillation: AppDistillation,
  pub distillation_path: PathBuf,
  pub report_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppValidateOutput {
  pub validation: AppValidation,
  pub validation_path: PathBuf,
  pub report_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppDistillation {
  pub distill_version: String,
  pub created_at_millis: u128,
  pub source_analysis_path: PathBuf,
  pub app_identity: AppIdentity,
  pub candidates: Vec<AppDistilledCandidate>,
  pub known_boundaries: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppValidation {
  pub validate_version: String,
  pub created_at_millis: u128,
  pub source_distillation_path: PathBuf,
  pub source_analysis_path: PathBuf,
  pub app_identity: AppIdentity,
  pub candidates: Vec<AppValidatedCandidate>,
  pub known_boundaries: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppDistilledCandidate {
  pub recipe_id: String,
  pub taxonomy_id: String,
  pub status: AssessmentStatus,
  pub rationale: String,
  pub recipe_path: PathBuf,
  pub case_matrix_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppValidatedCandidate {
  pub recipe_id: String,
  pub taxonomy_id: String,
  pub status: AppValidationStatus,
  pub rationale: String,
  pub recipe_path: PathBuf,
  pub case_matrix_path: PathBuf,
  pub selected_case_count: usize,
  pub unresolved_inputs: Vec<String>,
  pub failure_message: Option<String>,
  pub resolved_inputs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppAnalysis {
  pub analysis_version: String,
  pub created_at_millis: u128,
  pub probe_path: PathBuf,
  pub app_identity: AppIdentity,
  pub window_context: AppWindowContext,
  pub permissions: AppPermissionState,
  pub available_surfaces: AppAvailableSurfaces,
  pub grounding_assessment: AppGroundingAssessment,
  pub control_assessment: AppControlAssessment,
  pub verification_assessment: AppVerificationAssessment,
  pub disturbance_profile: AppDisturbanceProfile,
  pub known_boundaries: Vec<String>,
  pub recommended_strategies: Vec<AppRecommendedStrategy>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppWindowContext {
  pub observed_window_count: usize,
  pub observed_at: String,
  pub frontmost_app_name: String,
  pub frontmost_window_title: String,
  pub primary_window_title: String,
  pub primary_window_bounds: Option<AppRect>,
  pub primary_window_display_scale: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppPermissionState {
  pub screen_recording: String,
  pub accessibility: String,
  pub automation_to_system_events: String,
  pub launch_host_process: String,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AssessmentStatus {
  Available,
  Partial,
  Candidate,
  Likely,
  Unknown,
  Unavailable,
}

impl AssessmentStatus {
  fn as_str(&self) -> &'static str {
    match self {
      Self::Available => "available",
      Self::Partial => "partial",
      Self::Candidate => "candidate",
      Self::Likely => "likely",
      Self::Unknown => "unknown",
      Self::Unavailable => "unavailable",
    }
  }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AppValidationStatus {
  Validated,
  Candidate,
  Rejected,
}

impl AppValidationStatus {
  fn as_str(&self) -> &'static str {
    match self {
      Self::Validated => "validated",
      Self::Candidate => "candidate",
      Self::Rejected => "rejected",
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppAvailableSurfaces {
  pub accessibility_tree: AssessmentStatus,
  pub menu_surface: AssessmentStatus,
  pub shortcut_surface: AssessmentStatus,
  pub apple_script_surface: AssessmentStatus,
  pub url_scheme_surface: AssessmentStatus,
  pub keyboard_first_surface: AssessmentStatus,
  pub pointer_fallback_surface: AssessmentStatus,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppGroundingAssessment {
  pub ocr_sample_query: String,
  pub ocr_sample_status: AssessmentStatus,
  pub ocr_sample_match_count: usize,
  pub stable_anchor_candidates: Vec<String>,
  pub stable_region_candidates: Vec<String>,
  pub overlay_debug_artifacts_recommended: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppControlAssessment {
  pub preferred_path: String,
  pub non_pointer_path: AssessmentStatus,
  pub keyboard_path: AssessmentStatus,
  pub pointer_fallback: AssessmentStatus,
  pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppVerificationAssessment {
  pub ax_verify: AssessmentStatus,
  pub image_verify: AssessmentStatus,
  pub ui_state_verify: AssessmentStatus,
  pub semantic_success: AssessmentStatus,
  pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppDisturbanceProfile {
  pub observation: Vec<String>,
  pub non_pointer_control: Vec<String>,
  pub pointer_fallback: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppRecommendedStrategy {
  pub taxonomy_id: String,
  pub status: AssessmentStatus,
  pub rationale: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppRect {
  pub x: i64,
  pub y: i64,
  pub width: i64,
  pub height: i64,
}

impl AppRect {
  fn from_observed(rect: &ObservedRect) -> Self {
    Self {
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
    }
  }

  fn center(&self) -> (i64, i64) {
    (self.x + self.width / 2, self.y + self.height / 2)
  }

  fn render_compact(&self) -> String {
    format!("{},{},{},{}", self.x, self.y, self.width, self.height)
  }
}

pub fn probe_app(
  project_root: &Path,
  runtime: &Runtime,
  bundle_id: &str,
  output_dir: Option<PathBuf>,
) -> AuvResult<AppProbe> {
  let app = resolve_app_identity(bundle_id)?;
  let output_dir = output_dir.unwrap_or_else(|| default_probe_output_dir(project_root, bundle_id));
  if output_dir.exists() {
    return Err(format!(
      "probe output directory already exists: {}",
      output_dir.display()
    ));
  }
  fs::create_dir_all(&output_dir).map_err(|error| {
    format!(
      "failed to create app probe directory {}: {error}",
      output_dir.display()
    )
  })?;

  let mut steps = Vec::new();
  steps.push(invoke_probe_step(
    project_root,
    runtime,
    "probe-permissions",
    "debug.probePermissions",
    None,
    BTreeMap::new(),
  )?);
  steps.push(invoke_probe_step(
    project_root,
    runtime,
    "probe-displays",
    "debug.probeDisplays",
    None,
    BTreeMap::new(),
  )?);
  steps.push(invoke_probe_step(
    project_root,
    runtime,
    "probe-coordinate-readiness",
    "debug.probeCoordinateReadiness",
    None,
    BTreeMap::new(),
  )?);

  let mut window_inputs = BTreeMap::new();
  window_inputs.insert("limit".to_string(), "20".to_string());
  steps.push(invoke_probe_step(
    project_root,
    runtime,
    "observe-windows",
    "debug.observeWindows",
    Some(bundle_id.to_string()),
    window_inputs,
  )?);

  let mut tree_inputs = BTreeMap::new();
  tree_inputs.insert("max_depth".to_string(), "6".to_string());
  tree_inputs.insert("max_children".to_string(), "24".to_string());
  steps.push(invoke_probe_step(
    project_root,
    runtime,
    "observe-window-tree",
    "debug.observeWindowTree",
    Some(bundle_id.to_string()),
    tree_inputs,
  )?);

  let capture_label = format!("app-probe-{}", sanitized_artifact_name(bundle_id));
  let mut capture_inputs = BTreeMap::new();
  capture_inputs.insert("label".to_string(), capture_label);
  capture_inputs.insert(
    "activate_target_before_capture".to_string(),
    "true".to_string(),
  );
  let capture_step = invoke_probe_step(
    project_root,
    runtime,
    "capture-screen",
    "debug.captureScreen",
    Some(bundle_id.to_string()),
    capture_inputs,
  )?;
  let screenshot_artifact_path = capture_step
    .artifact_paths
    .iter()
    .find(|path| {
      path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("png"))
    })
    .cloned()
    .ok_or_else(|| {
      format!(
        "capture-screen probe step did not produce a screenshot artifact for {}",
        bundle_id
      )
    })?;
  steps.push(capture_step);

  let mut ocr_inputs = BTreeMap::new();
  ocr_inputs.insert(
    "image_path".to_string(),
    screenshot_artifact_path.display().to_string(),
  );
  ocr_inputs.insert(
    "query".to_string(),
    if app.app_name.trim().is_empty() {
      app.bundle_id.clone()
    } else {
      app.app_name.clone()
    },
  );
  ocr_inputs.insert("min_confidence".to_string(), "0.55".to_string());
  steps.push(invoke_probe_step(
    project_root,
    runtime,
    "ocr-sample",
    "debug.findImageText",
    None,
    ocr_inputs,
  )?);

  let probe = AppProbe {
    probe_version: APP_PROBE_VERSION.to_string(),
    created_at_millis: now_millis(),
    project_root: project_root.to_path_buf(),
    output_dir: output_dir.clone(),
    app,
    steps,
  };
  let probe_path = output_dir.join("probe.json");
  write_pretty_json(&probe_path, &probe)?;
  Ok(probe)
}

pub fn analyze_app_probe(query: &Path) -> AuvResult<AppAnalyzeOutput> {
  let probe_path = resolve_probe_path(query)?;
  let probe: AppProbe = read_json(&probe_path)?;
  let analysis = build_app_analysis(&probe_path, &probe)?;
  let analysis_path = probe.output_dir.join("analysis.json");
  let report_path = probe.output_dir.join("report.md");
  write_pretty_json(&analysis_path, &analysis)?;
  fs::write(&report_path, render_app_analysis_report(&analysis)).map_err(|error| {
    format!(
      "failed to write app analysis report {}: {error}",
      report_path.display()
    )
  })?;
  Ok(AppAnalyzeOutput {
    analysis,
    analysis_path,
    report_path,
  })
}

pub fn distill_app_analysis(
  query: &Path,
  output_dir: Option<PathBuf>,
) -> AuvResult<AppDistillOutput> {
  let analysis_path = resolve_analysis_path(query)?;
  let analysis: AppAnalysis = read_json(&analysis_path)?;
  let output_dir =
    output_dir.unwrap_or_else(|| default_distill_output_dir(&analysis_path, &analysis));
  if output_dir.exists() {
    return Err(format!(
      "distillation output directory already exists: {}",
      output_dir.display()
    ));
  }
  fs::create_dir_all(output_dir.join("candidates")).map_err(|error| {
    format!(
      "failed to create app distillation directory {}: {error}",
      output_dir.display()
    )
  })?;

  let mut candidates = Vec::new();
  for strategy in &analysis.recommended_strategies {
    let recipe_value = render_candidate_recipe(&analysis, strategy)?;
    let matrix_value = render_candidate_case_matrix(&analysis, strategy)?;
    let manifest: SkillManifest =
      serde_json::from_value(recipe_value.clone()).map_err(|error| {
        format!(
          "failed to parse generated candidate recipe for {}: {error}",
          strategy.taxonomy_id
        )
      })?;
    validate_skill_manifest(&manifest)?;
    let matrix: SkillCaseMatrix =
      serde_json::from_value(matrix_value.clone()).map_err(|error| {
        format!(
          "failed to parse generated candidate case matrix for {}: {error}",
          strategy.taxonomy_id
        )
      })?;
    validate_case_matrix_manifest(&matrix)?;
    validate_case_matrix_against_skill(&manifest, &matrix)?;

    let candidate_slug = candidate_slug(&strategy.taxonomy_id);
    let recipe_path = output_dir
      .join("candidates")
      .join(format!("{candidate_slug}.recipe.json"));
    let case_matrix_path = output_dir
      .join("candidates")
      .join(format!("{candidate_slug}.cases.json"));
    write_pretty_json(&recipe_path, &recipe_value)?;
    write_pretty_json(&case_matrix_path, &matrix_value)?;
    candidates.push(AppDistilledCandidate {
      recipe_id: manifest.recipe_id.clone(),
      taxonomy_id: strategy.taxonomy_id.clone(),
      status: strategy.status,
      rationale: strategy.rationale.clone(),
      recipe_path,
      case_matrix_path,
    });
  }

  let distillation = AppDistillation {
    distill_version: APP_DISTILL_VERSION.to_string(),
    created_at_millis: now_millis(),
    source_analysis_path: analysis_path.clone(),
    app_identity: analysis.app_identity.clone(),
    candidates,
    known_boundaries: analysis.known_boundaries.clone(),
  };
  let distillation_path = output_dir.join("distillation.json");
  let report_path = output_dir.join("report.md");
  write_pretty_json(&distillation_path, &distillation)?;
  fs::write(
    &report_path,
    render_app_distillation_report(&analysis, &distillation),
  )
  .map_err(|error| {
    format!(
      "failed to write app distillation report {}: {error}",
      report_path.display()
    )
  })?;

  Ok(AppDistillOutput {
    distillation,
    distillation_path,
    report_path,
  })
}

pub fn validate_app_distillation(runtime: &Runtime, query: &Path) -> AuvResult<AppValidateOutput> {
  let distillation_path = resolve_distillation_path(query)?;
  let distillation: AppDistillation = read_json(&distillation_path)?;
  let analysis: AppAnalysis = read_json(&distillation.source_analysis_path)?;
  let probe = read_json::<AppProbe>(&analysis.probe_path).ok();
  let ax_snapshot = probe
    .as_ref()
    .and_then(|probe| parse_ax_snapshot(probe).ok());

  let mut candidates = Vec::new();
  for candidate in &distillation.candidates {
    let manifest: SkillManifest = read_json(&candidate.recipe_path)?;
    let mut matrix: SkillCaseMatrix = read_json(&candidate.case_matrix_path)?;
    let mut resolved_inputs = BTreeMap::new();
    let unresolved_inputs = apply_candidate_grounding(
      &analysis,
      ax_snapshot.as_ref(),
      &candidate.taxonomy_id,
      &mut matrix,
      &mut resolved_inputs,
    );
    let selected_case_count = matrix.cases.len();

    let validated = if unresolved_inputs.is_empty() {
      match run_skill_case_matrix_inline(
        runtime,
        &manifest,
        &matrix,
        SkillCaseRunOptions {
          dry_run: false,
          max_disturbance: None,
          only_case_ids: Vec::new(),
          include_nonvalidated: true,
        },
      ) {
        Ok(()) => AppValidatedCandidate {
          recipe_id: candidate.recipe_id.clone(),
          taxonomy_id: candidate.taxonomy_id.clone(),
          status: AppValidationStatus::Validated,
          rationale: format!(
            "All {} candidate case(s) executed successfully through the shared runtime.",
            selected_case_count
          ),
          recipe_path: candidate.recipe_path.clone(),
          case_matrix_path: candidate.case_matrix_path.clone(),
          selected_case_count,
          unresolved_inputs,
          failure_message: None,
          resolved_inputs,
        },
        Err(error) => AppValidatedCandidate {
          recipe_id: candidate.recipe_id.clone(),
          taxonomy_id: candidate.taxonomy_id.clone(),
          status: AppValidationStatus::Rejected,
          rationale: "The candidate was runnable, but live execution failed.".to_string(),
          recipe_path: candidate.recipe_path.clone(),
          case_matrix_path: candidate.case_matrix_path.clone(),
          selected_case_count,
          unresolved_inputs,
          failure_message: Some(error),
          resolved_inputs,
        },
      }
    } else {
      AppValidatedCandidate {
        recipe_id: candidate.recipe_id.clone(),
        taxonomy_id: candidate.taxonomy_id.clone(),
        status: AppValidationStatus::Candidate,
        rationale:
          "The candidate still requires live grounding for unresolved inputs before execution."
            .to_string(),
        recipe_path: candidate.recipe_path.clone(),
        case_matrix_path: candidate.case_matrix_path.clone(),
        selected_case_count,
        unresolved_inputs,
        failure_message: None,
        resolved_inputs,
      }
    };
    candidates.push(validated);
  }

  let validation = AppValidation {
    validate_version: APP_VALIDATE_VERSION.to_string(),
    created_at_millis: now_millis(),
    source_distillation_path: distillation_path.clone(),
    source_analysis_path: distillation.source_analysis_path.clone(),
    app_identity: distillation.app_identity.clone(),
    candidates,
    known_boundaries: distillation.known_boundaries.clone(),
  };
  let validation_root = distillation_path
    .parent()
    .unwrap_or_else(|| Path::new("."))
    .to_path_buf();
  let validation_path = validation_root.join("validation.json");
  let report_path = validation_root.join("validation-report.md");
  write_pretty_json(&validation_path, &validation)?;
  fs::write(&report_path, render_app_validation_report(&validation)).map_err(|error| {
    format!(
      "failed to write app validation report {}: {error}",
      report_path.display()
    )
  })?;
  Ok(AppValidateOutput {
    validation,
    validation_path,
    report_path,
  })
}

fn build_app_analysis(probe_path: &Path, probe: &AppProbe) -> AuvResult<AppAnalysis> {
  let permission_state = parse_permission_state(probe)?;
  let display_snapshot = parse_display_step(probe)?;
  let coordinate_readiness = parse_coordinate_readiness(probe)?;
  let window_snapshot = parse_window_snapshot(probe)?;
  let ax_snapshot = parse_ax_snapshot(probe)?;
  let ocr_snapshot = parse_ocr_snapshot(probe)?;

  let primary_window = choose_primary_window(&window_snapshot.windows);
  let primary_window_bounds = primary_window.map(|window| AppRect::from_observed(&window.bounds));
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

  let mut known_boundaries = Vec::new();
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
      "captureScreenEvidence",
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
  if !ocr_snapshot.matches.is_empty() && has_collection_like_surface(&ax_snapshot) {
    recommended_strategies.push(recommended_strategy(
      "result-selection",
      "ocr-anchor",
      "pointer-click",
      "captureScreenEvidence",
      AssessmentStatus::Candidate,
      "The sample OCR query produced filtered matches on the captured image, so OCR-anchor result selection is a candidate grounding strategy.",
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
    known_boundaries,
    recommended_strategies,
  })
}

fn render_app_analysis_report(analysis: &AppAnalysis) -> String {
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
    format!("- app path: `{}`", analysis.app_identity.app_path.display()),
    format!(
      "- version: `{}` (`build {}`)",
      analysis.app_identity.version, analysis.app_identity.build_version
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
  lines.push("## 4. Control Strategy".to_string());
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
  lines.push("## 5. Verification Assessment".to_string());
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
  lines.push("## Known Boundaries".to_string());
  lines.push(String::new());
  if analysis.known_boundaries.is_empty() {
    lines.push("- none recorded".to_string());
  } else {
    for note in &analysis.known_boundaries {
      lines.push(format!("- {note}"));
    }
  }
  lines.push(String::new());
  lines.push("## Recommended Candidate Strategies".to_string());
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

fn render_app_distillation_report(
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

fn render_app_validation_report(validation: &AppValidation) -> String {
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
  for candidate in &validation.candidates {
    *by_status.entry(candidate.status.as_str()).or_insert(0) += 1;
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
      lines.push(format!("- rationale: {}", candidate.rationale));
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

fn apply_candidate_grounding(
  analysis: &AppAnalysis,
  ax_snapshot: Option<&ObservedAxTreeSnapshot>,
  taxonomy_id: &str,
  matrix: &mut SkillCaseMatrix,
  resolved_inputs: &mut BTreeMap<String, String>,
) -> Vec<String> {
  let mut unresolved = BTreeSet::new();
  let search_entry_query = choose_search_entry_query(ax_snapshot);
  let native_text_query = choose_native_text_focus_query(ax_snapshot);
  let stable_anchor =
    first_stable_anchor_value(&analysis.grounding_assessment.stable_anchor_candidates);

  for case in &mut matrix.cases {
    for (key, value) in &mut case.inputs {
      if !looks_like_placeholder(value) {
        resolved_inputs
          .entry(key.clone())
          .or_insert_with(|| value.clone());
        continue;
      }

      let replacement = match (taxonomy_id, key.as_str()) {
        ("search-entry.ax-text-input.clipboard-submit.capture-screen-evidence", "focus_query") => {
          search_entry_query.clone()
        }
        ("search-entry.ax-text-input.clipboard-submit.capture-screen-evidence", "query") => {
          Some(format!(
            "AUV_{}",
            recipe_app_slug(&analysis.app_identity).to_ascii_uppercase()
          ))
        }
        ("native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text", "focus_query") => {
          native_text_query.clone()
        }
        ("result-selection.ocr-anchor.pointer-click.capture-screen-evidence", "anchor_text") => {
          stable_anchor.clone()
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

  unresolved.into_iter().collect()
}

fn choose_search_entry_query(snapshot: Option<&ObservedAxTreeSnapshot>) -> Option<String> {
  let snapshot = snapshot?;
  snapshot.nodes.iter().find_map(|node| {
    let looks_like_search = node.subrole == "AXSearchField"
      || node.role == "AXSearchField"
      || node.placeholder.to_lowercase().contains("search")
      || node.title.to_lowercase().contains("search")
      || node.description.to_lowercase().contains("search")
      || node.placeholder.contains("搜索")
      || node.title.contains("搜索")
      || node.description.contains("搜索");
    if !looks_like_search {
      return None;
    }
    preferred_ax_query_text(node)
  })
}

fn choose_native_text_focus_query(snapshot: Option<&ObservedAxTreeSnapshot>) -> Option<String> {
  let snapshot = snapshot?;
  snapshot
    .nodes
    .iter()
    .find(|node| {
      let role = node.role.as_str();
      let subrole = node.subrole.as_str();
      role == "AXTextField"
        || role == "AXTextArea"
        || role == "AXComboBox"
        || subrole == "AXSearchField"
        || !node.placeholder.trim().is_empty()
    })
    .and_then(preferred_ax_query_text)
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
  let report = read_named_text_artifact(probe, "probe-displays", None)?;
  parse_display_snapshot(&report)
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
  let report = read_named_text_artifact(probe, "observe-windows", None)?;
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

fn parse_ax_snapshot(probe: &AppProbe) -> AuvResult<ObservedAxTreeSnapshot> {
  let report = read_named_text_artifact(probe, "observe-window-tree", None)?;
  parse_observed_ax_tree(&report)
}

fn parse_ocr_snapshot(probe: &AppProbe) -> AuvResult<OcrTextSnapshot> {
  let report = read_named_text_artifact(probe, "ocr-sample", None)?;
  parse_ocr_text_snapshot(&report)
}

fn read_named_text_artifact(
  probe: &AppProbe,
  step_id: &str,
  file_name_hint: Option<&str>,
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
        .is_some_and(|value| value.eq_ignore_ascii_case("txt"))
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
        "probe step {} did not produce the expected text artifact",
        step_id
      )
    })?;
  fs::read_to_string(&artifact_path).map_err(|error| {
    format!(
      "failed to read probe artifact {}: {error}",
      artifact_path.display()
    )
  })
}

fn choose_primary_window(windows: &[ObservedWindow]) -> Option<&ObservedWindow> {
  windows.iter().max_by_key(|window| {
    let area = window.bounds.width.max(0) * window.bounds.height.max(0);
    let titled_bonus = if window.title.trim().is_empty() { 0 } else { 1 };
    (area, titled_bonus)
  })
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

fn recommended_strategy(
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

fn resolve_app_identity(bundle_id: &str) -> AuvResult<AppIdentity> {
  let escaped_bundle_id = bundle_id.replace('"', "\\\"");
  let path_script = format!("POSIX path of (path to application id \"{escaped_bundle_id}\")");
  let app_path_raw = run_command_capture("osascript", &["-e", &path_script])?;
  let app_path = PathBuf::from(app_path_raw.trim());
  let info_plist_path = app_path.join("Contents/Info.plist");
  let info_json = run_command_capture(
    "plutil",
    &[
      "-convert",
      "json",
      "-o",
      "-",
      info_plist_path
        .to_str()
        .ok_or_else(|| format!("non-utf8 Info.plist path {}", info_plist_path.display()))?,
    ],
  )?;
  let info: Value = serde_json::from_str(&info_json).map_err(|error| {
    format!(
      "failed to parse Info.plist JSON for {}: {error}",
      app_path.display()
    )
  })?;
  let app_name = first_non_empty_string(&[
    json_string(&info, "CFBundleDisplayName"),
    json_string(&info, "CFBundleName"),
    app_path
      .file_stem()
      .and_then(|value| value.to_str())
      .map(|value| value.to_string()),
  ])
  .unwrap_or_else(|| bundle_id.to_string());
  let version =
    json_string(&info, "CFBundleShortVersionString").unwrap_or_else(|| "unknown".to_string());
  let build_version =
    json_string(&info, "CFBundleVersion").unwrap_or_else(|| "unknown".to_string());
  let main_executable_path = json_string(&info, "CFBundleExecutable")
    .map(|value| app_path.join("Contents/MacOS").join(value));
  let url_schemes = info
    .get("CFBundleURLTypes")
    .and_then(Value::as_array)
    .map(|entries| {
      entries
        .iter()
        .filter_map(|entry| entry.get("CFBundleURLSchemes"))
        .filter_map(Value::as_array)
        .flat_map(|schemes| schemes.iter())
        .filter_map(Value::as_str)
        .map(ToString::to_string)
        .collect::<Vec<_>>()
    })
    .unwrap_or_default();
  let apple_script_addressable = run_command_capture(
    "osascript",
    &[
      "-e",
      &format!("tell application id \"{escaped_bundle_id}\" to get name"),
    ],
  )
  .is_ok();

  Ok(AppIdentity {
    bundle_id: bundle_id.to_string(),
    app_name,
    app_path,
    main_executable_path,
    version,
    build_version,
    url_schemes,
    apple_script_addressable,
  })
}

fn default_probe_output_dir(project_root: &Path, bundle_id: &str) -> PathBuf {
  project_root.join(".auv").join("app-probes").join(format!(
    "{}-{}",
    sanitized_artifact_name(bundle_id),
    now_millis()
  ))
}

fn invoke_probe_step(
  project_root: &Path,
  runtime: &Runtime,
  step_id: &str,
  command_id: &str,
  target_application_id: Option<String>,
  inputs: BTreeMap<String, String>,
) -> AuvResult<AppProbeStep> {
  let request = InvokeRequest {
    command_id: command_id.to_string(),
    target: ExecutionTarget {
      application_id: target_application_id.clone(),
    },
    inputs: inputs.clone(),
  };
  let result = runtime.invoke(request)?;
  if result.status != RunStatus::Completed {
    return Err(format!(
      "probe step {} ({}) failed: {}",
      step_id,
      command_id,
      result
        .failure_message
        .clone()
        .unwrap_or_else(|| result.output_summary.clone())
    ));
  }
  Ok(AppProbeStep {
    id: step_id.to_string(),
    command_id: command_id.to_string(),
    target_application_id,
    inputs,
    inspect_path: project_root
      .join(".auv")
      .join("runs")
      .join(&result.run_id)
      .join("inspect.txt"),
    run_id: result.run_id,
    status: result.status.as_str().to_string(),
    output_summary: result.output_summary,
    artifact_paths: result.artifact_paths,
    failure_message: result.failure_message,
  })
}

fn resolve_probe_path(query: &Path) -> AuvResult<PathBuf> {
  if query.is_file() {
    return Ok(query.to_path_buf());
  }
  if query.is_dir() {
    let candidate = query.join("probe.json");
    if candidate.exists() {
      return Ok(candidate);
    }
    return Err(format!(
      "probe directory {} does not contain probe.json",
      query.display()
    ));
  }
  Err(format!("probe path does not exist: {}", query.display()))
}

fn resolve_analysis_path(query: &Path) -> AuvResult<PathBuf> {
  if query.is_file() {
    return Ok(query.to_path_buf());
  }
  if query.is_dir() {
    let candidate = query.join("analysis.json");
    if candidate.exists() {
      return Ok(candidate);
    }
    return Err(format!(
      "analysis directory {} does not contain analysis.json",
      query.display()
    ));
  }
  Err(format!("analysis path does not exist: {}", query.display()))
}

fn resolve_distillation_path(query: &Path) -> AuvResult<PathBuf> {
  if query.is_file() {
    return Ok(query.to_path_buf());
  }
  if query.is_dir() {
    let candidate = query.join("distillation.json");
    if candidate.exists() {
      return Ok(candidate);
    }
    return Err(format!(
      "distillation directory {} does not contain distillation.json",
      query.display()
    ));
  }
  Err(format!(
    "distillation path does not exist: {}",
    query.display()
  ))
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> AuvResult<T> {
  let raw = fs::read_to_string(path)
    .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  serde_json::from_str(&raw).map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn write_pretty_json<T: Serialize>(path: &Path, value: &T) -> AuvResult<()> {
  let rendered = serde_json::to_string_pretty(value).map_err(|error| {
    format!(
      "failed to serialize JSON output {}: {error}",
      path.display()
    )
  })?;
  fs::write(path, rendered).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn run_command_capture(binary: &str, args: &[&str]) -> AuvResult<String> {
  let output = Command::new(binary)
    .args(args)
    .output()
    .map_err(|error| format!("failed to launch {}: {error}", binary))?;
  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    return Err(format!(
      "{} {:?} exited with status {}{}{}",
      binary,
      args,
      output.status,
      if stderr.is_empty() { "" } else { "; stderr=" },
      if stderr.is_empty() {
        stdout.as_str()
      } else {
        stderr.as_str()
      }
    ));
  }
  String::from_utf8(output.stdout)
    .map(|value| value.trim().to_string())
    .map_err(|error| format!("{} produced non-utf8 stdout: {error}", binary))
}

fn json_string(value: &Value, key: &str) -> Option<String> {
  value
    .get(key)
    .and_then(Value::as_str)
    .map(ToString::to_string)
}

fn first_non_empty_string(values: &[Option<String>]) -> Option<String> {
  values.iter().find_map(|value| {
    let value = value.as_deref()?.trim();
    if value.is_empty() {
      None
    } else {
      Some(value.to_string())
    }
  })
}

fn default_distill_output_dir(analysis_path: &Path, analysis: &AppAnalysis) -> PathBuf {
  let base = analysis_path.parent().unwrap_or_else(|| Path::new("."));
  base.join("distill").join(format!(
    "{}-{}",
    recipe_app_slug(&analysis.app_identity),
    now_millis()
  ))
}

fn recipe_app_slug(app: &AppIdentity) -> String {
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

fn candidate_slug(taxonomy_id: &str) -> String {
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

fn render_candidate_recipe(
  analysis: &AppAnalysis,
  strategy: &AppRecommendedStrategy,
) -> AuvResult<Value> {
  match strategy.taxonomy_id.as_str() {
    "search-entry.ax-text-input.clipboard-submit.capture-screen-evidence" => {
      Ok(render_search_entry_candidate_recipe(analysis))
    }
    "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text" => {
      Ok(render_native_text_candidate_recipe(analysis))
    }
    "result-selection.ocr-anchor.pointer-click.capture-screen-evidence" => {
      Ok(render_result_selection_candidate_recipe(analysis))
    }
    other => Err(format!(
      "no candidate distillation template exists yet for strategy taxonomy {}",
      other
    )),
  }
}

fn render_candidate_case_matrix(
  analysis: &AppAnalysis,
  strategy: &AppRecommendedStrategy,
) -> AuvResult<Value> {
  match strategy.taxonomy_id.as_str() {
    "search-entry.ax-text-input.clipboard-submit.capture-screen-evidence" => {
      Ok(render_search_entry_candidate_cases(analysis))
    }
    "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text" => {
      Ok(render_native_text_candidate_cases(analysis))
    }
    "result-selection.ocr-anchor.pointer-click.capture-screen-evidence" => {
      Ok(render_result_selection_candidate_cases(analysis))
    }
    other => Err(format!(
      "no candidate case-matrix distillation template exists yet for strategy taxonomy {}",
      other
    )),
  }
}

fn render_search_entry_candidate_recipe(analysis: &AppAnalysis) -> Value {
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
      "verificationContract": "captureScreenEvidence"
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
        "command_id": "debug.captureScreen",
        "disturbance": { "classes": ["none"], "max": "none" },
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

fn render_native_text_candidate_recipe(analysis: &AppAnalysis) -> Value {
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
        "expect": { "output_must_contain": ["${target_text}"], "artifact_count_at_least": 1 },
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

fn render_result_selection_candidate_recipe(analysis: &AppAnalysis) -> Value {
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
      "verificationContract": "captureScreenEvidence"
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
        "command_id": "debug.captureScreen",
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

fn render_search_entry_candidate_cases(analysis: &AppAnalysis) -> Value {
  json!({
    "skill_id": format!("macos.{}.search_entry_candidate.v0", recipe_app_slug(&analysis.app_identity)),
    "version": "0.1.0",
    "status": "candidate-case-matrix",
    "cases": [
      {
        "case_id": "default-candidate",
        "status": "candidate",
        "inputs": {
          "focus_query": "TODO_FOCUS_QUERY",
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

fn render_native_text_candidate_cases(analysis: &AppAnalysis) -> Value {
  json!({
    "skill_id": format!("macos.{}.native_text_candidate.v0", recipe_app_slug(&analysis.app_identity)),
    "version": "0.1.0",
    "status": "candidate-case-matrix",
    "cases": [
      {
        "case_id": "default-candidate",
        "status": "candidate",
        "inputs": {
          "focus_query": "TODO_TEXT_SURFACE_QUERY"
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

fn render_result_selection_candidate_cases(analysis: &AppAnalysis) -> Value {
  json!({
    "skill_id": format!("macos.{}.result_selection_candidate.v0", recipe_app_slug(&analysis.app_identity)),
    "version": "0.1.0",
    "status": "candidate-case-matrix",
    "cases": [
      {
        "case_id": "default-candidate",
        "status": "candidate",
        "inputs": {
          "anchor_text": "TODO_VISIBLE_ANCHOR_TEXT"
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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn parse_probe_directory_resolves_probe_json() {
    let root = temp_dir("app-probe-resolve");
    fs::write(root.join("probe.json"), "{}").expect("probe.json should be writable");
    let resolved = resolve_probe_path(&root).expect("directory should resolve");
    assert_eq!(resolved, root.join("probe.json"));
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn parse_distillation_directory_resolves_distillation_json() {
    let root = temp_dir("app-distill-resolve");
    fs::write(root.join("distillation.json"), "{}").expect("distillation.json should be writable");
    let resolved = resolve_distillation_path(&root).expect("directory should resolve");
    assert_eq!(resolved, root.join("distillation.json"));
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn recommended_strategy_uses_stable_taxonomy_id() {
    let strategy = recommended_strategy(
      "native-text",
      "ax-text",
      "pointer-focus-clipboard-paste",
      "verifyAxText",
      AssessmentStatus::Candidate,
      "test rationale",
    )
    .expect("taxonomy should be valid");
    assert_eq!(
      strategy.taxonomy_id,
      "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text"
    );
  }

  #[test]
  fn report_renders_expected_sections() {
    let analysis = AppAnalysis {
      analysis_version: APP_ANALYSIS_VERSION.to_string(),
      created_at_millis: 0,
      probe_path: PathBuf::from("/tmp/probe.json"),
      app_identity: AppIdentity {
        bundle_id: "com.example.App".to_string(),
        app_name: "Example".to_string(),
        app_path: PathBuf::from("/Applications/Example.app"),
        main_executable_path: None,
        version: "1.0".to_string(),
        build_version: "100".to_string(),
        url_schemes: vec!["example".to_string()],
        apple_script_addressable: true,
      },
      window_context: AppWindowContext {
        observed_window_count: 1,
        observed_at: "2026-05-18T00:00:00Z".to_string(),
        frontmost_app_name: "Example".to_string(),
        frontmost_window_title: "Example".to_string(),
        primary_window_title: "Example".to_string(),
        primary_window_bounds: Some(AppRect {
          x: 0,
          y: 0,
          width: 100,
          height: 100,
        }),
        primary_window_display_scale: Some(2.0),
      },
      permissions: AppPermissionState {
        screen_recording: "granted".to_string(),
        accessibility: "granted".to_string(),
        automation_to_system_events: "granted".to_string(),
        launch_host_process: "Atlas".to_string(),
      },
      available_surfaces: AppAvailableSurfaces {
        accessibility_tree: AssessmentStatus::Available,
        menu_surface: AssessmentStatus::Unknown,
        shortcut_surface: AssessmentStatus::Candidate,
        apple_script_surface: AssessmentStatus::Available,
        url_scheme_surface: AssessmentStatus::Available,
        keyboard_first_surface: AssessmentStatus::Candidate,
        pointer_fallback_surface: AssessmentStatus::Likely,
      },
      grounding_assessment: AppGroundingAssessment {
        ocr_sample_query: "Example".to_string(),
        ocr_sample_status: AssessmentStatus::Candidate,
        ocr_sample_match_count: 2,
        stable_anchor_candidates: vec!["appName: Example".to_string()],
        stable_region_candidates: vec!["primaryWindow=0,0,100,100".to_string()],
        overlay_debug_artifacts_recommended: false,
      },
      control_assessment: AppControlAssessment {
        preferred_path: "non-pointer first".to_string(),
        non_pointer_path: AssessmentStatus::Candidate,
        keyboard_path: AssessmentStatus::Candidate,
        pointer_fallback: AssessmentStatus::Likely,
        notes: vec!["test note".to_string()],
      },
      verification_assessment: AppVerificationAssessment {
        ax_verify: AssessmentStatus::Candidate,
        image_verify: AssessmentStatus::Candidate,
        ui_state_verify: AssessmentStatus::Candidate,
        semantic_success: AssessmentStatus::Unknown,
        notes: vec!["verification note".to_string()],
      },
      disturbance_profile: AppDisturbanceProfile {
        observation: vec!["none".to_string()],
        non_pointer_control: vec!["keyboard".to_string()],
        pointer_fallback: vec!["pointer".to_string()],
      },
      known_boundaries: vec!["one boundary".to_string()],
      recommended_strategies: vec![
        recommended_strategy(
          "search-entry",
          "ax-text-input",
          "clipboard-submit",
          "captureScreenEvidence",
          AssessmentStatus::Candidate,
          "test rationale",
        )
        .expect("strategy should render"),
      ],
    };

    let report = render_app_analysis_report(&analysis);
    assert!(report.contains("## 1. App Basic Information"));
    assert!(report.contains("## 2. Available Surfaces"));
    assert!(report.contains("## 3. Grounding Assessment"));
    assert!(report.contains("## 4. Control Strategy"));
    assert!(report.contains("## 5. Verification Assessment"));
    assert!(report.contains("Recommended Candidate Strategies"));
  }

  #[test]
  fn search_entry_distillation_template_validates() {
    let analysis = sample_analysis_with_strategy(
      "search-entry.ax-text-input.clipboard-submit.capture-screen-evidence",
    );
    let recipe = render_search_entry_candidate_recipe(&analysis);
    let manifest: SkillManifest =
      serde_json::from_value(recipe).expect("candidate recipe should parse");
    validate_skill_manifest(&manifest).expect("candidate recipe should validate");
    let matrix_value = render_search_entry_candidate_cases(&analysis);
    let matrix: SkillCaseMatrix =
      serde_json::from_value(matrix_value).expect("candidate matrix should parse");
    validate_case_matrix_manifest(&matrix).expect("candidate matrix should validate");
    validate_case_matrix_against_skill(&manifest, &matrix).expect("candidate matrix should align");
  }

  #[test]
  fn native_text_distillation_template_validates() {
    let analysis = sample_analysis_with_strategy(
      "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text",
    );
    let recipe = render_native_text_candidate_recipe(&analysis);
    let manifest: SkillManifest =
      serde_json::from_value(recipe).expect("candidate recipe should parse");
    validate_skill_manifest(&manifest).expect("candidate recipe should validate");
    let matrix_value = render_native_text_candidate_cases(&analysis);
    let matrix: SkillCaseMatrix =
      serde_json::from_value(matrix_value).expect("candidate matrix should parse");
    validate_case_matrix_manifest(&matrix).expect("candidate matrix should validate");
    validate_case_matrix_against_skill(&manifest, &matrix).expect("candidate matrix should align");
  }

  #[test]
  fn apply_candidate_grounding_marks_unresolved_search_entry_without_search_signal() {
    let analysis = sample_analysis_with_strategy(
      "search-entry.ax-text-input.clipboard-submit.capture-screen-evidence",
    );
    let mut matrix: SkillCaseMatrix =
      serde_json::from_value(render_search_entry_candidate_cases(&analysis))
        .expect("candidate matrix should parse");
    let mut resolved = BTreeMap::new();
    let unresolved = apply_candidate_grounding(
      &analysis,
      None,
      "search-entry.ax-text-input.clipboard-submit.capture-screen-evidence",
      &mut matrix,
      &mut resolved,
    );
    assert!(unresolved.iter().any(|key| key == "focus_query"));
    assert!(resolved.get("query").is_some());
  }

  fn sample_analysis_with_strategy(taxonomy_id: &str) -> AppAnalysis {
    AppAnalysis {
      analysis_version: APP_ANALYSIS_VERSION.to_string(),
      created_at_millis: 0,
      probe_path: PathBuf::from("/tmp/probe.json"),
      app_identity: AppIdentity {
        bundle_id: "com.example.App".to_string(),
        app_name: "Example".to_string(),
        app_path: PathBuf::from("/Applications/Example.app"),
        main_executable_path: None,
        version: "1.0".to_string(),
        build_version: "100".to_string(),
        url_schemes: vec![],
        apple_script_addressable: false,
      },
      window_context: AppWindowContext {
        observed_window_count: 1,
        observed_at: "2026-05-18T00:00:00Z".to_string(),
        frontmost_app_name: "Example".to_string(),
        frontmost_window_title: "Example".to_string(),
        primary_window_title: "Example".to_string(),
        primary_window_bounds: None,
        primary_window_display_scale: None,
      },
      permissions: AppPermissionState {
        screen_recording: "granted".to_string(),
        accessibility: "granted".to_string(),
        automation_to_system_events: "granted".to_string(),
        launch_host_process: "Atlas".to_string(),
      },
      available_surfaces: AppAvailableSurfaces {
        accessibility_tree: AssessmentStatus::Candidate,
        menu_surface: AssessmentStatus::Unknown,
        shortcut_surface: AssessmentStatus::Candidate,
        apple_script_surface: AssessmentStatus::Unavailable,
        url_scheme_surface: AssessmentStatus::Unavailable,
        keyboard_first_surface: AssessmentStatus::Candidate,
        pointer_fallback_surface: AssessmentStatus::Likely,
      },
      grounding_assessment: AppGroundingAssessment {
        ocr_sample_query: "Example".to_string(),
        ocr_sample_status: AssessmentStatus::Candidate,
        ocr_sample_match_count: 1,
        stable_anchor_candidates: vec![],
        stable_region_candidates: vec![],
        overlay_debug_artifacts_recommended: false,
      },
      control_assessment: AppControlAssessment {
        preferred_path: "non-pointer first".to_string(),
        non_pointer_path: AssessmentStatus::Candidate,
        keyboard_path: AssessmentStatus::Candidate,
        pointer_fallback: AssessmentStatus::Likely,
        notes: vec![],
      },
      verification_assessment: AppVerificationAssessment {
        ax_verify: AssessmentStatus::Candidate,
        image_verify: AssessmentStatus::Candidate,
        ui_state_verify: AssessmentStatus::Candidate,
        semantic_success: AssessmentStatus::Unknown,
        notes: vec![],
      },
      disturbance_profile: AppDisturbanceProfile {
        observation: vec!["none".to_string()],
        non_pointer_control: vec!["keyboard".to_string()],
        pointer_fallback: vec!["pointer".to_string()],
      },
      known_boundaries: vec![],
      recommended_strategies: vec![AppRecommendedStrategy {
        taxonomy_id: taxonomy_id.to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
      }],
    }
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }
}
