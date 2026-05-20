use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::driver::{
  ObservedAxTreeSnapshot, ObservedDisplay, ObservedDisplaySnapshot, ObservedOcrRow, ObservedRect,
  ObservedWindow, OcrTextSnapshot, compute_combined_bounds, group_ocr_matches_into_rows,
  parse_observed_ax_tree, parse_ocr_text_snapshot, parse_window_line, report_value,
  sanitized_artifact_name,
};
use crate::model::{AuvResult, ExecutionTarget, InvokeRequest, RunStatus, now_millis};
use crate::recording::{RecordingRun, RunFinish, RunSpec, SpanFinish, SpanRef};
use crate::runtime::Runtime;
use crate::skill::{
  SkillCaseMatrix, SkillCaseRunOptions, SkillManifest, SkillStrategy,
  run_skill_case_matrix_into_run, validate_case_matrix_against_skill,
  validate_case_matrix_manifest, validate_skill_manifest,
};
use crate::trace::{
  RunType, SPAN_API_VERSION, SpanRecordV1Alpha1, TraceState, TraceStatusCode, new_span_id,
  string_attr,
};

const APP_PROBE_VERSION: &str = "v0";
const APP_ANALYSIS_VERSION: &str = "v0";
const APP_DISTILL_VERSION: &str = "v0";
const APP_VALIDATE_VERSION: &str = "v0";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppIdentity {
  pub bundle_id: String,
  pub app_name: String,
  pub app_path: Option<PathBuf>,
  pub main_executable_path: Option<PathBuf>,
  pub version: String,
  pub build_version: String,
  pub url_schemes: Vec<String>,
  pub apple_script_addressable: bool,
  pub launch_services_resolved: bool,
  pub resolution_notes: Vec<String>,
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
  pub suggested_annotation_ids: Vec<String>,
  #[serde(default)]
  pub candidate_shape: AppDistilledCandidateShape,
  pub recipe_path: PathBuf,
  pub case_matrix_path: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppValidatedCandidate {
  pub recipe_id: String,
  pub taxonomy_id: String,
  pub status: AppValidationStatus,
  #[serde(default)]
  pub verification_mode: AppVerificationMode,
  pub rationale: String,
  pub used_annotation_ids: Vec<String>,
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
  pub annotation_candidates: Vec<AppSurfaceCandidate>,
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

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AppVerificationMode {
  EvidenceOnly,
  #[default]
  MachineAsserted,
}

impl AppVerificationMode {
  fn as_str(&self) -> &'static str {
    match self {
      Self::EvidenceOnly => "evidence-only",
      Self::MachineAsserted => "machine-asserted",
    }
  }

  fn manual_review_required(&self) -> bool {
    matches!(self, Self::EvidenceOnly)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppCandidateGroundingTaxonomy {
  SearchEntryAxTextInputClipboardSubmitCaptureEvidence,
  NativeTextAxTextPointerFocusClipboardPasteVerifyAxText,
  ResultSelectionOcrAnchorPointerClickCaptureEvidence,
  WindowActionWindowPointPointerClickCaptureEvidence,
}

impl AppCandidateGroundingTaxonomy {
  fn parse(raw: &str) -> AuvResult<Self> {
    match raw.trim() {
      "search-entry.ax-text-input.clipboard-submit.capture-evidence" => {
        Ok(Self::SearchEntryAxTextInputClipboardSubmitCaptureEvidence)
      }
      "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text" => {
        Ok(Self::NativeTextAxTextPointerFocusClipboardPasteVerifyAxText)
      }
      "result-selection.ocr-anchor.pointer-click.capture-evidence" => {
        Ok(Self::ResultSelectionOcrAnchorPointerClickCaptureEvidence)
      }
      "window-action.window-point.pointer-click.capture-evidence" => {
        Ok(Self::WindowActionWindowPointPointerClickCaptureEvidence)
      }
      other => Err(format!(
        "unsupported candidate grounding taxonomy {}. allowed values: {}",
        other,
        Self::allowed_ids().join(", ")
      )),
    }
  }

  fn allowed_ids() -> &'static [&'static str] {
    &[
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
      "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text",
      "result-selection.ocr-anchor.pointer-click.capture-evidence",
      "window-action.window-point.pointer-click.capture-evidence",
    ]
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

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppPoint {
  pub x: i64,
  pub y: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppSurfaceCandidate {
  pub candidate_id: String,
  pub area: String,
  pub kind: String,
  pub source: String,
  pub status: AssessmentStatus,
  pub primary_text: String,
  #[serde(default)]
  pub secondary_text: String,
  #[serde(default)]
  pub query_value: String,
  #[serde(default)]
  pub coordinate_space: String,
  pub bounds: Option<AppRect>,
  pub click_point: Option<AppPoint>,
  pub confidence: Option<f64>,
  pub evidence_step_id: String,
  #[serde(default)]
  pub input_bindings: BTreeMap<String, String>,
  #[serde(default)]
  pub compatibility: AppCandidateCompatibility,
  #[serde(default)]
  pub notes: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppCandidateCompatibility {
  #[serde(default)]
  pub direct_taxonomy_ids: Vec<String>,
  #[serde(default)]
  pub context_taxonomy_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppDistilledCandidateShape {
  #[serde(default)]
  pub direct_candidate_ids: Vec<String>,
  #[serde(default)]
  pub context_candidate_ids: Vec<String>,
  #[serde(default)]
  pub provided_inputs: BTreeMap<String, String>,
  #[serde(default)]
  pub notes: Vec<String>,
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

  fn center_point(&self) -> AppPoint {
    let (x, y) = self.center();
    AppPoint { x, y }
  }

  fn render_compact(&self) -> String {
    format!("{},{},{},{}", self.x, self.y, self.width, self.height)
  }

  fn relative_point(&self, point: &AppPoint) -> Option<(f64, f64)> {
    if self.width <= 0 || self.height <= 0 {
      return None;
    }
    let relative_x = (point.x - self.x) as f64 / self.width as f64;
    let relative_y = (point.y - self.y) as f64 / self.height as f64;
    Some((relative_x, relative_y))
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

  let mut run = runtime.start_run(RunSpec::new(RunType::Probe, "auv.probe"))?;
  let root_span = run.root_span();
  let result = probe_app_into_run(
    project_root,
    runtime,
    &app,
    &output_dir,
    &mut run,
    &root_span,
  );
  match result {
    Ok(probe) => {
      runtime.finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some(format!("Probed app {}", probe.app.bundle_id)),
          failure: None,
        },
      )?;
      Ok(probe)
    }
    Err(error) => {
      finish_failed_app_run(runtime, run, error, format!("App probe {bundle_id} failed"))
    }
  }
}

fn probe_app_into_run(
  project_root: &Path,
  runtime: &Runtime,
  app: &AppIdentity,
  output_dir: &Path,
  run: &mut RecordingRun,
  parent: &SpanRef,
) -> AuvResult<AppProbe> {
  let mut steps = Vec::new();
  steps.push(invoke_probe_step(
    runtime,
    run,
    parent,
    "probe-permissions",
    "debug.probePermissions",
    None,
    BTreeMap::new(),
    false,
  )?);
  steps.push(invoke_probe_step(
    runtime,
    run,
    parent,
    "list-displays",
    "debug.listDisplays",
    None,
    BTreeMap::new(),
    false,
  )?);
  steps.push(invoke_probe_step(
    runtime,
    run,
    parent,
    "probe-coordinate-readiness",
    "debug.probeCoordinateReadiness",
    None,
    BTreeMap::new(),
    false,
  )?);
  let mut activate_inputs = BTreeMap::new();
  activate_inputs.insert("settle_ms".to_string(), "250".to_string());
  steps.push(invoke_probe_step(
    runtime,
    run,
    parent,
    "activate-target-app",
    "debug.activateApp",
    Some(app.bundle_id.clone()),
    activate_inputs,
    true,
  )?);

  let mut window_inputs = BTreeMap::new();
  window_inputs.insert("limit".to_string(), "20".to_string());
  steps.push(invoke_probe_step(
    runtime,
    run,
    parent,
    "observe-windows",
    "debug.observeWindows",
    Some(app.bundle_id.clone()),
    window_inputs,
    true,
  )?);

  let mut tree_inputs = BTreeMap::new();
  tree_inputs.insert("max_depth".to_string(), "6".to_string());
  tree_inputs.insert("max_children".to_string(), "24".to_string());
  steps.push(invoke_probe_step(
    runtime,
    run,
    parent,
    "observe-window-tree",
    "debug.observeWindowTree",
    Some(app.bundle_id.clone()),
    tree_inputs,
    true,
  )?);

  let capture_label = format!("app-probe-{}", sanitized_artifact_name(&app.bundle_id));
  let mut capture_inputs = BTreeMap::new();
  capture_inputs.insert("label".to_string(), capture_label);
  capture_inputs.insert(
    "activate_target_before_capture".to_string(),
    "true".to_string(),
  );
  let capture_step = invoke_probe_step(
    runtime,
    run,
    parent,
    "capture-display",
    "debug.captureDisplay",
    Some(app.bundle_id.clone()),
    capture_inputs,
    true,
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
    .cloned();
  steps.push(capture_step);

  if let Some(screenshot_artifact_path) = screenshot_artifact_path {
    let ocr_sample_query = resolve_probe_ocr_sample_query(app, &steps);
    let mut ocr_inputs = BTreeMap::new();
    ocr_inputs.insert(
      "image_path".to_string(),
      screenshot_artifact_path.display().to_string(),
    );
    ocr_inputs.insert("query".to_string(), ocr_sample_query);
    ocr_inputs.insert("min_confidence".to_string(), "0.55".to_string());
    steps.push(invoke_probe_step(
      runtime,
      run,
      parent,
      "ocr-sample",
      "debug.findImageText",
      None,
      ocr_inputs,
      true,
    )?);
  }

  let probe = AppProbe {
    probe_version: APP_PROBE_VERSION.to_string(),
    created_at_millis: now_millis(),
    project_root: project_root.to_path_buf(),
    output_dir: output_dir.to_path_buf(),
    app: app.clone(),
    steps,
  };
  let probe_path = output_dir.join("probe.json");
  write_pretty_json(&probe_path, &probe)?;
  stage_app_artifact(
    runtime,
    run,
    parent,
    "probe.output",
    &probe_path,
    "probe.json",
  )?;
  Ok(probe)
}

pub fn analyze_app_probe(runtime: &Runtime, query: &Path) -> AuvResult<AppAnalyzeOutput> {
  let mut run = runtime.start_run(RunSpec::new(RunType::Analyze, "auv.analyze"))?;
  let root_span = run.root_span();
  let result = analyze_app_probe_into_run(runtime, &mut run, &root_span, query);
  match result {
    Ok(output) => {
      runtime.finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some(format!(
            "Analyzed app {}",
            output.analysis.app_identity.bundle_id
          )),
          failure: None,
        },
      )?;
      Ok(output)
    }
    Err(error) => finish_failed_app_run(runtime, run, error, "App analysis failed".to_string()),
  }
}

fn analyze_app_probe_into_run(
  runtime: &Runtime,
  run: &mut RecordingRun,
  span: &SpanRef,
  query: &Path,
) -> AuvResult<AppAnalyzeOutput> {
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
  stage_app_artifact(
    runtime,
    run,
    span,
    "analysis.output",
    &analysis_path,
    "analysis.json",
  )?;
  stage_app_artifact(
    runtime,
    run,
    span,
    "analysis.report",
    &report_path,
    "analysis-report.md",
  )?;
  Ok(AppAnalyzeOutput {
    analysis,
    analysis_path,
    report_path,
  })
}

pub fn distill_app_analysis(
  runtime: &Runtime,
  query: &Path,
  output_dir: Option<PathBuf>,
) -> AuvResult<AppDistillOutput> {
  let mut run = runtime.start_run(RunSpec::new(RunType::Distill, "auv.distill"))?;
  let root_span = run.root_span();
  let result = distill_app_analysis_into_run(runtime, &mut run, &root_span, query, output_dir);
  match result {
    Ok(output) => {
      runtime.finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some(format!(
            "Distilled app {}",
            output.distillation.app_identity.bundle_id
          )),
          failure: None,
        },
      )?;
      Ok(output)
    }
    Err(error) => finish_failed_app_run(runtime, run, error, "App distillation failed".to_string()),
  }
}

fn distill_app_analysis_into_run(
  runtime: &Runtime,
  run: &mut RecordingRun,
  span: &SpanRef,
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
    let candidate_shape = build_distilled_candidate_shape(&analysis, &strategy.taxonomy_id);
    let recipe_value = render_candidate_recipe(&analysis, strategy, &candidate_shape)?;
    let matrix_value = render_candidate_case_matrix(&analysis, strategy, &candidate_shape)?;
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
    stage_app_artifact(
      runtime,
      run,
      span,
      "distillation.candidate.recipe",
      &recipe_path,
      &format!("{candidate_slug}.recipe.json"),
    )?;
    stage_app_artifact(
      runtime,
      run,
      span,
      "distillation.candidate.case_matrix",
      &case_matrix_path,
      &format!("{candidate_slug}.cases.json"),
    )?;
    candidates.push(AppDistilledCandidate {
      recipe_id: manifest.recipe_id.clone(),
      taxonomy_id: strategy.taxonomy_id.clone(),
      status: strategy.status,
      rationale: strategy.rationale.clone(),
      suggested_annotation_ids: suggested_annotation_ids_for_candidate_shape(&candidate_shape),
      candidate_shape,
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
  stage_app_artifact(
    runtime,
    run,
    span,
    "distillation.output",
    &distillation_path,
    "distillation.json",
  )?;
  stage_app_artifact(
    runtime,
    run,
    span,
    "distillation.report",
    &report_path,
    "distillation-report.md",
  )?;

  Ok(AppDistillOutput {
    distillation,
    distillation_path,
    report_path,
  })
}

pub fn validate_app_distillation(runtime: &Runtime, query: &Path) -> AuvResult<AppValidateOutput> {
  let mut run = runtime.start_run(RunSpec::new(RunType::Validate, "auv.validate"))?;
  let root_span = run.root_span();
  let result = validate_app_distillation_into_run(runtime, &mut run, &root_span, query);
  match result {
    Ok(output) => {
      runtime.finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some(format!(
            "Validated app {}",
            output.validation.app_identity.bundle_id
          )),
          failure: None,
        },
      )?;
      Ok(output)
    }
    Err(error) => finish_failed_app_run(runtime, run, error, "App validation failed".to_string()),
  }
}

fn validate_app_distillation_into_run(
  runtime: &Runtime,
  run: &mut RecordingRun,
  span: &SpanRef,
  query: &Path,
) -> AuvResult<AppValidateOutput> {
  let distillation_path = resolve_distillation_path(query)?;
  let distillation: AppDistillation = read_json(&distillation_path)?;
  let analysis: AppAnalysis = read_json(&distillation.source_analysis_path)?;
  let probe = read_json::<AppProbe>(&analysis.probe_path).ok();
  let ax_snapshot = probe
    .as_ref()
    .and_then(|probe| parse_ax_snapshot(probe).ok());

  let mut candidates = Vec::new();
  let mut unresolved_candidate_failures = Vec::new();
  for candidate in &distillation.candidates {
    let manifest: SkillManifest = read_json(&candidate.recipe_path)?;
    let mut matrix: SkillCaseMatrix = read_json(&candidate.case_matrix_path)?;
    let verification_mode =
      verification_mode_for_strategy(&manifest.strategy).map_err(|error| {
        format!(
          "candidate {} uses an unsupported verification contract: {error}",
          candidate.recipe_id
        )
      })?;
    let mut resolved_inputs = BTreeMap::new();
    let mut used_annotation_ids = if candidate.candidate_shape.provided_inputs.is_empty() {
      Vec::new()
    } else {
      candidate.candidate_shape.direct_candidate_ids.clone()
    };
    apply_distilled_candidate_shape_inputs(
      &candidate.candidate_shape,
      &mut matrix,
      &mut resolved_inputs,
    );
    let (unresolved_inputs, grounded_annotation_ids) = apply_candidate_grounding(
      &analysis,
      ax_snapshot.as_ref(),
      &candidate.taxonomy_id,
      &mut matrix,
      &mut resolved_inputs,
    )
    .map_err(|error| {
      format!(
        "candidate {} uses an unsupported grounding taxonomy: {error}",
        candidate.recipe_id
      )
    })?;
    for candidate_id in grounded_annotation_ids {
      if !used_annotation_ids
        .iter()
        .any(|existing| existing == &candidate_id)
      {
        used_annotation_ids.push(candidate_id);
      }
    }
    let selected_case_count = matrix.cases.len();

    let validated = if unresolved_inputs.is_empty() {
      let candidate_span = run.start_span(
        span,
        app_span_record(
          "auv.app.validate.candidate",
          BTreeMap::from([(
            "auv.recipe.id".to_string(),
            string_attr(candidate.recipe_id.clone()),
          )]),
        ),
      )?;
      let case_matrix_result = run_skill_case_matrix_into_run(
        runtime,
        run,
        &candidate_span,
        &manifest,
        &matrix,
        SkillCaseRunOptions {
          dry_run: false,
          max_disturbance: None,
          only_case_ids: Vec::new(),
          include_nonvalidated: true,
        },
      );
      match case_matrix_result {
        Ok(_) => {
          run.finish_span(
            &candidate_span,
            SpanFinish {
              status_code: TraceStatusCode::Ok,
              summary: Some(format!("Validated candidate {}", candidate.recipe_id)),
              failure: None,
            },
          )?;
          AppValidatedCandidate {
            recipe_id: candidate.recipe_id.clone(),
            taxonomy_id: candidate.taxonomy_id.clone(),
            status: AppValidationStatus::Validated,
            verification_mode,
            rationale: validated_candidate_rationale(selected_case_count, verification_mode),
            used_annotation_ids: used_annotation_ids.clone(),
            recipe_path: candidate.recipe_path.clone(),
            case_matrix_path: candidate.case_matrix_path.clone(),
            selected_case_count,
            unresolved_inputs,
            failure_message: None,
            resolved_inputs,
          }
        }
        Err(error) => {
          run.finish_span(
            &candidate_span,
            SpanFinish {
              status_code: TraceStatusCode::Error,
              summary: Some(format!(
                "Candidate {} failed validation",
                candidate.recipe_id
              )),
              failure: Some(error.clone()),
            },
          )?;
          AppValidatedCandidate {
            recipe_id: candidate.recipe_id.clone(),
            taxonomy_id: candidate.taxonomy_id.clone(),
            status: AppValidationStatus::Rejected,
            verification_mode,
            rationale: "The candidate was runnable, but live execution failed.".to_string(),
            used_annotation_ids: used_annotation_ids.clone(),
            recipe_path: candidate.recipe_path.clone(),
            case_matrix_path: candidate.case_matrix_path.clone(),
            selected_case_count,
            unresolved_inputs,
            failure_message: Some(error),
            resolved_inputs,
          }
        }
      }
    } else {
      let unresolved_summary = format!(
        "Validation could not execute {} because grounding left unresolved inputs: {}.",
        candidate.recipe_id,
        unresolved_inputs.join(", ")
      );
      unresolved_candidate_failures.push(unresolved_summary.clone());
      AppValidatedCandidate {
        recipe_id: candidate.recipe_id.clone(),
        taxonomy_id: candidate.taxonomy_id.clone(),
        status: AppValidationStatus::Rejected,
        verification_mode,
        rationale: "Validation failed before execution because candidate grounding was incomplete."
          .to_string(),
        used_annotation_ids,
        recipe_path: candidate.recipe_path.clone(),
        case_matrix_path: candidate.case_matrix_path.clone(),
        selected_case_count,
        unresolved_inputs,
        failure_message: Some(unresolved_summary),
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
  stage_app_artifact(
    runtime,
    run,
    span,
    "validation.output",
    &validation_path,
    "validation.json",
  )?;
  stage_app_artifact(
    runtime,
    run,
    span,
    "validation.report",
    &report_path,
    "validation-report.md",
  )?;
  if !unresolved_candidate_failures.is_empty() {
    return Err(format!(
      "app validation failed because candidate grounding left unresolved inputs:\n- {}",
      unresolved_candidate_failures.join("\n- ")
    ));
  }
  Ok(AppValidateOutput {
    validation,
    validation_path,
    report_path,
  })
}

fn build_app_analysis(probe_path: &Path, probe: &AppProbe) -> AuvResult<AppAnalysis> {
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
  let annotation_candidates = build_annotation_candidates(
    &probe.app,
    primary_window,
    primary_window_bounds.as_ref(),
    &ax_snapshot,
    &ocr_snapshot,
    has_collection_like_surface(&ax_snapshot),
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
  if !ocr_snapshot.matches.is_empty() && has_collection_like_surface(&ax_snapshot) {
    recommended_strategies.push(recommended_strategy(
      "result-selection",
      "ocr-anchor",
      "pointer-click",
      "captureEvidence",
      AssessmentStatus::Candidate,
      "The sample OCR query produced filtered matches on the captured image, so OCR-anchor result selection is a candidate grounding strategy.",
    )?);
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

fn summarize_failed_probe_steps(probe: &AppProbe) -> Vec<String> {
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

fn apply_distilled_candidate_shape_inputs(
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

fn apply_candidate_grounding(
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

fn build_annotation_candidates(
  app: &AppIdentity,
  primary_window: Option<&ObservedWindow>,
  primary_window_bounds: Option<&AppRect>,
  ax_snapshot: &ObservedAxTreeSnapshot,
  ocr_snapshot: &OcrTextSnapshot,
  has_collection_surface: bool,
) -> Vec<AppSurfaceCandidate> {
  let mut candidates = Vec::new();

  if let Some(bounds) = primary_window_bounds.cloned() {
    let compact_bounds = bounds.render_compact();
    let click_point = bounds.center_point();
    let input_bindings = window_region_input_bindings(&compact_bounds, &click_point, &bounds);
    let evidence_step_id = if primary_window.is_some() {
      "observe-windows"
    } else {
      "observe-window-tree"
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
      "observe-window-tree",
      candidate_compatibility(
        &["search-entry.ax-text-input.clipboard-submit.capture-evidence"],
        &[],
      ),
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
      "observe-window-tree",
      candidate_compatibility(
        &["native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text"],
        &[],
      ),
      "AX-exposed editable text-surface candidate.",
    ));
  }

  for matched in ocr_snapshot.matches.iter().take(8) {
    let bounds = AppRect::from_observed(&matched.bounds);
    let area = if has_collection_surface && ocr_snapshot.matches.len() >= 2 {
      "result-selection"
    } else {
      "ocr-visible-text"
    };
    let mut notes =
      vec!["Visible OCR text candidate from the sampled screenshot artifact.".to_string()];
    if matched.text == ocr_snapshot.query {
      notes.push("This match equals the sampled OCR query.".to_string());
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
      input_bindings: BTreeMap::from([("anchor_text".to_string(), matched.text.clone())]),
      compatibility: if area == "result-selection" {
        candidate_compatibility(
          &["result-selection.ocr-anchor.pointer-click.capture-evidence"],
          &[],
        )
      } else {
        AppCandidateCompatibility::default()
      },
      notes,
    });
  }

  if has_collection_surface && ocr_snapshot.matches.len() >= 2 {
    let rows = group_ocr_rows_from_ocr_snapshot(ocr_snapshot);
    for row in rows.into_iter().take(8) {
      candidates.push(row_candidate(row));
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
    query_value,
    coordinate_space: "global-logical".to_string(),
    click_point: Some(bounds.center_point()),
    bounds: Some(bounds),
    confidence: None,
    evidence_step_id: evidence_step_id.to_string(),
    input_bindings: BTreeMap::from([("focus_query".to_string(), focus_query)]),
    compatibility,
    notes: vec![note.to_string()],
  }
}

fn group_ocr_rows_from_ocr_snapshot(snapshot: &OcrTextSnapshot) -> Vec<ObservedOcrRow> {
  let matches = snapshot.matches.iter().collect::<Vec<_>>();
  group_ocr_matches_into_rows(&matches)
}

fn row_candidate(row: ObservedOcrRow) -> AppSurfaceCandidate {
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

fn candidate_compatibility(
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

fn build_distilled_candidate_shape(
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

fn verification_mode_for_strategy(strategy: &SkillStrategy) -> AuvResult<AppVerificationMode> {
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

fn validated_candidate_rationale(
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

fn suggested_annotation_ids_for_candidate_shape(
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

fn resolve_probe_ocr_sample_query(app: &AppIdentity, steps: &[AppProbeStep]) -> String {
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
  let mut resolution_notes = Vec::new();
  let launch_services_path = match resolve_launch_services_app_path(&escaped_bundle_id) {
    Ok(path) => Some(path),
    Err(error) => {
      resolution_notes.push(format!(
        "LaunchServices could not resolve `{bundle_id}` to an application path: {error}"
      ));
      None
    }
  };
  let app_path =
    launch_services_path
      .clone()
      .or_else(|| match resolve_spotlight_app_path(bundle_id) {
        Ok(path) => Some(path),
        Err(error) => {
          resolution_notes.push(format!(
            "Spotlight could not resolve `{bundle_id}` to an installed app bundle: {error}"
          ));
          None
        }
      });
  let launch_services_resolved = launch_services_path.is_some();
  let info = app_path
    .as_ref()
    .map(|app_path| read_app_info_plist(app_path.as_path()))
    .transpose()?;
  let app_name = first_non_empty_string(&[
    info
      .as_ref()
      .and_then(|info| json_string(info, "CFBundleDisplayName")),
    info
      .as_ref()
      .and_then(|info| json_string(info, "CFBundleName")),
    app_path.as_ref().and_then(|app_path| {
      app_path
        .file_stem()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
    }),
  ])
  .unwrap_or_else(|| bundle_id.to_string());
  let version = info
    .as_ref()
    .and_then(|info| json_string(info, "CFBundleShortVersionString"))
    .unwrap_or_else(|| "unknown".to_string());
  let build_version = info
    .as_ref()
    .and_then(|info| json_string(info, "CFBundleVersion"))
    .unwrap_or_else(|| "unknown".to_string());
  let main_executable_path = info
    .as_ref()
    .and_then(|info| json_string(info, "CFBundleExecutable"))
    .and_then(|value| {
      app_path
        .as_ref()
        .map(|app_path| app_path.join("Contents/MacOS").join(value))
    });
  let url_schemes = info
    .as_ref()
    .and_then(|info| info.get("CFBundleURLTypes"))
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
    launch_services_resolved,
    resolution_notes,
  })
}

fn resolve_launch_services_app_path(escaped_bundle_id: &str) -> AuvResult<PathBuf> {
  let path_script = format!("POSIX path of (path to application id \"{escaped_bundle_id}\")");
  let app_path_raw = run_command_capture("osascript", &["-e", &path_script])?;
  Ok(PathBuf::from(app_path_raw.trim()))
}

fn resolve_spotlight_app_path(bundle_id: &str) -> AuvResult<PathBuf> {
  let query = format!("kMDItemCFBundleIdentifier == \"{bundle_id}\"");
  let raw = run_command_capture("mdfind", &[&query])?;
  let candidate = raw
    .lines()
    .map(str::trim)
    .find(|line| !line.is_empty())
    .ok_or_else(|| format!("no Spotlight match for bundle id `{bundle_id}`"))?;
  Ok(PathBuf::from(candidate))
}

fn read_app_info_plist(app_path: &Path) -> AuvResult<Value> {
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
  serde_json::from_str(&info_json).map_err(|error| {
    format!(
      "failed to parse Info.plist JSON for {}: {error}",
      app_path.display()
    )
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
  runtime: &Runtime,
  run: &mut RecordingRun,
  parent: &SpanRef,
  step_id: &str,
  command_id: &str,
  target_application_id: Option<String>,
  inputs: BTreeMap<String, String>,
  allow_failure: bool,
) -> AuvResult<AppProbeStep> {
  let step_span = run.start_span(
    parent,
    app_span_record(
      "auv.probe.step",
      BTreeMap::from([("auv.probe.step_id".to_string(), string_attr(step_id))]),
    ),
  )?;
  let request = InvokeRequest {
    command_id: command_id.to_string(),
    target: ExecutionTarget {
      application_id: target_application_id.clone(),
    },
    inputs: inputs.clone(),
  };
  let result = match runtime.invoke_in_span(run, &step_span, request) {
    Ok(result) => result,
    Err(error) => {
      if let Err(finish_error) = run.finish_span(
        &step_span,
        SpanFinish {
          status_code: TraceStatusCode::Error,
          summary: Some(format!("Probe step {step_id} failed")),
          failure: Some(error.clone()),
        },
      ) {
        return Err(format!(
          "{error}; additionally failed to finish failed probe step span: {finish_error}"
        ));
      }
      if !allow_failure {
        return Err(error.clone());
      }
      return Ok(AppProbeStep {
        id: step_id.to_string(),
        command_id: command_id.to_string(),
        target_application_id,
        inputs,
        run_id: run.id().to_string(),
        status: RunStatus::Failed.as_str().to_string(),
        output_summary: format!("Probe step {step_id} failed"),
        artifact_paths: Vec::new(),
        failure_message: Some(error),
      });
    }
  };
  let status_code = if result.status == RunStatus::Completed {
    TraceStatusCode::Ok
  } else {
    TraceStatusCode::Error
  };
  run.finish_span(
    &step_span,
    SpanFinish {
      status_code,
      summary: Some(result.output_summary.clone()),
      failure: result.failure_message.clone(),
    },
  )?;
  if result.status != RunStatus::Completed && !allow_failure {
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
    run_id: run.id().to_string(),
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

fn stage_app_artifact(
  runtime: &Runtime,
  run: &mut RecordingRun,
  span: &SpanRef,
  role: &str,
  path: &Path,
  preferred_name: &str,
) -> AuvResult<()> {
  runtime.stage_artifact_file(
    run,
    span,
    role,
    path,
    preferred_name,
    Some(format!("Generated app workflow artifact {role}")),
  )?;
  Ok(())
}

fn finish_failed_app_run<T>(
  runtime: &Runtime,
  run: RecordingRun,
  error: String,
  summary: String,
) -> AuvResult<T> {
  if let Err(finish_error) = runtime.finish_run(
    run,
    RunFinish {
      status_code: TraceStatusCode::Error,
      summary: Some(summary),
      failure: Some(error.clone()),
    },
  ) {
    return Err(format!(
      "{error}; additionally failed to persist failed workflow run: {finish_error}"
    ));
  }
  Err(error)
}

fn app_span_record(
  name: impl Into<String>,
  attributes: crate::recording::Attributes,
) -> SpanRecordV1Alpha1 {
  SpanRecordV1Alpha1 {
    api_version: SPAN_API_VERSION.to_string(),
    span_id: new_span_id(),
    parent_span_id: None,
    name: name.into(),
    state: TraceState::Running,
    status_code: TraceStatusCode::Unset,
    started_at_millis: now_millis(),
    finished_at_millis: None,
    attributes,
    summary: None,
    failure: None,
  }
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

fn render_candidate_case_matrix(
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

fn render_search_entry_candidate_cases(
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

fn render_native_text_candidate_cases(
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

fn render_result_selection_candidate_cases(
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

fn render_window_action_candidate_recipe(analysis: &AppAnalysis) -> Value {
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
        "disturbance": { "classes": ["none"], "max": "none" },
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

fn render_window_action_candidate_cases(
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
  use crate::catalog::CommandCatalog;
  use crate::driver::{Driver, DriverRegistry};
  use crate::model::{CommandSpec, DisturbanceClass, DriverCall, DriverDescriptor, DriverResponse};
  use crate::recording::{MemoryRunEventSink, RunSpec, RunStreamEvent};
  use crate::store::LocalStore;
  use crate::trace::RunType;
  use std::sync::Arc;

  struct TestProbeDriver;

  impl Driver for TestProbeDriver {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test.probe",
        summary: "Test probe driver",
        capabilities: &["test"],
        donor_boundary: "test",
      }
    }

    fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
      Ok(DriverResponse {
        summary: format!("{} ok", call.operation),
        artifacts: Vec::new(),
        notes: Vec::new(),
        signals: BTreeMap::from([("outcome".to_string(), "ok".to_string())]),
        backend: None,
      })
    }
  }

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
      "search-entry",
      "ax-text-input",
      "clipboard-submit",
      "captureEvidence",
      AssessmentStatus::Candidate,
      "test rationale",
    )
    .expect("taxonomy should be valid");
    assert_eq!(
      strategy.taxonomy_id,
      "search-entry.ax-text-input.clipboard-submit.capture-evidence"
    );
  }

  #[test]
  fn invoke_probe_steps_share_parent_probe_run_id() {
    let root = temp_dir("probe-step-parent-run");
    let runtime = test_runtime(root.clone());
    let mut run = runtime
      .start_run(RunSpec::new(RunType::Probe, "auv.probe"))
      .expect("probe run should start");
    let root_span = run.root_span();

    let first = invoke_probe_step(
      &runtime,
      &mut run,
      &root_span,
      "first",
      "test.first",
      None,
      BTreeMap::new(),
      false,
    )
    .expect("first step should complete");
    let second = invoke_probe_step(
      &runtime,
      &mut run,
      &root_span,
      "second",
      "test.second",
      None,
      BTreeMap::new(),
      false,
    )
    .expect("second step should complete");

    assert_eq!(first.run_id, run.id().as_str());
    assert_eq!(second.run_id, run.id().as_str());
    assert_eq!(first.run_id, second.run_id);
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn resolve_probe_ocr_sample_query_prefers_frontmost_window_or_app_name() {
    let root = temp_dir("probe-ocr-query");
    let windows_path = root.join("observe-windows.txt");
    let ax_path = root.join("observe-window-tree.txt");
    fs::write(
      &windows_path,
      "frontmostAppName=网易云音乐\nfrontmostWindowTitle=\nobservedAt=2026-05-20T00:00:00Z\nwindowCount=0\n",
    )
    .expect("window report should write");
    fs::write(
      &ax_path,
      "observedAt=2026-05-20T00:00:00Z\nappName=网易云音乐\nbundleId=com.netease.163music\nwindowTitle=\nrootRole=AXWindow\nnodeCount=0\n",
    )
    .expect("ax report should write");

    let steps = vec![
      probe_step_fixture(
        "observe-windows",
        "debug.observeWindows",
        vec![windows_path],
      ),
      probe_step_fixture(
        "observe-window-tree",
        "debug.observeWindowTree",
        vec![ax_path],
      ),
    ];
    let app = AppIdentity {
      bundle_id: "com.netease.163music".to_string(),
      app_name: "NeteaseMusic".to_string(),
      app_path: None,
      main_executable_path: None,
      version: "1.0".to_string(),
      build_version: "1".to_string(),
      url_schemes: Vec::new(),
      apple_script_addressable: true,
      launch_services_resolved: true,
      resolution_notes: Vec::new(),
    };

    assert_eq!(
      resolve_probe_ocr_sample_query(&app, &steps),
      "网易云音乐".to_string()
    );
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn resolve_probe_ocr_sample_query_falls_back_to_app_metadata() {
    let app = AppIdentity {
      bundle_id: "com.example.App".to_string(),
      app_name: "Example".to_string(),
      app_path: None,
      main_executable_path: None,
      version: "1.0".to_string(),
      build_version: "1".to_string(),
      url_schemes: Vec::new(),
      apple_script_addressable: true,
      launch_services_resolved: true,
      resolution_notes: Vec::new(),
    };

    assert_eq!(resolve_probe_ocr_sample_query(&app, &[]), "Example");
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
        app_path: Some(PathBuf::from("/Applications/Example.app")),
        main_executable_path: None,
        version: "1.0".to_string(),
        build_version: "100".to_string(),
        url_schemes: vec!["example".to_string()],
        apple_script_addressable: true,
        launch_services_resolved: true,
        resolution_notes: vec![],
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
      annotation_candidates: vec![AppSurfaceCandidate {
        candidate_id: "search-entry-focus-ax".to_string(),
        area: "search-entry".to_string(),
        kind: "focus-query".to_string(),
        source: "ax".to_string(),
        status: AssessmentStatus::Candidate,
        primary_text: "Search".to_string(),
        secondary_text: "role=AXTextField path=0.1".to_string(),
        query_value: "Search".to_string(),
        coordinate_space: "global-logical".to_string(),
        bounds: Some(AppRect {
          x: 10,
          y: 10,
          width: 80,
          height: 20,
        }),
        click_point: Some(AppPoint { x: 50, y: 20 }),
        confidence: None,
        evidence_step_id: "observe-window-tree".to_string(),
        input_bindings: BTreeMap::from([("focus_query".to_string(), "Search".to_string())]),
        compatibility: candidate_compatibility(
          &["search-entry.ax-text-input.clipboard-submit.capture-evidence"],
          &[],
        ),
        notes: vec!["sample note".to_string()],
      }],
      known_boundaries: vec!["one boundary".to_string()],
      recommended_strategies: vec![
        recommended_strategy(
          "search-entry",
          "ax-text-input",
          "clipboard-submit",
          "captureEvidence",
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
    assert!(report.contains("## 4. Candidate / Annotation Layer"));
    assert!(report.contains("coordinateSpace"));
    assert!(report.contains("inputBindings"));
    assert!(report.contains("## 5. Control Strategy"));
    assert!(report.contains("## 6. Verification Assessment"));
    assert!(report.contains("Recommended Candidate Strategies"));
  }

  #[test]
  fn search_entry_distillation_template_validates() {
    let analysis =
      sample_analysis_with_strategy("search-entry.ax-text-input.clipboard-submit.capture-evidence");
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, &analysis.recommended_strategies[0].taxonomy_id);
    let recipe = render_candidate_recipe(
      &analysis,
      &analysis.recommended_strategies[0],
      &candidate_shape,
    )
    .expect("candidate recipe should render");
    let manifest: SkillManifest =
      serde_json::from_value(recipe).expect("candidate recipe should parse");
    validate_skill_manifest(&manifest).expect("candidate recipe should validate");
    let matrix_value = render_candidate_case_matrix(
      &analysis,
      &analysis.recommended_strategies[0],
      &candidate_shape,
    )
    .expect("candidate matrix should render");
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
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, &analysis.recommended_strategies[0].taxonomy_id);
    let matrix_value = render_native_text_candidate_cases(&analysis, &candidate_shape);
    let matrix: SkillCaseMatrix =
      serde_json::from_value(matrix_value).expect("candidate matrix should parse");
    validate_case_matrix_manifest(&matrix).expect("candidate matrix should validate");
    validate_case_matrix_against_skill(&manifest, &matrix).expect("candidate matrix should align");
  }

  #[test]
  fn window_action_distillation_template_validates() {
    let analysis =
      sample_analysis_with_strategy("window-action.window-point.pointer-click.capture-evidence");
    let recipe = render_window_action_candidate_recipe(&analysis);
    let manifest: SkillManifest =
      serde_json::from_value(recipe).expect("candidate recipe should parse");
    validate_skill_manifest(&manifest).expect("candidate recipe should validate");
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, &analysis.recommended_strategies[0].taxonomy_id);
    let matrix_value = render_window_action_candidate_cases(&analysis, &candidate_shape);
    let matrix: SkillCaseMatrix =
      serde_json::from_value(matrix_value).expect("candidate matrix should parse");
    validate_case_matrix_manifest(&matrix).expect("candidate matrix should validate");
    validate_case_matrix_against_skill(&manifest, &matrix).expect("candidate matrix should align");
  }

  #[test]
  fn build_distilled_candidate_shape_projects_direct_inputs() {
    let mut analysis =
      sample_analysis_with_strategy("window-action.window-point.pointer-click.capture-evidence");
    analysis.annotation_candidates.push(AppSurfaceCandidate {
      candidate_id: "window-primary-region".to_string(),
      area: "window.primary".to_string(),
      kind: "region".to_string(),
      source: "ax".to_string(),
      status: AssessmentStatus::Candidate,
      primary_text: "Example".to_string(),
      secondary_text: "com.example.App".to_string(),
      query_value: "100,200,800,600".to_string(),
      coordinate_space: "global-logical".to_string(),
      bounds: Some(AppRect {
        x: 100,
        y: 200,
        width: 800,
        height: 600,
      }),
      click_point: Some(AppPoint { x: 500, y: 500 }),
      confidence: None,
      evidence_step_id: "observe-window-tree".to_string(),
      input_bindings: BTreeMap::from([
        ("window_bounds".to_string(), "100,200,800,600".to_string()),
        ("relative_x".to_string(), "0.500000".to_string()),
        ("relative_y".to_string(), "0.500000".to_string()),
      ]),
      compatibility: candidate_compatibility(
        &["window-action.window-point.pointer-click.capture-evidence"],
        &[],
      ),
      notes: vec!["sample window region".to_string()],
    });

    let candidate_shape = build_distilled_candidate_shape(
      &analysis,
      "window-action.window-point.pointer-click.capture-evidence",
    );
    assert_eq!(
      candidate_shape.direct_candidate_ids,
      vec!["window-primary-region".to_string()]
    );
    assert_eq!(
      candidate_shape.provided_inputs.get("relative_x"),
      Some(&"0.500000".to_string())
    );
    assert_eq!(
      candidate_shape.provided_inputs.get("relative_y"),
      Some(&"0.500000".to_string())
    );
    assert!(candidate_shape.notes.is_empty());
  }

  #[test]
  fn apply_candidate_grounding_marks_unresolved_search_entry_without_search_signal() {
    let analysis =
      sample_analysis_with_strategy("search-entry.ax-text-input.clipboard-submit.capture-evidence");
    let mut matrix: SkillCaseMatrix = serde_json::from_value(render_search_entry_candidate_cases(
      &analysis,
      &AppDistilledCandidateShape::default(),
    ))
    .expect("candidate matrix should parse");
    let mut resolved = BTreeMap::new();
    let (unresolved, used_annotations) = apply_candidate_grounding(
      &analysis,
      None,
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
      &mut matrix,
      &mut resolved,
    )
    .expect("known taxonomy should ground");
    assert!(unresolved.iter().any(|key| key == "focus_query"));
    assert!(used_annotations.is_empty());
    assert!(resolved.contains_key("query"));
  }

  #[test]
  fn apply_candidate_grounding_resolves_window_action_from_window_region_annotation() {
    let mut analysis =
      sample_analysis_with_strategy("window-action.window-point.pointer-click.capture-evidence");
    analysis.annotation_candidates.push(AppSurfaceCandidate {
      candidate_id: "window-primary-region".to_string(),
      area: "window.primary".to_string(),
      kind: "region".to_string(),
      source: "ax".to_string(),
      status: AssessmentStatus::Candidate,
      primary_text: "Example".to_string(),
      secondary_text: "com.example.App".to_string(),
      query_value: "100,200,800,600".to_string(),
      coordinate_space: "global-logical".to_string(),
      bounds: Some(AppRect {
        x: 100,
        y: 200,
        width: 800,
        height: 600,
      }),
      click_point: Some(AppPoint { x: 500, y: 500 }),
      confidence: None,
      evidence_step_id: "observe-window-tree".to_string(),
      input_bindings: BTreeMap::from([
        ("window_bounds".to_string(), "100,200,800,600".to_string()),
        ("relative_x".to_string(), "0.500000".to_string()),
        ("relative_y".to_string(), "0.500000".to_string()),
      ]),
      compatibility: candidate_compatibility(
        &["window-action.window-point.pointer-click.capture-evidence"],
        &[],
      ),
      notes: vec!["sample window region".to_string()],
    });
    let mut matrix: SkillCaseMatrix = serde_json::from_value(render_window_action_candidate_cases(
      &analysis,
      &AppDistilledCandidateShape::default(),
    ))
    .expect("candidate matrix should parse");
    let mut resolved = BTreeMap::new();
    let (unresolved, used_annotations) = apply_candidate_grounding(
      &analysis,
      None,
      "window-action.window-point.pointer-click.capture-evidence",
      &mut matrix,
      &mut resolved,
    )
    .expect("known taxonomy should ground");
    assert!(unresolved.is_empty());
    assert_eq!(resolved.get("relative_x"), Some(&"0.500000".to_string()));
    assert_eq!(resolved.get("relative_y"), Some(&"0.500000".to_string()));
    assert!(
      used_annotations
        .iter()
        .any(|candidate_id| candidate_id == "window-primary-region")
    );
    assert_eq!(
      matrix.cases[0].inputs.get("relative_x"),
      Some(&"0.500000".to_string())
    );
    assert_eq!(
      matrix.cases[0].inputs.get("relative_y"),
      Some(&"0.500000".to_string())
    );
  }

  #[test]
  fn apply_candidate_grounding_rejects_unknown_taxonomy() {
    let analysis =
      sample_analysis_with_strategy("search-entry.ax-text-input.clipboard-submit.capture-evidence");
    let mut matrix: SkillCaseMatrix = serde_json::from_value(render_search_entry_candidate_cases(
      &analysis,
      &AppDistilledCandidateShape::default(),
    ))
    .expect("candidate matrix should parse");
    let mut resolved = BTreeMap::new();
    let error = apply_candidate_grounding(
      &analysis,
      None,
      "search-entry.ax-text-input.clipboard-submit.unknown-contract",
      &mut matrix,
      &mut resolved,
    )
    .expect_err("unknown taxonomy should fail fast");
    assert!(error.contains("unsupported candidate grounding taxonomy"));
  }

  #[test]
  fn validate_app_distillation_nests_case_runs_in_app_validate_run() {
    let root = temp_dir("app-validate-nested-cases");
    let sink = Arc::new(MemoryRunEventSink::new());
    let runtime = test_runtime(root.clone()).with_event_sink(sink.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("candidate.recipe.json");
    let case_matrix_path = root.join("candidate.cases.json");

    let mut analysis = sample_analysis_with_strategy(
      "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text",
    );
    analysis.probe_path = root.join("missing-probe.json");
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    write_pretty_json(&recipe_path, &test_candidate_manifest_value())
      .expect("candidate recipe should write");
    write_pretty_json(&case_matrix_path, &test_candidate_matrix_value())
      .expect("candidate matrix should write");
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: analysis_path,
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.recorded.skill".to_string(),
        taxonomy_id: "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: Vec::new(),
        candidate_shape: AppDistilledCandidateShape::default(),
        recipe_path,
        case_matrix_path,
      }],
      known_boundaries: Vec::new(),
    };
    write_pretty_json(&distillation_path, &distillation).expect("distillation should write");

    validate_app_distillation(&runtime, &distillation_path).expect("validation should complete");

    let finished_runs = sink
      .drain_for_test()
      .into_iter()
      .filter_map(|event| match event {
        RunStreamEvent::RunFinished { run, .. } => Some(run),
        _ => None,
      })
      .collect::<Vec<_>>();
    assert_eq!(
      finished_runs.len(),
      1,
      "app validation should not create standalone skill or case-matrix runs"
    );
    assert_eq!(finished_runs[0].run_type, RunType::Validate);
    let canonical = runtime
      .read_run(finished_runs[0].run_id.as_str())
      .expect("app validate run should read");
    assert_eq!(canonical.spans[0].name, "auv.validate");
    assert!(canonical.spans.iter().any(|span| span.name == "auv.case"));
    assert!(
      canonical
        .spans
        .iter()
        .any(|span| span.name == "auv.execute")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn validate_app_distillation_keeps_failed_case_inside_app_validate_run() {
    let root = temp_dir("app-validate-failed-nested-case");
    let sink = Arc::new(MemoryRunEventSink::new());
    let runtime = test_runtime(root.clone()).with_event_sink(sink.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("candidate.recipe.json");
    let case_matrix_path = root.join("candidate.cases.json");

    let mut analysis = sample_analysis_with_strategy(
      "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text",
    );
    analysis.probe_path = root.join("missing-probe.json");
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    let mut manifest_value = test_candidate_manifest_value();
    manifest_value["steps"][0]["expect"]["output_must_contain"] =
      serde_json::json!(["definitely-missing"]);
    write_pretty_json(&recipe_path, &manifest_value).expect("candidate recipe should write");
    write_pretty_json(&case_matrix_path, &test_candidate_matrix_value())
      .expect("candidate matrix should write");
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: analysis_path,
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.recorded.skill".to_string(),
        taxonomy_id: "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: Vec::new(),
        candidate_shape: AppDistilledCandidateShape::default(),
        recipe_path,
        case_matrix_path,
      }],
      known_boundaries: Vec::new(),
    };
    write_pretty_json(&distillation_path, &distillation).expect("distillation should write");

    let output =
      validate_app_distillation(&runtime, &distillation_path).expect("validation should complete");
    assert_eq!(
      output.validation.candidates[0].status,
      AppValidationStatus::Rejected
    );

    let finished_runs = sink
      .drain_for_test()
      .into_iter()
      .filter_map(|event| match event {
        RunStreamEvent::RunFinished { run, .. } => Some(run),
        _ => None,
      })
      .collect::<Vec<_>>();
    assert_eq!(
      finished_runs.len(),
      1,
      "failed candidate validation should still stay inside the app validate run"
    );
    assert_eq!(finished_runs[0].run_type, RunType::Validate);
    let canonical = runtime
      .read_run(finished_runs[0].run_id.as_str())
      .expect("app validate run should read");
    assert!(
      canonical
        .spans
        .iter()
        .any(|span| { span.name == "auv.execute" && span.status_code == TraceStatusCode::Error })
    );
    assert!(canonical.spans.iter().any(|span| {
      span.name == "auv.app.validate.candidate" && span.status_code == TraceStatusCode::Error
    }));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn validate_app_distillation_validates_window_action_after_auto_grounding() {
    let root = temp_dir("app-validate-window-action");
    let sink = Arc::new(MemoryRunEventSink::new());
    let runtime = test_runtime(root.clone()).with_event_sink(sink.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("window-action.recipe.json");
    let case_matrix_path = root.join("window-action.cases.json");

    let mut analysis =
      sample_analysis_with_strategy("window-action.window-point.pointer-click.capture-evidence");
    analysis.probe_path = root.join("missing-probe.json");
    analysis.annotation_candidates.push(AppSurfaceCandidate {
      candidate_id: "window-primary-region".to_string(),
      area: "window.primary".to_string(),
      kind: "region".to_string(),
      source: "ax".to_string(),
      status: AssessmentStatus::Candidate,
      primary_text: "Example".to_string(),
      secondary_text: "com.example.App".to_string(),
      query_value: "100,200,800,600".to_string(),
      coordinate_space: "global-logical".to_string(),
      bounds: Some(AppRect {
        x: 100,
        y: 200,
        width: 800,
        height: 600,
      }),
      click_point: Some(AppPoint { x: 500, y: 500 }),
      confidence: None,
      evidence_step_id: "observe-window-tree".to_string(),
      input_bindings: BTreeMap::from([
        ("window_bounds".to_string(), "100,200,800,600".to_string()),
        ("relative_x".to_string(), "0.500000".to_string()),
        ("relative_y".to_string(), "0.500000".to_string()),
      ]),
      compatibility: candidate_compatibility(
        &["window-action.window-point.pointer-click.capture-evidence"],
        &[],
      ),
      notes: vec!["sample window region".to_string()],
    });
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    write_pretty_json(&recipe_path, &test_window_action_candidate_manifest_value())
      .expect("candidate recipe should write");
    write_pretty_json(
      &case_matrix_path,
      &test_window_action_candidate_matrix_value(),
    )
    .expect("candidate matrix should write");
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: analysis_path,
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.window.action".to_string(),
        taxonomy_id: "window-action.window-point.pointer-click.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: vec!["window-primary-region".to_string()],
        candidate_shape: AppDistilledCandidateShape::default(),
        recipe_path,
        case_matrix_path,
      }],
      known_boundaries: Vec::new(),
    };
    write_pretty_json(&distillation_path, &distillation).expect("distillation should write");

    let output =
      validate_app_distillation(&runtime, &distillation_path).expect("validation should complete");
    assert_eq!(
      output.validation.candidates[0].status,
      AppValidationStatus::Validated
    );
    assert_eq!(
      output.validation.candidates[0].verification_mode,
      AppVerificationMode::EvidenceOnly
    );
    assert!(
      output.validation.candidates[0]
        .rationale
        .contains("evidence-only")
    );
    assert_eq!(
      output.validation.candidates[0]
        .resolved_inputs
        .get("relative_x"),
      Some(&"0.500000".to_string())
    );
    assert_eq!(
      output.validation.candidates[0]
        .resolved_inputs
        .get("relative_y"),
      Some(&"0.500000".to_string())
    );
    assert!(
      output.validation.candidates[0]
        .used_annotation_ids
        .iter()
        .any(|candidate_id| candidate_id == "window-primary-region")
    );
    assert!(output.validation.candidates[0].unresolved_inputs.is_empty());
    let report = fs::read_to_string(&output.report_path).expect("report should exist");
    assert!(report.contains("verification mode: `evidence-only`"));
    assert!(report.contains("manual review required: `yes`"));

    let finished_runs = sink
      .drain_for_test()
      .into_iter()
      .filter_map(|event| match event {
        RunStreamEvent::RunFinished { run, .. } => Some(run),
        _ => None,
      })
      .collect::<Vec<_>>();
    assert_eq!(finished_runs.len(), 1);
    assert_eq!(finished_runs[0].run_type, RunType::Validate);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn validate_app_distillation_fails_on_unresolved_grounding_inputs() {
    let root = temp_dir("app-validate-unresolved-grounding");
    let runtime = test_runtime(root.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("window-action.recipe.json");
    let case_matrix_path = root.join("window-action.cases.json");

    let mut analysis =
      sample_analysis_with_strategy("window-action.window-point.pointer-click.capture-evidence");
    analysis.probe_path = root.join("missing-probe.json");
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    write_pretty_json(&recipe_path, &test_window_action_candidate_manifest_value())
      .expect("candidate recipe should write");
    write_pretty_json(
      &case_matrix_path,
      &test_window_action_candidate_matrix_value(),
    )
    .expect("candidate matrix should write");
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: analysis_path,
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.window.action".to_string(),
        taxonomy_id: "window-action.window-point.pointer-click.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: Vec::new(),
        candidate_shape: AppDistilledCandidateShape::default(),
        recipe_path,
        case_matrix_path,
      }],
      known_boundaries: Vec::new(),
    };
    write_pretty_json(&distillation_path, &distillation).expect("distillation should write");

    let error = validate_app_distillation(&runtime, &distillation_path)
      .expect_err("unresolved grounding should fail validation");
    assert!(error.contains("relative_x"));
    assert!(error.contains("relative_y"));

    let validation: AppValidation =
      read_json(&root.join("validation.json")).expect("validation output should still write");
    assert_eq!(
      validation.candidates[0].status,
      AppValidationStatus::Rejected
    );
    assert_eq!(
      validation.candidates[0].unresolved_inputs,
      vec!["relative_x".to_string(), "relative_y".to_string()]
    );
    assert!(
      validation.candidates[0]
        .failure_message
        .as_deref()
        .is_some_and(|message| message.contains("grounding left unresolved inputs"))
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn validate_app_distillation_uses_candidate_shape_inputs_before_analysis_fallback() {
    let root = temp_dir("app-validate-window-action-shape");
    let runtime = test_runtime(root.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("window-action.recipe.json");
    let case_matrix_path = root.join("window-action.cases.json");

    let mut analysis =
      sample_analysis_with_strategy("window-action.window-point.pointer-click.capture-evidence");
    analysis.probe_path = root.join("missing-probe.json");
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    write_pretty_json(&recipe_path, &test_window_action_candidate_manifest_value())
      .expect("candidate recipe should write");
    write_pretty_json(
      &case_matrix_path,
      &test_window_action_candidate_matrix_value(),
    )
    .expect("candidate matrix should write");
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: analysis_path,
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.window.action".to_string(),
        taxonomy_id: "window-action.window-point.pointer-click.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: vec!["window-primary-region".to_string()],
        candidate_shape: AppDistilledCandidateShape {
          direct_candidate_ids: vec!["window-primary-region".to_string()],
          context_candidate_ids: Vec::new(),
          provided_inputs: BTreeMap::from([
            ("window_bounds".to_string(), "100,200,800,600".to_string()),
            ("relative_x".to_string(), "0.500000".to_string()),
            ("relative_y".to_string(), "0.500000".to_string()),
          ]),
          notes: Vec::new(),
        },
        recipe_path,
        case_matrix_path,
      }],
      known_boundaries: Vec::new(),
    };
    write_pretty_json(&distillation_path, &distillation).expect("distillation should write");

    let output =
      validate_app_distillation(&runtime, &distillation_path).expect("validation should complete");
    assert_eq!(
      output.validation.candidates[0].status,
      AppValidationStatus::Validated
    );
    assert_eq!(
      output.validation.candidates[0].verification_mode,
      AppVerificationMode::EvidenceOnly
    );
    assert_eq!(
      output.validation.candidates[0].used_annotation_ids,
      vec!["window-primary-region".to_string()]
    );
    assert_eq!(
      output.validation.candidates[0]
        .resolved_inputs
        .get("relative_x"),
      Some(&"0.500000".to_string())
    );
    assert_eq!(
      output.validation.candidates[0]
        .resolved_inputs
        .get("relative_y"),
      Some(&"0.500000".to_string())
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_app_analysis_tolerates_partial_probe_failures() {
    let root = temp_dir("partial-app-probe");
    let probe_path = root.join("probe.json");
    let permissions_path = root.join("artifact_probe-permissions.txt");
    let displays_path = root.join("artifact_display-list.json");
    let readiness_path = root.join("artifact_coordinate-readiness-report.txt");
    let windows_path = root.join("artifact_observe-windows.txt");

    fs::write(
      &permissions_path,
      "screenRecording=granted\naccessibility=granted\nautomationToSystemEvents=granted\nlaunchHostProcess=Atlas\n",
    )
    .expect("permissions artifact should write");
    fs::write(
      &displays_path,
      serde_json::to_string(&vec![serde_json::json!({
        "displayId": 1,
        "isMain": true,
        "isBuiltIn": true,
        "bounds": {"x": 0, "y": 0, "width": 1512, "height": 982},
        "visibleBounds": {"x": 0, "y": 0, "width": 1512, "height": 982},
        "scaleFactor": 2.0,
        "pixelWidth": 3024,
        "pixelHeight": 1964
      })])
      .expect("display artifact should serialize"),
    )
    .expect("display artifact should write");
    fs::write(&readiness_path, "readyForLogicalInput=true\nreason=ok\n")
      .expect("readiness artifact should write");
    fs::write(
      &windows_path,
      "observedAt=2026-05-19T00:00:00Z\nfrontmostAppName=\nfrontmostWindowTitle=\n",
    )
    .expect("windows artifact should write");

    let probe = AppProbe {
      probe_version: APP_PROBE_VERSION.to_string(),
      created_at_millis: 0,
      project_root: root.clone(),
      output_dir: root.clone(),
      app: AppIdentity {
        bundle_id: "com.netease.163music".to_string(),
        app_name: "com.netease.163music".to_string(),
        app_path: None,
        main_executable_path: None,
        version: "unknown".to_string(),
        build_version: "unknown".to_string(),
        url_schemes: vec![],
        apple_script_addressable: false,
        launch_services_resolved: false,
        resolution_notes: vec![
          "LaunchServices could not resolve `com.netease.163music`.".to_string(),
        ],
      },
      steps: vec![
        probe_step_fixture(
          "probe-permissions",
          "debug.probePermissions",
          vec![permissions_path],
        ),
        probe_step_fixture("list-displays", "debug.listDisplays", vec![displays_path]),
        probe_step_fixture(
          "probe-coordinate-readiness",
          "debug.probeCoordinateReadiness",
          vec![readiness_path],
        ),
        probe_step_fixture(
          "observe-windows",
          "debug.observeWindows",
          vec![windows_path],
        ),
        failed_probe_step_fixture(
          "observe-window-tree",
          "debug.observeWindowTree",
          "app not available",
        ),
        failed_probe_step_fixture(
          "capture-display",
          "debug.captureDisplay",
          "app not available",
        ),
      ],
    };

    let analysis = build_app_analysis(&probe_path, &probe).expect("analysis should still build");
    assert!(analysis.annotation_candidates.is_empty());
    assert!(analysis.recommended_strategies.is_empty());
    assert!(
      analysis
        .known_boundaries
        .iter()
        .any(|entry| entry.contains("LaunchServices could not resolve"))
    );
    assert!(
      analysis
        .known_boundaries
        .iter()
        .any(|entry| entry.contains("observe-window-tree"))
    );
    assert_eq!(
      analysis.available_surfaces.accessibility_tree,
      AssessmentStatus::Unavailable
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_app_analysis_emits_window_action_from_ax_root_window() {
    let root = temp_dir("ax-root-window-app-probe");
    let probe_path = root.join("probe.json");
    let permissions_path = root.join("artifact_probe-permissions.txt");
    let displays_path = root.join("artifact_display-list.json");
    let readiness_path = root.join("artifact_coordinate-readiness-report.txt");
    let windows_path = root.join("artifact_observe-windows.txt");
    let ax_path = root.join("artifact_window-tree.txt");

    fs::write(
      &permissions_path,
      "screenRecording=granted\naccessibility=granted\nautomationToSystemEvents=granted\nlaunchHostProcess=Atlas\n",
    )
    .expect("permissions artifact should write");
    fs::write(
      &displays_path,
      serde_json::to_string(&vec![serde_json::json!({
        "display_ref": "display_0",
        "native_display_id": "1",
        "is_main": true,
        "is_builtin": true,
        "global_logical_bounds": {"x": 0, "y": 0, "width": 1512, "height": 982},
        "visible_logical_bounds": {"x": 0, "y": 0, "width": 1512, "height": 982},
        "scale_factor": 2.0,
        "physical_pixel_size": {"width": 3024, "height": 1964}
      })])
      .expect("display artifact should serialize"),
    )
    .expect("display artifact should write");
    fs::write(
      &readiness_path,
      "readyForLogicalInput=false\nreason=main display pixels are 2x logical points\n",
    )
    .expect("readiness artifact should write");
    fs::write(
      &windows_path,
      "observedAt=2026-05-19T00:00:00Z\nfrontmostAppName=网易云音乐\nfrontmostWindowTitle=\nwindowCount=0\n",
    )
    .expect("windows artifact should write");
    fs::write(
      &ax_path,
      "observedAt=2026-05-19T00:00:00Z\nappName=网易云音乐\nbundleId=com.netease.163music\npid=44741\nwindowTitle=\nrootRole=AXWindow\nnode\t0\t0\tAXWindow\tAXStandardWindow\t\t\t\t\t\t\t227\t100\t1058\t752\nnodeCount=1\n",
    )
    .expect("ax artifact should write");

    let probe = AppProbe {
      probe_version: APP_PROBE_VERSION.to_string(),
      created_at_millis: 0,
      project_root: root.clone(),
      output_dir: root.clone(),
      app: AppIdentity {
        bundle_id: "com.netease.163music".to_string(),
        app_name: "NeteaseMusic".to_string(),
        app_path: Some(PathBuf::from("/Applications/NeteaseMusic.app")),
        main_executable_path: None,
        version: "3.1.7".to_string(),
        build_version: "3283".to_string(),
        url_schemes: vec!["orpheus".to_string()],
        apple_script_addressable: true,
        launch_services_resolved: true,
        resolution_notes: vec![],
      },
      steps: vec![
        probe_step_fixture(
          "probe-permissions",
          "debug.probePermissions",
          vec![permissions_path],
        ),
        probe_step_fixture("list-displays", "debug.listDisplays", vec![displays_path]),
        probe_step_fixture(
          "probe-coordinate-readiness",
          "debug.probeCoordinateReadiness",
          vec![readiness_path],
        ),
        probe_step_fixture(
          "observe-windows",
          "debug.observeWindows",
          vec![windows_path],
        ),
        probe_step_fixture(
          "observe-window-tree",
          "debug.observeWindowTree",
          vec![ax_path],
        ),
      ],
    };

    let analysis = build_app_analysis(&probe_path, &probe).expect("analysis should still build");
    assert_eq!(
      analysis.window_context.primary_window_bounds,
      Some(AppRect {
        x: 227,
        y: 100,
        width: 1058,
        height: 752,
      })
    );
    let window_candidate = analysis
      .annotation_candidates
      .iter()
      .find(|candidate| candidate.candidate_id == "window-primary-region")
      .expect("window candidate should exist");
    assert_eq!(window_candidate.source, "ax");
    assert_eq!(window_candidate.evidence_step_id, "observe-window-tree");
    assert_eq!(
      window_candidate.input_bindings.get("relative_x"),
      Some(&"0.500000".to_string())
    );
    assert_eq!(
      window_candidate.input_bindings.get("relative_y"),
      Some(&"0.500000".to_string())
    );
    assert!(analysis.recommended_strategies.iter().any(|strategy| {
      strategy.taxonomy_id == "window-action.window-point.pointer-click.capture-evidence"
    }));

    let _ = fs::remove_dir_all(root);
  }

  fn sample_analysis_with_strategy(taxonomy_id: &str) -> AppAnalysis {
    AppAnalysis {
      analysis_version: APP_ANALYSIS_VERSION.to_string(),
      created_at_millis: 0,
      probe_path: PathBuf::from("/tmp/probe.json"),
      app_identity: AppIdentity {
        bundle_id: "com.example.App".to_string(),
        app_name: "Example".to_string(),
        app_path: Some(PathBuf::from("/Applications/Example.app")),
        main_executable_path: None,
        version: "1.0".to_string(),
        build_version: "100".to_string(),
        url_schemes: vec![],
        apple_script_addressable: false,
        launch_services_resolved: true,
        resolution_notes: vec![],
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
      annotation_candidates: vec![],
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

  fn probe_step_fixture(id: &str, command_id: &str, artifact_paths: Vec<PathBuf>) -> AppProbeStep {
    AppProbeStep {
      id: id.to_string(),
      command_id: command_id.to_string(),
      target_application_id: None,
      inputs: BTreeMap::new(),
      run_id: "run_fixture".to_string(),
      status: RunStatus::Completed.as_str().to_string(),
      output_summary: "ok".to_string(),
      artifact_paths,
      failure_message: None,
    }
  }

  fn failed_probe_step_fixture(id: &str, command_id: &str, error: &str) -> AppProbeStep {
    AppProbeStep {
      id: id.to_string(),
      command_id: command_id.to_string(),
      target_application_id: None,
      inputs: BTreeMap::new(),
      run_id: "run_fixture".to_string(),
      status: RunStatus::Failed.as_str().to_string(),
      output_summary: format!("Probe step {id} failed"),
      artifact_paths: Vec::new(),
      failure_message: Some(error.to_string()),
    }
  }

  fn test_runtime(project_root: PathBuf) -> Runtime {
    let commands = CommandCatalog::new(vec![
      CommandSpec {
        id: "test.first",
        summary: "Test first command",
        driver_id: "test.probe",
        operation: "first",
        disturbance_classes: &[DisturbanceClass::None],
        max_disturbance: DisturbanceClass::None,
      },
      CommandSpec {
        id: "test.second",
        summary: "Test second command",
        driver_id: "test.probe",
        operation: "second",
        disturbance_classes: &[DisturbanceClass::None],
        max_disturbance: DisturbanceClass::None,
      },
      CommandSpec {
        id: "test.skill.invoke",
        summary: "Test skill command",
        driver_id: "test.probe",
        operation: "test_operation",
        disturbance_classes: &[DisturbanceClass::None],
        max_disturbance: DisturbanceClass::None,
      },
    ]);
    let drivers = DriverRegistry::new(vec![Box::new(TestProbeDriver)]);
    Runtime::new(
      project_root.clone(),
      commands,
      drivers,
      LocalStore::new(project_root).expect("store should initialize"),
    )
  }

  fn test_candidate_manifest_value() -> Value {
    serde_json::json!({
      "recipe_id": "test.recorded.skill",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "fixture.app", "display_mode": "fixture" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test app validation nesting",
      "disturbance_policy": {
        "max_disturbance": "none",
        "declared_classes": ["none"]
      },
      "steps": [{
        "id": "first",
        "command_id": "test.skill.invoke",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        },
        "expect": {
          "output_must_contain": ["outcome=ok"]
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    })
  }

  fn test_candidate_matrix_value() -> Value {
    serde_json::json!({
      "skill_id": "test.recorded.skill",
      "version": "0.1.0",
      "status": "active-case-matrix",
      "cases": [{
        "case_id": "baseline",
        "status": "validated",
        "inputs": {},
        "disturbance": "none"
      }]
    })
  }

  fn test_window_action_candidate_manifest_value() -> Value {
    serde_json::json!({
      "recipe_id": "test.window.action",
      "version": "0.1.0",
      "status": "candidate-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "fixture.app", "display_mode": "fixture" },
      "strategy": {
        "family": "window-action",
        "grounding": "window-point",
        "activation": "pointer-click",
        "verificationContract": "captureEvidence"
      },
      "objective": "test window action validation",
      "disturbance_policy": {
        "max_disturbance": "none",
        "declared_classes": ["none"]
      },
      "inputs": {
        "relative_x": { "type": "number" },
        "relative_y": { "type": "number" }
      },
      "steps": [{
        "id": "first",
        "command_id": "test.skill.invoke",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        },
        "expect": {
          "output_must_contain": ["outcome=ok"]
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    })
  }

  fn test_window_action_candidate_matrix_value() -> Value {
    serde_json::json!({
      "skill_id": "test.window.action",
      "version": "0.1.0",
      "status": "candidate-case-matrix",
      "cases": [{
        "case_id": "default-candidate",
        "status": "candidate",
        "inputs": {
          "relative_x": "TODO_RELATIVE_X",
          "relative_y": "TODO_RELATIVE_Y"
        },
        "disturbance": "none"
      }]
    })
  }
}
