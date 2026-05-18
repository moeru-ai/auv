mod cli;

use std::env;
use std::path::PathBuf;
use std::process;

use auv_cli::app::{analyze_app_probe, distill_app_analysis, probe_app, validate_app_distillation};
use auv_cli::build_default_runtime;
use auv_cli::bundle::{
  SkillBundleCatalog, export_bundle, render_bundle_package_coverage, verify_bundle,
  verify_exported_bundle_package_standalone,
};
use auv_cli::model::RunStatus;
use auv_cli::skill::{
  SkillCaseMatrixCatalog, SkillCatalog, render_skill_case_matrix_report, run_skill,
  run_skill_case_matrix,
};
use cli::{CliCommand, help_text, parse_cli};

fn main() {
  if let Err(error) = run() {
    eprintln!("error: {error}");
    process::exit(1);
  }
}

fn run() -> Result<(), String> {
  let arguments = env::args().skip(1).collect::<Vec<_>>();
  let command = parse_cli(&arguments)?;
  let project_root =
    env::current_dir().map_err(|error| format!("failed to resolve current directory: {error}"))?;
  let runtime = build_default_runtime(project_root.clone())?;
  let runtime_version = env!("CARGO_PKG_VERSION").to_string();
  let skill_catalog = SkillCatalog::discover(&project_root)?;
  let bundle_catalog = SkillBundleCatalog::discover(&project_root)?;
  let case_matrix_catalog = SkillCaseMatrixCatalog::discover(&project_root)?;

  match command {
    CliCommand::Help => {
      print!("{}", help_text());
    }
    CliCommand::ListCommands => {
      for command in runtime.list_commands() {
        println!(
          "{} -> {}.{}",
          command.id, command.driver_id, command.operation
        );
        println!("  {}", command.summary);
        println!(
          "  disturbance: {} (max: {})",
          command
            .disturbance_classes
            .iter()
            .map(|class| class.as_str())
            .collect::<Vec<_>>()
            .join(", "),
          command.max_disturbance.as_str()
        );
      }
    }
    CliCommand::ListDrivers => {
      for driver in runtime.list_drivers() {
        println!("{}", driver.id);
        println!("  {}", driver.summary);
        println!("  capabilities: {}", driver.capabilities.join(", "));
        println!("  donor boundary: {}", driver.donor_boundary);
      }
    }
    CliCommand::AppProbe {
      bundle_id,
      output_dir,
    } => {
      let probe = probe_app(
        &project_root,
        &runtime,
        &bundle_id,
        output_dir.map(PathBuf::from),
      )?;
      println!("app: {}", probe.app.bundle_id);
      println!("status: captured");
      println!("probe: {}", probe.output_dir.join("probe.json").display());
      println!("steps: {}", probe.steps.len());
    }
    CliCommand::AppAnalyze { query } => {
      let output = analyze_app_probe(&PathBuf::from(query))?;
      println!("app: {}", output.analysis.app_identity.bundle_id);
      println!("status: analyzed");
      println!("analysis: {}", output.analysis_path.display());
      println!("report: {}", output.report_path.display());
    }
    CliCommand::AppDistill { query, output_dir } => {
      let output = distill_app_analysis(&PathBuf::from(query), output_dir.map(PathBuf::from))?;
      println!("app: {}", output.distillation.app_identity.bundle_id);
      println!("status: distilled");
      println!("distillation: {}", output.distillation_path.display());
      println!("report: {}", output.report_path.display());
      println!("candidates: {}", output.distillation.candidates.len());
    }
    CliCommand::AppValidate { query } => {
      let output = validate_app_distillation(&runtime, &PathBuf::from(query))?;
      println!("app: {}", output.validation.app_identity.bundle_id);
      println!("status: assessed");
      println!("validation: {}", output.validation_path.display());
      println!("report: {}", output.report_path.display());
      println!("candidates: {}", output.validation.candidates.len());
    }
    CliCommand::Invoke(request) => {
      let result = runtime.invoke(request)?;
      println!("runId: {}", result.run_id);
      println!("status: {}", result.status.as_str());
      println!("output: {}", result.output_summary);
      for artifact in &result.artifact_paths {
        println!("artifact: {}", artifact.display());
      }

      if let Some(failure) = &result.failure_message {
        return Err(format!(
          "{} (inspect with `auv-cli inspect {}`)",
          failure, result.run_id
        ));
      }

      if result.status == RunStatus::Failed {
        return Err(format!("run {} finished in failed state", result.run_id));
      }
    }
    CliCommand::Inspect { run_id } => {
      print!("{}", runtime.inspect(&run_id)?);
    }
    CliCommand::SkillList => {
      for entry in skill_catalog.entries() {
        println!("{}", entry.manifest.recipe_id);
        println!("  {}", entry.manifest.objective);
        println!(
          "  strategy: {}/{}/{} -> {}",
          entry.manifest.strategy.family,
          entry.manifest.strategy.grounding,
          entry.manifest.strategy.activation,
          entry.manifest.strategy.verification_contract
        );
        if let Ok(taxonomy_id) = entry.manifest.strategy.taxonomy_id() {
          println!("  strategy taxonomy: {}", taxonomy_id);
        }
        if !entry.manifest.status.is_empty() {
          println!("  status: {}", entry.manifest.status);
        }
        println!("  path: {}", entry.path.display());
      }
    }
    CliCommand::SkillShow { query } => {
      let entry = skill_catalog.resolve(&project_root, &query)?;
      let raw = std::fs::read_to_string(&entry.path).map_err(|error| {
        format!(
          "failed to read skill manifest {}: {error}",
          entry.path.display()
        )
      })?;
      let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse {}: {error}", entry.path.display()))?;
      println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|error| format!(
          "failed to render skill manifest {}: {error}",
          entry.path.display()
        ))?
      );
    }
    CliCommand::SkillBundleList => {
      for entry in bundle_catalog.entries() {
        println!("{}", entry.manifest.metadata.id);
        println!("  {}", entry.manifest.metadata.name);
        if !entry.manifest.metadata.status.is_empty() {
          println!("  status: {}", entry.manifest.metadata.status);
        }
        println!("  path: {}", entry.path.display());
      }
    }
    CliCommand::SkillBundleShow { query } => {
      let entry = bundle_catalog.resolve(&project_root, &query)?;
      let raw = std::fs::read_to_string(&entry.path).map_err(|error| {
        format!(
          "failed to read bundle manifest {}: {error}",
          entry.path.display()
        )
      })?;
      let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse {}: {error}", entry.path.display()))?;
      println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|error| format!(
          "failed to render bundle manifest {}: {error}",
          entry.path.display()
        ))?
      );
    }
    CliCommand::SkillBundleCoverage { query } => {
      let entry = bundle_catalog.resolve(&project_root, &query)?;
      print!(
        "{}",
        render_bundle_package_coverage(entry, &skill_catalog, &case_matrix_catalog, &project_root,)?
      );
    }
    CliCommand::SkillBundleVerify { query } => {
      let entry = bundle_catalog.resolve(&project_root, &query)?;
      verify_bundle(
        &project_root,
        &runtime_version,
        &skill_catalog,
        &case_matrix_catalog,
        entry,
      )?;
      println!("bundle: {}", entry.manifest.metadata.id);
      println!("status: verified");
      println!("path: {}", entry.path.display());
    }
    CliCommand::SkillBundleExport { query, output_dir } => {
      let entry = bundle_catalog.resolve(&project_root, &query)?;
      verify_bundle(
        &project_root,
        &runtime_version,
        &skill_catalog,
        &case_matrix_catalog,
        entry,
      )?;
      export_bundle(
        &project_root,
        &skill_catalog,
        &case_matrix_catalog,
        entry,
        PathBuf::from(output_dir),
      )?;
      println!("bundle: {}", entry.manifest.metadata.id);
      println!("status: exported");
    }
    CliCommand::SkillBundlePackageVerify { package_dir } => {
      let package_root = PathBuf::from(package_dir);
      let bundle_id = verify_exported_bundle_package_standalone(&package_root)?;
      println!("bundle: {}", bundle_id);
      println!("status: verified");
      println!("package: {}", package_root.display());
    }
    CliCommand::SkillCasesList => {
      for entry in case_matrix_catalog.entries() {
        println!("{}", entry.matrix.skill_id);
        if !entry.matrix.status.is_empty() {
          println!("  status: {}", entry.matrix.status);
        }
        println!("  cases: {}", entry.matrix.cases.len());
        println!("  path: {}", entry.path.display());
      }
    }
    CliCommand::SkillCasesShow { query } => {
      let entry = case_matrix_catalog.resolve(&project_root, &query)?;
      let raw = std::fs::read_to_string(&entry.path).map_err(|error| {
        format!(
          "failed to read case-matrix manifest {}: {error}",
          entry.path.display()
        )
      })?;
      let value: serde_json::Value = serde_json::from_str(&raw)
        .map_err(|error| format!("failed to parse {}: {error}", entry.path.display()))?;
      println!(
        "{}",
        serde_json::to_string_pretty(&value).map_err(|error| format!(
          "failed to render case-matrix manifest {}: {error}",
          entry.path.display()
        ))?
      );
    }
    CliCommand::SkillCasesReport { query } => {
      let matrix_entry = case_matrix_catalog.resolve(&project_root, &query)?;
      let skill_entry = skill_catalog.resolve_recipe_id(&matrix_entry.matrix.skill_id)?;
      print!(
        "{}",
        render_skill_case_matrix_report(skill_entry, matrix_entry)?
      );
    }
    CliCommand::SkillCasesRun {
      query,
      dry_run,
      max_disturbance,
      only_case_ids,
      include_nonvalidated,
    } => {
      let entry = case_matrix_catalog.resolve(&project_root, &query)?;
      run_skill_case_matrix(
        &runtime,
        &skill_catalog,
        entry,
        auv_cli::skill::SkillCaseRunOptions {
          dry_run,
          max_disturbance,
          only_case_ids,
          include_nonvalidated,
        },
      )?;
    }
    CliCommand::SkillRun {
      query,
      dry_run,
      max_disturbance,
      overrides,
    } => {
      let entry = skill_catalog.resolve(&project_root, &query)?;
      run_skill(
        &runtime,
        entry,
        auv_cli::skill::SkillRunOptions {
          dry_run,
          max_disturbance,
          overrides,
        },
      )?;
    }
  }

  Ok(())
}
