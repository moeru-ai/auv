//! Core human-readable run inspection (library-only).
//!
//! NOTICE(inspect-composition / S4): Donor/product sections live in
//! `auv-product`. This module only emits core sections (run summary,
//! verifications, observations, detector lineage, view-parser proof, scene
//! state). Product CLI/MCP must inject a product composer; do not re-add
//! donor wiring here.

use auv_inspect_model::InspectComposer;
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_view::memory::{ViewMemory, ViewParserInspect, format_view_resolution_summary_text};

use crate::contract::{FailureLayer, ObservationSnapshot, ObservationSource, VerificationMethod, VerificationResult};
use crate::model::AuvResult;
use crate::run_read::DetectorRecognitionLineage;
use crate::{scene_state_read, view_parser_read};

mod sections;

pub use sections::{CorePrefixSection, CoreSuffixSection, build_core_inspect_composer};

pub fn read_run(store: &LocalStore, run_id: &str) -> AuvResult<CanonicalRun> {
  crate::run_read::read_run(store, run_id)
}

pub fn list_verifications(store: &LocalStore, run_id: &str) -> AuvResult<Vec<VerificationResult>> {
  crate::run_read::list_verifications(store, run_id)
}

pub fn list_observation_snapshots(store: &LocalStore, run_id: &str) -> AuvResult<Vec<ObservationSnapshot>> {
  crate::run_read::list_observation_snapshots(store, run_id)
}

pub fn list_detector_recognition_lineage(store: &LocalStore, run_id: &str) -> AuvResult<Vec<DetectorRecognitionLineage>> {
  crate::run_read::list_detector_recognition_lineage(store, run_id)
}

pub fn list_view_memory_writes(store: &LocalStore, run_id: &str) -> AuvResult<Vec<ViewMemory>> {
  view_parser_read::list_view_memory_writes(store, run_id)
}

pub fn view_parser_inspect(store: &LocalStore, run_id: &str) -> AuvResult<ViewParserInspect> {
  let run = read_run(store, run_id)?;
  view_parser_read::build_view_parser_inspect(store, &run)
}

/// Core-only inspect text via the shared composer path.
pub fn inspect_run(store: &LocalStore, run_id: &str) -> AuvResult<String> {
  let composer = build_core_inspect_composer().map_err(|error| error.to_string())?;
  inspect_run_with(&composer, store, run_id)
}

pub fn inspect_run_with(composer: &InspectComposer, store: &LocalStore, run_id: &str) -> Result<String, String> {
  composer.inspect_text(store, run_id).map_err(|error| error.to_string())
}

pub(crate) fn inspect_run_core_prefix_body(store: &LocalStore, run_id: &str) -> AuvResult<String> {
  let canonical = read_run(store, run_id)?;
  let verifications = list_verifications(store, run_id)?;
  let observation_snapshots = list_observation_snapshots(store, run_id)?;
  let detector_recognition_lineage = list_detector_recognition_lineage(store, run_id)?;
  Ok(render_core_run_text(&canonical, &verifications, &observation_snapshots, &detector_recognition_lineage))
}

pub(crate) fn inspect_run_core_suffix_body(store: &LocalStore, run_id: &str) -> AuvResult<String> {
  let canonical = read_run(store, run_id)?;
  let mut output = String::new();
  append_view_parser_proof_text_from_run(store, &canonical, &mut output)?;
  append_scene_state_text_from_run(store, &canonical, &mut output)?;
  Ok(output)
}

/// Full core body (prefix + suffix). Kept for focused tests / callers that need
/// the historical contiguous core text without product donors.
#[allow(dead_code)]
pub(crate) fn inspect_run_core_body(store: &LocalStore, run_id: &str) -> AuvResult<String> {
  let mut output = inspect_run_core_prefix_body(store, run_id)?;
  output.push_str(&inspect_run_core_suffix_body(store, run_id)?);
  Ok(output)
}

fn append_view_parser_proof_text_from_run(store: &LocalStore, run: &CanonicalRun, output: &mut String) -> AuvResult<()> {
  let view_parser = view_parser_read::build_view_parser_inspect(store, run)?;
  if view_parser.resolution_summaries.is_empty() {
    return Ok(());
  }
  output.push_str("\nView parser proof:\n");
  for summary in &view_parser.resolution_summaries {
    output.push_str(&format_view_resolution_summary_text(summary));
  }
  Ok(())
}

fn append_scene_state_text_from_run(store: &LocalStore, run: &CanonicalRun, output: &mut String) -> AuvResult<()> {
  let outcome = scene_state_read::build_scene_state_inspect_for_run(store, run).map_err(|error| error.to_string())?;
  output.push_str(&scene_state_read::format_scene_state_read_text(&outcome));
  Ok(())
}

fn render_core_run_text(
  run: &CanonicalRun,
  verifications: &[VerificationResult],
  observation_snapshots: &[ObservationSnapshot],
  detector_recognition_lineage: &[DetectorRecognitionLineage],
) -> String {
  let mut output = format!(
    "Run {}\nType: {}\nStatus: {}\nState: {}\n",
    run.run.run_id,
    run.run.run_type.as_str(),
    run.run.status_code.as_str(),
    run.run.state.as_str()
  );
  if let Some(summary) = &run.run.summary {
    output.push_str(&format!("Summary: {summary}\n"));
  }
  if let Some(failure) = &run.run.failure {
    output.push_str(&format!("Failure: {}\n", failure.message));
  }

  output.push_str(&format!("\nSpans: {}\n", run.spans.len()));
  for span in run.spans.iter().take(20) {
    output.push_str(&format!(
      "- {} name={} parent={} status={}\n",
      span.span_id,
      span.name,
      span.parent_span_id.as_ref().map(|span_id| span_id.as_str()).unwrap_or("n/a"),
      span.status_code.as_str()
    ));
  }
  if run.spans.len() > 20 {
    output.push_str(&format!("- … {} more\n", run.spans.len() - 20));
  }

  output.push_str(&format!("\nEvents: {}\n", run.events.len()));
  for event in run.events.iter().take(20) {
    let message = event.message.as_deref().unwrap_or("");
    output.push_str(&format!("- {} span={} name={} {}\n", event.event_id, event.span_id, event.name, message));
  }
  if run.events.len() > 20 {
    output.push_str(&format!("- … {} more\n", run.events.len() - 20));
  }

  output.push_str(&format!("\nArtifacts: {}\n", run.artifacts.len()));
  for artifact in run.artifacts.iter().take(20) {
    output.push_str(&format!("- {} span={} role={} path={}\n", artifact.artifact_id, artifact.span_id, artifact.role, artifact.path));
  }
  if run.artifacts.len() > 20 {
    output.push_str(&format!("- … {} more\n", run.artifacts.len() - 20));
  }

  let command_boundary_claims = run.events.iter().filter(|event| event.name == "command.verification").collect::<Vec<_>>();
  let command_known_limits = run.events.iter().filter(|event| event.name == "command.known_limit").collect::<Vec<_>>();
  output.push_str("\nCommand Boundary Claims:\n");
  if command_boundary_claims.is_empty() && command_known_limits.is_empty() {
    output.push_str("- none\n");
  } else {
    for event in command_boundary_claims {
      output.push_str(&format!("- verification={} span={}\n", event.message.as_deref().unwrap_or("n/a"), event.span_id));
    }
    for event in command_known_limits {
      output.push_str(&format!("- known_limit={} span={}\n", event.message.as_deref().unwrap_or("n/a"), event.span_id));
    }
  }

  output.push_str("\nVerifications:\n");
  if verifications.is_empty() {
    output.push_str("- none\n");
  } else {
    for verification in verifications {
      output.push_str(&format!(
        "- method={} executed={} state_changed={} semantic_matched={} failure_layer={} evidence={} observed_label={}\n",
        render_verification_method(&verification.method),
        verification.executed,
        verification.state_changed,
        render_optional_bool(verification.semantic_matched),
        render_failure_layer(verification.failure_layer),
        verification.evidence.len(),
        verification.observed_label.as_deref().unwrap_or("n/a")
      ));
    }
  }

  output.push_str("\nObservations:\n");
  if observation_snapshots.is_empty() {
    output.push_str("- none\n");
  } else {
    for snapshot in observation_snapshots {
      output.push_str(&format!(
        "- {} span={} source={} nodes={} evidence={} limits={}\n",
        snapshot.snapshot_id,
        snapshot.span_id,
        render_observation_source(snapshot.source),
        snapshot.nodes.len(),
        snapshot.evidence.len(),
        snapshot.known_limits.len()
      ));
    }
  }

  output.push_str("\nDetector Recognition Lineage:\n");
  if detector_recognition_lineage.is_empty() {
    output.push_str("- none\n");
  } else {
    for lineage in detector_recognition_lineage {
      output.push_str(&format!(
        "- artifact={} status={} source={} model={} backend={} items={}/{} best={} projection={} capture={} limits={}\n",
        lineage.artifact.artifact_id,
        render_detector_status(&lineage.status),
        lineage.source.map(render_recognition_source).unwrap_or("n/a"),
        lineage.model_id.as_deref().unwrap_or("n/a"),
        lineage.backend.as_deref().unwrap_or("n/a"),
        lineage.filtered_count.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
        lineage.all_count.map(|value| value.to_string()).unwrap_or_else(|| "n/a".to_string()),
        lineage.best_item_id.as_deref().unwrap_or("n/a"),
        lineage.runtime_projection_kind.as_deref().unwrap_or("n/a"),
        lineage.capture_artifact.as_ref().and_then(|artifact| artifact.path.as_deref()).unwrap_or("n/a"),
        lineage.known_limits.len()
      ));
      output.push_str(&format!(
        "  evidence={} class_label_source={} provider={} issue={}\n",
        lineage.evidence_artifacts.len(),
        lineage.class_label_source_kind.as_deref().unwrap_or("n/a"),
        lineage.execution_provider.as_deref().unwrap_or("n/a"),
        lineage.issue.as_deref().unwrap_or("n/a")
      ));
      if !lineage.known_limits.is_empty() {
        output.push_str(&format!("  known_limits={}\n", lineage.known_limits.join(" | ")));
      }
    }
  }

  output
}

fn render_detector_status(status: &crate::run_read::DetectorRecognitionLineageStatus) -> &'static str {
  match status {
    crate::run_read::DetectorRecognitionLineageStatus::Ready => "ready",
    crate::run_read::DetectorRecognitionLineageStatus::MissingCaptureArtifact => "missing_capture_artifact",
    crate::run_read::DetectorRecognitionLineageStatus::MissingEvidence => "missing_evidence",
    crate::run_read::DetectorRecognitionLineageStatus::CaptureArtifactUnresolved => "capture_artifact_unresolved",
    crate::run_read::DetectorRecognitionLineageStatus::Malformed => "malformed",
  }
}

fn render_optional_bool(value: Option<bool>) -> &'static str {
  match value {
    Some(true) => "true",
    Some(false) => "false",
    None => "n/a",
  }
}

fn render_failure_layer(layer: Option<FailureLayer>) -> &'static str {
  match layer {
    Some(FailureLayer::GroundingFailed) => "grounding_failed",
    Some(FailureLayer::CandidateExpired) => "candidate_expired",
    Some(FailureLayer::ControlFailed) => "control_failed",
    Some(FailureLayer::VerificationUnreliable) => "verification_unreliable",
    Some(FailureLayer::StateChangedNoMatch) => "state_changed_no_match",
    Some(FailureLayer::SemanticMismatch) => "semantic_mismatch",
    None => "n/a",
  }
}

fn render_verification_method(method: &VerificationMethod) -> String {
  match method {
    VerificationMethod::TextVisible => "text_visible".to_string(),
    VerificationMethod::AxText => "ax_text".to_string(),
    VerificationMethod::StateChanged => "state_changed".to_string(),
    VerificationMethod::CandidateAlive => "candidate_alive".to_string(),
    VerificationMethod::SemanticMatch => "semantic_match".to_string(),
    VerificationMethod::NoProgressBoundary => "no_progress_boundary".to_string(),
    VerificationMethod::Custom { name } => format!("custom:{name}"),
  }
}

fn render_observation_source(source: ObservationSource) -> &'static str {
  match source {
    ObservationSource::Ax => "ax",
    ObservationSource::Ocr => "ocr",
    ObservationSource::Visual => "visual",
    ObservationSource::Merged => "merged",
  }
}

fn render_recognition_source(source: crate::contract::RecognitionSource) -> &'static str {
  match source {
    crate::contract::RecognitionSource::OcrText => "ocr_text",
    crate::contract::RecognitionSource::OcrRow => "ocr_row",
    crate::contract::RecognitionSource::VisualRow => "visual_row",
    crate::contract::RecognitionSource::SegmentedRegion => "segmented_region",
    crate::contract::RecognitionSource::IconMatch => "icon_match",
    crate::contract::RecognitionSource::Custom => "custom",
  }
}
