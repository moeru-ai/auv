// File: src/app/model.rs
use std::collections::BTreeMap;
use std::path::PathBuf;

use auv_driver_macos::types::ObservedRect;
use serde::{Deserialize, Serialize};

use crate::contract::ArtifactRef;
use crate::model::AuvResult;

pub(crate) const APP_PROBE_VERSION: &str = "v0";
pub(crate) const APP_ANALYSIS_VERSION: &str = "v0";
pub(crate) const APP_DISTILL_VERSION: &str = "v0";
pub(crate) const APP_VALIDATE_VERSION: &str = "v0";

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
  pub(crate) fn as_str(&self) -> &'static str {
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
  pub(crate) fn as_str(&self) -> &'static str {
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
  pub(crate) fn as_str(&self) -> &'static str {
    match self {
      Self::EvidenceOnly => "evidence-only",
      Self::MachineAsserted => "machine-asserted",
    }
  }

  pub(crate) fn manual_review_required(&self) -> bool {
    matches!(self, Self::EvidenceOnly)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AppCandidateGroundingTaxonomy {
  SearchEntryAxTextInputClipboardSubmitCaptureEvidence,
  NativeTextAxTextAxPerformActionClipboardPasteVerifyAxText,
  ResultSelectionOcrAnchorPointerClickCaptureEvidence,
  WindowActionWindowPointPointerClickCaptureEvidence,
}

pub(crate) const SEARCH_ENTRY_TAXONOMY_ID: &str =
  "search-entry.ax-text-input.clipboard-submit.capture-evidence";
pub(crate) const NATIVE_TEXT_CANONICAL_TAXONOMY_ID: &str =
  "native-text.ax-text.ax-perform-action-clipboard-paste.verify-ax-text";
pub(crate) const NATIVE_TEXT_LEGACY_TAXONOMY_ID: &str =
  "native-text.ax-text.pointer-focus-clipboard-paste.verify-ax-text";
pub(crate) const RESULT_SELECTION_TAXONOMY_ID: &str =
  "result-selection.ocr-anchor.pointer-click.capture-evidence";
pub(crate) const WINDOW_ACTION_TAXONOMY_ID: &str =
  "window-action.window-point.pointer-click.capture-evidence";

pub(crate) fn is_native_text_taxonomy_id(raw: &str) -> bool {
  matches!(
    raw.trim(),
    NATIVE_TEXT_CANONICAL_TAXONOMY_ID | NATIVE_TEXT_LEGACY_TAXONOMY_ID
  )
}

pub(crate) fn canonicalize_app_candidate_grounding_taxonomy_id(raw: &str) -> &str {
  if is_native_text_taxonomy_id(raw) {
    NATIVE_TEXT_CANONICAL_TAXONOMY_ID
  } else {
    raw.trim()
  }
}

impl AppCandidateGroundingTaxonomy {
  pub(crate) fn parse(raw: &str) -> AuvResult<Self> {
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

  pub(crate) fn allowed_ids() -> &'static [&'static str] {
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
  pub(crate) fn from_observed(rect: &ObservedRect) -> Self {
    Self {
      x: rect.x,
      y: rect.y,
      width: rect.width,
      height: rect.height,
    }
  }

  pub(crate) fn center(&self) -> (i64, i64) {
    (self.x + self.width / 2, self.y + self.height / 2)
  }

  pub(crate) fn center_point(&self) -> AppPoint {
    let (x, y) = self.center();
    AppPoint { x, y }
  }

  pub(crate) fn render_compact(&self) -> String {
    format!("{},{},{},{}", self.x, self.y, self.width, self.height)
  }

  pub(crate) fn relative_point(&self, point: &AppPoint) -> Option<(f64, f64)> {
    if self.width <= 0 || self.height <= 0 {
      return None;
    }
    let relative_x = (point.x - self.x) as f64 / self.width as f64;
    let relative_y = (point.y - self.y) as f64 / self.height as f64;
    Some((relative_x, relative_y))
  }
}
