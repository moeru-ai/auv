mod cli;

use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process;

use auv_cli::build_default_runtime;
use auv_cli::bundle::SkillBundleCatalog;
use auv_cli::model::RunStatus;
use auv_cli::skill::{SkillCaseMatrixCatalog, SkillCatalog, run_skill, run_skill_case_matrix};
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
      export_bundle(&project_root, entry, PathBuf::from(output_dir))?;
      println!("bundle: {}", entry.manifest.metadata.id);
      println!("status: exported");
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

fn export_bundle(
  project_root: &Path,
  entry: &auv_cli::bundle::SkillBundleCatalogEntry,
  output_dir: PathBuf,
) -> Result<(), String> {
  if output_dir.as_os_str().is_empty() {
    return Err("output_dir must not be empty".to_string());
  }
  let output_dir = if output_dir.is_absolute() {
    output_dir
  } else {
    project_root.join(output_dir)
  };
  fs::create_dir_all(&output_dir)
    .map_err(|error| format!("failed to create bundle export directory {}: {error}", output_dir.display()))?;

  let manifest_path = output_dir.join("bundle.json");
  fs::copy(&entry.path, &manifest_path).map_err(|error| {
    format!(
      "failed to copy bundle manifest {} -> {}: {error}",
      entry.path.display(),
      manifest_path.display()
    )
  })?;

  let readme_path = output_dir.join("README.md");
  fs::write(
    &readme_path,
    format!(
      "# {}\n\nExported from {}\n",
      entry.manifest.metadata.name,
      entry.path.display()
    ),
  )
  .map_err(|error| format!("failed to write bundle export readme {}: {error}", readme_path.display()))?;

  Ok(())
}

fn verify_bundle(
  project_root: &Path,
  runtime_version: &str,
  skill_catalog: &SkillCatalog,
  case_matrix_catalog: &SkillCaseMatrixCatalog,
  entry: &auv_cli::bundle::SkillBundleCatalogEntry,
) -> Result<(), String> {
  if entry.manifest.kind != "SkillBundle" {
    return Err(format!(
      "bundle {} has unsupported kind {}",
      entry.manifest.metadata.id, entry.manifest.kind
    ));
  }
  if entry.manifest.api_version != "auv.ai/v1alpha1" {
    return Err(format!(
      "bundle {} has unsupported apiVersion {}",
      entry.manifest.metadata.id, entry.manifest.api_version
    ));
  }
  if entry.manifest.metadata.id.trim().is_empty() {
    return Err("bundle metadata.id must not be empty".to_string());
  }
  if entry.manifest.metadata.name.trim().is_empty() {
    return Err(format!(
      "bundle {} must have a non-empty metadata.name",
      entry.manifest.metadata.id
    ));
  }
  if entry.manifest.metadata.version.trim().is_empty() {
    return Err(format!(
      "bundle {} must have a non-empty metadata.version",
      entry.manifest.metadata.id
    ));
  }
  if entry.manifest.target.application_family.trim().is_empty() {
    return Err(format!(
      "bundle {} must declare a target.applicationFamily",
      entry.manifest.metadata.id
    ));
  }
  if entry.manifest.target.platform.trim().is_empty() {
    return Err(format!(
      "bundle {} must declare a target.platform",
      entry.manifest.metadata.id
    ));
  }
  if entry.manifest.versions.auv.trim().is_empty() {
    return Err(format!(
      "bundle {} must declare versions.auv",
      entry.manifest.metadata.id
    ));
  }
  if entry.manifest.members.is_empty() {
    return Err(format!("bundle {} has no members", entry.manifest.metadata.id));
  }
  if entry.manifest.verification.expected_signals.is_empty() {
    return Err(format!(
      "bundle {} must declare expectedSignals",
      entry.manifest.metadata.id
    ));
  }
  if entry.manifest.verification.success_criteria.is_empty() {
    return Err(format!(
      "bundle {} must declare successCriteria",
      entry.manifest.metadata.id
    ));
  }

  let runtime_req = semver::VersionReq::parse(&entry.manifest.versions.auv).map_err(|error| {
    format!(
      "bundle {} has invalid versions.auv {}: {error}",
      entry.manifest.metadata.id, entry.manifest.versions.auv
    )
  })?;
  let runtime_version = semver::Version::parse(runtime_version).map_err(|error| {
    format!("current runtime version {} is not parseable: {error}", runtime_version)
  })?;
  if !runtime_req.matches(&runtime_version) {
    return Err(format!(
      "bundle {} requires runtime {} but current runtime is {}",
      entry.manifest.metadata.id, entry.manifest.versions.auv, runtime_version
    ));
  }

  let mut seen = std::collections::BTreeSet::new();
  for member in &entry.manifest.members {
    if member.recipe_id.trim().is_empty() {
      return Err(format!(
        "bundle {} has a member with empty recipeId",
        entry.manifest.metadata.id
      ));
    }
    if !seen.insert(member.recipe_id.clone()) {
      return Err(format!(
        "bundle {} contains duplicate member recipeId {}",
        entry.manifest.metadata.id, member.recipe_id
      ));
    }
    if member.case_matrix_id.trim().is_empty() {
      return Err(format!(
        "bundle {} has a member with empty caseMatrixId",
        entry.manifest.metadata.id
      ));
    }
    skill_catalog.resolve_recipe_id(&member.recipe_id).map_err(|error| {
      format!(
        "bundle {} references unknown recipe {}: {error}",
        entry.manifest.metadata.id, member.recipe_id
      )
    })?;

    let case_matrix_entry = case_matrix_catalog
      .resolve(project_root, &member.case_matrix_id)
      .map_err(|error| {
        format!(
          "bundle {} references unknown case matrix {}: {error}",
          entry.manifest.metadata.id, member.case_matrix_id
        )
      })?;
    if case_matrix_entry.matrix.skill_id != member.recipe_id {
      return Err(format!(
        "bundle {} member recipeId {} does not match caseMatrixId {} skillId {}",
        entry.manifest.metadata.id,
        member.recipe_id,
        member.case_matrix_id,
        case_matrix_entry.matrix.skill_id
      ));
    }

    for validated_case_id in &member.validated_case_ids {
      let Some(case) = case_matrix_entry
        .matrix
        .cases
        .iter()
        .find(|case| case.case_id == *validated_case_id)
      else {
        return Err(format!(
          "bundle {} references missing validated case {} in matrix {}",
          entry.manifest.metadata.id, validated_case_id, member.case_matrix_id
        ));
      };
      if case.status != "validated" {
        return Err(format!(
          "bundle {} validated case {} in matrix {} has status {}",
          entry.manifest.metadata.id, validated_case_id, member.case_matrix_id, case.status
        ));
      }
    }

    for candidate_case_id in &member.candidate_case_ids {
      if !case_matrix_entry
        .matrix
        .cases
        .iter()
        .any(|case| case.case_id == *candidate_case_id)
      {
        return Err(format!(
          "bundle {} references missing candidate case {} in matrix {}",
          entry.manifest.metadata.id, candidate_case_id, member.case_matrix_id
        ));
      }
    }
  }

  Ok(())
}
