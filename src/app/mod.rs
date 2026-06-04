// File: src/app/mod.rs
//! App-centric workflows: probe → analyze → distill → validate.
//!
//! This module is a tooling pipeline that turns observed runs/artifacts into:
//! (1) an app probe snapshot, (2) analysis reports, (3) distilled candidate
//! shapes and recipe scaffolding, and (4) validation runs against case matrices.
//!
//! Boundary: this is not the core `Runtime` executor and does not implement
//! macOS automation primitives (drivers do). It exists to make the "how do we
//! author/refresh recipes" path inspectable and reproducible.

mod analysis;
mod infra;
mod recipe;
mod report;

use analysis::{
  apply_candidate_grounding, apply_distilled_candidate_shape_inputs, build_app_analysis,
  build_distilled_candidate_shape, parse_ax_snapshot, promoted_candidate_for_candidate_shape,
  resolve_probe_ocr_sample_query, source_evidence_refs_for_candidate_shape,
  suggested_annotation_ids_for_candidate_shape, validated_candidate_rationale,
  verification_mode_for_strategy,
};
use infra::{
  app_span_record, default_probe_output_dir, finish_failed_app_run, invoke_probe_step, read_json,
  resolve_analysis_path, resolve_app_identity, resolve_distillation_path, resolve_probe_path,
  stage_app_artifact, write_pretty_json,
};
use recipe::{
  candidate_slug, recipe_app_slug, render_candidate_case_matrix, render_candidate_recipe,
};
use report::{
  render_app_analysis_report, render_app_distillation_report, render_app_validation_report,
};

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use auv_driver_macos::types::ObservedRect;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::contract::ArtifactRef;
use crate::model::{AuvResult, now_millis};
use crate::run_builder::{RecordingRun, RunFinish, RunSpec, SpanFinish, SpanRef};
use crate::runtime::Runtime;
use crate::skill::{
  SkillCaseMatrix, SkillCaseRunOptions, SkillManifest, run_skill_case_matrix_into_run,
  validate_case_matrix_against_skill, validate_case_matrix_manifest, validate_skill_manifest,
};
use crate::store::sanitized_artifact_name;
use crate::trace::{RunType, TraceStatusCode, string_attr};

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
  pub created_at_millis: u64,
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
  #[serde(default)]
  pub span_id: String,
  pub status: String,
  pub output_summary: String,
  pub artifact_paths: Vec<PathBuf>,
  #[serde(default)]
  pub artifacts: Vec<AppProbeArtifact>,
  pub failure_message: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppProbeArtifact {
  pub artifact_id: String,
  pub span_id: String,
  pub path: PathBuf,
  pub role: String,
  #[serde(default)]
  pub captured_event_id: Option<String>,
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
  pub created_at_millis: u64,
  pub source_analysis_path: PathBuf,
  pub app_identity: AppIdentity,
  pub candidates: Vec<AppDistilledCandidate>,
  pub known_boundaries: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppValidation {
  pub validate_version: String,
  pub created_at_millis: u64,
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
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub source_evidence_refs: Vec<ArtifactRef>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub promoted_candidate: Option<crate::contract::Candidate>,
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
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub observed_consumer: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub observed_candidate_local_id: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub candidate_source: Option<String>,
  pub unresolved_inputs: Vec<String>,
  pub failure_message: Option<String>,
  pub resolved_inputs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppAnalysis {
  pub analysis_version: String,
  pub created_at_millis: u64,
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
  NativeTextAxTextAxPerformActionClipboardPasteVerifyAxText,
  ResultSelectionOcrAnchorPointerClickCaptureEvidence,
  WindowActionWindowPointPointerClickCaptureEvidence,
}

const SEARCH_ENTRY_TAXONOMY_ID: &str =
  "search-entry.ax-text-input.clipboard-submit.capture-evidence";
const NATIVE_TEXT_CANONICAL_TAXONOMY_ID: &str =
  "native-text.ax-text.ax-perform-action-clipboard-paste.verify-ax-text";
const NATIVE_TEXT_LEGACY_TAXONOMY_ID: &str =
  "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text";
const RESULT_SELECTION_TAXONOMY_ID: &str =
  "result-selection.ocr-anchor.pointer-click.capture-evidence";
const WINDOW_ACTION_TAXONOMY_ID: &str = "window-action.window-point.pointer-click.capture-evidence";

fn is_native_text_taxonomy_id(raw: &str) -> bool {
  matches!(
    raw.trim(),
    NATIVE_TEXT_CANONICAL_TAXONOMY_ID | NATIVE_TEXT_LEGACY_TAXONOMY_ID
  )
}

fn canonicalize_app_candidate_grounding_taxonomy_id(raw: &str) -> &str {
  if is_native_text_taxonomy_id(raw) {
    NATIVE_TEXT_CANONICAL_TAXONOMY_ID
  } else {
    raw.trim()
  }
}

impl AppCandidateGroundingTaxonomy {
  fn parse(raw: &str) -> AuvResult<Self> {
    match canonicalize_app_candidate_grounding_taxonomy_id(raw) {
      SEARCH_ENTRY_TAXONOMY_ID => Ok(Self::SearchEntryAxTextInputClipboardSubmitCaptureEvidence),
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID => {
        Ok(Self::NativeTextAxTextAxPerformActionClipboardPasteVerifyAxText)
      }
      RESULT_SELECTION_TAXONOMY_ID => Ok(Self::ResultSelectionOcrAnchorPointerClickCaptureEvidence),
      WINDOW_ACTION_TAXONOMY_ID => Ok(Self::WindowActionWindowPointPointerClickCaptureEvidence),
      other => Err(format!(
        "unsupported candidate grounding taxonomy {}. allowed values: {}",
        other,
        Self::allowed_ids().join(", ")
      )),
    }
  }

  fn allowed_ids() -> &'static [&'static str] {
    &[
      SEARCH_ENTRY_TAXONOMY_ID,
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      NATIVE_TEXT_LEGACY_TAXONOMY_ID,
      RESULT_SELECTION_TAXONOMY_ID,
      WINDOW_ACTION_TAXONOMY_ID,
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
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub evidence_refs: Vec<ArtifactRef>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub candidate_query: Option<crate::contract::CandidateQuery>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub promotion_gate: Option<AppCandidatePromotionGate>,
  #[serde(default)]
  pub input_bindings: BTreeMap<String, String>,
  #[serde(default)]
  pub compatibility: AppCandidateCompatibility,
  #[serde(default)]
  pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppCandidatePromotionGate {
  pub status: AppCandidatePromotionStatus,
  #[serde(default)]
  pub missing_gates: Vec<String>,
  #[serde(default)]
  pub notes: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppCandidatePromotionStatus {
  Blocked,
  DistillStrategyOnly,
  ActionGradeCandidate,
}

impl AppCandidatePromotionStatus {
  pub(crate) fn as_str(&self) -> &'static str {
    match self {
      Self::Blocked => "blocked",
      Self::DistillStrategyOnly => "distill_strategy_only",
      Self::ActionGradeCandidate => "action_grade_candidate",
    }
  }
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
    "list-windows",
    "debug.listWindows",
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
    "capture-ax-tree",
    "debug.captureAxTree",
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
      source_evidence_refs: source_evidence_refs_for_candidate_shape(&analysis, &candidate_shape),
      promoted_candidate: promoted_candidate_for_candidate_shape(
        &analysis,
        &strategy.taxonomy_id,
        &candidate_shape,
      ),
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
    let mut manifest: SkillManifest = read_json(&candidate.recipe_path)?;
    let mut matrix: SkillCaseMatrix = read_json(&candidate.case_matrix_path)?;
    let verification_mode =
      verification_mode_for_strategy(&manifest.strategy).map_err(|error| {
        format!(
          "candidate {} uses an unsupported verification contract: {error}",
          candidate.recipe_id
        )
      })?;
    let mut resolved_inputs: BTreeMap<String, String> = BTreeMap::new();
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
    inject_promoted_candidate_runtime_inputs(
      candidate,
      &mut manifest,
      &mut matrix,
      &mut resolved_inputs,
    )
    .map_err(|error| {
      format!(
        "candidate {} has invalid promoted candidate payload: {error}",
        candidate.recipe_id
      )
    })?;
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
        Ok(case_summary) => {
          let promoted_runtime_contract =
            promoted_candidate_runtime_contract(&candidate.taxonomy_id);
          let observed_consumer = promoted_runtime_contract.as_ref().and_then(|contract| {
            observed_signal_from_exported_variables(
              &case_summary.exported_variables,
              contract.consumer_signal_key,
            )
          });
          let observed_candidate_local_id =
            promoted_runtime_contract.as_ref().and_then(|contract| {
              observed_signal_from_exported_variables(
                &case_summary.exported_variables,
                contract.candidate_id_signal_key,
              )
            });
          let candidate_source = candidate_source_from_validation_observation(
            observed_consumer.as_deref(),
            observed_candidate_local_id.as_deref(),
          );
          let success_outcome = classify_successful_validation_outcome(
            &candidate.taxonomy_id,
            selected_case_count,
            verification_mode,
            observed_consumer.as_deref(),
            candidate.promoted_candidate.is_some(),
          );
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
            status: success_outcome.status,
            verification_mode,
            rationale: success_outcome.rationale,
            used_annotation_ids: used_annotation_ids.clone(),
            recipe_path: candidate.recipe_path.clone(),
            case_matrix_path: candidate.case_matrix_path.clone(),
            selected_case_count,
            observed_consumer,
            observed_candidate_local_id,
            candidate_source,
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
            observed_consumer: None,
            observed_candidate_local_id: None,
            candidate_source: None,
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
        observed_consumer: None,
        observed_candidate_local_id: None,
        candidate_source: None,
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

fn inject_promoted_candidate_runtime_inputs(
  candidate: &AppDistilledCandidate,
  manifest: &mut SkillManifest,
  matrix: &mut SkillCaseMatrix,
  resolved_inputs: &mut BTreeMap<String, String>,
) -> AuvResult<()> {
  let Some(promoted_candidate) = candidate.promoted_candidate.as_ref() else {
    return Ok(());
  };

  let Some(contract) = promoted_candidate_runtime_contract(&candidate.taxonomy_id) else {
    return Ok(());
  };

  let serialized = serde_json::to_string(promoted_candidate)
    .map_err(|error| format!("failed to serialize promoted candidate: {error}"))?;
  ensure_manifest_string_input(
    manifest,
    contract.candidate_input_key,
    Some(Value::String(serialized.clone())),
    contract.candidate_note,
  );
  if let Some(fallback_input_key) = contract.fallback_input_key
    && !candidate
      .candidate_shape
      .provided_inputs
      .contains_key(fallback_input_key)
    && !resolved_inputs.contains_key(fallback_input_key)
    && let Some(anchor_text) = promoted_candidate.target_spec.anchor_text.as_ref()
  {
    ensure_manifest_string_input(
      manifest,
      fallback_input_key,
      Some(Value::String(anchor_text.clone())),
      contract.fallback_note,
    );
  }
  enforce_promoted_candidate_consumer_expectations(manifest, &contract, promoted_candidate);
  for case in &mut matrix.cases {
    case
      .inputs
      .entry(contract.candidate_input_key.to_string())
      .or_insert_with(|| serialized.clone());
    if let Some(fallback_input_key) = contract.fallback_input_key
      && let Some(anchor_text) = promoted_candidate.target_spec.anchor_text.as_ref()
    {
      case
        .inputs
        .entry(fallback_input_key.to_string())
        .or_insert_with(|| anchor_text.clone());
    }
  }
  resolved_inputs
    .entry(contract.candidate_input_key.to_string())
    .or_insert(serialized);
  if let Some(fallback_input_key) = contract.fallback_input_key
    && let Some(anchor_text) = promoted_candidate.target_spec.anchor_text.as_ref()
  {
    resolved_inputs
      .entry(fallback_input_key.to_string())
      .or_insert_with(|| anchor_text.clone());
  }

  Ok(())
}

#[derive(Clone, Copy)]
struct PromotedCandidateRuntimeContract {
  candidate_input_key: &'static str,
  candidate_note: &'static str,
  fallback_input_key: Option<&'static str>,
  fallback_note: &'static str,
  consumer_signal_key: &'static str,
  candidate_id_signal_key: &'static str,
}

fn promoted_candidate_runtime_contract(
  taxonomy_id: &str,
) -> Option<PromotedCandidateRuntimeContract> {
  match canonicalize_app_candidate_grounding_taxonomy_id(taxonomy_id) {
    SEARCH_ENTRY_TAXONOMY_ID => Some(PromotedCandidateRuntimeContract {
      candidate_input_key: "focus_candidate",
      candidate_note: "Validate injects the promoted search-entry contract::Candidate here so debug.focusTextInput can consume the typed target without reopening app-only schema.",
      fallback_input_key: Some("focus_query"),
      fallback_note: "Legacy fallback for search-entry validate. TODO(app-search-entry-query-fallback-removal): remove once the query-only path is no longer needed by existing recipes.",
      consumer_signal_key: "focusTextInput.consumer",
      candidate_id_signal_key: "focusTextInput.candidateLocalId",
    }),
    NATIVE_TEXT_CANONICAL_TAXONOMY_ID => Some(PromotedCandidateRuntimeContract {
      candidate_input_key: "focus_candidate",
      candidate_note: "Validate injects the promoted native-text contract::Candidate here so debug.focusTextInput can consume the typed target without reopening app-only schema.",
      fallback_input_key: Some("focus_query"),
      fallback_note: "Legacy fallback for native-text validate. TODO(app-native-text-query-fallback-removal): remove once the query-only path is no longer needed by existing recipes.",
      consumer_signal_key: "focusTextInput.consumer",
      candidate_id_signal_key: "focusTextInput.candidateLocalId",
    }),
    WINDOW_ACTION_TAXONOMY_ID => Some(PromotedCandidateRuntimeContract {
      candidate_input_key: "click_candidate",
      candidate_note: "Validate injects the promoted window-action contract::Candidate here so debug.clickWindowPoint can consume the typed target without reopening app-only schema.",
      fallback_input_key: None,
      fallback_note: "",
      consumer_signal_key: "clickWindowPoint.consumer",
      candidate_id_signal_key: "clickWindowPoint.candidateLocalId",
    }),
    RESULT_SELECTION_TAXONOMY_ID => Some(PromotedCandidateRuntimeContract {
      candidate_input_key: "click_candidate",
      candidate_note: "Validate injects the promoted result-selection contract::Candidate here so debug.clickWindowText can consume the typed OCR anchor target without reopening app-only schema.",
      fallback_input_key: Some("anchor_text"),
      fallback_note: "Legacy fallback for result-selection validate. TODO(app-result-selection-anchor-fallback-removal): remove once the query-only path is no longer needed by existing recipes.",
      consumer_signal_key: "clickWindowText.consumer",
      candidate_id_signal_key: "clickWindowText.candidateLocalId",
    }),
    _ => None,
  }
}

fn enforce_promoted_candidate_consumer_expectations(
  manifest: &mut SkillManifest,
  contract: &PromotedCandidateRuntimeContract,
  promoted_candidate: &crate::contract::Candidate,
) {
  for step in &mut manifest.steps {
    if !step_references_input(step, contract.candidate_input_key) {
      continue;
    }
    step.expect.signal_equals.insert(
      contract.consumer_signal_key.to_string(),
      "contract-candidate".to_string(),
    );
    step.expect.signal_equals.insert(
      contract.candidate_id_signal_key.to_string(),
      promoted_candidate.candidate_local_id.clone(),
    );
  }
}

fn step_references_input(step: &crate::skill::SkillStep, input_key: &str) -> bool {
  step
    .args
    .values()
    .any(|value| value_references_input(value, input_key))
}

fn value_references_input(value: &Value, input_key: &str) -> bool {
  let placeholder = format!("${{{input_key}}}");
  match value {
    Value::String(string) => string == &placeholder,
    Value::Array(values) => values
      .iter()
      .any(|nested| value_references_input(nested, input_key)),
    Value::Object(map) => map
      .values()
      .any(|nested| value_references_input(nested, input_key)),
    _ => false,
  }
}

fn observed_signal_from_resolved_inputs(
  resolved_inputs: &BTreeMap<String, String>,
  signal_key: &str,
) -> Option<String> {
  let suffix = format!(
    "_signal_{}",
    sanitize_validation_signal_component(signal_key)
  );
  resolved_inputs
    .iter()
    .find_map(|(key, value)| key.ends_with(&suffix).then(|| value.clone()))
}

fn observed_signal_from_exported_variables(
  exported_variables: &BTreeMap<String, String>,
  signal_key: &str,
) -> Option<String> {
  observed_signal_from_resolved_inputs(exported_variables, signal_key)
}

fn candidate_source_from_validation_observation(
  observed_consumer: Option<&str>,
  observed_candidate_local_id: Option<&str>,
) -> Option<String> {
  match observed_consumer {
    Some("contract-candidate") if observed_candidate_local_id.is_some() => {
      Some("promoted_candidate".to_string())
    }
    Some("query") => Some("query_fallback".to_string()),
    Some(other) => Some(format!("consumer:{other}")),
    None => None,
  }
}

fn sanitize_validation_signal_component(raw: &str) -> String {
  let lowered = raw.trim().to_lowercase().replace('-', "_");
  let collapsed = lowered
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() || character == '_' {
        character
      } else {
        '_'
      }
    })
    .collect::<String>();
  collapsed
    .split('_')
    .filter(|segment| !segment.is_empty())
    .collect::<Vec<_>>()
    .join("_")
}

struct SuccessfulValidationOutcome {
  status: AppValidationStatus,
  rationale: String,
}

fn classify_successful_validation_outcome(
  taxonomy_id: &str,
  selected_case_count: usize,
  verification_mode: AppVerificationMode,
  observed_consumer: Option<&str>,
  has_promoted_candidate: bool,
) -> SuccessfulValidationOutcome {
  // TODO(app-validate-consumer-status-v1): extend consumer-aware success
  // classification to the other promoted consumer seams once the owner asks for
  // the same tightening beyond native-text.
  // TODO(app-native-text-ax-focus-adoption-v1): native-text can now validate
  // through debug.axFocusTextInput's promoted consumer signals, but recipe/app
  // adoption still needs to move the real consumer surface off the legacy
  // pointer-warp focus path where appropriate.
  if is_native_text_taxonomy_id(taxonomy_id) {
    return match observed_consumer {
      Some("contract-candidate") => SuccessfulValidationOutcome {
        status: AppValidationStatus::Validated,
        rationale: validated_candidate_rationale(selected_case_count, verification_mode),
      },
      Some("query") => SuccessfulValidationOutcome {
        status: AppValidationStatus::Candidate,
        rationale: if has_promoted_candidate {
          format!(
            "All {} candidate case(s) executed successfully through the shared runtime and {} verification passed, but validate still observed the legacy `query` consumer instead of `contract-candidate`. Keep this native-text slice as candidate until the promoted consumer seam is exercised end-to-end.",
            selected_case_count,
            verification_mode.as_str(),
          )
        } else {
          format!(
            "All {} candidate case(s) executed successfully through the shared runtime and {} verification passed, but validate only exercised the legacy `query` fallback for native-text. Keep this slice as candidate until the promoted consumer seam is exercised end-to-end.",
            selected_case_count,
            verification_mode.as_str(),
          )
        },
      },
      Some(other) => SuccessfulValidationOutcome {
        status: AppValidationStatus::Candidate,
        rationale: format!(
          "All {} candidate case(s) executed successfully through the shared runtime and {} verification passed, but validate observed unexpected native-text consumer `{}`. Keep this slice as candidate until the promoted consumer seam is explicit and stable.",
          selected_case_count,
          verification_mode.as_str(),
          other,
        ),
      },
      None => SuccessfulValidationOutcome {
        status: AppValidationStatus::Candidate,
        rationale: format!(
          "All {} candidate case(s) executed successfully through the shared runtime and {} verification passed, but validate did not observe a native-text consumer signal. Keep this slice as candidate until the promoted consumer seam is explicit and stable.",
          selected_case_count,
          verification_mode.as_str(),
        ),
      },
    };
  }

  SuccessfulValidationOutcome {
    status: AppValidationStatus::Validated,
    rationale: validated_candidate_rationale(selected_case_count, verification_mode),
  }
}

fn ensure_manifest_string_input(
  manifest: &mut SkillManifest,
  input_key: &str,
  default: Option<Value>,
  note: &str,
) {
  use std::collections::btree_map::Entry;

  match manifest.inputs.entry(input_key.to_string()) {
    Entry::Occupied(mut entry) => {
      if entry.get().kind.trim().is_empty() {
        entry.get_mut().kind = "string".to_string();
      }
      if entry.get().default.is_none() {
        entry.get_mut().default = default;
      }
      if entry.get().note.trim().is_empty() {
        entry.get_mut().note = note.to_string();
      }
    }
    Entry::Vacant(entry) => {
      entry.insert(crate::skill::SkillInputSpec {
        kind: "string".to_string(),
        default,
        note: note.to_string(),
      });
    }
  }
}

fn default_distill_output_dir(analysis_path: &Path, analysis: &AppAnalysis) -> PathBuf {
  let base = analysis_path.parent().unwrap_or_else(|| Path::new("."));
  base.join("distill").join(format!(
    "{}-{}",
    recipe_app_slug(&analysis.app_identity),
    now_millis()
  ))
}

#[cfg(test)]
mod tests {
  use super::analysis::{
    apply_candidate_grounding, build_annotation_candidates, build_app_analysis,
    build_distilled_candidate_shape, candidate_compatibility, recommended_strategy,
    suggested_annotation_ids_for_candidate_shape,
  };
  use super::infra::{
    invoke_probe_step, read_json, resolve_distillation_path, resolve_probe_path, write_pretty_json,
  };
  use super::recipe::{
    render_native_text_candidate_cases, render_native_text_candidate_recipe,
    render_search_entry_candidate_cases, render_window_action_candidate_cases,
    render_window_action_candidate_recipe,
  };
  use super::*;
  use crate::catalog::CommandCatalog;
  use crate::contract::{
    AnchorRecheckPrecondition, ArtifactRef, CandidateEvidence, CandidateLiveness, CandidateQuery,
    ControlRequirements, LivenessPreconditions, RatioRegion, SelectorScope, SurfaceSelector,
    SurfaceSelectorClause, TargetSpec, WindowRefPrecondition,
  };
  use crate::driver::{Driver, DriverRegistry};
  use crate::model::RunStatus;
  use crate::model::{
    CommandSpec, DisturbanceClass, DriverCall, DriverDescriptor, DriverResponse, ProducedArtifact,
  };
  use crate::recording::{MemoryRunRecorder, RunUpdate};
  use crate::run_builder::RunSpec;
  use crate::skill::SkillCase;
  use crate::store::LocalStore;
  use crate::trace::RunType;
  use auv_driver_macos::types::{
    ObservedAxNode, ObservedAxTreeSnapshot, ObservedRect, OcrTextMatch, OcrTextSnapshot,
  };
  use serde_json::Value;
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
      if call.operation == "artifact" {
        let first_path = call.working_directory.join("probe-first-artifact.txt");
        let second_path = call.working_directory.join("probe-second-artifact.txt");
        fs::write(&first_path, "first artifact").expect("first artifact should write");
        fs::write(&second_path, "second artifact").expect("second artifact should write");
        return Ok(DriverResponse {
          summary: "artifact ok".to_string(),
          artifacts: vec![
            ProducedArtifact {
              kind: "text".to_string(),
              source_path: first_path,
              preferred_name: "first.txt".to_string(),
              note: Some("first".to_string()),
            },
            ProducedArtifact {
              kind: "text".to_string(),
              source_path: second_path,
              preferred_name: "second.txt".to_string(),
              note: Some("second".to_string()),
            },
          ],
          notes: Vec::new(),
          signals: BTreeMap::from([("outcome".to_string(), "ok".to_string())]),
          backend: None,
        });
      }

      if call.operation == "test_operation"
        && call
          .inputs
          .get("require_focus_candidate")
          .map(String::as_str)
          == Some("true")
      {
        let raw_candidate = call
          .inputs
          .get("candidate")
          .ok_or_else(|| "expected candidate input".to_string())?;
        let candidate: crate::contract::Candidate = serde_json::from_str(raw_candidate)
          .map_err(|error| format!("candidate was not valid Candidate JSON: {error}"))?;
        if candidate.target_spec.grounding != crate::contract::TargetGrounding::AxNode {
          return Err(format!(
            "expected AxNode candidate grounding, got {:?}",
            candidate.target_spec.grounding
          ));
        }
        let expected_query = call
          .inputs
          .get("query")
          .ok_or_else(|| "expected query fallback input".to_string())?;
        if candidate.target_spec.anchor_text.as_deref() != Some(expected_query.as_str()) {
          return Err(format!(
            "candidate anchor_text {:?} did not match query {}",
            candidate.target_spec.anchor_text, expected_query
          ));
        }
        return Ok(DriverResponse {
          summary: format!("{} ok", call.operation),
          artifacts: Vec::new(),
          notes: Vec::new(),
          signals: BTreeMap::from([
            ("outcome".to_string(), "ok".to_string()),
            (
              "focusTextInput.consumer".to_string(),
              "contract-candidate".to_string(),
            ),
            (
              "focusTextInput.candidateLocalId".to_string(),
              candidate.candidate_local_id,
            ),
          ]),
          backend: None,
        });
      }

      if call.operation == "test_operation"
        && call
          .inputs
          .get("query")
          .map(|query| !query.is_empty())
          .unwrap_or(false)
      {
        return Ok(DriverResponse {
          summary: format!("{} ok", call.operation),
          artifacts: Vec::new(),
          notes: Vec::new(),
          signals: BTreeMap::from([
            ("outcome".to_string(), "ok".to_string()),
            ("focusTextInput.consumer".to_string(), "query".to_string()),
          ]),
          backend: None,
        });
      }

      if call.operation == "test_operation"
        && call
          .inputs
          .get("require_click_candidate")
          .map(String::as_str)
          == Some("true")
      {
        let raw_candidate = call
          .inputs
          .get("click_candidate")
          .ok_or_else(|| "expected click_candidate input".to_string())?;
        let candidate: crate::contract::Candidate = serde_json::from_str(raw_candidate)
          .map_err(|error| format!("click_candidate was not valid Candidate JSON: {error}"))?;
        let signals = match candidate.target_spec.grounding {
          crate::contract::TargetGrounding::OcrAnchor => {
            let expected_query = call
              .inputs
              .get("anchor_text")
              .ok_or_else(|| "expected anchor_text fallback input".to_string())?;
            if candidate.target_spec.anchor_text.as_deref() != Some(expected_query.as_str()) {
              return Err(format!(
                "click_candidate anchor_text {:?} did not match anchor_text {}",
                candidate.target_spec.anchor_text, expected_query
              ));
            }
            BTreeMap::from([
              ("outcome".to_string(), "ok".to_string()),
              (
                "clickWindowText.consumer".to_string(),
                "contract-candidate".to_string(),
              ),
              (
                "clickWindowText.candidateLocalId".to_string(),
                candidate.candidate_local_id,
              ),
            ])
          }
          crate::contract::TargetGrounding::Coordinate => BTreeMap::from([
            ("outcome".to_string(), "ok".to_string()),
            (
              "clickWindowPoint.consumer".to_string(),
              "contract-candidate".to_string(),
            ),
            (
              "clickWindowPoint.candidateLocalId".to_string(),
              candidate.candidate_local_id,
            ),
          ]),
          other => {
            return Err(format!(
              "expected OcrAnchor or Coordinate click_candidate grounding, got {:?}",
              other
            ));
          }
        };
        return Ok(DriverResponse {
          summary: format!("{} ok", call.operation),
          artifacts: Vec::new(),
          notes: Vec::new(),
          signals,
          backend: None,
        });
      }

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
  fn recommended_native_text_strategy_uses_ax_backed_taxonomy_id() {
    let strategy = recommended_strategy(
      "native-text",
      "ax-text",
      "ax-perform-action-clipboard-paste",
      "verifyAxText",
      AssessmentStatus::Candidate,
      "test rationale",
    )
    .expect("taxonomy should be valid");
    assert_eq!(strategy.taxonomy_id, NATIVE_TEXT_CANONICAL_TAXONOMY_ID);
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

    let run_id = runtime
      .finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some("probe complete".to_string()),
          failure: None,
        },
      )
      .expect("probe run should finish");
    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    let first_probe_span = canonical
      .spans
      .iter()
      .find(|span| span.name == "auv.probe.step")
      .expect("first probe step span should be recorded");
    assert_eq!(
      first_probe_span.attributes.get("auv.probe.step_id"),
      Some(&serde_json::json!("first"))
    );
    assert_eq!(
      first_probe_span.attributes.get("auv.step.id"),
      Some(&serde_json::json!("first"))
    );
    assert_eq!(
      first_probe_span.attributes.get("auv.step.kind"),
      Some(&serde_json::json!("probe"))
    );
    assert!(!first_probe_span.attributes.contains_key("auv.step.index"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn invoke_probe_step_preserves_artifact_metadata_order() {
    let root = temp_dir("probe-step-artifact-metadata");
    let runtime = test_runtime(root.clone());
    let mut run = runtime
      .start_run(RunSpec::new(RunType::Probe, "auv.probe"))
      .expect("probe run should start");
    let root_span = run.root_span();

    let step = invoke_probe_step(
      &runtime,
      &mut run,
      &root_span,
      "artifact-step",
      "test.artifact",
      None,
      BTreeMap::new(),
      false,
    )
    .expect("artifact step should complete");

    assert_eq!(step.artifact_paths.len(), 2);
    assert_eq!(step.artifacts.len(), 2);
    assert_eq!(step.artifacts[0].artifact_id, "artifact_0001");
    assert_eq!(step.artifacts[1].artifact_id, "artifact_0002");
    assert_eq!(step.artifacts[0].path, step.artifact_paths[0]);
    assert_eq!(step.artifacts[1].path, step.artifact_paths[1]);
    assert_eq!(step.artifacts[0].role, "text");
    assert_eq!(step.artifacts[1].role, "text");
    assert_eq!(step.artifacts[0].span_id, step.artifacts[1].span_id);
    assert_ne!(step.artifacts[0].span_id, step.span_id);

    let _ = runtime
      .finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some("probe complete".to_string()),
          failure: None,
        },
      )
      .expect("probe run should finish");
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
        evidence_step_id: "capture-ax-tree".to_string(),
        candidate_query: Some(CandidateQuery {
          query_id: "search-entry-focus-ax".to_string(),
          selector: SurfaceSelector {
            any_of: vec![SurfaceSelectorClause::Ax {
              role: Some("AXTextField".to_string()),
              label: Some("Search".to_string()),
              path: Some("0.1".to_string()),
              enabled: None,
              visible: Some(true),
            }],
            within: SelectorScope::TargetWindow,
            require_visible: true,
          },
          output_kind: Some("focus-query".to_string()),
          known_limits: vec!["test query".to_string()],
        }),
        evidence_refs: vec![ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        }],
        promotion_gate: Some(AppCandidatePromotionGate {
          status: AppCandidatePromotionStatus::ActionGradeCandidate,
          missing_gates: Vec::new(),
          notes: vec!["Sample candidate satisfies the v0 search-entry promotion seam.".to_string()],
        }),
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
    assert!(report.contains("candidateQuery"));
    assert!(report.contains("sources=`ax`"));
    assert!(report.contains("evidenceRefs"));
    assert!(report.contains("promotionGate: `action_grade_candidate`"));
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
    assert!(manifest.inputs.contains_key("focus_candidate"));
    assert_eq!(
      manifest.steps[1].args.get("candidate"),
      Some(&serde_json::json!("${focus_candidate}"))
    );
  }

  #[test]
  fn native_text_distillation_template_validates() {
    let analysis = sample_analysis_with_strategy(NATIVE_TEXT_CANONICAL_TAXONOMY_ID);
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, &analysis.recommended_strategies[0].taxonomy_id);
    let recipe = render_native_text_candidate_recipe(&analysis, &candidate_shape)
      .expect("recipe should render");
    let manifest: SkillManifest =
      serde_json::from_value(recipe).expect("candidate recipe should parse");
    validate_skill_manifest(&manifest).expect("candidate recipe should validate");
    let matrix_value = render_native_text_candidate_cases(&analysis, &candidate_shape)
      .expect("matrix should render");
    let matrix: SkillCaseMatrix =
      serde_json::from_value(matrix_value).expect("candidate matrix should parse");
    validate_case_matrix_manifest(&matrix).expect("candidate matrix should validate");
    validate_case_matrix_against_skill(&manifest, &matrix).expect("candidate matrix should align");
  }

  #[test]
  fn native_text_distillation_template_keeps_query_scaffold_without_promotion() {
    let analysis = sample_analysis_with_strategy(NATIVE_TEXT_CANONICAL_TAXONOMY_ID);
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, &analysis.recommended_strategies[0].taxonomy_id);
    let recipe = render_native_text_candidate_recipe(&analysis, &candidate_shape)
      .expect("recipe should render");
    let manifest: SkillManifest =
      serde_json::from_value(recipe).expect("candidate recipe should parse");

    validate_skill_manifest(&manifest).expect("candidate recipe should validate");
    assert_eq!(
      manifest.strategy.activation.as_str(),
      "ax-perform-action-clipboard-paste"
    );
    assert_eq!(
      manifest.disturbance_policy.max_disturbance.as_str(),
      "clipboard"
    );
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-text-surface")
      .expect("focus-text-surface step should exist");
    assert_eq!(step.command_id, "debug.axFocusTextInput");
    assert_eq!(step.disturbance.max.as_str(), "keyboard");
    assert_eq!(
      step.disturbance.classes,
      vec!["foreground_app".to_string(), "keyboard".to_string()]
    );
    assert_eq!(
      manifest
        .inputs
        .get("focus_candidate")
        .and_then(|input| input.default.as_ref()),
      Some(&serde_json::json!(""))
    );
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"query".to_string())
    );
    assert_eq!(
      step
        .expect
        .signal_equals
        .get("focusTextInput.candidateLocalId"),
      None
    );
  }

  #[test]
  fn native_text_distillation_template_defaults_to_contract_candidate_when_promotable() {
    let analysis = sample_promotable_ax_focus_analysis(
      "native-text",
      "native-text-focus-ax",
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      "Editor",
      "Sample candidate satisfies the v0 native-text promotion seam.",
    );
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, &analysis.recommended_strategies[0].taxonomy_id);
    let promoted_candidate = promoted_candidate_for_candidate_shape(
      &analysis,
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      &candidate_shape,
    )
    .expect("native-text candidate should promote");
    let manifest: SkillManifest = serde_json::from_value(
      render_native_text_candidate_recipe(&analysis, &candidate_shape)
        .expect("candidate recipe should render"),
    )
    .expect("candidate recipe should parse");
    let matrix: SkillCaseMatrix = serde_json::from_value(
      render_native_text_candidate_cases(&analysis, &candidate_shape)
        .expect("candidate matrix should render"),
    )
    .expect("candidate matrix should parse");

    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-text-surface")
      .expect("focus-text-surface step should exist");
    assert_eq!(
      manifest.strategy.activation.as_str(),
      "ax-perform-action-clipboard-paste"
    );
    assert_eq!(
      manifest.disturbance_policy.max_disturbance.as_str(),
      "clipboard"
    );
    assert_eq!(step.command_id, "debug.axFocusTextInput");
    assert_eq!(step.disturbance.max.as_str(), "keyboard");
    assert_eq!(
      step.disturbance.classes,
      vec!["foreground_app".to_string(), "keyboard".to_string()]
    );
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"contract-candidate".to_string())
    );
    assert_eq!(
      step
        .expect
        .signal_equals
        .get("focusTextInput.candidateLocalId"),
      Some(&promoted_candidate.candidate_local_id)
    );
    let serialized_candidate = manifest
      .inputs
      .get("focus_candidate")
      .and_then(|input| input.default.as_ref())
      .and_then(|value| value.as_str())
      .expect("focus_candidate default should be serialized candidate");
    let parsed_candidate: crate::contract::Candidate = serde_json::from_str(serialized_candidate)
      .expect("focus_candidate default should stay valid candidate JSON");
    assert_eq!(parsed_candidate.candidate_local_id, "native-text-focus-ax");
    assert_eq!(
      matrix.cases[0].inputs.get("focus_candidate"),
      Some(&serialized_candidate.to_string())
    );
    assert_eq!(
      matrix.cases[0].inputs.get("focus_query"),
      Some(&"Editor".to_string())
    );
  }

  #[test]
  fn search_entry_distillation_template_remains_pointer_focus_authoring() {
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
    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-search-input")
      .expect("focus-search-input step should exist");

    assert_eq!(manifest.strategy.activation.as_str(), "clipboard-submit");
    assert_eq!(step.command_id, "debug.focusTextInput");
    assert_eq!(step.disturbance.max.as_str(), "pointer");
  }

  #[test]
  fn promoted_candidate_consumer_expectations_preserve_unrelated_expect_fields() {
    let analysis = sample_promotable_ax_focus_analysis(
      "native-text",
      "native-text-focus-ax",
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      "Editor",
      "Sample candidate satisfies the v0 native-text promotion seam.",
    );
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, &analysis.recommended_strategies[0].taxonomy_id);
    let promoted_candidate = promoted_candidate_for_candidate_shape(
      &analysis,
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      &candidate_shape,
    )
    .expect("native-text candidate should promote");
    let mut manifest: SkillManifest = serde_json::from_value(
      render_native_text_candidate_recipe(&analysis, &candidate_shape)
        .expect("candidate recipe should render"),
    )
    .expect("candidate recipe should parse");
    let step = manifest
      .steps
      .iter_mut()
      .find(|step| step.id == "focus-text-surface")
      .expect("focus-text-surface step should exist");
    step
      .expect
      .signal_contains
      .insert("focusTextInput.debug".to_string(), "candidate".to_string());
    step.expect.artifact_count_at_least = Some(1);
    let contract = promoted_candidate_runtime_contract(NATIVE_TEXT_CANONICAL_TAXONOMY_ID)
      .expect("native-text contract should exist");

    enforce_promoted_candidate_consumer_expectations(&mut manifest, &contract, &promoted_candidate);

    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-text-surface")
      .expect("focus-text-surface step should exist");
    assert_eq!(
      step.expect.signal_contains.get("focusTextInput.debug"),
      Some(&"candidate".to_string())
    );
    assert_eq!(step.expect.artifact_count_at_least, Some(1));
  }

  #[test]
  fn promoted_candidate_consumer_expectations_override_legacy_query_focus_consumer() {
    let analysis = sample_promotable_ax_focus_analysis(
      "native-text",
      "native-text-focus-ax",
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      "Editor",
      "Sample candidate satisfies the v0 native-text promotion seam.",
    );
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, &analysis.recommended_strategies[0].taxonomy_id);
    let promoted_candidate = promoted_candidate_for_candidate_shape(
      &analysis,
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      &candidate_shape,
    )
    .expect("native-text candidate should promote");
    let mut manifest: SkillManifest = serde_json::from_value(
      render_native_text_candidate_recipe(&analysis, &candidate_shape)
        .expect("candidate recipe should render"),
    )
    .expect("candidate recipe should parse");
    let contract = promoted_candidate_runtime_contract(NATIVE_TEXT_CANONICAL_TAXONOMY_ID)
      .expect("native-text contract should exist");

    enforce_promoted_candidate_consumer_expectations(&mut manifest, &contract, &promoted_candidate);

    let step = manifest
      .steps
      .iter()
      .find(|step| step.id == "focus-text-surface")
      .expect("focus-text-surface step should exist");
    assert_eq!(
      step.expect.signal_equals.get("focusTextInput.consumer"),
      Some(&"contract-candidate".to_string())
    );
    assert_eq!(
      step
        .expect
        .signal_equals
        .get("focusTextInput.candidateLocalId"),
      Some(&promoted_candidate.candidate_local_id)
    );
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
    assert!(manifest.inputs.contains_key("click_candidate"));
    assert_eq!(
      manifest.steps[1].args.get("candidate"),
      Some(&serde_json::json!("${click_candidate}"))
    );
  }

  #[test]
  fn result_selection_distillation_template_validates() {
    let analysis =
      sample_analysis_with_strategy("result-selection.ocr-anchor.pointer-click.capture-evidence");
    let recipe = render_candidate_recipe(
      &analysis,
      &analysis.recommended_strategies[0],
      &AppDistilledCandidateShape::default(),
    )
    .expect("candidate recipe should render");
    let manifest: SkillManifest =
      serde_json::from_value(recipe).expect("candidate recipe should parse");
    validate_skill_manifest(&manifest).expect("candidate recipe should validate");
    let matrix_value = render_candidate_case_matrix(
      &analysis,
      &analysis.recommended_strategies[0],
      &AppDistilledCandidateShape::default(),
    )
    .expect("candidate matrix should render");
    let matrix: SkillCaseMatrix =
      serde_json::from_value(matrix_value).expect("candidate matrix should parse");
    validate_case_matrix_manifest(&matrix).expect("candidate matrix should validate");
    validate_case_matrix_against_skill(&manifest, &matrix).expect("candidate matrix should align");
    assert!(manifest.inputs.contains_key("click_candidate"));
    assert_eq!(manifest.steps[1].command_id, "debug.clickWindowText");
    assert_eq!(
      manifest.steps[1].args.get("candidate"),
      Some(&serde_json::json!("${click_candidate}"))
    );
    assert_eq!(manifest.steps[2].command_id, "debug.captureWindow");
  }

  #[test]
  fn recommended_surface_strategy_projects_to_direct_candidate_shape() {
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
        primary_window_bounds: Some(AppRect {
          x: 100,
          y: 200,
          width: 800,
          height: 600,
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
        stable_region_candidates: vec!["primaryWindow=100,200,800,600".to_string()],
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
      annotation_candidates: vec![AppSurfaceCandidate {
        candidate_id: "window-primary-region".to_string(),
        area: "window.primary".to_string(),
        kind: "region".to_string(),
        source: "window".to_string(),
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
        evidence_step_id: "list-windows".to_string(),
        candidate_query: None,
        evidence_refs: vec![ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0002"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        }],
        promotion_gate: Some(AppCandidatePromotionGate {
          status: AppCandidatePromotionStatus::ActionGradeCandidate,
          missing_gates: Vec::new(),
          notes: vec!["Window region satisfies the v0 window-action promotion seam.".to_string()],
        }),
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
      }],
      known_boundaries: vec![],
      recommended_strategies: vec![AppRecommendedStrategy {
        taxonomy_id: "window-action.window-point.pointer-click.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
      }],
    };

    let candidate_shape = build_distilled_candidate_shape(
      &analysis,
      "window-action.window-point.pointer-click.capture-evidence",
    );

    assert_eq!(
      candidate_shape.direct_candidate_ids,
      vec!["window-primary-region".to_string()]
    );
    assert!(candidate_shape.context_candidate_ids.is_empty());
    assert!(candidate_shape.notes.is_empty());
  }

  #[test]
  fn search_entry_candidates_expose_action_grade_promotion_gate() {
    let analysis = sample_promotable_ax_focus_analysis(
      "search-entry",
      "search-entry-focus-ax",
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
      "Search",
      "Sample candidate satisfies the v0 search-entry promotion seam.",
    );
    let candidate = analysis
      .annotation_candidates
      .iter()
      .find(|candidate| candidate.candidate_id == "search-entry-focus-ax")
      .expect("search-entry candidate should exist");
    let promotion_gate = candidate
      .promotion_gate
      .as_ref()
      .expect("search-entry candidate should expose promotion gate");
    assert_eq!(
      promotion_gate.status,
      AppCandidatePromotionStatus::ActionGradeCandidate
    );
    assert!(promotion_gate.missing_gates.is_empty());
  }

  #[test]
  fn window_action_candidates_expose_action_grade_promotion_gate() {
    let analysis = sample_promotable_window_action_analysis();
    let candidate = analysis
      .annotation_candidates
      .iter()
      .find(|candidate| candidate.candidate_id == "window-primary-region")
      .expect("window candidate should exist");
    let promotion_gate = candidate
      .promotion_gate
      .as_ref()
      .expect("window candidate should expose promotion gate");
    assert_eq!(
      promotion_gate.status,
      AppCandidatePromotionStatus::ActionGradeCandidate
    );
    assert!(promotion_gate.missing_gates.is_empty());
  }

  #[test]
  fn suggested_annotation_ids_preserve_direct_then_context_candidates() {
    let candidate_shape = AppDistilledCandidateShape {
      direct_candidate_ids: vec![
        "window-primary-region".to_string(),
        "search-entry-focus-ax".to_string(),
      ],
      context_candidate_ids: vec!["visible-row-1".to_string()],
      provided_inputs: BTreeMap::new(),
      notes: vec![],
    };

    assert_eq!(
      suggested_annotation_ids_for_candidate_shape(&candidate_shape),
      vec![
        "window-primary-region".to_string(),
        "search-entry-focus-ax".to_string(),
        "visible-row-1".to_string(),
      ]
    );
  }

  #[test]
  fn distill_candidate_shape_records_note_when_strategy_has_no_direct_surface_candidate() {
    let analysis =
      sample_analysis_with_strategy("search-entry.ax-text-input.clipboard-submit.capture-evidence");

    let candidate_shape = build_distilled_candidate_shape(
      &analysis,
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
    );

    assert!(candidate_shape.direct_candidate_ids.is_empty());
    assert!(
      candidate_shape
        .notes
        .iter()
        .any(|note| note.contains("No direct candidate shape was available"))
    );
  }

  #[test]
  fn row_surface_candidates_stay_context_only_in_distilled_shape() {
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
      annotation_candidates: vec![AppSurfaceCandidate {
        candidate_id: "visible-row-1".to_string(),
        area: "result-selection".to_string(),
        kind: "row".to_string(),
        source: "ocr-text".to_string(),
        status: AssessmentStatus::Candidate,
        primary_text: "AURORA | Playlist".to_string(),
        secondary_text: "rowIndex=1".to_string(),
        query_value: "1".to_string(),
        coordinate_space: "global-logical".to_string(),
        bounds: Some(AppRect {
          x: 10,
          y: 20,
          width: 120,
          height: 24,
        }),
        click_point: Some(AppPoint { x: 70, y: 32 }),
        confidence: None,
        evidence_step_id: "ocr-sample".to_string(),
        candidate_query: Some(CandidateQuery {
          query_id: "visible-row-1".to_string(),
          selector: SurfaceSelector {
            any_of: vec![SurfaceSelectorClause::Row {
              row_index: Some(1),
              contains_text: Some("AURORA".to_string()),
              region_hint: None,
            }],
            within: SelectorScope::TargetWindow,
            require_visible: true,
          },
          output_kind: Some("row".to_string()),
          known_limits: vec!["row only".to_string()],
        }),
        evidence_refs: Vec::new(),
        promotion_gate: Some(AppCandidatePromotionGate {
          status: AppCandidatePromotionStatus::Blocked,
          missing_gates: vec![
            "row_action_contract".to_string(),
            "semantic_verification_contract".to_string(),
          ],
          notes: vec!["row candidate stays structural".to_string()],
        }),
        input_bindings: BTreeMap::from([("row_index".to_string(), "1".to_string())]),
        compatibility: candidate_compatibility(
          &[],
          &["result-selection.ocr-anchor.pointer-click.capture-evidence"],
        ),
        notes: vec!["sample row candidate".to_string()],
      }],
      known_boundaries: vec![],
      recommended_strategies: vec![AppRecommendedStrategy {
        taxonomy_id: "result-selection.ocr-anchor.pointer-click.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "row candidates stay review-only until a row action exists".to_string(),
      }],
    };

    let candidate_shape = build_distilled_candidate_shape(
      &analysis,
      "result-selection.ocr-anchor.pointer-click.capture-evidence",
    );

    assert!(candidate_shape.direct_candidate_ids.is_empty());
    assert_eq!(
      candidate_shape.context_candidate_ids,
      vec!["visible-row-1".to_string()]
    );
    assert!(candidate_shape.provided_inputs.is_empty());
    assert!(
      candidate_shape
        .notes
        .iter()
        .any(|note| note.contains("No direct candidate shape was available"))
    );
    assert!(
      candidate_shape
        .notes
        .iter()
        .any(|note| note.contains("Context-only candidates were recorded"))
    );
  }

  #[test]
  fn analysis_report_surfaces_row_context_and_blocked_promotion() {
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
      annotation_candidates: vec![AppSurfaceCandidate {
        candidate_id: "visible-row-1".to_string(),
        area: "result-selection".to_string(),
        kind: "row".to_string(),
        source: "ocr-text".to_string(),
        status: AssessmentStatus::Candidate,
        primary_text: "AURORA | Playlist".to_string(),
        secondary_text: "rowIndex=1".to_string(),
        query_value: "1".to_string(),
        coordinate_space: "global-logical".to_string(),
        bounds: Some(AppRect {
          x: 10,
          y: 20,
          width: 120,
          height: 24,
        }),
        click_point: Some(AppPoint { x: 70, y: 32 }),
        confidence: None,
        evidence_step_id: "ocr-sample".to_string(),
        candidate_query: Some(CandidateQuery {
          query_id: "visible-row-1".to_string(),
          selector: SurfaceSelector {
            any_of: vec![SurfaceSelectorClause::Row {
              row_index: Some(1),
              contains_text: Some("AURORA".to_string()),
              region_hint: None,
            }],
            within: SelectorScope::TargetWindow,
            require_visible: true,
          },
          output_kind: Some("row".to_string()),
          known_limits: vec!["row only".to_string()],
        }),
        evidence_refs: vec![ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        }],
        promotion_gate: Some(AppCandidatePromotionGate {
          status: AppCandidatePromotionStatus::Blocked,
          missing_gates: vec![
            "row_action_contract".to_string(),
            "semantic_verification_contract".to_string(),
          ],
          notes: vec!["row candidate stays structural".to_string()],
        }),
        input_bindings: BTreeMap::from([("row_index".to_string(), "1".to_string())]),
        compatibility: candidate_compatibility(
          &[],
          &["result-selection.ocr-anchor.pointer-click.capture-evidence"],
        ),
        notes: vec!["sample row candidate".to_string()],
      }],
      known_boundaries: vec![
        "Grouped visible rows remain surface candidates until a row action exists.".to_string(),
      ],
      recommended_strategies: vec![],
    };

    let report = render_app_analysis_report(&analysis);

    assert!(report.contains("`visible-row-1`: area=`result-selection`, kind=`row`"));
    assert!(report.contains("candidateQuery: `visible-row-1` sources=`row`"));
    assert!(report.contains("evidenceRefs:"));
    assert!(report.contains("promotionGate: `blocked`"));
    assert!(report.contains("`row_action_contract`"));
    assert!(report.contains("`semantic_verification_contract`"));
    assert!(report.contains("contextTaxonomyIds:"));
    assert!(report.contains("`result-selection.ocr-anchor.pointer-click.capture-evidence`"));
    assert!(report.contains("Grouped visible rows remain surface candidates"));
  }

  #[test]
  fn result_selection_grounding_ignores_row_only_candidates() {
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
      annotation_candidates: vec![AppSurfaceCandidate {
        candidate_id: "visible-row-1".to_string(),
        area: "result-selection".to_string(),
        kind: "row".to_string(),
        source: "ocr-text".to_string(),
        status: AssessmentStatus::Candidate,
        primary_text: "AURORA | Playlist".to_string(),
        secondary_text: "rowIndex=1".to_string(),
        query_value: "1".to_string(),
        coordinate_space: "global-logical".to_string(),
        bounds: Some(AppRect {
          x: 10,
          y: 20,
          width: 120,
          height: 24,
        }),
        click_point: Some(AppPoint { x: 70, y: 32 }),
        confidence: None,
        evidence_step_id: "ocr-sample".to_string(),
        candidate_query: Some(CandidateQuery {
          query_id: "visible-row-1".to_string(),
          selector: SurfaceSelector {
            any_of: vec![SurfaceSelectorClause::Row {
              row_index: Some(1),
              contains_text: Some("AURORA".to_string()),
              region_hint: None,
            }],
            within: SelectorScope::TargetWindow,
            require_visible: true,
          },
          output_kind: Some("row".to_string()),
          known_limits: vec!["row only".to_string()],
        }),
        evidence_refs: Vec::new(),
        promotion_gate: Some(AppCandidatePromotionGate {
          status: AppCandidatePromotionStatus::Blocked,
          missing_gates: vec![
            "row_action_contract".to_string(),
            "semantic_verification_contract".to_string(),
          ],
          notes: vec!["row candidate stays structural".to_string()],
        }),
        input_bindings: BTreeMap::from([("row_index".to_string(), "1".to_string())]),
        compatibility: candidate_compatibility(
          &[],
          &["result-selection.ocr-anchor.pointer-click.capture-evidence"],
        ),
        notes: vec!["sample row candidate".to_string()],
      }],
      known_boundaries: vec![],
      recommended_strategies: vec![],
    };
    let mut matrix = SkillCaseMatrix {
      version: "0.1.0".to_string(),
      skill_id: "test.result.selection".to_string(),
      status: "validated".to_string(),
      cases: vec![SkillCase {
        case_id: "row-only".to_string(),
        status: "validated".to_string(),
        inputs: BTreeMap::from([("anchor_text".to_string(), "TODO_ANCHOR_TEXT".to_string())]),
        disturbance: String::new(),
        notes: Vec::new(),
      }],
    };
    let mut resolved = BTreeMap::new();

    let (unresolved, used_annotations) = apply_candidate_grounding(
      &analysis,
      None,
      "result-selection.ocr-anchor.pointer-click.capture-evidence",
      &mut matrix,
      &mut resolved,
    )
    .expect("known taxonomy should ground");

    assert!(used_annotations.is_empty());
    assert!(unresolved.iter().any(|key| key == "anchor_text"));
    assert!(!resolved.contains_key("anchor_text"));
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
      candidate_query: None,
      evidence_refs: Vec::new(),
      promotion_gate: Some(AppCandidatePromotionGate {
        status: AppCandidatePromotionStatus::DistillStrategyOnly,
        missing_gates: vec!["artifact_ref".to_string()],
        notes: vec![
          "Candidate can seed a known distillation strategy, but this slice does not promote this surface family into contract::Candidate.".to_string(),
          "Candidate has no ArtifactRef; action consumers cannot reconstruct the source evidence chain.".to_string(),
        ],
      }),
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
    let promotion_gate = analysis
      .annotation_candidates
      .iter()
      .find(|candidate| candidate.candidate_id == "window-primary-region")
      .expect("window candidate should exist")
      .promotion_gate
      .as_ref()
      .expect("window candidate should expose promotion gate");
    assert_eq!(
      promotion_gate.status,
      AppCandidatePromotionStatus::DistillStrategyOnly
    );
    assert!(
      promotion_gate
        .missing_gates
        .iter()
        .any(|item| item == "artifact_ref")
    );
  }

  #[test]
  fn distilled_candidates_preserve_source_evidence_refs_from_analysis() {
    let mut analysis =
      sample_analysis_with_strategy("search-entry.ax-text-input.clipboard-submit.capture-evidence");
    analysis.annotation_candidates.push(AppSurfaceCandidate {
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
      evidence_step_id: "capture-ax-tree".to_string(),
      candidate_query: None,
      evidence_refs: vec![ArtifactRef {
        run_id: crate::trace::RunId::new("run_probe"),
        span_id: crate::trace::SpanId::new("span_probe"),
        artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
        captured_event_id: Some(crate::trace::EventId::new("event_probe")),
      }],
      promotion_gate: Some(AppCandidatePromotionGate {
        status: AppCandidatePromotionStatus::ActionGradeCandidate,
        missing_gates: Vec::new(),
        notes: vec!["test candidate".to_string()],
      }),
      input_bindings: BTreeMap::from([("focus_query".to_string(), "Search".to_string())]),
      compatibility: candidate_compatibility(
        &["search-entry.ax-text-input.clipboard-submit.capture-evidence"],
        &[],
      ),
      notes: vec!["sample note".to_string()],
    });
    let candidate_shape = build_distilled_candidate_shape(
      &analysis,
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
    );
    let evidence_refs = source_evidence_refs_for_candidate_shape(&analysis, &candidate_shape);

    assert_eq!(evidence_refs.len(), 1);
    assert_eq!(evidence_refs[0].artifact_id.as_str(), "artifact_0001");
    assert_eq!(evidence_refs[0].span_id.as_str(), "span_probe");
  }

  #[test]
  fn distillation_projects_search_entry_promoted_candidate() {
    let analysis = sample_promotable_ax_focus_analysis(
      "search-entry",
      "search-entry-focus-ax",
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
      "Search",
      "Sample candidate satisfies the v0 search-entry promotion seam.",
    );
    let candidate_shape = build_distilled_candidate_shape(
      &analysis,
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
    );
    let promoted = promoted_candidate_for_candidate_shape(
      &analysis,
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
      &candidate_shape,
    )
    .expect("search-entry candidate should promote");

    assert_eq!(promoted.candidate_local_id, "search-entry-focus-ax");
    assert_eq!(promoted.kind, "search_entry");
    assert_eq!(
      promoted.target_spec.grounding,
      crate::contract::TargetGrounding::AxNode
    );
    assert_eq!(promoted.control.requires_app_frontmost, true);
    assert_eq!(promoted.control.requires_window_focus, true);
    assert_eq!(
      promoted.evidence.artifact_ref.artifact_id.as_str(),
      "artifact_0001"
    );
  }

  #[test]
  fn distillation_projects_native_text_promoted_candidate() {
    let analysis = sample_promotable_ax_focus_analysis(
      "native-text",
      "native-text-focus-ax",
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      "Editor",
      "Sample candidate satisfies the v0 native-text promotion seam.",
    );
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, NATIVE_TEXT_CANONICAL_TAXONOMY_ID);
    let promoted = promoted_candidate_for_candidate_shape(
      &analysis,
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      &candidate_shape,
    )
    .expect("native-text candidate should promote");

    assert_eq!(promoted.candidate_local_id, "native-text-focus-ax");
    assert_eq!(promoted.kind, "native_text");
    assert_eq!(
      promoted.target_spec.grounding,
      crate::contract::TargetGrounding::AxNode
    );
    assert_eq!(promoted.control.requires_app_frontmost, true);
    assert_eq!(promoted.control.requires_window_focus, true);
    assert_eq!(
      promoted.evidence.artifact_ref.artifact_id.as_str(),
      "artifact_0001"
    );
  }

  #[test]
  fn legacy_native_text_taxonomy_alias_still_promotes_candidate() {
    let analysis = sample_promotable_ax_focus_analysis(
      "native-text",
      "native-text-focus-ax",
      NATIVE_TEXT_LEGACY_TAXONOMY_ID,
      "Editor",
      "Sample legacy native-text taxonomy still maps to the v0 promotion seam.",
    );
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, NATIVE_TEXT_CANONICAL_TAXONOMY_ID);
    let promoted = promoted_candidate_for_candidate_shape(
      &analysis,
      NATIVE_TEXT_LEGACY_TAXONOMY_ID,
      &candidate_shape,
    )
    .expect("legacy native-text taxonomy should still promote");

    assert_eq!(promoted.candidate_local_id, "native-text-focus-ax");
    assert_eq!(promoted.kind, "native_text");
  }

  #[test]
  fn distillation_projects_window_action_promoted_candidate() {
    let analysis = sample_promotable_window_action_analysis();
    let candidate_shape = build_distilled_candidate_shape(
      &analysis,
      "window-action.window-point.pointer-click.capture-evidence",
    );
    let promoted = promoted_candidate_for_candidate_shape(
      &analysis,
      "window-action.window-point.pointer-click.capture-evidence",
      &candidate_shape,
    )
    .expect("window-action candidate should promote");

    assert_eq!(promoted.candidate_local_id, "window-primary-region");
    assert_eq!(promoted.kind, "window_action");
    assert_eq!(
      promoted.target_spec.grounding,
      crate::contract::TargetGrounding::Coordinate
    );
    assert_eq!(promoted.control.requires_app_frontmost, true);
    assert_eq!(promoted.control.requires_window_focus, true);
    assert_eq!(
      promoted.evidence.artifact_ref.artifact_id.as_str(),
      "artifact_0002"
    );
  }

  #[test]
  fn promote_search_entry_candidate_leaves_window_title_substring_unenforced() {
    let mut analysis = sample_promotable_ax_focus_analysis(
      "search-entry",
      "search-entry-focus-ax",
      SEARCH_ENTRY_TAXONOMY_ID,
      "Search",
      "Sample candidate satisfies the v0 search-entry promotion seam.",
    );
    analysis.window_context.frontmost_window_title = "Untitled 5".to_string();
    analysis.window_context.primary_window_title = "未命名3".to_string();
    let candidate_shape = build_distilled_candidate_shape(&analysis, SEARCH_ENTRY_TAXONOMY_ID);
    let promoted =
      promoted_candidate_for_candidate_shape(&analysis, SEARCH_ENTRY_TAXONOMY_ID, &candidate_shape)
        .expect("search-entry candidate should promote");

    let window_ref = promoted
      .liveness
      .preconditions
      .window_ref
      .as_ref()
      .expect("window_ref precondition should exist");
    assert_eq!(
      window_ref.window_title_substring, None,
      "doc-style apps mutate or localize window titles between probe and validate; window_title_substring must stay unenforced",
    );
    let observed_title = promoted
      .evidence
      .observation
      .get("window_context")
      .and_then(|context| context.get("window_title"))
      .and_then(|title| title.as_str());
    assert_eq!(
      observed_title,
      Some("Untitled 5"),
      "observation should record the frontmost window title as the observed signal",
    );
  }

  #[test]
  fn promote_native_text_candidate_leaves_window_title_substring_unenforced() {
    let mut analysis = sample_promotable_ax_focus_analysis(
      "native-text",
      "native-text-focus-ax",
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      "Editor",
      "Sample candidate satisfies the v0 native-text promotion seam.",
    );
    analysis.window_context.frontmost_window_title = "Untitled 5".to_string();
    analysis.window_context.primary_window_title = "未命名3".to_string();
    let candidate_shape =
      build_distilled_candidate_shape(&analysis, NATIVE_TEXT_CANONICAL_TAXONOMY_ID);
    let promoted = promoted_candidate_for_candidate_shape(
      &analysis,
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      &candidate_shape,
    )
    .expect("native-text candidate should promote");

    let window_ref = promoted
      .liveness
      .preconditions
      .window_ref
      .as_ref()
      .expect("window_ref precondition should exist");
    assert_eq!(
      window_ref.window_title_substring, None,
      "doc-style apps mutate or localize window titles between probe and validate; window_title_substring must stay unenforced",
    );
    let observed_title = promoted
      .evidence
      .observation
      .get("window_context")
      .and_then(|context| context.get("window_title"))
      .and_then(|title| title.as_str());
    assert_eq!(
      observed_title,
      Some("Untitled 5"),
      "observation should record the frontmost window title as the observed signal",
    );
  }

  #[test]
  fn promote_window_action_candidate_leaves_window_title_substring_unenforced() {
    let mut analysis = sample_promotable_window_action_analysis();
    analysis.window_context.frontmost_window_title = "Untitled 5".to_string();
    analysis.window_context.primary_window_title = "未命名3".to_string();
    let candidate_shape = build_distilled_candidate_shape(&analysis, WINDOW_ACTION_TAXONOMY_ID);
    let promoted = promoted_candidate_for_candidate_shape(
      &analysis,
      WINDOW_ACTION_TAXONOMY_ID,
      &candidate_shape,
    )
    .expect("window-action candidate should promote");

    let window_ref = promoted
      .liveness
      .preconditions
      .window_ref
      .as_ref()
      .expect("window_ref precondition should exist");
    assert_eq!(
      window_ref.window_title_substring, None,
      "doc-style apps mutate or localize window titles between probe and validate; window_title_substring must stay unenforced",
    );
    let observed_title = promoted
      .evidence
      .observation
      .get("window_context")
      .and_then(|context| context.get("window_title"))
      .and_then(|title| title.as_str());
    assert_eq!(
      observed_title,
      Some("Untitled 5"),
      "observation should record the frontmost window title as the observed signal",
    );
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
      evidence_step_id: "capture-ax-tree".to_string(),
      candidate_query: None,
      evidence_refs: Vec::new(),
      promotion_gate: Some(AppCandidatePromotionGate {
        status: AppCandidatePromotionStatus::DistillStrategyOnly,
        missing_gates: vec!["artifact_ref".to_string()],
        notes: vec![
          "Candidate can seed a known distillation strategy, but this slice does not promote this surface family into contract::Candidate.".to_string(),
          "Candidate has no ArtifactRef; action consumers cannot reconstruct the source evidence chain.".to_string(),
        ],
      }),
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
    assert_eq!(used_annotations.len(), 1);
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
    let recorder = Arc::new(MemoryRunRecorder::new());
    let runtime = test_runtime(root.clone()).with_recorder(recorder.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("candidate.recipe.json");
    let case_matrix_path = root.join("candidate.cases.json");

    let mut analysis = sample_analysis_with_strategy(NATIVE_TEXT_CANONICAL_TAXONOMY_ID);
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
        taxonomy_id: NATIVE_TEXT_CANONICAL_TAXONOMY_ID.to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: Vec::new(),
        source_evidence_refs: vec![ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        }],
        promoted_candidate: None,
        candidate_shape: AppDistilledCandidateShape::default(),
        recipe_path,
        case_matrix_path,
      }],
      known_boundaries: Vec::new(),
    };
    write_pretty_json(&distillation_path, &distillation).expect("distillation should write");

    validate_app_distillation(&runtime, &distillation_path).expect("validation should complete");

    let finished_runs = recorder
      .drain_for_test()
      .into_iter()
      .filter_map(|update| match update {
        RunUpdate::RunFinished { run, .. } => Some(run),
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
    let recorder = Arc::new(MemoryRunRecorder::new());
    let runtime = test_runtime(root.clone()).with_recorder(recorder.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("candidate.recipe.json");
    let case_matrix_path = root.join("candidate.cases.json");

    let mut analysis = sample_analysis_with_strategy(NATIVE_TEXT_CANONICAL_TAXONOMY_ID);
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
        taxonomy_id: NATIVE_TEXT_CANONICAL_TAXONOMY_ID.to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: Vec::new(),
        source_evidence_refs: Vec::new(),
        promoted_candidate: None,
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

    let finished_runs = recorder
      .drain_for_test()
      .into_iter()
      .filter_map(|update| match update {
        RunUpdate::RunFinished { run, .. } => Some(run),
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
    let recorder = Arc::new(MemoryRunRecorder::new());
    let runtime = test_runtime(root.clone()).with_recorder(recorder.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("window-action.recipe.json");
    let case_matrix_path = root.join("window-action.cases.json");

    let mut analysis = sample_promotable_window_action_analysis();
    analysis.probe_path = root.join("missing-probe.json");
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    write_pretty_json(&recipe_path, &test_window_action_candidate_manifest_value())
      .expect("candidate recipe should write");
    write_pretty_json(
      &case_matrix_path,
      &serde_json::json!({
        "skill_id": "test.window.action",
        "version": "0.1.0",
        "status": "candidate-case-matrix",
        "cases": [{
          "case_id": "default-candidate",
          "status": "candidate",
          "inputs": {
            "relative_x": "TODO_RELATIVE_X",
            "relative_y": "TODO_RELATIVE_Y",
            "require_click_candidate": "true"
          },
          "disturbance": "none"
        }]
      }),
    )
    .expect("candidate matrix should write");
    let candidate_shape = build_distilled_candidate_shape(
      &analysis,
      "window-action.window-point.pointer-click.capture-evidence",
    );
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
        source_evidence_refs: Vec::new(),
        promoted_candidate: Some(
          promoted_candidate_for_candidate_shape(
            &analysis,
            "window-action.window-point.pointer-click.capture-evidence",
            &candidate_shape,
          )
          .expect("window-action candidate should promote"),
        ),
        candidate_shape,
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
        .contains_key("click_candidate"),
      true
    );
    let click_candidate = output.validation.candidates[0]
      .resolved_inputs
      .get("click_candidate")
      .expect("validate should inject click_candidate");
    let parsed_candidate: crate::contract::Candidate = serde_json::from_str(click_candidate)
      .expect("click_candidate should stay valid candidate JSON");
    assert_eq!(parsed_candidate.candidate_local_id, "window-primary-region");
    assert_eq!(
      parsed_candidate.target_spec.grounding,
      crate::contract::TargetGrounding::Coordinate
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

    let finished_runs = recorder
      .drain_for_test()
      .into_iter()
      .filter_map(|update| match update {
        RunUpdate::RunFinished { run, .. } => Some(run),
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
        source_evidence_refs: Vec::new(),
        promoted_candidate: None,
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
        source_evidence_refs: Vec::new(),
        promoted_candidate: None,
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
  fn validate_app_distillation_injects_search_entry_promoted_candidate_into_runtime_inputs() {
    let root = temp_dir("app-validate-search-entry-promoted-consumer");
    let runtime = test_runtime(root.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("search-entry.recipe.json");
    let case_matrix_path = root.join("search-entry.cases.json");

    let analysis = sample_promotable_ax_focus_analysis(
      "search-entry",
      "search-entry-focus-ax",
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
      "Search",
      "Sample candidate satisfies the v0 search-entry promotion seam.",
    );
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    write_pretty_json(&recipe_path, &test_candidate_manifest_value())
      .expect("candidate recipe should write");
    write_pretty_json(
      &case_matrix_path,
      &serde_json::json!({
        "skill_id": "test.recorded.skill",
        "version": "0.1.0",
        "status": "active-case-matrix",
        "cases": [{
          "case_id": "baseline",
          "status": "validated",
          "inputs": {
            "require_focus_candidate": "true"
          },
          "disturbance": "none"
        }]
      }),
    )
    .expect("candidate matrix should write");

    let candidate_shape = build_distilled_candidate_shape(
      &analysis,
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
    );
    let promoted_candidate = promoted_candidate_for_candidate_shape(
      &analysis,
      "search-entry.ax-text-input.clipboard-submit.capture-evidence",
      &candidate_shape,
    )
    .expect("search-entry candidate should promote");
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: analysis_path,
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.recorded.skill".to_string(),
        taxonomy_id: "search-entry.ax-text-input.clipboard-submit.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: vec!["search-entry-focus-ax".to_string()],
        source_evidence_refs: Vec::new(),
        promoted_candidate: Some(promoted_candidate),
        candidate_shape,
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
      output.validation.candidates[0].candidate_source.as_deref(),
      Some("promoted_candidate")
    );
    assert!(
      output.validation.candidates[0]
        .used_annotation_ids
        .iter()
        .any(|candidate_id| candidate_id == "search-entry-focus-ax")
    );
    assert_eq!(
      output.validation.candidates[0]
        .resolved_inputs
        .get("focus_query"),
      Some(&"Search".to_string())
    );
    let focus_candidate = output.validation.candidates[0]
      .resolved_inputs
      .get("focus_candidate")
      .expect("validate should inject focus_candidate");
    let parsed_candidate: crate::contract::Candidate = serde_json::from_str(focus_candidate)
      .expect("focus_candidate should stay valid candidate JSON");
    assert_eq!(parsed_candidate.candidate_local_id, "search-entry-focus-ax");
    assert_eq!(
      output.validation.candidates[0].observed_consumer.as_deref(),
      Some("contract-candidate")
    );
    assert_eq!(
      output.validation.candidates[0]
        .observed_candidate_local_id
        .as_deref(),
      Some("search-entry-focus-ax")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn validate_app_distillation_injects_native_text_promoted_candidate_into_runtime_inputs() {
    let root = temp_dir("app-validate-native-text-promoted-consumer");
    let runtime = test_runtime(root.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("native-text.recipe.json");
    let case_matrix_path = root.join("native-text.cases.json");

    let analysis = sample_promotable_ax_focus_analysis(
      "native-text",
      "native-text-focus-ax",
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      "Editor",
      "Sample candidate satisfies the v0 native-text promotion seam.",
    );
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    write_pretty_json(&recipe_path, &test_candidate_manifest_value())
      .expect("candidate recipe should write");
    write_pretty_json(
      &case_matrix_path,
      &serde_json::json!({
        "skill_id": "test.recorded.skill",
        "version": "0.1.0",
        "status": "active-case-matrix",
        "cases": [{
          "case_id": "baseline",
          "status": "validated",
          "inputs": {
            "require_focus_candidate": "true"
          },
          "disturbance": "none"
        }]
      }),
    )
    .expect("candidate matrix should write");

    let candidate_shape =
      build_distilled_candidate_shape(&analysis, NATIVE_TEXT_CANONICAL_TAXONOMY_ID);
    let promoted_candidate = promoted_candidate_for_candidate_shape(
      &analysis,
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      &candidate_shape,
    )
    .expect("native-text candidate should promote");
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: analysis_path,
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.recorded.skill".to_string(),
        taxonomy_id: NATIVE_TEXT_CANONICAL_TAXONOMY_ID.to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: vec!["native-text-focus-ax".to_string()],
        source_evidence_refs: Vec::new(),
        promoted_candidate: Some(promoted_candidate),
        candidate_shape,
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
      output.validation.candidates[0].candidate_source.as_deref(),
      Some("promoted_candidate")
    );
    assert!(
      output.validation.candidates[0]
        .used_annotation_ids
        .iter()
        .any(|candidate_id| candidate_id == "native-text-focus-ax")
    );
    assert_eq!(
      output.validation.candidates[0]
        .resolved_inputs
        .get("focus_query"),
      Some(&"Editor".to_string())
    );
    let focus_candidate = output.validation.candidates[0]
      .resolved_inputs
      .get("focus_candidate")
      .expect("validate should inject focus_candidate");
    let parsed_candidate: crate::contract::Candidate = serde_json::from_str(focus_candidate)
      .expect("focus_candidate should stay valid candidate JSON");
    assert_eq!(parsed_candidate.candidate_local_id, "native-text-focus-ax");
    assert_eq!(
      output.validation.candidates[0].observed_consumer.as_deref(),
      Some("contract-candidate")
    );
    assert_eq!(
      output.validation.candidates[0]
        .observed_candidate_local_id
        .as_deref(),
      Some("native-text-focus-ax")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn validate_app_distillation_keeps_native_text_query_consumer_without_promoted_candidate() {
    let root = temp_dir("app-validate-native-text-query-consumer");
    let runtime = test_runtime(root.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("native-text.recipe.json");
    let case_matrix_path = root.join("native-text.cases.json");

    let analysis = sample_promotable_ax_focus_analysis(
      "native-text",
      "native-text-focus-ax",
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      "Editor",
      "Sample candidate satisfies the v0 native-text promotion seam.",
    );
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    write_pretty_json(&recipe_path, &test_candidate_manifest_value())
      .expect("candidate recipe should write");
    write_pretty_json(
      &case_matrix_path,
      &serde_json::json!({
        "skill_id": "test.recorded.skill",
        "version": "0.1.0",
        "status": "active-case-matrix",
        "cases": [{
          "case_id": "baseline",
          "status": "validated",
          "inputs": {
            "focus_query": "Editor",
            "require_focus_candidate": "false"
          },
          "disturbance": "none"
        }]
      }),
    )
    .expect("candidate matrix should write");

    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: analysis_path,
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.recorded.skill".to_string(),
        taxonomy_id: NATIVE_TEXT_CANONICAL_TAXONOMY_ID.to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: vec!["native-text-focus-ax".to_string()],
        source_evidence_refs: Vec::new(),
        promoted_candidate: None,
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
      AppValidationStatus::Candidate
    );
    assert_eq!(
      output.validation.candidates[0].candidate_source.as_deref(),
      Some("query_fallback")
    );
    assert_eq!(
      output.validation.candidates[0]
        .resolved_inputs
        .get("focus_query"),
      Some(&"Editor".to_string())
    );
    assert!(
      !output.validation.candidates[0]
        .resolved_inputs
        .contains_key("focus_candidate")
    );
    assert_eq!(
      output.validation.candidates[0].observed_consumer.as_deref(),
      Some("query")
    );
    assert_eq!(
      output.validation.candidates[0]
        .observed_candidate_local_id
        .as_deref(),
      None
    );
    assert!(
      output.validation.candidates[0]
        .rationale
        .contains("legacy `query` fallback")
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn classify_native_text_ax_focus_contract_without_consumer_signal_stays_candidate() {
    let outcome = classify_successful_validation_outcome(
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      1,
      AppVerificationMode::MachineAsserted,
      None,
      true,
    );

    assert_eq!(outcome.status, AppValidationStatus::Candidate);
    assert!(
      outcome
        .rationale
        .contains("did not observe a native-text consumer signal")
    );
  }

  #[test]
  fn classify_native_text_contract_candidate_consumer_is_validated() {
    let outcome = classify_successful_validation_outcome(
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID,
      1,
      AppVerificationMode::MachineAsserted,
      Some("contract-candidate"),
      true,
    );

    assert_eq!(outcome.status, AppValidationStatus::Validated);
    assert!(outcome.rationale.contains("shared runtime"));
  }

  #[test]
  fn validate_app_distillation_injects_result_selection_promoted_candidate_into_runtime_inputs() {
    let root = temp_dir("app-validate-result-selection-promoted-consumer");
    let runtime = test_runtime(root.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("result-selection.recipe.json");
    let case_matrix_path = root.join("result-selection.cases.json");

    let analysis = sample_promotable_result_selection_analysis();
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    write_pretty_json(
      &recipe_path,
      &serde_json::json!({
        "recipe_id": "test.result.selection",
        "version": "0.1.0",
        "status": "candidate-recipe",
        "platform": "macOS",
        "target_app": { "bundle_id": "fixture.app", "display_mode": "fixture" },
        "strategy": {
          "family": "result-selection",
          "grounding": "ocr-anchor",
          "activation": "pointer-click",
          "verificationContract": "captureEvidence"
        },
        "objective": "test result selection validation",
        "disturbance_policy": {
          "max_disturbance": "none",
          "declared_classes": ["none"]
        },
        "inputs": {
          "click_candidate": { "type": "string", "default": "" },
          "anchor_text": { "type": "string", "default": "" },
          "require_click_candidate": { "type": "string", "default": "false" }
        },
        "steps": [{
          "id": "first",
          "command_id": "test.skill.invoke",
          "disturbance": {
            "classes": ["none"],
            "max": "none"
          },
          "args": {
            "click_candidate": "${click_candidate}",
            "anchor_text": "${anchor_text}",
            "require_click_candidate": "${require_click_candidate}"
          },
          "expect": {
            "output_must_contain": ["outcome=ok"]
          }
        }],
        "verification": {
          "expected_signals": ["signal"],
          "success_criteria": ["criteria"]
        }
      }),
    )
    .expect("candidate recipe should write");
    write_pretty_json(
      &case_matrix_path,
      &serde_json::json!({
        "skill_id": "test.result.selection",
        "version": "0.1.0",
        "status": "active-case-matrix",
        "cases": [{
          "case_id": "baseline",
          "status": "validated",
          "inputs": {
            "require_click_candidate": "true"
          },
          "disturbance": "none"
        }]
      }),
    )
    .expect("candidate matrix should write");

    let candidate_shape = build_distilled_candidate_shape(
      &analysis,
      "result-selection.ocr-anchor.pointer-click.capture-evidence",
    );
    let promoted_candidate = sample_result_selection_promoted_candidate();
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: analysis_path,
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.result.selection".to_string(),
        taxonomy_id: "result-selection.ocr-anchor.pointer-click.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: vec!["result-selection-anchor-ax".to_string()],
        source_evidence_refs: Vec::new(),
        promoted_candidate: Some(promoted_candidate),
        candidate_shape,
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
    assert!(
      output.validation.candidates[0]
        .used_annotation_ids
        .iter()
        .any(|candidate_id| candidate_id == "result-selection-anchor-ax")
    );
    assert_eq!(
      output.validation.candidates[0]
        .resolved_inputs
        .get("anchor_text"),
      Some(&"Play Now".to_string())
    );
    let click_candidate = output.validation.candidates[0]
      .resolved_inputs
      .get("click_candidate")
      .expect("validate should inject click_candidate");
    let parsed_candidate: crate::contract::Candidate = serde_json::from_str(click_candidate)
      .expect("click_candidate should stay valid candidate JSON");
    assert_eq!(
      parsed_candidate.candidate_local_id,
      "result-selection-anchor-ax"
    );
    assert_eq!(
      parsed_candidate.target_spec.grounding,
      crate::contract::TargetGrounding::OcrAnchor
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn validate_app_distillation_keeps_row_context_candidates_review_only() {
    let root = temp_dir("app-validate-row-context-only");
    let runtime = test_runtime(root.clone());
    let analysis_path = root.join("analysis.json");
    let distillation_path = root.join("distillation.json");
    let recipe_path = root.join("result-selection.recipe.json");
    let case_matrix_path = root.join("result-selection.cases.json");

    let analysis = AppAnalysis {
      analysis_version: APP_ANALYSIS_VERSION.to_string(),
      created_at_millis: 0,
      probe_path: root.join("missing-probe.json"),
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
      annotation_candidates: vec![AppSurfaceCandidate {
        candidate_id: "visible-row-1".to_string(),
        area: "result-selection".to_string(),
        kind: "row".to_string(),
        source: "ocr-text".to_string(),
        status: AssessmentStatus::Candidate,
        primary_text: "AURORA | Playlist".to_string(),
        secondary_text: "rowIndex=1".to_string(),
        query_value: "1".to_string(),
        coordinate_space: "global-logical".to_string(),
        bounds: Some(AppRect {
          x: 10,
          y: 20,
          width: 120,
          height: 24,
        }),
        click_point: Some(AppPoint { x: 70, y: 32 }),
        confidence: None,
        evidence_step_id: "ocr-sample".to_string(),
        candidate_query: Some(CandidateQuery {
          query_id: "visible-row-1".to_string(),
          selector: SurfaceSelector {
            any_of: vec![SurfaceSelectorClause::Row {
              row_index: Some(1),
              contains_text: Some("AURORA".to_string()),
              region_hint: None,
            }],
            within: SelectorScope::TargetWindow,
            require_visible: true,
          },
          output_kind: Some("row".to_string()),
          known_limits: vec!["row only".to_string()],
        }),
        evidence_refs: Vec::new(),
        promotion_gate: Some(AppCandidatePromotionGate {
          status: AppCandidatePromotionStatus::Blocked,
          missing_gates: vec![
            "row_action_contract".to_string(),
            "semantic_verification_contract".to_string(),
          ],
          notes: vec!["row candidate stays structural".to_string()],
        }),
        input_bindings: BTreeMap::from([("row_index".to_string(), "1".to_string())]),
        compatibility: candidate_compatibility(
          &[],
          &["result-selection.ocr-anchor.pointer-click.capture-evidence"],
        ),
        notes: vec!["sample row candidate".to_string()],
      }],
      known_boundaries: vec![],
      recommended_strategies: vec![],
    };
    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");

    let strategy = recommended_strategy(
      "native-text",
      "ax-text",
      "ax-perform-action-clipboard-paste",
      "verifyAxText",
      AssessmentStatus::Candidate,
      "test",
    )
    .expect("strategy should render");
    let candidate_shape = AppDistilledCandidateShape {
      direct_candidate_ids: Vec::new(),
      context_candidate_ids: vec!["visible-row-1".to_string()],
      provided_inputs: BTreeMap::new(),
      notes: vec![
        format!(
          "No direct candidate shape was available for taxonomy {} during distill.",
          NATIVE_TEXT_CANONICAL_TAXONOMY_ID
        ),
        "Context-only candidates were recorded for later review, but they did not project directly into recipe inputs.".to_string(),
      ],
    };
    write_pretty_json(
      &recipe_path,
      &render_candidate_recipe(&analysis, &strategy, &candidate_shape)
        .expect("candidate recipe should render"),
    )
    .expect("candidate recipe should write");
    write_pretty_json(
      &case_matrix_path,
      &render_candidate_case_matrix(&analysis, &strategy, &candidate_shape)
        .expect("candidate matrix should render"),
    )
    .expect("candidate matrix should write");
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: analysis_path,
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.recorded.skill".to_string(),
        taxonomy_id: NATIVE_TEXT_CANONICAL_TAXONOMY_ID.to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
        suggested_annotation_ids: vec!["visible-row-1".to_string()],
        source_evidence_refs: Vec::new(),
        promoted_candidate: None,
        candidate_shape: candidate_shape.clone(),
        recipe_path,
        case_matrix_path,
      }],
      known_boundaries: Vec::new(),
    };
    write_pretty_json(&distillation_path, &distillation).expect("distillation should write");

    let error = validate_app_distillation(&runtime, &distillation_path)
      .expect_err("row context-only distillation should fail validation");
    assert!(error.contains("candidate grounding left unresolved inputs"));
    assert!(error.contains("test.recorded.skill"));
    assert!(error.contains("focus_query"));

    let validation: AppValidation =
      read_json(&root.join("validation.json")).expect("validation output should still write");
    assert_eq!(
      validation.candidates[0].status,
      AppValidationStatus::Rejected
    );
    assert!(validation.candidates[0].used_annotation_ids.is_empty());
    assert_eq!(
      validation.candidates[0].unresolved_inputs,
      vec!["focus_query".to_string()]
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
  fn distillation_report_keeps_row_suggestions_review_only() {
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
      known_boundaries: vec![
        "Grouped visible rows remain surface candidates until a row action exists.".to_string(),
      ],
      recommended_strategies: vec![],
    };
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: PathBuf::from("/tmp/analysis.json"),
      app_identity: analysis.app_identity.clone(),
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.result.selection".to_string(),
        taxonomy_id: "result-selection.ocr-anchor.pointer-click.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "row suggestions stay review-only".to_string(),
        suggested_annotation_ids: vec!["visible-row-1".to_string()],
        source_evidence_refs: vec![ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        }],
        promoted_candidate: None,
        candidate_shape: AppDistilledCandidateShape {
          direct_candidate_ids: Vec::new(),
          context_candidate_ids: vec!["visible-row-1".to_string()],
          provided_inputs: BTreeMap::new(),
          notes: vec![
            "No direct candidate shape was available for taxonomy result-selection.ocr-anchor.pointer-click.capture-evidence during distill.".to_string(),
            "Context-only candidates were recorded for later review, but they did not project directly into recipe inputs.".to_string(),
          ],
        },
        recipe_path: PathBuf::from("/tmp/result-selection.recipe.json"),
        case_matrix_path: PathBuf::from("/tmp/result-selection.cases.json"),
      }],
      known_boundaries: analysis.known_boundaries.clone(),
    };

    let report = render_app_distillation_report(&analysis, &distillation);

    assert!(report.contains("suggested annotations:"));
    assert!(report.contains("`visible-row-1`"));
    assert!(report.contains("source evidence refs:"));
    assert!(report.contains("context candidate ids:"));
    assert!(!report.contains("direct candidate ids:"));
    assert!(!report.contains("candidate shape inputs:"));
    assert!(report.contains("shape note: No direct candidate shape was available"));
    assert!(report.contains("shape note: Context-only candidates were recorded"));
    assert!(report.contains("Grouped visible rows remain surface candidates"));
  }

  #[test]
  fn validation_report_keeps_row_context_failures_review_only() {
    let validation = AppValidation {
      validate_version: APP_VALIDATE_VERSION.to_string(),
      created_at_millis: 0,
      source_distillation_path: PathBuf::from("/tmp/distillation.json"),
      source_analysis_path: PathBuf::from("/tmp/analysis.json"),
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
      candidates: vec![AppValidatedCandidate {
        recipe_id: "test.recorded.skill".to_string(),
        taxonomy_id: "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text"
          .to_string(),
        status: AppValidationStatus::Rejected,
        verification_mode: AppVerificationMode::MachineAsserted,
        rationale: "row context-only suggestions must not become executable".to_string(),
        selected_case_count: 1,
        recipe_path: PathBuf::from("/tmp/native-text.recipe.json"),
        case_matrix_path: PathBuf::from("/tmp/native-text.cases.json"),
        used_annotation_ids: Vec::new(),
        observed_consumer: Some("contract-candidate".to_string()),
        observed_candidate_local_id: Some("native-text-focus-ax".to_string()),
        candidate_source: Some("promoted_candidate".to_string()),
        resolved_inputs: BTreeMap::new(),
        unresolved_inputs: vec!["focus_query".to_string()],
        failure_message: Some(
          "Validation could not execute test.recorded.skill because grounding left unresolved inputs: focus_query."
            .to_string(),
        ),
      }],
      known_boundaries: vec![
        "Grouped visible rows remain surface candidates until a row action exists."
          .to_string(),
      ],
    };

    let report = render_app_validation_report(&validation);

    assert!(report.contains("manual review required: `no`"));
    assert!(report.contains(
      "- canonical taxonomy: `native-text.ax-text.ax-perform-action-clipboard-paste.verify-ax-text`"
    ));
    assert!(report.contains("- legacy taxonomy alias: `yes`"));
    assert!(report.contains("- observed consumer: `contract-candidate`"));
    assert!(report.contains("- observed candidate local id: `native-text-focus-ax`"));
    assert!(report.contains("- candidate source: `promoted_candidate`"));
    assert!(report.contains("- unresolved inputs:"));
    assert!(report.contains("`focus_query`"));
    assert!(report.contains("- failure:"));
    assert!(report.contains("grounding left unresolved inputs"));
    assert!(report.contains("Grouped visible rows remain surface candidates"));
    assert!(!report.contains("- used annotations:"));
    assert!(!report.contains("`visible-row-1`"));
  }

  #[test]
  fn validation_report_explains_candidate_status_as_review_boundary() {
    let validation = AppValidation {
      validate_version: APP_VALIDATE_VERSION.to_string(),
      created_at_millis: 0,
      source_distillation_path: PathBuf::from("/tmp/distillation.json"),
      source_analysis_path: PathBuf::from("/tmp/analysis.json"),
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
      candidates: vec![AppValidatedCandidate {
        recipe_id: "test.recorded.skill".to_string(),
        taxonomy_id: "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text".to_string(),
        status: AppValidationStatus::Candidate,
        verification_mode: AppVerificationMode::MachineAsserted,
        rationale: "validate observed the legacy query fallback instead of contract-candidate"
          .to_string(),
        selected_case_count: 1,
        recipe_path: PathBuf::from("/tmp/native-text.recipe.json"),
        case_matrix_path: PathBuf::from("/tmp/native-text.cases.json"),
        used_annotation_ids: vec!["native-text-focus-ax".to_string()],
        observed_consumer: Some("query".to_string()),
        observed_candidate_local_id: None,
        candidate_source: Some("query_fallback".to_string()),
        resolved_inputs: BTreeMap::from([("focus_query".to_string(), "Editor".to_string())]),
        unresolved_inputs: Vec::new(),
        failure_message: None,
      }],
      known_boundaries: Vec::new(),
    };

    let report = render_app_validation_report(&validation);

    assert!(report.contains("- `candidate` means the live run succeeded"));
    assert!(report.contains("review boundary"));
    assert!(report.contains("### test.recorded.skill [candidate]"));
    assert!(report.contains(
      "- canonical taxonomy: `native-text.ax-text.ax-perform-action-clipboard-paste.verify-ax-text`"
    ));
    assert!(report.contains("- legacy taxonomy alias: `yes`"));
    assert!(report.contains("- observed consumer: `query`"));
    assert!(report.contains("- candidate source: `query_fallback`"));
  }

  #[test]
  fn build_annotation_candidates_keeps_raw_ocr_as_visible_text_and_adds_selectors() {
    let app = AppIdentity {
      bundle_id: "com.example.music".to_string(),
      app_name: "ExampleMusic".to_string(),
      app_path: None,
      main_executable_path: None,
      version: "1.0".to_string(),
      build_version: "100".to_string(),
      url_schemes: Vec::new(),
      apple_script_addressable: false,
      launch_services_resolved: true,
      resolution_notes: Vec::new(),
    };
    let ax_snapshot = ObservedAxTreeSnapshot {
      observed_at: "2026-05-28T00:00:00Z".to_string(),
      app_name: "ExampleMusic".to_string(),
      bundle_id: "com.example.music".to_string(),
      pid: 4242,
      window_title: "Example".to_string(),
      nodes: vec![
        ObservedAxNode {
          depth: 0,
          path: "0".to_string(),
          role: "AXWindow".to_string(),
          subrole: "AXStandardWindow".to_string(),
          title: String::new(),
          description: String::new(),
          help: String::new(),
          identifier: String::new(),
          placeholder: String::new(),
          value: String::new(),
          bounds: ObservedRect {
            x: 0,
            y: 0,
            width: 1000,
            height: 800,
          },
        },
        ObservedAxNode {
          depth: 1,
          path: "0.0".to_string(),
          role: "AXRow".to_string(),
          subrole: String::new(),
          title: String::new(),
          description: String::new(),
          help: String::new(),
          identifier: String::new(),
          placeholder: String::new(),
          value: String::new(),
          bounds: ObservedRect {
            x: 100,
            y: 190,
            width: 800,
            height: 44,
          },
        },
        ObservedAxNode {
          depth: 1,
          path: "0.1".to_string(),
          role: "AXRow".to_string(),
          subrole: String::new(),
          title: String::new(),
          description: String::new(),
          help: String::new(),
          identifier: String::new(),
          placeholder: String::new(),
          value: String::new(),
          bounds: ObservedRect {
            x: 100,
            y: 250,
            width: 800,
            height: 44,
          },
        },
      ],
    };
    let ocr_snapshot = OcrTextSnapshot {
      recognized_at: "2026-05-28T00:00:00Z".to_string(),
      image_path: PathBuf::from("/tmp/example.png"),
      image_width: 1000,
      image_height: 800,
      query: "Cure For Me".to_string(),
      exact: false,
      case_sensitive: false,
      matches: vec![
        OcrTextMatch {
          match_index: 0,
          text: "Cure For Me".to_string(),
          confidence: 0.97,
          bounds: ObservedRect {
            x: 110,
            y: 200,
            width: 120,
            height: 24,
          },
        },
        OcrTextMatch {
          match_index: 1,
          text: "AURORA".to_string(),
          confidence: 0.95,
          bounds: ObservedRect {
            x: 245,
            y: 203,
            width: 80,
            height: 22,
          },
        },
      ],
    };

    let probe_steps = vec![
      probe_step_fixture(
        "capture-ax-tree",
        "debug.captureAxTree",
        vec![PathBuf::from("/tmp/ax.txt")],
      ),
      probe_step_fixture(
        "ocr-sample",
        "debug.findImageText",
        vec![PathBuf::from("/tmp/ocr.txt")],
      ),
    ];
    let candidates = build_annotation_candidates(
      &app,
      None,
      None,
      &ax_snapshot,
      &ocr_snapshot,
      &probe_steps,
      true,
    );
    let ocr_candidates = candidates
      .iter()
      .filter(|candidate| candidate.source == "ocr" && candidate.kind == "anchor-text")
      .collect::<Vec<_>>();
    assert_eq!(ocr_candidates.len(), 2);
    for candidate in ocr_candidates {
      assert_eq!(candidate.area, "ocr-visible-text");
      assert!(candidate.compatibility.direct_taxonomy_ids.is_empty());
      let gate = candidate
        .promotion_gate
        .as_ref()
        .expect("OCR candidate should expose promotion gate");
      assert_eq!(gate.status, AppCandidatePromotionStatus::Blocked);
      assert!(
        gate
          .missing_gates
          .iter()
          .any(|item| item == "action_contract")
      );
      assert!(
        gate
          .missing_gates
          .iter()
          .any(|item| item == "semantic_verification_contract")
      );
      assert!(
        candidate
          .notes
          .iter()
          .any(|note| { note.contains("OCR text alone is title-level evidence") })
      );
      let query = candidate
        .candidate_query
        .as_ref()
        .expect("OCR candidate should expose selector query");
      assert_eq!(query.selector.within, SelectorScope::TargetWindow);
      assert!(matches!(
        query.selector.any_of.as_slice(),
        [SurfaceSelectorClause::Ocr { .. }]
      ));
      assert_eq!(candidate.evidence_refs.len(), 1);
      assert_eq!(
        candidate.evidence_refs[0].artifact_id.as_str(),
        "artifact_0001"
      );
    }

    let row_candidates = candidates
      .iter()
      .filter(|candidate| candidate.source == "ocr-text" && candidate.kind == "row")
      .collect::<Vec<_>>();
    assert_eq!(row_candidates.len(), 1);
    let row_query = row_candidates[0]
      .candidate_query
      .as_ref()
      .expect("row candidate should expose selector query");
    let row_gate = row_candidates[0]
      .promotion_gate
      .as_ref()
      .expect("row candidate should expose promotion gate");
    assert_eq!(row_gate.status, AppCandidatePromotionStatus::Blocked);
    assert!(
      row_gate
        .missing_gates
        .iter()
        .any(|item| item == "row_action_contract")
    );
    assert!(matches!(
      row_query.selector.any_of.as_slice(),
      [SurfaceSelectorClause::Row {
        row_index: Some(1),
        ..
      }]
    ));
    assert_eq!(row_candidates[0].evidence_refs.len(), 1);
    assert_eq!(
      row_candidates[0].evidence_refs[0].artifact_id.as_str(),
      "artifact_0001"
    );
  }

  #[test]
  fn app_surface_candidate_serializes_promotion_gate_for_machine_consumers() {
    let candidate = AppSurfaceCandidate {
      candidate_id: "candidate_ocr_0".to_string(),
      area: "ocr-visible-text".to_string(),
      kind: "anchor-text".to_string(),
      source: "ocr".to_string(),
      status: AssessmentStatus::Available,
      primary_text: "AURORA".to_string(),
      secondary_text: String::new(),
      query_value: "AURORA".to_string(),
      coordinate_space: "target-window".to_string(),
      bounds: Some(AppRect {
        x: 120,
        y: 80,
        width: 90,
        height: 24,
      }),
      click_point: None,
      confidence: Some(0.95),
      evidence_step_id: "ocr-sample".to_string(),
      candidate_query: None,
      evidence_refs: Vec::new(),
      compatibility: AppCandidateCompatibility {
        direct_taxonomy_ids: Vec::new(),
        context_taxonomy_ids: Vec::new(),
      },
      input_bindings: BTreeMap::new(),
      notes: vec!["visible OCR text only".to_string()],
      promotion_gate: Some(AppCandidatePromotionGate {
        status: AppCandidatePromotionStatus::Blocked,
        missing_gates: vec![
          "action_contract".to_string(),
          "semantic_verification_contract".to_string(),
        ],
        notes: Vec::new(),
      }),
    };

    let value = serde_json::to_value(&candidate).expect("candidate should serialize");
    let promotion_gate = value
      .get("promotion_gate")
      .expect("promotion gate should exist in JSON");
    assert_eq!(
      promotion_gate.get("status"),
      Some(&serde_json::json!("blocked"))
    );
    assert_eq!(
      promotion_gate.get("missing_gates"),
      Some(&serde_json::json!([
        "action_contract",
        "semantic_verification_contract"
      ]))
    );
  }

  #[test]
  fn app_analysis_json_round_trip_preserves_surface_contract_fields() {
    let root = temp_dir("app-analysis-round-trip");
    let analysis_path = root.join("analysis.json");
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
        ocr_sample_match_count: 1,
        stable_anchor_candidates: vec!["appName: Example".to_string()],
        stable_region_candidates: vec!["primaryWindow=0,0,100,100".to_string()],
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
        evidence_step_id: "capture-ax-tree".to_string(),
        candidate_query: Some(CandidateQuery {
          query_id: "search-entry-focus-ax".to_string(),
          selector: SurfaceSelector {
            any_of: vec![SurfaceSelectorClause::Ax {
              role: Some("AXTextField".to_string()),
              label: Some("Search".to_string()),
              path: Some("0.1".to_string()),
              enabled: None,
              visible: Some(true),
            }],
            within: SelectorScope::TargetWindow,
            require_visible: true,
          },
          output_kind: Some("focus-query".to_string()),
          known_limits: vec!["test query".to_string()],
        }),
        evidence_refs: vec![ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        }],
        promotion_gate: Some(AppCandidatePromotionGate {
          status: AppCandidatePromotionStatus::ActionGradeCandidate,
          missing_gates: Vec::new(),
          notes: vec!["Sample candidate satisfies the v0 search-entry promotion seam.".to_string()],
        }),
        input_bindings: BTreeMap::from([("focus_query".to_string(), "Search".to_string())]),
        compatibility: candidate_compatibility(
          &["search-entry.ax-text-input.clipboard-submit.capture-evidence"],
          &[],
        ),
        notes: vec!["sample note".to_string()],
      }],
      known_boundaries: vec![],
      recommended_strategies: vec![],
    };

    write_pretty_json(&analysis_path, &analysis).expect("analysis should write");
    let loaded: AppAnalysis = read_json(&analysis_path).expect("analysis should read");

    let candidate = loaded
      .annotation_candidates
      .first()
      .expect("sample analysis should carry one candidate");
    assert!(candidate.candidate_query.is_some());
    assert_eq!(candidate.evidence_refs.len(), 1);
    let promotion_gate = candidate
      .promotion_gate
      .as_ref()
      .expect("promotion gate should survive round trip");
    assert_eq!(
      promotion_gate.status,
      AppCandidatePromotionStatus::ActionGradeCandidate
    );
    assert!(promotion_gate.missing_gates.is_empty());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn app_distillation_json_round_trip_preserves_row_review_only_fields() {
    let root = temp_dir("app-distillation-round-trip");
    let distillation_path = root.join("distillation.json");
    let distillation = AppDistillation {
      distill_version: APP_DISTILL_VERSION.to_string(),
      created_at_millis: 0,
      source_analysis_path: PathBuf::from("/tmp/analysis.json"),
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
      candidates: vec![AppDistilledCandidate {
        recipe_id: "test.result.selection".to_string(),
        taxonomy_id: "result-selection.ocr-anchor.pointer-click.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "row suggestions stay review-only".to_string(),
        suggested_annotation_ids: vec!["visible-row-1".to_string()],
        source_evidence_refs: vec![ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        }],
        promoted_candidate: None,
        candidate_shape: AppDistilledCandidateShape {
          direct_candidate_ids: Vec::new(),
          context_candidate_ids: vec!["visible-row-1".to_string()],
          provided_inputs: BTreeMap::new(),
          notes: vec![
            "No direct candidate shape was available for taxonomy result-selection.ocr-anchor.pointer-click.capture-evidence during distill.".to_string(),
            "Context-only candidates were recorded for later review, but they did not project directly into recipe inputs.".to_string(),
          ],
        },
        recipe_path: PathBuf::from("/tmp/result-selection.recipe.json"),
        case_matrix_path: PathBuf::from("/tmp/result-selection.cases.json"),
      }],
      known_boundaries: vec![
        "Grouped visible rows remain surface candidates until a row action exists."
          .to_string(),
      ],
    };

    write_pretty_json(&distillation_path, &distillation).expect("distillation should write");

    let reloaded: AppDistillation =
      read_json(&distillation_path).expect("distillation should read");
    let candidate = &reloaded.candidates[0];

    assert_eq!(
      candidate.suggested_annotation_ids,
      vec!["visible-row-1".to_string()]
    );
    assert!(candidate.promoted_candidate.is_none());
    assert!(candidate.candidate_shape.direct_candidate_ids.is_empty());
    assert_eq!(
      candidate.candidate_shape.context_candidate_ids,
      vec!["visible-row-1".to_string()]
    );
    assert!(candidate.candidate_shape.provided_inputs.is_empty());
    assert!(
      candidate
        .candidate_shape
        .notes
        .iter()
        .any(|note| note.contains("No direct candidate shape was available"))
    );
    assert!(
      candidate
        .candidate_shape
        .notes
        .iter()
        .any(|note| note.contains("Context-only candidates were recorded"))
    );
    assert_eq!(candidate.source_evidence_refs.len(), 1);
    assert_eq!(
      candidate.source_evidence_refs[0].artifact_id.as_str(),
      "artifact_0001"
    );
    assert!(
      reloaded
        .known_boundaries
        .iter()
        .any(|note| note.contains("Grouped visible rows remain surface candidates"))
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
    let windows_path = root.join("artifact_window-list.txt");

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
        bundle_id: "com.example.missing".to_string(),
        app_name: "com.example.missing".to_string(),
        app_path: None,
        main_executable_path: None,
        version: "unknown".to_string(),
        build_version: "unknown".to_string(),
        url_schemes: vec![],
        apple_script_addressable: false,
        launch_services_resolved: false,
        resolution_notes: vec![
          "LaunchServices could not resolve `com.example.missing`.".to_string(),
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
        probe_step_fixture("list-windows", "debug.listWindows", vec![windows_path]),
        failed_probe_step_fixture(
          "capture-ax-tree",
          "debug.captureAxTree",
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
        .any(|entry| entry.contains("capture-ax-tree"))
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
    let windows_path = root.join("artifact_window-list.txt");
    let ax_path = root.join("artifact_ax-tree.txt");

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
      "observedAt=2026-05-19T00:00:00Z\nfrontmostAppName=ExampleMusic\nfrontmostWindowTitle=\nwindowCount=0\n",
    )
    .expect("windows artifact should write");
    fs::write(
      &ax_path,
      "observedAt=2026-05-19T00:00:00Z\nappName=ExampleMusic\nbundleId=com.example.music\npid=44741\nwindowTitle=\nrootRole=AXWindow\nnode\t0\t0\tAXWindow\tAXStandardWindow\t\t\t\t\t\t\t227\t100\t1058\t752\nnodeCount=1\n",
    )
    .expect("ax artifact should write");

    let probe = AppProbe {
      probe_version: APP_PROBE_VERSION.to_string(),
      created_at_millis: 0,
      project_root: root.clone(),
      output_dir: root.clone(),
      app: AppIdentity {
        bundle_id: "com.example.music".to_string(),
        app_name: "ExampleMusic".to_string(),
        app_path: Some(PathBuf::from("/Applications/ExampleMusic.app")),
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
        probe_step_fixture("list-windows", "debug.listWindows", vec![windows_path]),
        probe_step_fixture("capture-ax-tree", "debug.captureAxTree", vec![ax_path]),
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
    assert_eq!(window_candidate.evidence_step_id, "capture-ax-tree");
    assert_eq!(window_candidate.evidence_refs.len(), 1);
    assert_eq!(
      window_candidate.evidence_refs[0].artifact_id.as_str(),
      "artifact_0001"
    );
    assert_eq!(
      window_candidate.input_bindings.get("relative_x"),
      Some(&"0.500000".to_string())
    );
    assert_eq!(
      window_candidate.input_bindings.get("relative_y"),
      Some(&"0.500000".to_string())
    );
    let promotion_gate = window_candidate
      .promotion_gate
      .as_ref()
      .expect("window candidate should expose promotion gate");
    assert_eq!(
      promotion_gate.status,
      AppCandidatePromotionStatus::ActionGradeCandidate
    );
    assert!(promotion_gate.missing_gates.is_empty());
    assert!(analysis.recommended_strategies.iter().any(|strategy| {
      strategy.taxonomy_id == "window-action.window-point.pointer-click.capture-evidence"
    }));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_app_analysis_keeps_plain_text_editor_out_of_search_entry_recommendations() {
    let root = temp_dir("plain-text-editor-app-probe");
    let probe_path = root.join("probe.json");
    let permissions_path = root.join("artifact_probe-permissions.txt");
    let displays_path = root.join("artifact_display-list.json");
    let readiness_path = root.join("artifact_coordinate-readiness-report.txt");
    let windows_path = root.join("artifact_window-list.txt");
    let ax_path = root.join("artifact_ax-tree.txt");

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
      "readyForLogicalInput=true\nreason=logical input is aligned\n",
    )
    .expect("readiness artifact should write");
    fs::write(
      &windows_path,
      "observedAt=2026-06-04T00:00:00Z\nfrontmostAppName=TextEdit\nfrontmostWindowTitle=Untitled\nwindowCount=1\nwindow\t0\tTextEdit\tUntitled\t200\t180\t900\t640\n",
    )
    .expect("windows artifact should write");
    fs::write(
      &ax_path,
      "observedAt=2026-06-04T00:00:00Z\nappName=TextEdit\nbundleId=com.apple.TextEdit\npid=12345\nwindowTitle=Untitled\nrootRole=AXWindow\nnode\t0\t0\tAXWindow\tAXStandardWindow\t\t\t\t\t\t\t200\t180\t900\t640\nnode\t1\t0.0\tAXScrollArea\t\t\t\t\t\t\t\t210\t220\t860\t560\nnode\t2\t0.0.0\tAXTextArea\t\t\t\t\t\tFirst Text View\tbody text\t220\t230\t840\t520\nnodeCount=3\n",
    )
    .expect("ax artifact should write");

    let probe = AppProbe {
      probe_version: APP_PROBE_VERSION.to_string(),
      created_at_millis: 0,
      project_root: root.clone(),
      output_dir: root.clone(),
      app: AppIdentity {
        bundle_id: "com.apple.TextEdit".to_string(),
        app_name: "TextEdit".to_string(),
        app_path: Some(PathBuf::from("/Applications/TextEdit.app")),
        main_executable_path: None,
        version: "1.0".to_string(),
        build_version: "1".to_string(),
        url_schemes: vec![],
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
        probe_step_fixture("list-windows", "debug.listWindows", vec![windows_path]),
        probe_step_fixture("capture-ax-tree", "debug.captureAxTree", vec![ax_path]),
      ],
    };

    let analysis = build_app_analysis(&probe_path, &probe).expect("analysis should still build");
    assert!(analysis.annotation_candidates.iter().any(|candidate| {
      candidate.candidate_id == "native-text-focus-ax" && candidate.area == "native-text"
    }));
    assert!(!analysis.recommended_strategies.iter().any(|strategy| {
      strategy.taxonomy_id == "search-entry.ax-text-input.clipboard-submit.capture-evidence"
    }));
    assert!(
      analysis
        .recommended_strategies
        .iter()
        .any(|strategy| { strategy.taxonomy_id == NATIVE_TEXT_CANONICAL_TAXONOMY_ID })
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn build_app_analysis_recommends_search_entry_for_real_search_field() {
    let root = temp_dir("search-entry-app-probe");
    let probe_path = root.join("probe.json");
    let permissions_path = root.join("artifact_probe-permissions.txt");
    let displays_path = root.join("artifact_display-list.json");
    let readiness_path = root.join("artifact_coordinate-readiness-report.txt");
    let windows_path = root.join("artifact_window-list.txt");
    let ax_path = root.join("artifact_ax-tree.txt");

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
      "readyForLogicalInput=true\nreason=logical input is aligned\n",
    )
    .expect("readiness artifact should write");
    fs::write(
      &windows_path,
      "observedAt=2026-06-04T00:00:00Z\nfrontmostAppName=Notes\nfrontmostWindowTitle=Notes\nwindowCount=1\nwindow\t0\tNotes\tNotes\t160\t40\t1200\t900\n",
    )
    .expect("windows artifact should write");
    fs::write(
      &ax_path,
      "observedAt=2026-06-04T00:00:00Z\nappName=Notes\nbundleId=com.apple.Notes\npid=22334\nwindowTitle=Notes\nrootRole=AXWindow\nnode\t0\t0\tAXWindow\tAXStandardWindow\t\t\t\t\t\t\t160\t40\t1200\t900\nnode\t1\t0.0\tAXGroup\t\t\t\t\t\t\t\t170\t50\t1180\t880\nnode\t2\t0.0.0\tAXTextField\tAXSearchField\t\t\t\tSearch\t\t\t880\t60\t280\t36\nnode\t2\t0.0.1\tAXTextArea\t\t\t\t\t\tNote Body Text View\tbody\t240\t120\t880\t760\nnodeCount=4\n",
    )
    .expect("ax artifact should write");

    let probe = AppProbe {
      probe_version: APP_PROBE_VERSION.to_string(),
      created_at_millis: 0,
      project_root: root.clone(),
      output_dir: root.clone(),
      app: AppIdentity {
        bundle_id: "com.apple.Notes".to_string(),
        app_name: "Notes".to_string(),
        app_path: Some(PathBuf::from("/Applications/Notes.app")),
        main_executable_path: None,
        version: "1.0".to_string(),
        build_version: "1".to_string(),
        url_schemes: vec![],
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
        probe_step_fixture("list-windows", "debug.listWindows", vec![windows_path]),
        probe_step_fixture("capture-ax-tree", "debug.captureAxTree", vec![ax_path]),
      ],
    };

    let analysis = build_app_analysis(&probe_path, &probe).expect("analysis should still build");
    assert!(analysis.annotation_candidates.iter().any(|candidate| {
      candidate.candidate_id == "search-entry-focus-ax" && candidate.area == "search-entry"
    }));
    assert!(analysis.recommended_strategies.iter().any(|strategy| {
      strategy.taxonomy_id == "search-entry.ax-text-input.clipboard-submit.capture-evidence"
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
  fn sample_promotable_ax_focus_analysis(
    area: &str,
    candidate_id: &str,
    taxonomy_id: &str,
    query_value: &str,
    promotion_note: &str,
  ) -> AppAnalysis {
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
        stable_region_candidates: vec!["primaryWindow=0,0,100,100".to_string()],
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
      annotation_candidates: vec![AppSurfaceCandidate {
        candidate_id: candidate_id.to_string(),
        area: area.to_string(),
        kind: "focus-query".to_string(),
        source: "ax".to_string(),
        status: AssessmentStatus::Candidate,
        primary_text: query_value.to_string(),
        secondary_text: "role=AXTextField path=0.1".to_string(),
        query_value: query_value.to_string(),
        coordinate_space: "global-logical".to_string(),
        bounds: Some(AppRect {
          x: 10,
          y: 10,
          width: 80,
          height: 20,
        }),
        click_point: Some(AppPoint { x: 50, y: 20 }),
        confidence: None,
        evidence_step_id: "capture-ax-tree".to_string(),
        evidence_refs: vec![ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        }],
        candidate_query: Some(CandidateQuery {
          query_id: candidate_id.to_string(),
          selector: SurfaceSelector {
            any_of: vec![SurfaceSelectorClause::Ax {
              role: Some("AXTextField".to_string()),
              label: Some(query_value.to_string()),
              path: Some("0.1".to_string()),
              enabled: None,
              visible: Some(true),
            }],
            within: SelectorScope::TargetWindow,
            require_visible: true,
          },
          output_kind: Some("focus-query".to_string()),
          known_limits: vec!["test query".to_string()],
        }),
        promotion_gate: Some(AppCandidatePromotionGate {
          status: AppCandidatePromotionStatus::ActionGradeCandidate,
          missing_gates: Vec::new(),
          notes: vec![promotion_note.to_string()],
        }),
        input_bindings: BTreeMap::from([("focus_query".to_string(), query_value.to_string())]),
        compatibility: candidate_compatibility(&[taxonomy_id], &[]),
        notes: vec!["sample note".to_string()],
      }],
      known_boundaries: vec![],
      recommended_strategies: vec![AppRecommendedStrategy {
        taxonomy_id: taxonomy_id.to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
      }],
    }
  }

  fn sample_promotable_window_action_analysis() -> AppAnalysis {
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
        primary_window_bounds: Some(AppRect {
          x: 100,
          y: 200,
          width: 800,
          height: 600,
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
        stable_region_candidates: vec!["primaryWindow=100,200,800,600".to_string()],
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
      annotation_candidates: vec![AppSurfaceCandidate {
        candidate_id: "window-primary-region".to_string(),
        area: "window.primary".to_string(),
        kind: "region".to_string(),
        source: "window".to_string(),
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
        evidence_step_id: "list-windows".to_string(),
        candidate_query: None,
        evidence_refs: vec![ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0002"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        }],
        promotion_gate: Some(AppCandidatePromotionGate {
          status: AppCandidatePromotionStatus::ActionGradeCandidate,
          missing_gates: Vec::new(),
          notes: vec![
            "Sample candidate satisfies the v0 window-action promotion seam.".to_string(),
          ],
        }),
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
      }],
      known_boundaries: vec![],
      recommended_strategies: vec![AppRecommendedStrategy {
        taxonomy_id: "window-action.window-point.pointer-click.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
      }],
    }
  }

  fn sample_promotable_result_selection_analysis() -> AppAnalysis {
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
        primary_window_bounds: Some(AppRect {
          x: 100,
          y: 200,
          width: 800,
          height: 600,
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
        accessibility_tree: AssessmentStatus::Candidate,
        menu_surface: AssessmentStatus::Unknown,
        shortcut_surface: AssessmentStatus::Candidate,
        apple_script_surface: AssessmentStatus::Unavailable,
        url_scheme_surface: AssessmentStatus::Unavailable,
        keyboard_first_surface: AssessmentStatus::Candidate,
        pointer_fallback_surface: AssessmentStatus::Likely,
      },
      grounding_assessment: AppGroundingAssessment {
        ocr_sample_query: "Play Now".to_string(),
        ocr_sample_status: AssessmentStatus::Candidate,
        ocr_sample_match_count: 1,
        stable_anchor_candidates: vec!["appName: Example".to_string()],
        stable_region_candidates: vec!["primaryWindow=100,200,800,600".to_string()],
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
      annotation_candidates: vec![AppSurfaceCandidate {
        candidate_id: "result-selection-anchor-ax".to_string(),
        area: "result-selection".to_string(),
        kind: "anchor-text".to_string(),
        source: "ocr".to_string(),
        status: AssessmentStatus::Candidate,
        primary_text: "Play Now".to_string(),
        secondary_text: "visible result anchor".to_string(),
        query_value: "Play Now".to_string(),
        coordinate_space: "global-logical".to_string(),
        bounds: Some(AppRect {
          x: 180,
          y: 260,
          width: 160,
          height: 32,
        }),
        click_point: Some(AppPoint { x: 260, y: 276 }),
        confidence: Some(0.97),
        evidence_step_id: "ocr-sample".to_string(),
        candidate_query: Some(CandidateQuery {
          query_id: "result-selection-anchor-ax".to_string(),
          selector: SurfaceSelector {
            any_of: vec![SurfaceSelectorClause::Ocr {
              text: "Play Now".to_string(),
              region_hint: Some(crate::contract::RatioRegion {
                left: 0.10,
                top: 0.10,
                right: 0.35,
                bottom: 0.25,
              }),
              min_provider_score: Some(0.97),
            }],
            within: SelectorScope::TargetWindow,
            require_visible: true,
          },
          output_kind: Some("anchor-text".to_string()),
          known_limits: vec!["test query".to_string()],
        }),
        evidence_refs: vec![ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0003"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        }],
        promotion_gate: Some(AppCandidatePromotionGate {
          status: AppCandidatePromotionStatus::ActionGradeCandidate,
          missing_gates: Vec::new(),
          notes: vec![
            "Sample candidate satisfies the v0 result-selection promotion seam.".to_string(),
          ],
        }),
        input_bindings: BTreeMap::from([("anchor_text".to_string(), "Play Now".to_string())]),
        compatibility: candidate_compatibility(
          &["result-selection.ocr-anchor.pointer-click.capture-evidence"],
          &[],
        ),
        notes: vec!["sample note".to_string()],
      }],
      known_boundaries: vec![],
      recommended_strategies: vec![AppRecommendedStrategy {
        taxonomy_id: "result-selection.ocr-anchor.pointer-click.capture-evidence".to_string(),
        status: AssessmentStatus::Candidate,
        rationale: "test".to_string(),
      }],
    }
  }

  fn sample_result_selection_promoted_candidate() -> crate::contract::Candidate {
    crate::contract::Candidate {
      candidate_local_id: "result-selection-anchor-ax".to_string(),
      kind: "result_selection".to_string(),
      label: Some("Play Now".to_string()),
      target_spec: TargetSpec {
        grounding: crate::contract::TargetGrounding::OcrAnchor,
        anchor_text: Some("Play Now".to_string()),
        region_hint: Some(RatioRegion {
          left: 0.10,
          top: 0.10,
          right: 0.35,
          bottom: 0.25,
        }),
        row_index: None,
      },
      evidence: CandidateEvidence {
        artifact_ref: ArtifactRef {
          run_id: crate::trace::RunId::new("run_probe"),
          span_id: crate::trace::SpanId::new("span_probe"),
          artifact_id: crate::trace::ArtifactId::new("artifact_0003"),
          captured_event_id: Some(crate::trace::EventId::new("event_probe")),
        },
        observation: serde_json::json!({
          "source": "ocr",
          "surface_candidate_id": "result-selection-anchor-ax",
          "evidence_step_id": "ocr-sample",
          "query": {
            "query_id": "result-selection-anchor-ax",
            "output_kind": "anchor-text",
            "selector_within": "target_window",
            "require_visible": true,
            "ocr": {
              "text": "Play Now",
              "region_hint": {
                "left": 0.10,
                "top": 0.10,
                "right": 0.35,
                "bottom": 0.25
              },
              "min_provider_score": 0.97
            }
          },
          "match_index": 1,
          "window_context": {
            "app_bundle_id": "com.example.App",
            "window_title": "Example"
          }
        }),
      },
      liveness: CandidateLiveness {
        preconditions: LivenessPreconditions {
          window_ref: Some(WindowRefPrecondition {
            app_bundle_id: "com.example.App".to_string(),
            window_title_substring: Some("Example".to_string()),
            window_number: None,
          }),
          anchor_recheck: Some(AnchorRecheckPrecondition {
            text: "Play Now".to_string(),
            region_hint: Some(RatioRegion {
              left: 0.10,
              top: 0.10,
              right: 0.35,
              bottom: 0.25,
            }),
            expected_min_confidence: 0.97,
            max_pixel_distance: 48.0,
          }),
        },
        ttl_hint_ms: Some(5_000),
      },
      control: ControlRequirements {
        requires_app_frontmost: true,
        requires_window_focus: true,
      },
      known_limits: vec![
        "Promotion only covers refinding the OCR anchor surface; semantic success still relies on later verification.".to_string(),
      ],
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
      span_id: "span_fixture".to_string(),
      status: RunStatus::Completed.as_str().to_string(),
      output_summary: "ok".to_string(),
      artifacts: artifact_paths
        .iter()
        .enumerate()
        .map(|(index, path)| AppProbeArtifact {
          artifact_id: format!("artifact_{:04}", index + 1),
          span_id: "span_fixture".to_string(),
          path: path.clone(),
          role: path
            .extension()
            .and_then(|value| value.to_str())
            .map(|extension| format!("fixture-{extension}"))
            .unwrap_or_else(|| "fixture".to_string()),
          captured_event_id: None,
        })
        .collect(),
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
      span_id: "span_fixture".to_string(),
      status: RunStatus::Failed.as_str().to_string(),
      output_summary: format!("Probe step {id} failed"),
      artifact_paths: Vec::new(),
      artifacts: Vec::new(),
      failure_message: Some(error.to_string()),
    }
  }

  fn test_runtime(project_root: PathBuf) -> Runtime {
    let commands = CommandCatalog::new(vec![
      CommandSpec {
        id: "test.first",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test first command",
        driver_id: "test.probe",
        operation: "first",
        disturbance_classes: &[DisturbanceClass::None],
        max_disturbance: DisturbanceClass::None,
      },
      CommandSpec {
        id: "test.second",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test second command",
        driver_id: "test.probe",
        operation: "second",
        disturbance_classes: &[DisturbanceClass::None],
        max_disturbance: DisturbanceClass::None,
      },
      CommandSpec {
        id: "test.skill.invoke",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test skill command",
        driver_id: "test.probe",
        operation: "test_operation",
        disturbance_classes: &[DisturbanceClass::None],
        max_disturbance: DisturbanceClass::None,
      },
      CommandSpec {
        id: "test.artifact",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test artifact command",
        driver_id: "test.probe",
        operation: "artifact",
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
        "activation": "ax-perform-action-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test app validation nesting",
      "disturbance_policy": {
        "max_disturbance": "none",
        "declared_classes": ["none"]
      },
      "inputs": {
        "focus_candidate": { "type": "string", "default": "" },
        "focus_query": { "type": "string", "default": "" },
        "require_focus_candidate": { "type": "string", "default": "false" }
      },
      "steps": [{
        "id": "first",
        "command_id": "test.skill.invoke",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        },
        "args": {
          "candidate": "${focus_candidate}",
          "query": "${focus_query}",
          "require_focus_candidate": "${require_focus_candidate}"
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
        "inputs": {
          "require_focus_candidate": "false"
        },
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
        "click_candidate": { "type": "string", "default": "" },
        "relative_x": { "type": "number" },
        "relative_y": { "type": "number" },
        "require_click_candidate": { "type": "string", "default": "false" }
      },
      "steps": [{
        "id": "first",
        "command_id": "test.skill.invoke",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        },
        "args": {
          "click_candidate": "${click_candidate}",
          "require_click_candidate": "${require_click_candidate}"
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
          "relative_y": "TODO_RELATIVE_Y",
          "require_click_candidate": "false"
        },
        "disturbance": "none"
      }]
    })
  }
}
