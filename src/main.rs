mod cli;

use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process;
use std::process::Command;

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
  skill_catalog: &SkillCatalog,
  case_matrix_catalog: &SkillCaseMatrixCatalog,
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

  let package_root = output_dir.join(sanitized_bundle_package_name(&entry.manifest.metadata.id));
  if package_root.exists() {
    fs::remove_dir_all(&package_root).map_err(|error| {
      format!(
        "failed to clear existing bundle export package {}: {error}",
        package_root.display()
      )
    })?;
  }
  fs::create_dir_all(&package_root).map_err(|error| {
    format!(
      "failed to create bundle export package directory {}: {error}",
      package_root.display()
    )
  })?;

  let manifest_path = package_root.join("bundle.json");
  fs::copy(&entry.path, &manifest_path).map_err(|error| {
    format!(
      "failed to copy bundle manifest {} -> {}: {error}",
      entry.path.display(),
      manifest_path.display()
    )
  })?;

  let members_root = package_root.join("members");
  fs::create_dir_all(&members_root).map_err(|error| {
    format!(
      "failed to create bundle member package directory {}: {error}",
      members_root.display()
    )
  })?;

  let mut package_index = Vec::new();
  package_index.push(format!("bundleId={}", entry.manifest.metadata.id));
  package_index.push(format!("bundleName={}", entry.manifest.metadata.name));
  package_index.push(format!("sourceManifest={}", entry.path.display()));
  package_index.push("members=".to_string());
  let mut package_members = Vec::new();
  for member in &entry.manifest.members {
    let recipe_entry = skill_catalog.resolve_recipe_id(&member.recipe_id).map_err(|error| {
      format!(
        "failed to resolve recipe {} while exporting bundle: {error}",
        member.recipe_id
      )
    })?;
    let case_matrix_entry = case_matrix_catalog
      .resolve(project_root, &member.case_matrix_id)
      .map_err(|error| {
        format!(
          "failed to resolve case matrix {} while exporting bundle: {error}",
          member.case_matrix_id
        )
      })?;

    let member_dir = members_root.join(sanitized_bundle_package_name(&member.recipe_id));
    fs::create_dir_all(&member_dir).map_err(|error| {
      format!(
        "failed to create bundle member directory {}: {error}",
        member_dir.display()
      )
    })?;

    let recipe_path = member_dir.join("recipe.json");
    fs::copy(&recipe_entry.path, &recipe_path).map_err(|error| {
      format!(
        "failed to copy recipe {} -> {}: {error}",
        recipe_entry.path.display(),
        recipe_path.display()
      )
    })?;

    let case_matrix_path = member_dir.join("cases.json");
    fs::copy(&case_matrix_entry.path, &case_matrix_path).map_err(|error| {
      format!(
        "failed to copy case matrix {} -> {}: {error}",
        case_matrix_entry.path.display(),
        case_matrix_path.display()
      )
    })?;

    let evidence_path = member_dir.join("evidence.txt");
    fs::write(&evidence_path, render_bundle_member_evidence(member)).map_err(|error| {
      format!(
        "failed to write bundle member evidence {}: {error}",
        evidence_path.display()
      )
    })?;

    let summary_path = member_dir.join("summary.txt");
    fs::write(
      &summary_path,
      render_bundle_member_summary(member, &member_dir, &recipe_entry.path, &case_matrix_entry.path),
    )
    .map_err(|error| {
      format!(
        "failed to write bundle member summary {}: {error}",
        summary_path.display()
      )
    })?;

    let evidence_refs_dir = member_dir.join("evidence");
    fs::create_dir_all(&evidence_refs_dir).map_err(|error| {
      format!(
        "failed to create bundle evidence directory {}: {error}",
        evidence_refs_dir.display()
      )
    })?;
    for evidence_ref in &member.evidence_refs {
      let evidence_source = project_root.join(evidence_ref);
      if evidence_source.exists() {
        let destination = evidence_refs_dir.join(sanitized_bundle_package_name(evidence_ref));
        if evidence_source.is_dir() {
          copy_directory(&evidence_source, &destination)?;
        } else {
          fs::copy(&evidence_source, &destination).map_err(|error| {
            format!(
              "failed to copy evidence ref {} -> {}: {error}",
              evidence_source.display(),
              destination.display()
            )
          })?;
        }
      }
    }

    package_index.push(format!(
      "  - recipeId={} caseMatrixId={} role={} contract={} memberDir={}",
      member.recipe_id,
      member.case_matrix_id,
      member.role,
      member.contract,
      member_dir.display()
    ));

    package_members.push(render_bundle_package_member(
      member,
      &member_dir,
      &recipe_entry.path,
      &case_matrix_entry.path,
    ));
  }

  let index_path = package_root.join("index.txt");
  fs::write(&index_path, package_index.join("\n") + "\n").map_err(|error| {
    format!(
      "failed to write bundle export index {}: {error}",
      index_path.display()
    )
  })?;

  let readme_path = package_root.join("README.md");
  let mut readme = String::new();
  readme.push_str(&format!("# {}\n\n", entry.manifest.metadata.name));
  readme.push_str("This package is a self-contained export of the current bundle-shaped artifact.\n\n");
  readme.push_str("Contents:\n");
  readme.push_str("- `bundle.json`: canonical bundle manifest\n");
  readme.push_str("- `index.txt`: compact package index for downstream consumers\n\n");
  readme.push_str("- `members/<recipe-id>/recipe.json`: copied recipe manifest\n");
  readme.push_str("- `members/<recipe-id>/cases.json`: copied case matrix\n");
  readme.push_str("- `members/<recipe-id>/evidence.txt`: member evidence index\n");
  readme.push_str("- `members/<recipe-id>/summary.txt`: member summary\n\n");
  readme.push_str("Source manifest:\n");
  readme.push_str(&format!("- `{}`\n", entry.path.display()));
  readme.push_str("\nMembers:\n");
  for member in &package_members {
    readme.push_str("- ");
    readme.push_str(member);
    readme.push('\n');
  }
  fs::write(&readme_path, readme).map_err(|error| {
    format!(
      "failed to write bundle export readme {}: {error}",
      readme_path.display()
    )
  })?;

  let package_manifest_path = package_root.join("package.json");
  fs::write(
    &package_manifest_path,
    render_bundle_package_manifest(entry, project_root, &package_members),
  )
  .map_err(|error| {
    format!(
      "failed to write bundle export package manifest {}: {error}",
      package_manifest_path.display()
    )
  })?;

  Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
  fs::create_dir_all(destination).map_err(|error| {
    format!(
      "failed to create evidence directory {}: {error}",
      destination.display()
    )
  })?;

  for entry in fs::read_dir(source).map_err(|error| {
    format!(
      "failed to read evidence directory {}: {error}",
      source.display()
    )
  })? {
    let entry = entry.map_err(|error| format!("failed to enumerate evidence directory entry: {error}"))?;
    let path = entry.path();
    let destination_path = destination.join(entry.file_name());
    if path.is_dir() {
      copy_directory(&path, &destination_path)?;
    } else {
      fs::copy(&path, &destination_path).map_err(|error| {
        format!(
          "failed to copy evidence file {} -> {}: {error}",
          path.display(),
          destination_path.display()
        )
      })?;
    }
  }

  Ok(())
}

fn render_bundle_package_manifest(
  entry: &auv_cli::bundle::SkillBundleCatalogEntry,
  project_root: &Path,
  package_members: &[String],
) -> String {
  let members = entry
    .manifest
    .members
    .iter()
    .zip(package_members.iter())
    .map(|(member, member_path)| {
      serde_json::json!({
        "recipeId": member.recipe_id,
        "caseMatrixId": member.case_matrix_id,
        "role": member.role,
        "contract": member.contract,
        "appBundleId": member.app_bundle_id,
        "targetApplication": member.target_application,
        "validatedCaseIds": member.validated_case_ids,
        "candidateCaseIds": member.candidate_case_ids,
        "evidenceRefs": member.evidence_refs,
        "packageDir": member_path,
      })
    })
    .collect::<Vec<_>>();

  let value = serde_json::json!({
    "bundleId": entry.manifest.metadata.id,
    "bundleName": entry.manifest.metadata.name,
    "bundleVersion": entry.manifest.metadata.version,
    "bundleStatus": entry.manifest.metadata.status,
    "sourceManifest": entry.path.display().to_string(),
    "projectRoot": project_root.display().to_string(),
    "members": members,
    "verification": {
      "expectedSignals": entry.manifest.verification.expected_signals,
      "successCriteria": entry.manifest.verification.success_criteria,
      "nonGoals": entry.manifest.verification.non_goals,
    },
    "knownLimits": entry.manifest.known_limits,
  });

  serde_json::to_string_pretty(&value)
    .unwrap_or_else(|error| format!("{{\"error\":\"failed to render bundle package manifest: {error}\"}}\n"))
}

fn render_bundle_member_evidence(member: &auv_cli::bundle::SkillBundleMember) -> String {
  let mut lines = vec![
    format!("recipeId={}", member.recipe_id),
    format!("caseMatrixId={}", member.case_matrix_id),
    format!("role={}", member.role),
    format!("contract={}", member.contract),
  ];
  if !member.app_bundle_id.is_empty() {
    lines.push(format!("appBundleId={}", member.app_bundle_id));
  }
  if !member.target_application.is_empty() {
    lines.push(format!("targetApplication={}", member.target_application));
  }
  if !member.validated_case_ids.is_empty() {
    lines.push("validatedCaseIds=".to_string());
    for case_id in &member.validated_case_ids {
      lines.push(format!("  - {}", case_id));
    }
  }
  if !member.candidate_case_ids.is_empty() {
    lines.push("candidateCaseIds=".to_string());
    for case_id in &member.candidate_case_ids {
      lines.push(format!("  - {}", case_id));
    }
  }
  if !member.evidence_refs.is_empty() {
    lines.push("evidenceRefs=".to_string());
    for evidence in &member.evidence_refs {
      lines.push(format!("  - {}", evidence));
    }
  }
  lines.join("\n") + "\n"
}

fn render_bundle_member_summary(
  member: &auv_cli::bundle::SkillBundleMember,
  member_dir: &Path,
  recipe_path: &Path,
  case_matrix_path: &Path,
) -> String {
  let evidence = if member.evidence_refs.is_empty() {
    "none".to_string()
  } else {
    member.evidence_refs.join(", ")
  };
  format!(
    "`{}` -> `{}` ({})\n  - dir: `{}`\n  - recipe: `{}`\n  - case matrix: `{}`\n  - evidence refs: {}",
    member.recipe_id,
    member.case_matrix_id,
    member.contract,
    member_dir.display(),
    recipe_path.display(),
    case_matrix_path.display(),
    evidence
  )
}

fn render_bundle_package_member(
  member: &auv_cli::bundle::SkillBundleMember,
  member_dir: &Path,
  recipe_path: &Path,
  case_matrix_path: &Path,
) -> String {
  format!(
    "{} -> {} [{}] dir={} recipe={} cases={}",
    member.recipe_id,
    member.case_matrix_id,
    member.contract,
    member_dir.display(),
    recipe_path.display(),
    case_matrix_path.display()
  )
}

fn sanitized_bundle_package_name(raw: &str) -> String {
  let lowered = raw.trim().to_lowercase();
  let collapsed = lowered
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() {
        character
      } else {
        '-'
      }
    })
    .collect::<String>();
  let trimmed = collapsed
    .split('-')
    .filter(|segment| !segment.is_empty())
    .collect::<Vec<_>>()
    .join("-");
  if trimmed.is_empty() {
    "bundle-export".to_string()
  } else {
    trimmed
  }
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
  if entry.manifest.metadata.status.trim().is_empty() {
    return Err(format!(
      "bundle {} must have a non-empty metadata.status",
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
    if member.contract.trim().is_empty() {
      return Err(format!(
        "bundle {} member {} must declare contract",
        entry.manifest.metadata.id, member.recipe_id
      ));
    }
    skill_catalog.resolve_recipe_id(&member.recipe_id).map_err(|error| {
      format!(
        "bundle {} references unknown recipe {}: {error}",
        entry.manifest.metadata.id, member.recipe_id
      )
    })?;
    if !member.target_application.trim().is_empty() {
      let member_target_req =
        semver::VersionReq::parse(&member.target_application).map_err(|error| {
          format!(
            "bundle {} member {} has invalid targetApplication {}: {error}",
          entry.manifest.metadata.id, member.recipe_id, member.target_application
        )
      })?;
    let member_app_version =
      resolve_member_target_app_version(skill_catalog, &member.recipe_id, &member.app_bundle_id)?;
      if !member_target_req.matches(&member_app_version) {
        return Err(format!(
          "bundle {} member {} requires targetApplication {} but app version is {}",
          entry.manifest.metadata.id,
          member.recipe_id,
          member.target_application,
          member_app_version
        ));
      }
    }

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

fn resolve_member_target_app_version(
  skill_catalog: &SkillCatalog,
  recipe_id: &str,
  app_bundle_id: &str,
) -> Result<semver::Version, String> {
  if app_bundle_id.trim().is_empty() {
    return Err(format!(
      "bundle member for recipe {} does not declare appBundleId",
      recipe_id
    ));
  }

  skill_catalog.resolve_recipe_id(recipe_id)?;
  let app_path = resolve_installed_app_path(app_bundle_id)?;
  let version = read_app_version(&app_path)?;
  semver::Version::parse(&version).map_err(|error| {
    format!(
      "installed app version {} for {} is not parseable: {error}",
      version, app_bundle_id
    )
  })
}

fn resolve_installed_app_path(bundle_id: &str) -> Result<PathBuf, String> {
  let query = format!(r#"kMDItemCFBundleIdentifier == "{}""#, bundle_id);
  let output = Command::new("mdfind")
    .arg(query)
    .output()
    .map_err(|error| format!("failed to run mdfind for {bundle_id}: {error}"))?;
  if !output.status.success() {
    return Err(format!(
      "mdfind failed for {bundle_id}: {}",
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }

  let stdout = String::from_utf8_lossy(&output.stdout);
  let Some(path) = stdout.lines().map(str::trim).find(|line| !line.is_empty())
  else {
    return Err(format!("no installed app found for bundle id {bundle_id}"));
  };

  Ok(PathBuf::from(path))
}

fn read_app_version(app_path: &Path) -> Result<String, String> {
  let info_plist = app_path.join("Contents").join("Info.plist");
  if !info_plist.exists() {
    return Err(format!(
      "missing Info.plist for app bundle {}",
      app_path.display()
    ));
  }

  let short_version = read_plist_value(&info_plist, "CFBundleShortVersionString")
    .or_else(|_| read_plist_value(&info_plist, "CFBundleVersion"))?;
  normalize_semver_version(&short_version)
}

fn read_plist_value(info_plist: &Path, key: &str) -> Result<String, String> {
  let output = Command::new("/usr/libexec/PlistBuddy")
    .args(["-c", &format!("Print {key}"), &info_plist.display().to_string()])
    .output()
    .map_err(|error| format!("failed to read {key} from {}: {error}", info_plist.display()))?;
  if !output.status.success() {
    return Err(format!(
      "PlistBuddy failed reading {key} from {}: {}",
      info_plist.display(),
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }
  Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn normalize_semver_version(raw: &str) -> Result<String, String> {
  let trimmed = raw.trim();
  if trimmed.is_empty() {
    return Err("empty version string".to_string());
  }
  if semver::Version::parse(trimmed).is_ok() {
    return Ok(trimmed.to_string());
  }

  let parts = trimmed.split('.').collect::<Vec<_>>();
  if parts.is_empty() || parts.iter().any(|part| part.trim().is_empty()) {
    return Err(format!("invalid dotted version string {trimmed}"));
  }

  let mut normalized = parts
    .iter()
    .take(3)
    .map(|part| part.trim().to_string())
    .collect::<Vec<_>>();
  while normalized.len() < 3 {
    normalized.push("0".to_string());
  }
  let candidate = normalized.join(".");
  semver::Version::parse(&candidate)
    .map(|_| candidate)
    .map_err(|error| format!("version {trimmed} could not be normalized: {error}"))
}
