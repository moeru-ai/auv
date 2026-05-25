// File: src/skill/case_matrix.rs
use std::collections::BTreeMap;

use crate::catalog::default_command_catalog;
use crate::model::AuvResult;
use crate::runtime::Runtime;
use crate::trace::{RunId, TraceStatusCode, string_attr};

use super::validate::validate_case_matrix_against_skill_with_commands;
use super::{
  SkillCase, SkillCaseMatrix, SkillCaseMatrixEntry, SkillCaseRunOptions, SkillCatalog,
  SkillCatalogEntry, SkillManifest, SkillRunOptions, finish_failed_recorded_run,
  run_skill_manifest_into_run, span_record,
};

pub fn run_skill_case_matrix(
  runtime: &Runtime,
  skill_catalog: &SkillCatalog,
  matrix_entry: &SkillCaseMatrixEntry,
  options: SkillCaseRunOptions,
) -> AuvResult<()> {
  let skill_entry = skill_catalog.resolve_recipe_id(&matrix_entry.matrix.skill_id)?;
  run_skill_case_matrix_inline(
    runtime,
    &skill_entry.manifest,
    &matrix_entry.matrix,
    options,
  )
}

pub(crate) fn run_skill_case_matrix_inline(
  runtime: &Runtime,
  manifest: &SkillManifest,
  matrix: &SkillCaseMatrix,
  options: SkillCaseRunOptions,
) -> AuvResult<()> {
  run_skill_case_matrix_recorded(runtime, manifest, matrix, options).map(|_| ())
}

pub(crate) fn run_skill_case_matrix_recorded(
  runtime: &Runtime,
  manifest: &SkillManifest,
  matrix: &SkillCaseMatrix,
  options: SkillCaseRunOptions,
) -> AuvResult<RunId> {
  let mut attributes = crate::run_builder::Attributes::new();
  attributes.insert(
    "auv.case_matrix.skill_id".to_string(),
    string_attr(matrix.skill_id.clone()),
  );
  let mut run = runtime.start_run(
    crate::run_builder::RunSpec::new(crate::trace::RunType::Validate, "auv.validate")
      .with_attributes(attributes),
  )?;
  let root = run.root_span();

  match run_skill_case_matrix_into_run(runtime, &mut run, &root, manifest, matrix, options) {
    Ok(selected_case_count) => runtime.finish_run(
      run,
      crate::run_builder::RunFinish {
        status_code: TraceStatusCode::Ok,
        summary: Some(format!(
          "Validated {} selected case(s) for {}",
          selected_case_count, matrix.skill_id
        )),
        failure: None,
      },
    ),
    Err(error) => finish_failed_recorded_run(
      runtime,
      run,
      error,
      format!("Case matrix {} failed", matrix.skill_id),
    ),
  }
}

pub(crate) fn run_skill_case_matrix_into_run(
  runtime: &Runtime,
  run: &mut crate::run_builder::RecordingRun,
  parent: &crate::run_builder::SpanRef,
  manifest: &SkillManifest,
  matrix: &SkillCaseMatrix,
  options: SkillCaseRunOptions,
) -> AuvResult<usize> {
  validate_case_matrix_against_skill_with_commands(manifest, matrix, runtime.list_commands())?;
  let cases = select_cases(matrix, &options)?;
  let selected_case_count = cases.len();

  println!("case-matrix: {}", matrix.skill_id);
  println!("version: {}", matrix.version);
  if !matrix.status.is_empty() {
    println!("status: {}", matrix.status);
  }
  println!("selected cases: {}", cases.len());

  let mut failures = Vec::new();
  for case in cases {
    println!("case: {} [{}]", case.case_id, case.status);
    let case_span = run.start_span(
      parent,
      span_record(
        "auv.case",
        BTreeMap::from([("auv.case.id".to_string(), string_attr(&case.case_id))]),
      ),
    )?;
    let execute_span = run.start_span(
      &case_span,
      span_record(
        "auv.execute",
        BTreeMap::from([(
          "auv.recipe.id".to_string(),
          string_attr(manifest.recipe_id.clone()),
        )]),
      ),
    )?;

    let case_result = run_skill_manifest_into_run(
      runtime,
      run,
      &execute_span,
      manifest,
      SkillRunOptions {
        dry_run: options.dry_run,
        max_disturbance: options.max_disturbance,
        overrides: case.inputs.clone(),
        quiet: false,
      },
    );

    match case_result {
      Ok(_summary) => {
        run.finish_span(
          &execute_span,
          crate::run_builder::SpanFinish {
            status_code: TraceStatusCode::Ok,
            summary: Some(format!("Executed skill {}", manifest.recipe_id)),
            failure: None,
          },
        )?;
        run.finish_span(
          &case_span,
          crate::run_builder::SpanFinish {
            status_code: TraceStatusCode::Ok,
            summary: Some(format!("Case {} passed", case.case_id)),
            failure: None,
          },
        )?;
        println!("case-result: ok");
      }
      Err(error) => {
        let finish_error =
          finish_case_spans_after_error(run, &execute_span, &case_span, manifest, case, &error);
        println!("case-result: failed");
        println!("case-error: {error}");
        failures.push((
          case.case_id.clone(),
          finish_error
            .map(|_| error.clone())
            .unwrap_or_else(|finish_error| format!("{error}; {finish_error}")),
        ));
      }
    }
  }

  if !failures.is_empty() {
    let summary = failures
      .iter()
      .map(|(case_id, error)| format!("- {case_id}: {error}"))
      .collect::<Vec<_>>()
      .join("\n");
    return Err(format!(
      "{} of {} selected case(s) failed:\n{}",
      failures.len(),
      selected_case_count,
      summary
    ));
  }

  Ok(selected_case_count)
}

pub fn render_skill_case_matrix_report(
  skill_entry: &SkillCatalogEntry,
  matrix_entry: &SkillCaseMatrixEntry,
) -> AuvResult<String> {
  let command_catalog = default_command_catalog();
  validate_case_matrix_against_skill_with_commands(
    &skill_entry.manifest,
    &matrix_entry.matrix,
    command_catalog.all(),
  )?;

  let manifest = &skill_entry.manifest;
  let matrix = &matrix_entry.matrix;

  let mut by_status = BTreeMap::<String, usize>::new();
  let mut by_disturbance = BTreeMap::<String, usize>::new();
  for case in &matrix.cases {
    *by_status.entry(case.status.clone()).or_insert(0) += 1;
    *by_disturbance.entry(case.disturbance.clone()).or_insert(0) += 1;
  }

  let target_app_display = if manifest.target_app.name.trim().is_empty() {
    manifest.target_app.bundle_id.clone()
  } else if manifest.target_app.bundle_id.trim().is_empty() {
    manifest.target_app.name.clone()
  } else {
    format!(
      "{} ({})",
      manifest.target_app.name.trim(),
      manifest.target_app.bundle_id.trim()
    )
  };

  let mut output = String::new();
  output.push_str(&format!("# Skill Case Report: {}\n\n", matrix.skill_id));
  output.push_str(&format!("- skill version: `{}`\n", manifest.version));
  output.push_str(&format!("- matrix version: `{}`\n", matrix.version));
  output.push_str(&format!("- matrix status: `{}`\n", matrix.status));
  output.push_str(&format!("- target app: `{}`\n", target_app_display));
  output.push_str(&format!(
    "- strategy family: `{}`\n",
    manifest.strategy.family
  ));
  output.push_str(&format!(
    "- strategy grounding: `{}`\n",
    manifest.strategy.grounding
  ));
  output.push_str(&format!(
    "- strategy activation: `{}`\n",
    manifest.strategy.activation
  ));
  output.push_str(&format!(
    "- strategy verification contract: `{}`\n",
    manifest.strategy.verification_contract
  ));
  if let Ok(taxonomy_id) = manifest.strategy.taxonomy_id() {
    output.push_str(&format!("- strategy taxonomy: `{}`\n", taxonomy_id));
  }
  output.push_str(&format!("- objective: {}\n", manifest.objective.trim()));
  output.push_str(&format!(
    "- max disturbance: `{}`\n",
    if manifest.disturbance_policy.max_disturbance.is_empty() {
      "pointer"
    } else {
      &manifest.disturbance_policy.max_disturbance
    }
  ));
  output.push_str(&format!("- case count: `{}`\n\n", matrix.cases.len()));

  output.push_str("## Status Counts\n\n");
  for (status, count) in &by_status {
    output.push_str(&format!("- `{}`: `{}`\n", status, count));
  }
  output.push_str("\n## Disturbance Counts\n\n");
  for (disturbance, count) in &by_disturbance {
    output.push_str(&format!("- `{}`: `{}`\n", disturbance, count));
  }

  output.push_str("\n## Cases\n\n");
  for case in &matrix.cases {
    output.push_str(&format!("### {} [{}]\n\n", case.case_id, case.status));
    output.push_str(&format!("- disturbance: `{}`\n", case.disturbance));
    if !case.inputs.is_empty() {
      output.push_str("- inputs:\n");
      for (key, value) in &case.inputs {
        output.push_str(&format!("  - `{}` = `{}`\n", key, value));
      }
    }
    if !case.notes.is_empty() {
      output.push_str("- notes:\n");
      for note in &case.notes {
        output.push_str(&format!("  - {}\n", note));
      }
    }
    if let (Some(requested_title), Some(target_title)) = (
      case.inputs.get("requested_title"),
      case.inputs.get("target_title"),
    ) && !requested_title.trim().is_empty()
      && requested_title != target_title
    {
      output.push_str("- verification gap:\n");
      output.push_str(&format!("  - requested_title = `{}`\n", requested_title));
      output.push_str(&format!("  - verified_target = `{}`\n", target_title));
      output.push_str(
        "  - this case validates the current activation path, not semantic target-title selection.\n",
      );
    }
    output.push('\n');
  }

  output.push_str("## Verification Contract\n\n");
  output.push_str("- expected signals:\n");
  for signal in &manifest.verification.expected_signals {
    output.push_str(&format!("  - {}\n", signal));
  }
  output.push_str("- success criteria:\n");
  for criterion in &manifest.verification.success_criteria {
    output.push_str(&format!("  - {}\n", criterion));
  }
  output.push_str("- non-goals:\n");
  for non_goal in &manifest.verification.non_goals {
    output.push_str(&format!("  - {}\n", non_goal));
  }

  Ok(output)
}

fn finish_case_spans_after_error(
  run: &mut crate::run_builder::RecordingRun,
  execute_span: &crate::run_builder::SpanRef,
  case_span: &crate::run_builder::SpanRef,
  manifest: &SkillManifest,
  case: &SkillCase,
  error: &str,
) -> AuvResult<()> {
  run.finish_span(
    execute_span,
    crate::run_builder::SpanFinish {
      status_code: TraceStatusCode::Error,
      summary: Some(format!("Skill {} failed", manifest.recipe_id)),
      failure: Some(error.to_string()),
    },
  )?;
  run.finish_span(
    case_span,
    crate::run_builder::SpanFinish {
      status_code: TraceStatusCode::Error,
      summary: Some(format!("Case {} failed", case.case_id)),
      failure: Some(error.to_string()),
    },
  )
}

fn select_cases<'a>(
  matrix: &'a SkillCaseMatrix,
  options: &SkillCaseRunOptions,
) -> AuvResult<Vec<&'a SkillCase>> {
  let selected = if options.only_case_ids.is_empty() {
    matrix
      .cases
      .iter()
      .filter(|case| options.include_nonvalidated || case.status == "validated")
      .collect::<Vec<_>>()
  } else {
    matrix
      .cases
      .iter()
      .filter(|case| {
        options
          .only_case_ids
          .iter()
          .any(|wanted| wanted == &case.case_id)
      })
      .collect::<Vec<_>>()
  };

  if selected.is_empty() {
    let reason = if options.only_case_ids.is_empty() {
      "no matching validated cases found"
    } else {
      "no matching cases found for requested case ids"
    };
    return Err(format!("{reason} in matrix {}", matrix.skill_id));
  }

  Ok(selected)
}
