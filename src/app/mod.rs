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
mod model;
mod recipe;
mod report;
#[cfg(test)]
mod tests;

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

pub(crate) use model::{
  APP_ANALYSIS_VERSION, APP_DISTILL_VERSION, APP_PROBE_VERSION, APP_VALIDATE_VERSION,
  AppCandidateGroundingTaxonomy, NATIVE_TEXT_CANONICAL_TAXONOMY_ID, RESULT_SELECTION_TAXONOMY_ID,
  SEARCH_ENTRY_TAXONOMY_ID, WINDOW_ACTION_TAXONOMY_ID,
  canonicalize_app_candidate_grounding_taxonomy_id, is_native_text_taxonomy_id,
};
pub use model::{
  AppAnalysis, AppAnalyzeOutput, AppAvailableSurfaces, AppCandidateCompatibility,
  AppCandidatePromotionGate, AppCandidatePromotionStatus, AppControlAssessment, AppDistillOutput,
  AppDistillation, AppDistilledCandidate, AppDistilledCandidateShape, AppDisturbanceProfile,
  AppGroundingAssessment, AppIdentity, AppPermissionState, AppPoint, AppProbe, AppProbeArtifact,
  AppProbeStep, AppRecommendedStrategy, AppRect, AppSurfaceCandidate, AppValidateOutput,
  AppValidatedCandidate, AppValidation, AppValidationStatus, AppVerificationAssessment,
  AppVerificationMode, AppWindowContext, AssessmentStatus,
};

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::model::{AuvResult, now_millis};
use crate::run_builder::{RecordingRun, RunFinish, RunSpec, SpanFinish, SpanRef};
use crate::runtime::Runtime;
use crate::skill::{
  SkillCaseMatrix, SkillCaseRunOptions, SkillManifest, run_skill_case_matrix_into_run,
  validate_case_matrix_against_skill, validate_case_matrix_manifest, validate_skill_manifest,
};
use crate::store::sanitized_artifact_name;
use crate::trace::{RunType, TraceStatusCode, string_attr};

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
