// File: src/bundle/export.rs
//! Bundle export (package builder).
//!
//! Exports a `bundles/*.json` manifest into a self-contained package directory
//! containing member recipes, case matrices, coverage reports, and referenced
//! evidence artifacts.
//!
//! Boundary: export is packaging + validation over existing manifests/evidence;
//! it does not execute automation.

use std::fs;
use std::path::{Path, PathBuf};

use crate::skill::{
  SkillCaseMatrix, SkillCaseMatrixCatalog, SkillCaseMatrixEntry, SkillCatalog, SkillCatalogEntry,
  SkillManifest, render_skill_case_matrix_report, validate_case_matrix_against_skill,
  validate_case_matrix_manifest, validate_skill_manifest,
};

use super::model::{ExportedBundlePackageManifest, SkillBundleCatalogEntry, SkillBundleManifest};
use super::paths::{
  bundle_member_cases_relative_path, bundle_member_coverage_relative_path,
  bundle_member_evidence_relative_dir, bundle_member_evidence_relative_path,
  bundle_member_recipe_relative_path, bundle_member_relative_dir,
  bundle_member_summary_relative_path, sanitized_bundle_package_name,
};
use super::render::{
  render_bundle_index_member_line, render_bundle_member_evidence, render_bundle_member_summary,
  render_bundle_package_coverage, render_bundle_package_manifest, render_bundle_package_member,
  render_bundle_standalone_coverage,
};

pub fn export_bundle(
  project_root: &Path,
  skill_catalog: &SkillCatalog,
  case_matrix_catalog: &SkillCaseMatrixCatalog,
  entry: &SkillBundleCatalogEntry,
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
  fs::create_dir_all(&output_dir).map_err(|error| {
    format!(
      "failed to create bundle export directory {}: {error}",
      output_dir.display()
    )
  })?;

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

  let mut package_index = vec![
    format!("bundleId={}", entry.manifest.metadata.id),
    format!("bundleName={}", entry.manifest.metadata.name),
    format!("sourceManifest={}", entry.path.display()),
    "members=".to_string(),
  ];
  let mut package_members = Vec::new();
  let mut package_member_readme_entries = Vec::new();
  for member in &entry.manifest.members {
    let recipe_entry = skill_catalog
      .resolve_recipe_id(&member.recipe_id)
      .map_err(|error| {
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

    let member_relative_dir = bundle_member_relative_dir(&member.recipe_id);
    let member_dir = package_root.join(&member_relative_dir);
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
      render_bundle_member_summary(
        member,
        &recipe_entry.manifest.strategy,
        &member_relative_dir,
        &recipe_entry.path,
        &case_matrix_entry.path,
      ),
    )
    .map_err(|error| {
      format!(
        "failed to write bundle member summary {}: {error}",
        summary_path.display()
      )
    })?;

    let coverage_path = member_dir.join("coverage.md");
    fs::write(
      &coverage_path,
      render_skill_case_matrix_report(recipe_entry, case_matrix_entry)?,
    )
    .map_err(|error| {
      format!(
        "failed to write bundle member coverage report {}: {error}",
        coverage_path.display()
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
      if !evidence_source.exists() {
        return Err(format!(
          "bundle {} member {} references missing evidence ref {} at {}",
          entry.manifest.metadata.id,
          member.recipe_id,
          evidence_ref,
          evidence_source.display()
        ));
      }
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

    package_index.push(render_bundle_index_member_line(member));
    package_members.push((
      member_relative_dir.clone(),
      recipe_entry.manifest.strategy.clone(),
    ));
    package_member_readme_entries.push(render_bundle_package_member(
      member,
      &recipe_entry.manifest.strategy,
      &member_relative_dir,
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

  let coverage_path = package_root.join("coverage.md");
  fs::write(
    &coverage_path,
    render_bundle_package_coverage(entry, skill_catalog, case_matrix_catalog, project_root)?,
  )
  .map_err(|error| {
    format!(
      "failed to write bundle export coverage report {}: {error}",
      coverage_path.display()
    )
  })?;

  let readme_path = package_root.join("README.md");
  let mut readme = String::new();
  readme.push_str(&format!("# {}\n\n", entry.manifest.metadata.name));
  readme
    .push_str("This package is a self-contained export of the current bundle-shaped artifact.\n\n");
  readme.push_str("Contents:\n");
  readme.push_str("- `bundle.json`: canonical bundle manifest\n");
  readme.push_str("- `index.txt`: compact package index for downstream consumers\n\n");
  readme.push_str("- `coverage.md`: bundle-level coverage and boundary summary\n\n");
  readme.push_str("- `members/<recipe-id>/recipe.json`: copied recipe manifest\n");
  readme.push_str("- `members/<recipe-id>/cases.json`: copied case matrix\n");
  readme.push_str("- `members/<recipe-id>/evidence.txt`: member evidence index\n");
  readme.push_str("- `members/<recipe-id>/summary.txt`: member summary\n\n");
  readme.push_str("- `members/<recipe-id>/coverage.md`: member coverage report\n\n");
  readme.push_str("Source manifest:\n");
  readme.push_str(&format!("- `{}`\n", entry.path.display()));
  readme.push_str("\nMembers:\n");
  for member in &package_member_readme_entries {
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

  verify_exported_bundle_package(
    project_root,
    skill_catalog,
    case_matrix_catalog,
    entry,
    &package_root,
  )?;

  Ok(())
}

pub fn verify_exported_bundle_package_standalone(package_root: &Path) -> Result<String, String> {
  let bundle_manifest_path = package_root.join("bundle.json");
  let package_manifest_path = package_root.join("package.json");
  let index_path = package_root.join("index.txt");
  let coverage_path = package_root.join("coverage.md");
  let readme_path = package_root.join("README.md");
  let members_root = package_root.join("members");

  for required in [
    &bundle_manifest_path,
    &package_manifest_path,
    &index_path,
    &coverage_path,
    &readme_path,
    &members_root,
  ] {
    if !required.exists() {
      return Err(format!(
        "exported bundle package is missing {}",
        required.display()
      ));
    }
  }

  let bundle_manifest: SkillBundleManifest =
    serde_json::from_value(read_json_file(&bundle_manifest_path)?).map_err(|error| {
      format!(
        "failed to parse exported bundle manifest {}: {error}",
        bundle_manifest_path.display()
      )
    })?;
  let package_manifest: ExportedBundlePackageManifest =
    serde_json::from_value(read_json_file(&package_manifest_path)?).map_err(|error| {
      format!(
        "failed to parse exported package manifest {}: {error}",
        package_manifest_path.display()
      )
    })?;

  if package_manifest.bundle_id != bundle_manifest.metadata.id
    || package_manifest.bundle_name != bundle_manifest.metadata.name
    || package_manifest.bundle_version != bundle_manifest.metadata.version
    || package_manifest.bundle_status != bundle_manifest.metadata.status
    || package_manifest.coverage_report != "coverage.md"
    || package_manifest.versions.auv != bundle_manifest.versions.auv
    || package_manifest.versions.target_application != bundle_manifest.versions.target_application
    || package_manifest.verification.expected_signals
      != bundle_manifest.verification.expected_signals
    || package_manifest.verification.success_criteria
      != bundle_manifest.verification.success_criteria
    || package_manifest.verification.non_goals != bundle_manifest.verification.non_goals
    || package_manifest.known_limits != bundle_manifest.known_limits
  {
    return Err(format!(
      "exported package manifest {} does not match bundle manifest {}",
      package_manifest_path.display(),
      bundle_manifest_path.display()
    ));
  }

  if package_manifest.members.len() != bundle_manifest.members.len() {
    return Err(format!(
      "exported package member count {} does not match bundle member count {}",
      package_manifest.members.len(),
      bundle_manifest.members.len()
    ));
  }

  for member in &bundle_manifest.members {
    let Some(package_member) = package_manifest
      .members
      .iter()
      .find(|candidate| candidate.recipe_id == member.recipe_id)
    else {
      return Err(format!(
        "exported package is missing member {}",
        member.recipe_id
      ));
    };

    if package_member.case_matrix_id != member.case_matrix_id
      || package_member.role != member.role
      || package_member.contract != member.contract
      || package_member.app_bundle_id != member.app_bundle_id
      || package_member.target_application != member.target_application
      || package_member.coverage_summary != member.coverage_summary
      || package_member.validated_case_ids != member.validated_case_ids
      || package_member.candidate_case_ids != member.candidate_case_ids
      || package_member.evidence_refs != member.evidence_refs
    {
      return Err(format!(
        "exported package metadata for member {} does not match bundle manifest",
        member.recipe_id
      ));
    }

    let expected_package_dir = bundle_member_relative_dir(&member.recipe_id);
    if package_member.package_dir != expected_package_dir {
      return Err(format!(
        "exported package member {} declares packageDir {} but expected {}",
        member.recipe_id, package_member.package_dir, expected_package_dir
      ));
    }
    if package_member.coverage_report != bundle_member_coverage_relative_path(&member.recipe_id) {
      return Err(format!(
        "exported package member {} declares coverageReport {} but expected {}",
        member.recipe_id,
        package_member.coverage_report,
        bundle_member_coverage_relative_path(&member.recipe_id)
      ));
    }

    let member_dir = package_root.join(&expected_package_dir);
    let recipe_export_path =
      package_root.join(bundle_member_recipe_relative_path(&member.recipe_id));
    let cases_export_path = package_root.join(bundle_member_cases_relative_path(&member.recipe_id));
    let evidence_export_path =
      package_root.join(bundle_member_evidence_relative_path(&member.recipe_id));
    let summary_export_path =
      package_root.join(bundle_member_summary_relative_path(&member.recipe_id));
    let coverage_export_path =
      package_root.join(bundle_member_coverage_relative_path(&member.recipe_id));
    let evidence_dir = package_root.join(bundle_member_evidence_relative_dir(&member.recipe_id));

    for required in [
      &member_dir,
      &recipe_export_path,
      &cases_export_path,
      &evidence_export_path,
      &summary_export_path,
      &coverage_export_path,
      &evidence_dir,
    ] {
      if !required.exists() {
        return Err(format!(
          "exported package member {} is missing {}",
          member.recipe_id,
          required.display()
        ));
      }
    }

    let recipe_manifest: SkillManifest =
      serde_json::from_value(read_json_file(&recipe_export_path)?).map_err(|error| {
        format!(
          "failed to parse exported recipe {}: {error}",
          recipe_export_path.display()
        )
      })?;
    let expected_taxonomy_id = recipe_manifest.strategy.taxonomy_id().map_err(|error| {
      format!(
        "exported recipe {} has invalid strategy taxonomy: {error}",
        recipe_export_path.display()
      )
    })?;
    if package_member.taxonomy_id != expected_taxonomy_id {
      return Err(format!(
        "exported package member {} declares taxonomyId {} but exported recipe strategy resolves to {}",
        member.recipe_id, package_member.taxonomy_id, expected_taxonomy_id
      ));
    }
    if package_member.strategy != recipe_manifest.strategy {
      return Err(format!(
        "exported package member {} strategy does not match exported recipe strategy",
        member.recipe_id
      ));
    }
    validate_skill_manifest(&recipe_manifest).map_err(|error| {
      format!(
        "exported recipe {} failed manifest validation: {error}",
        recipe_export_path.display()
      )
    })?;
    if recipe_manifest.recipe_id != member.recipe_id {
      return Err(format!(
        "exported recipe {} declares recipe_id {} but bundle member expects {}",
        recipe_export_path.display(),
        recipe_manifest.recipe_id,
        member.recipe_id
      ));
    }

    let case_matrix: SkillCaseMatrix = serde_json::from_value(read_json_file(&cases_export_path)?)
      .map_err(|error| {
        format!(
          "failed to parse exported case matrix {}: {error}",
          cases_export_path.display()
        )
      })?;
    validate_case_matrix_manifest(&case_matrix).map_err(|error| {
      format!(
        "exported case matrix {} failed validation: {error}",
        cases_export_path.display()
      )
    })?;
    validate_case_matrix_against_skill(&recipe_manifest, &case_matrix).map_err(|error| {
      format!(
        "exported case matrix {} does not match recipe {}: {error}",
        cases_export_path.display(),
        recipe_export_path.display()
      )
    })?;
    if case_matrix.skill_id != member.case_matrix_id {
      return Err(format!(
        "exported case matrix {} declares skill_id {} but bundle member expects {}",
        cases_export_path.display(),
        case_matrix.skill_id,
        member.case_matrix_id
      ));
    }

    for validated_case_id in &member.validated_case_ids {
      let Some(case) = case_matrix
        .cases
        .iter()
        .find(|case| case.case_id == *validated_case_id)
      else {
        return Err(format!(
          "exported package references missing validated case {} in {}",
          validated_case_id,
          cases_export_path.display()
        ));
      };
      if case.status != "validated" {
        return Err(format!(
          "exported package validated case {} in {} has status {}",
          validated_case_id,
          cases_export_path.display(),
          case.status
        ));
      }
    }

    for candidate_case_id in &member.candidate_case_ids {
      if !case_matrix
        .cases
        .iter()
        .any(|case| case.case_id == *candidate_case_id)
      {
        return Err(format!(
          "exported package references missing candidate case {} in {}",
          candidate_case_id,
          cases_export_path.display()
        ));
      }
    }

    let evidence_text = fs::read_to_string(&evidence_export_path).map_err(|error| {
      format!(
        "failed to read exported evidence index {}: {error}",
        evidence_export_path.display()
      )
    })?;
    if evidence_text != render_bundle_member_evidence(member) {
      return Err(format!(
        "exported evidence index {} does not match bundle member {}",
        evidence_export_path.display(),
        member.recipe_id
      ));
    }

    let coverage_text = fs::read_to_string(&coverage_export_path).map_err(|error| {
      format!(
        "failed to read exported member coverage {}: {error}",
        coverage_export_path.display()
      )
    })?;
    let expected_coverage = render_skill_case_matrix_report(
      &SkillCatalogEntry {
        manifest: recipe_manifest.clone(),
        path: recipe_export_path.clone(),
      },
      &SkillCaseMatrixEntry {
        matrix: case_matrix.clone(),
        path: cases_export_path.clone(),
      },
    )?;
    if coverage_text != expected_coverage {
      return Err(format!(
        "exported member coverage {} does not match bundle member {}",
        coverage_export_path.display(),
        member.recipe_id
      ));
    }

    for evidence_ref in &member.evidence_refs {
      let exported = evidence_dir.join(sanitized_bundle_package_name(evidence_ref));
      if !exported.exists() {
        return Err(format!(
          "exported evidence {} for member {} is missing",
          exported.display(),
          member.recipe_id
        ));
      }
    }
  }

  let index_text = fs::read_to_string(&index_path).map_err(|error| {
    format!(
      "failed to read exported index {}: {error}",
      index_path.display()
    )
  })?;
  let mut expected_index_lines = vec![
    format!("bundleId={}", bundle_manifest.metadata.id),
    format!("bundleName={}", bundle_manifest.metadata.name),
    format!("sourceManifest={}", package_manifest.source_manifest),
    "members=".to_string(),
  ];
  for member in &bundle_manifest.members {
    expected_index_lines.push(render_bundle_index_member_line(member));
  }
  let expected_index = expected_index_lines.join("\n") + "\n";
  if index_text != expected_index {
    return Err(format!(
      "exported index {} does not match expected bundle index contract",
      index_path.display()
    ));
  }

  let coverage_text = fs::read_to_string(&coverage_path).map_err(|error| {
    format!(
      "failed to read exported bundle coverage {}: {error}",
      coverage_path.display()
    )
  })?;
  let expected_coverage = render_bundle_standalone_coverage(&bundle_manifest, &package_manifest);
  if coverage_text != expected_coverage {
    return Err(format!(
      "exported bundle coverage {} does not match expected bundle coverage contract",
      coverage_path.display()
    ));
  }

  Ok(bundle_manifest.metadata.id)
}

pub(crate) fn copy_directory(source: &Path, destination: &Path) -> Result<(), String> {
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
    let entry =
      entry.map_err(|error| format!("failed to enumerate evidence directory entry: {error}"))?;
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

pub(crate) fn read_json_file(path: &Path) -> Result<serde_json::Value, String> {
  let raw = fs::read_to_string(path)
    .map_err(|error| format!("failed to read JSON file {}: {error}", path.display()))?;
  serde_json::from_str(&raw)
    .map_err(|error| format!("failed to parse JSON file {}: {error}", path.display()))
}

fn verify_exported_bundle_package(
  project_root: &Path,
  skill_catalog: &SkillCatalog,
  case_matrix_catalog: &SkillCaseMatrixCatalog,
  entry: &SkillBundleCatalogEntry,
  package_root: &Path,
) -> Result<(), String> {
  let bundle_manifest_path = package_root.join("bundle.json");
  let package_manifest_path = package_root.join("package.json");
  let index_path = package_root.join("index.txt");
  let coverage_path = package_root.join("coverage.md");
  let readme_path = package_root.join("README.md");
  let members_root = package_root.join("members");

  for required in [
    &bundle_manifest_path,
    &package_manifest_path,
    &index_path,
    &coverage_path,
    &readme_path,
    &members_root,
  ] {
    if !required.exists() {
      return Err(format!(
        "exported bundle package is missing {}",
        required.display()
      ));
    }
  }

  let source_bundle_value = read_json_file(&entry.path)?;
  let exported_bundle_value = read_json_file(&bundle_manifest_path)?;
  if source_bundle_value != exported_bundle_value {
    return Err(format!(
      "exported bundle manifest {} does not match source {}",
      bundle_manifest_path.display(),
      entry.path.display()
    ));
  }

  let package_manifest: ExportedBundlePackageManifest =
    serde_json::from_value(read_json_file(&package_manifest_path)?).map_err(|error| {
      format!(
        "failed to parse exported package manifest {}: {error}",
        package_manifest_path.display()
      )
    })?;

  if package_manifest.bundle_id != entry.manifest.metadata.id {
    return Err(format!(
      "exported package bundleId {} does not match source {}",
      package_manifest.bundle_id, entry.manifest.metadata.id
    ));
  }
  if package_manifest.bundle_name != entry.manifest.metadata.name {
    return Err(format!(
      "exported package bundleName {} does not match source {}",
      package_manifest.bundle_name, entry.manifest.metadata.name
    ));
  }
  if package_manifest.bundle_version != entry.manifest.metadata.version {
    return Err(format!(
      "exported package bundleVersion {} does not match source {}",
      package_manifest.bundle_version, entry.manifest.metadata.version
    ));
  }
  if package_manifest.bundle_status != entry.manifest.metadata.status {
    return Err(format!(
      "exported package bundleStatus {} does not match source {}",
      package_manifest.bundle_status, entry.manifest.metadata.status
    ));
  }
  if package_manifest.source_manifest != entry.path.display().to_string() {
    return Err(format!(
      "exported package sourceManifest {} does not match source {}",
      package_manifest.source_manifest,
      entry.path.display()
    ));
  }
  if package_manifest.project_root != project_root.display().to_string() {
    return Err(format!(
      "exported package projectRoot {} does not match {}",
      package_manifest.project_root,
      project_root.display()
    ));
  }
  if package_manifest.coverage_report != "coverage.md" {
    return Err(format!(
      "exported package coverageReport {} does not match expected coverage.md",
      package_manifest.coverage_report
    ));
  }
  if package_manifest.versions.auv != entry.manifest.versions.auv
    || package_manifest.versions.target_application != entry.manifest.versions.target_application
  {
    return Err("exported package versions do not match source bundle manifest".to_string());
  }
  if package_manifest.verification.expected_signals != entry.manifest.verification.expected_signals
    || package_manifest.verification.success_criteria
      != entry.manifest.verification.success_criteria
    || package_manifest.verification.non_goals != entry.manifest.verification.non_goals
  {
    return Err(
      "exported package verification block does not match source bundle manifest".to_string(),
    );
  }
  if package_manifest.known_limits != entry.manifest.known_limits {
    return Err("exported package knownLimits do not match source bundle manifest".to_string());
  }
  if package_manifest.members.len() != entry.manifest.members.len() {
    return Err(format!(
      "exported package member count {} does not match source {}",
      package_manifest.members.len(),
      entry.manifest.members.len()
    ));
  }

  for member in &entry.manifest.members {
    let Some(package_member) = package_manifest
      .members
      .iter()
      .find(|candidate| candidate.recipe_id == member.recipe_id)
    else {
      return Err(format!(
        "exported package is missing member {}",
        member.recipe_id
      ));
    };

    if package_member.case_matrix_id != member.case_matrix_id
      || package_member.role != member.role
      || package_member.contract != member.contract
      || package_member.app_bundle_id != member.app_bundle_id
      || package_member.target_application != member.target_application
      || package_member.coverage_summary != member.coverage_summary
      || package_member.validated_case_ids != member.validated_case_ids
      || package_member.candidate_case_ids != member.candidate_case_ids
      || package_member.evidence_refs != member.evidence_refs
    {
      return Err(format!(
        "exported package metadata for member {} does not match source bundle manifest",
        member.recipe_id
      ));
    }

    let expected_package_dir = bundle_member_relative_dir(&member.recipe_id);
    if package_member.package_dir != expected_package_dir {
      return Err(format!(
        "exported package member {} declares packageDir {} but expected {}",
        member.recipe_id, package_member.package_dir, expected_package_dir
      ));
    }
    if package_member.coverage_report != bundle_member_coverage_relative_path(&member.recipe_id) {
      return Err(format!(
        "exported package member {} declares coverageReport {} but expected {}",
        member.recipe_id,
        package_member.coverage_report,
        bundle_member_coverage_relative_path(&member.recipe_id)
      ));
    }

    let member_dir = package_root.join(&expected_package_dir);
    let recipe_entry = skill_catalog.resolve_recipe_id(&member.recipe_id)?;
    let expected_taxonomy_id = recipe_entry
      .manifest
      .strategy
      .taxonomy_id()
      .map_err(|error| {
        format!(
          "source recipe {} has invalid strategy taxonomy: {error}",
          recipe_entry.path.display()
        )
      })?;
    if package_member.taxonomy_id != expected_taxonomy_id {
      return Err(format!(
        "exported package member {} declares taxonomyId {} but source recipe strategy resolves to {}",
        member.recipe_id, package_member.taxonomy_id, expected_taxonomy_id
      ));
    }
    if package_member.strategy != recipe_entry.manifest.strategy {
      return Err(format!(
        "exported package member {} strategy does not match source recipe strategy",
        member.recipe_id
      ));
    }
    let case_matrix_entry = case_matrix_catalog.resolve(project_root, &member.case_matrix_id)?;
    let recipe_export_path =
      package_root.join(bundle_member_recipe_relative_path(&member.recipe_id));
    let cases_export_path = package_root.join(bundle_member_cases_relative_path(&member.recipe_id));
    let evidence_export_path =
      package_root.join(bundle_member_evidence_relative_path(&member.recipe_id));
    let summary_export_path =
      package_root.join(bundle_member_summary_relative_path(&member.recipe_id));
    let coverage_export_path =
      package_root.join(bundle_member_coverage_relative_path(&member.recipe_id));
    let evidence_dir = package_root.join(bundle_member_evidence_relative_dir(&member.recipe_id));

    for required in [
      &member_dir,
      &recipe_export_path,
      &cases_export_path,
      &evidence_export_path,
      &summary_export_path,
      &coverage_export_path,
      &evidence_dir,
    ] {
      if !required.exists() {
        return Err(format!(
          "exported package member {} is missing {}",
          member.recipe_id,
          required.display()
        ));
      }
    }

    if read_json_file(&recipe_export_path)? != read_json_file(&recipe_entry.path)? {
      return Err(format!(
        "exported recipe {} does not match source {}",
        recipe_export_path.display(),
        recipe_entry.path.display()
      ));
    }
    if read_json_file(&cases_export_path)? != read_json_file(&case_matrix_entry.path)? {
      return Err(format!(
        "exported case matrix {} does not match source {}",
        cases_export_path.display(),
        case_matrix_entry.path.display()
      ));
    }

    let evidence_text = fs::read_to_string(&evidence_export_path).map_err(|error| {
      format!(
        "failed to read exported evidence index {}: {error}",
        evidence_export_path.display()
      )
    })?;
    if evidence_text != render_bundle_member_evidence(member) {
      return Err(format!(
        "exported evidence index {} does not match source member {}",
        evidence_export_path.display(),
        member.recipe_id
      ));
    }

    let summary_text = fs::read_to_string(&summary_export_path).map_err(|error| {
      format!(
        "failed to read exported member summary {}: {error}",
        summary_export_path.display()
      )
    })?;
    let expected_summary = render_bundle_member_summary(
      member,
      &recipe_entry.manifest.strategy,
      &expected_package_dir,
      &recipe_entry.path,
      &case_matrix_entry.path,
    );
    if summary_text != expected_summary {
      return Err(format!(
        "exported member summary {} does not match source member {}",
        summary_export_path.display(),
        member.recipe_id
      ));
    }

    let coverage_text = fs::read_to_string(&coverage_export_path).map_err(|error| {
      format!(
        "failed to read exported member coverage {}: {error}",
        coverage_export_path.display()
      )
    })?;
    let expected_coverage = render_skill_case_matrix_report(recipe_entry, case_matrix_entry)?;
    if coverage_text != expected_coverage {
      return Err(format!(
        "exported member coverage {} does not match source member {}",
        coverage_export_path.display(),
        member.recipe_id
      ));
    }

    for evidence_ref in &member.evidence_refs {
      let exported = evidence_dir.join(sanitized_bundle_package_name(evidence_ref));
      if !exported.exists() {
        return Err(format!(
          "exported evidence {} for member {} is missing",
          exported.display(),
          member.recipe_id
        ));
      }
    }
  }

  let index_text = fs::read_to_string(&index_path).map_err(|error| {
    format!(
      "failed to read exported index {}: {error}",
      index_path.display()
    )
  })?;
  let mut expected_index_lines = vec![
    format!("bundleId={}", entry.manifest.metadata.id),
    format!("bundleName={}", entry.manifest.metadata.name),
    format!("sourceManifest={}", entry.path.display()),
    "members=".to_string(),
  ];
  for member in &entry.manifest.members {
    expected_index_lines.push(render_bundle_index_member_line(member));
  }
  let expected_index = expected_index_lines.join("\n") + "\n";
  if index_text != expected_index {
    return Err(format!(
      "exported index {} does not match expected bundle index contract",
      index_path.display()
    ));
  }

  let coverage_text = fs::read_to_string(&coverage_path).map_err(|error| {
    format!(
      "failed to read exported bundle coverage {}: {error}",
      coverage_path.display()
    )
  })?;
  let expected_coverage =
    render_bundle_package_coverage(entry, skill_catalog, case_matrix_catalog, project_root)?;
  if coverage_text != expected_coverage {
    return Err(format!(
      "exported bundle coverage {} does not match source bundle coverage contract",
      coverage_path.display()
    ));
  }

  Ok(())
}
