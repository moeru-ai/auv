use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::driver::{
  ObservedAxTreeSnapshot, ObservedDisplaySnapshot, ObservedRect, ObservedWindow, OcrTextSnapshot,
  parse_display_snapshot, parse_observed_ax_tree, parse_ocr_text_snapshot, parse_window_line,
  report_value, sanitized_artifact_name,
};
use crate::model::{AuvResult, ExecutionTarget, InvokeRequest, RunStatus, now_millis};
use crate::runtime::Runtime;
use crate::skill::SkillStrategy;

const APP_PROBE_VERSION: &str = "v0";
const APP_ANALYSIS_VERSION: &str = "v0";

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

  fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }
}
