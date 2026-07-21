// File: src/app/mod.rs
//! App-centric workflows: probe → analyze.
//!
//! This module is a tooling pipeline that turns observed runs/artifacts into:
//! (1) an app probe snapshot and (2) analysis reports.
//!
//! Boundary: this is not the core `Runtime` executor and does not implement
//! macOS automation primitives (drivers do). It exists to make app capability
//! probing and surface analysis inspectable and reproducible.

mod analysis;
mod infra;
mod report;

use analysis::{build_app_analysis, resolve_probe_ocr_sample_query};
use infra::{
  default_probe_output_dir, finish_failed_app_run, first_non_empty_string, invoke_probe_step, read_json, resolve_app_identity,
  resolve_probe_path, stage_app_artifact, write_pretty_json,
};
use report::render_app_analysis_report;

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use auv_driver_macos::types::ObservedRect;
use serde::{Deserialize, Serialize};

use crate::contract::ArtifactRef;
use crate::model::{AuvResult, now_millis};
use crate::runtime::Runtime;
use auv_tracing_driver::RecordingHandle;
use auv_tracing_driver::run_builder::{RecordingRun, RunFinish, RunSpec, SpanRef};
use auv_tracing_driver::store::sanitized_artifact_name;
use auv_tracing_driver::trace::{RunType, TraceStatusCode};

const APP_PROBE_VERSION: &str = "v0";
const APP_ANALYSIS_VERSION: &str = "v0";

pub(crate) fn resolve_probe_window_title(app: &AppIdentity, steps: &[AppProbeStep]) -> Option<String> {
  let window_report = steps.iter().find(|step| step.id == "list-windows").and_then(|step| {
    step.artifact_paths.iter().find_map(|path| {
      let is_window_report = path.extension().and_then(|value| value.to_str()).is_some_and(|value| value.eq_ignore_ascii_case("txt"))
        && path.file_name().and_then(|value| value.to_str()).is_some_and(|name| name.contains("window-list"));
      if !is_window_report {
        return None;
      }
      fs::read_to_string(path).ok()
    })
  });
  first_non_empty_string(&[
    window_report.as_deref().and_then(|report| extract_window_title_from_window_report(report, &app.bundle_id)),
    window_report
      .as_deref()
      .and_then(|report| report.lines().find_map(|line| line.strip_prefix("frontmostWindowTitle=")))
      .map(str::to_string),
    window_report.as_deref().and_then(|report| report.lines().find_map(|line| line.strip_prefix("frontmostAppName="))).map(str::to_string),
  ])
  .or_else(|| {
    steps
      .iter()
      .find(|step| step.id == "list-windows")
      .and_then(|step| step.output_summary.split_once("frontmost app is ").map(|(_, suffix)| suffix.trim_end_matches('.').to_string()))
      .filter(|value| !value.trim().is_empty())
  })
}

fn extract_window_title_from_window_report(report: &str, bundle_id: &str) -> Option<String> {
  report.lines().find_map(|line| {
    let fields = line.split('\t').collect::<Vec<_>>();
    if fields.first().copied() != Some("window") {
      return None;
    }
    if fields.len() >= 11 && fields.get(3).copied() == Some(bundle_id) {
      return fields.get(6).map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    }
    if fields.len() >= 4 {
      return fields.last().map(|value| value.trim().to_string()).filter(|value| !value.is_empty());
    }
    None
  })
}

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
  pub primary_window_bounds: Option<ObservedRect>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AppCandidateGroundingTaxonomy {
  SearchEntryAxTextInputClipboardSubmitCaptureEvidence,
  NativeTextAxTextAxPerformActionClipboardPasteVerifyAxText,
  ResultSelectionOcrAnchorPointerClickCaptureEvidence,
  WindowActionWindowPointPointerClickCaptureEvidence,
}

const SEARCH_ENTRY_TAXONOMY_ID: &str = "search-entry.ax-text-input.clipboard-submit.capture-evidence";
const NATIVE_TEXT_CANONICAL_TAXONOMY_ID: &str = "native-text.ax-text.ax-perform-action-clipboard-paste.verify-ax-text";
const NATIVE_TEXT_LEGACY_TAXONOMY_ID: &str = "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text";
const RESULT_SELECTION_TAXONOMY_ID: &str = "result-selection.ocr-anchor.pointer-click.capture-evidence";
const WINDOW_ACTION_TAXONOMY_ID: &str = "window-action.window-point.pointer-click.capture-evidence";

fn is_native_text_taxonomy_id(raw: &str) -> bool {
  matches!(raw.trim(), NATIVE_TEXT_CANONICAL_TAXONOMY_ID | NATIVE_TEXT_LEGACY_TAXONOMY_ID)
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
      NATIVE_TEXT_CANONICAL_TAXONOMY_ID => Ok(Self::NativeTextAxTextAxPerformActionClipboardPasteVerifyAxText),
      RESULT_SELECTION_TAXONOMY_ID => Ok(Self::ResultSelectionOcrAnchorPointerClickCaptureEvidence),
      WINDOW_ACTION_TAXONOMY_ID => Ok(Self::WindowActionWindowPointPointerClickCaptureEvidence),
      other => Err(format!("unsupported candidate grounding taxonomy {}. allowed values: {}", other, Self::allowed_ids().join(", "))),
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
  pub bounds: Option<ObservedRect>,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AppRecommendedStrategy {
  pub taxonomy_id: String,
  pub status: AssessmentStatus,
  pub rationale: String,
}

pub(crate) fn rect_center(rect: &ObservedRect) -> (i64, i64) {
  (rect.x + rect.width / 2, rect.y + rect.height / 2)
}

pub(crate) fn rect_center_point(rect: &ObservedRect) -> AppPoint {
  let (x, y) = rect_center(rect);
  AppPoint { x, y }
}

pub(crate) fn render_compact_rect(rect: &ObservedRect) -> String {
  format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height)
}

pub(crate) fn rect_relative_point(rect: &ObservedRect, point: &AppPoint) -> Option<(f64, f64)> {
  if rect.width <= 0 || rect.height <= 0 {
    return None;
  }
  let relative_x = (point.x - rect.x) as f64 / rect.width as f64;
  let relative_y = (point.y - rect.y) as f64 / rect.height as f64;
  Some((relative_x, relative_y))
}

pub fn probe_app(project_root: &Path, runtime: &Runtime, bundle_id: &str, output_dir: Option<PathBuf>) -> AuvResult<AppProbe> {
  let app = resolve_app_identity(bundle_id)?;
  let output_dir = output_dir.unwrap_or_else(|| default_probe_output_dir(project_root, bundle_id));
  if output_dir.exists() {
    return Err(format!("probe output directory already exists: {}", output_dir.display()));
  }
  fs::create_dir_all(&output_dir).map_err(|error| format!("failed to create app probe directory {}: {error}", output_dir.display()))?;

  let recording = runtime.recording().handle();
  let mut run = recording.start_run(RunSpec::new(RunType::Probe, "auv.probe"))?;
  let root_span = run.root_span();
  let result = probe_app_into_run(project_root, runtime, &app, &output_dir, &recording, &mut run, &root_span);
  match result {
    Ok(probe) => {
      recording.finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some(format!("Probed app {}", probe.app.bundle_id)),
          failure: None,
        },
      )?;
      Ok(probe)
    }
    Err(error) => finish_failed_app_run(&recording, run, error, format!("App probe {bundle_id} failed")),
  }
}

fn probe_app_into_run(
  project_root: &Path,
  runtime: &Runtime,
  app: &AppIdentity,
  output_dir: &Path,
  recording: &RecordingHandle,
  run: &mut RecordingRun,
  parent: &SpanRef,
) -> AuvResult<AppProbe> {
  let mut steps = Vec::new();
  steps.push(invoke_probe_step(runtime, run, parent, "probe-permissions", "app.probePermissions", None, BTreeMap::new(), false)?);
  steps.push(invoke_probe_step(runtime, run, parent, "list-displays", "display.list", None, BTreeMap::new(), false)?);
  let can_activate_target = app.apple_script_addressable;
  if can_activate_target {
    let mut activate_inputs = BTreeMap::new();
    activate_inputs.insert("settle_ms".to_string(), "250".to_string());
    steps.push(invoke_probe_step(
      runtime,
      run,
      parent,
      "activate-target-app",
      "app.activate",
      Some(app.bundle_id.clone()),
      activate_inputs,
      true,
    )?);
  }

  let mut window_inputs = BTreeMap::new();
  window_inputs.insert("limit".to_string(), "20".to_string());
  steps.push(invoke_probe_step(runtime, run, parent, "list-windows", "window.list", Some(app.bundle_id.clone()), window_inputs, true)?);

  let can_use_app_scoped_window_ops = can_activate_target || app.launch_services_resolved;
  let fallback_window_title = if can_use_app_scoped_window_ops {
    None
  } else {
    resolve_probe_window_title(app, &steps)
  };

  let mut tree_inputs = BTreeMap::new();
  tree_inputs.insert("max_depth".to_string(), "6".to_string());
  if !can_use_app_scoped_window_ops {
    if let Some(title) = fallback_window_title.clone() {
      tree_inputs.insert("title".to_string(), title);
    }
  }
  tree_inputs.insert("max_children".to_string(), "24".to_string());
  steps.push(invoke_probe_step(
    runtime,
    run,
    parent,
    "capture-ax-tree",
    "window.captureAxTree",
    Some(app.bundle_id.clone()),
    tree_inputs,
    true,
  )?);

  let capture_label = format!("app-probe-{}", sanitized_artifact_name(&app.bundle_id));
  let mut capture_inputs = BTreeMap::new();
  capture_inputs.insert("label".to_string(), capture_label);
  if can_activate_target {
    capture_inputs.insert("activate_target_before_capture".to_string(), "true".to_string());
  }
  if !can_use_app_scoped_window_ops {
    if let Some(title) = fallback_window_title.clone() {
      capture_inputs.insert("title".to_string(), title);
    }
  }
  let capture_step =
    invoke_probe_step(runtime, run, parent, "capture-window", "window.capture", Some(app.bundle_id.clone()), capture_inputs, true)?;
  steps.push(capture_step);

  let ocr_sample_query = resolve_probe_ocr_sample_query(app, &steps);
  let mut ocr_inputs = BTreeMap::new();
  ocr_inputs.insert("label".to_string(), "app-probe-ocr-sample".to_string());
  ocr_inputs.insert("query".to_string(), ocr_sample_query);
  ocr_inputs.insert("min_confidence".to_string(), "0.55".to_string());
  ocr_inputs.insert("region_left_ratio".to_string(), "0.0".to_string());
  ocr_inputs.insert("region_top_ratio".to_string(), "0.0".to_string());
  ocr_inputs.insert("region_right_ratio".to_string(), "1.0".to_string());
  ocr_inputs.insert("region_bottom_ratio".to_string(), "1.0".to_string());
  ocr_inputs.insert("max_observations".to_string(), "20".to_string());
  if !can_use_app_scoped_window_ops {
    if let Some(title) = fallback_window_title {
      ocr_inputs.insert("title".to_string(), title);
    }
  }
  steps.push(invoke_probe_step(runtime, run, parent, "ocr-sample", "window.observeRegion", Some(app.bundle_id.clone()), ocr_inputs, true)?);

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
  stage_app_artifact(&recording, run, parent, "probe.output", &probe_path, "probe.json")?;
  Ok(probe)
}

pub fn analyze_app_probe(runtime: &Runtime, query: &Path) -> AuvResult<AppAnalyzeOutput> {
  let recording = runtime.recording().handle();
  let mut run = recording.start_run(RunSpec::new(RunType::Analyze, "auv.analyze"))?;
  let root_span = run.root_span();
  let result = analyze_app_probe_into_run(runtime, &recording, &mut run, &root_span, query);
  match result {
    Ok(output) => {
      recording.finish_run(
        run,
        RunFinish {
          status_code: TraceStatusCode::Ok,
          summary: Some(format!("Analyzed app {}", output.analysis.app_identity.bundle_id)),
          failure: None,
        },
      )?;
      Ok(output)
    }
    Err(error) => finish_failed_app_run(&recording, run, error, "App analysis failed".to_string()),
  }
}

fn analyze_app_probe_into_run(
  _runtime: &Runtime,
  recording: &RecordingHandle,
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
  fs::write(&report_path, render_app_analysis_report(&analysis))
    .map_err(|error| format!("failed to write app analysis report {}: {error}", report_path.display()))?;
  stage_app_artifact(&recording, run, span, "analysis.output", &analysis_path, "analysis.json")?;
  stage_app_artifact(&recording, run, span, "analysis.report", &report_path, "analysis-report.md")?;
  Ok(AppAnalyzeOutput {
    analysis,
    analysis_path,
    report_path,
  })
}

#[cfg(test)]
mod tests;
