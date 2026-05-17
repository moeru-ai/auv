use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::model::AuvResult;
use crate::skill::{
  SkillCaseMatrix, SkillCaseMatrixCatalog, SkillCaseMatrixEntry, SkillCatalog, SkillCatalogEntry,
  SkillManifest, render_skill_case_matrix_report, validate_case_matrix_against_skill,
  validate_case_matrix_manifest, validate_skill_manifest,
};

#[derive(Clone, Debug, Deserialize)]
pub struct SkillBundleManifest {
  #[serde(rename = "apiVersion")]
  pub api_version: String,
  pub kind: String,
  pub metadata: SkillBundleMetadata,
  #[serde(default)]
  pub target: SkillBundleTarget,
  #[serde(default)]
  pub versions: SkillBundleVersions,
  #[serde(default)]
  pub members: Vec<SkillBundleMember>,
  #[serde(default)]
  pub verification: SkillBundleVerification,
  #[serde(default, rename = "knownLimits")]
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillBundleMetadata {
  #[serde(default)]
  pub id: String,
  #[serde(default)]
  pub name: String,
  #[serde(default)]
  pub version: String,
  #[serde(default)]
  pub status: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillBundleTarget {
  #[serde(default, rename = "applicationFamily")]
  pub application_family: String,
  #[serde(default)]
  pub platform: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillBundleVersions {
  #[serde(default)]
  pub auv: String,
  #[serde(default, rename = "targetApplication")]
  pub target_application: String,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillBundleMember {
  #[serde(default, rename = "recipeId")]
  pub recipe_id: String,
  #[serde(default, rename = "caseMatrixId")]
  pub case_matrix_id: String,
  #[serde(default)]
  pub role: String,
  #[serde(default, rename = "validatedCaseIds")]
  pub validated_case_ids: Vec<String>,
  #[serde(default, rename = "candidateCaseIds")]
  pub candidate_case_ids: Vec<String>,
  #[serde(default)]
  pub contract: String,
  #[serde(default, rename = "evidenceRefs")]
  pub evidence_refs: Vec<String>,
  #[serde(default, rename = "appBundleId")]
  pub app_bundle_id: String,
  #[serde(default, rename = "targetApplication")]
  pub target_application: String,
  #[serde(default, rename = "coverageSummary")]
  pub coverage_summary: SkillBundleMemberCoverageSummary,
}

#[derive(Clone, Debug, Deserialize, Default, PartialEq, Eq)]
pub struct SkillBundleMemberCoverageSummary {
  #[serde(default, rename = "activationStatus")]
  pub activation_status: String,
  #[serde(default, rename = "semanticSelectionStatus")]
  pub semantic_selection_status: String,
  #[serde(default, rename = "validatedClaims")]
  pub validated_claims: Vec<String>,
  #[serde(default, rename = "boundaryClaims")]
  pub boundary_claims: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
pub struct SkillBundleVerification {
  #[serde(default, rename = "expectedSignals")]
  pub expected_signals: Vec<String>,
  #[serde(default, rename = "successCriteria")]
  pub success_criteria: Vec<String>,
  #[serde(default, rename = "nonGoals")]
  pub non_goals: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct SkillBundleCatalogEntry {
  pub manifest: SkillBundleManifest,
  pub path: PathBuf,
}

pub struct SkillBundleCatalog {
  entries: Vec<SkillBundleCatalogEntry>,
}

impl SkillBundleCatalog {
  pub fn discover(project_root: &Path) -> AuvResult<Self> {
    let bundles_root = project_root.join("bundles");
    if !bundles_root.exists() {
      return Ok(Self {
        entries: Vec::new(),
      });
    }

    let mut entries = Vec::new();
    collect_bundle_entries(&bundles_root, &mut entries)?;
    entries.sort_by(|left, right| left.manifest.metadata.id.cmp(&right.manifest.metadata.id));
    Ok(Self { entries })
  }

  pub fn entries(&self) -> &[SkillBundleCatalogEntry] {
    &self.entries
  }

  pub fn resolve(&self, project_root: &Path, query: &str) -> AuvResult<&SkillBundleCatalogEntry> {
    let candidate = Path::new(query);
    if candidate.exists() {
      let absolute = fs::canonicalize(candidate)
        .map_err(|error| format!("failed to canonicalize bundle path {query}: {error}"))?;
      return self
        .entries
        .iter()
        .find(|entry| entry.path == absolute)
        .ok_or_else(|| {
          format!(
            "bundle path {} is not a registered bundle manifest",
            absolute.display()
          )
        });
    }

    let project_relative = project_root.join(query);
    if project_relative.exists() {
      let absolute = fs::canonicalize(&project_relative).map_err(|error| {
        format!(
          "failed to canonicalize project-relative bundle path {}: {error}",
          project_relative.display()
        )
      })?;
      return self
        .entries
        .iter()
        .find(|entry| entry.path == absolute)
        .ok_or_else(|| {
          format!(
            "bundle path {} is not a registered bundle manifest",
            absolute.display()
          )
        });
    }

    self
      .entries
      .iter()
      .find(|entry| entry.manifest.metadata.id == query)
      .ok_or_else(|| format!("unknown bundle {query}; use `auv-cli skill bundle list`"))
  }
}

fn collect_bundle_entries(
  root: &Path,
  entries: &mut Vec<SkillBundleCatalogEntry>,
) -> AuvResult<()> {
  for raw_entry in fs::read_dir(root).map_err(|error| {
    format!(
      "failed to read bundle directory {}: {error}",
      root.display()
    )
  })? {
    let raw_entry =
      raw_entry.map_err(|error| format!("failed to enumerate bundle directory entry: {error}"))?;
    let path = raw_entry.path();
    if path.is_dir() {
      collect_bundle_entries(&path, entries)?;
      continue;
    }
    if path.extension().and_then(|value| value.to_str()) != Some("json") {
      continue;
    }

    let raw = fs::read_to_string(&path)
      .map_err(|error| format!("failed to read bundle manifest {}: {error}", path.display()))?;
    if let Ok(manifest) = serde_json::from_str::<SkillBundleManifest>(&raw) {
      if manifest.kind != "SkillBundle" {
        continue;
      }
      entries.push(SkillBundleCatalogEntry {
        manifest,
        path: fs::canonicalize(&path)
          .map_err(|error| format!("failed to canonicalize {}: {error}", path.display()))?,
      });
    }
  }
  Ok(())
}

#[derive(Debug, Deserialize)]
struct ExportedBundlePackageManifest {
  #[serde(rename = "bundleId")]
  bundle_id: String,
  #[serde(rename = "bundleName")]
  bundle_name: String,
  #[serde(rename = "bundleVersion")]
  bundle_version: String,
  #[serde(rename = "bundleStatus")]
  bundle_status: String,
  #[serde(rename = "sourceManifest")]
  source_manifest: String,
  #[serde(rename = "projectRoot")]
  project_root: String,
  #[serde(rename = "coverageReport")]
  coverage_report: String,
  versions: ExportedBundlePackageVersions,
  members: Vec<ExportedBundlePackageMember>,
  verification: ExportedBundlePackageVerification,
  #[serde(default, rename = "knownLimits")]
  known_limits: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExportedBundlePackageVersions {
  auv: String,
  #[serde(default, rename = "targetApplication")]
  target_application: String,
}

#[derive(Debug, Deserialize)]
struct ExportedBundlePackageMember {
  #[serde(rename = "recipeId")]
  recipe_id: String,
  #[serde(rename = "caseMatrixId")]
  case_matrix_id: String,
  role: String,
  contract: String,
  #[serde(default, rename = "appBundleId")]
  app_bundle_id: String,
  #[serde(default, rename = "targetApplication")]
  target_application: String,
  #[serde(default, rename = "validatedCaseIds")]
  validated_case_ids: Vec<String>,
  #[serde(default, rename = "candidateCaseIds")]
  candidate_case_ids: Vec<String>,
  #[serde(default, rename = "evidenceRefs")]
  evidence_refs: Vec<String>,
  #[serde(rename = "packageDir")]
  package_dir: String,
  #[serde(rename = "coverageReport")]
  coverage_report: String,
  #[serde(default, rename = "coverageSummary")]
  coverage_summary: SkillBundleMemberCoverageSummary,
}

#[derive(Debug, Deserialize)]
struct ExportedBundlePackageVerification {
  #[serde(default, rename = "expectedSignals")]
  expected_signals: Vec<String>,
  #[serde(default, rename = "successCriteria")]
  success_criteria: Vec<String>,
  #[serde(default, rename = "nonGoals")]
  non_goals: Vec<String>,
}

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
  let mut package_member_dirs = Vec::new();
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

    package_index.push(render_bundle_index_member_line(member));
    package_member_dirs.push(member_relative_dir.clone());
    package_member_readme_entries.push(render_bundle_package_member(
      member,
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
    render_bundle_package_manifest(entry, project_root, &package_member_dirs),
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

pub fn verify_bundle(
  project_root: &Path,
  runtime_version: &str,
  skill_catalog: &SkillCatalog,
  case_matrix_catalog: &SkillCaseMatrixCatalog,
  entry: &SkillBundleCatalogEntry,
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
  verify_bundle_metadata_version(entry)?;
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
  let bundle_target_application = validate_bundle_target_application_scope(entry)?;
  if entry.manifest.members.is_empty() {
    return Err(format!(
      "bundle {} has no members",
      entry.manifest.metadata.id
    ));
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
    format!(
      "current runtime version {} is not parseable: {error}",
      runtime_version
    )
  })?;
  if !runtime_req.matches(&runtime_version) {
    return Err(format!(
      "bundle {} requires runtime {} but current runtime is {}",
      entry.manifest.metadata.id, entry.manifest.versions.auv, runtime_version
    ));
  }
  if let Some((bundle_app_bundle_id, bundle_target_req)) = bundle_target_application {
    let bundle_app_version = resolve_installed_app_version(&bundle_app_bundle_id)?;
    if !bundle_target_req.matches(&bundle_app_version) {
      return Err(format!(
        "bundle {} requires targetApplication {} but app {} version is {}",
        entry.manifest.metadata.id,
        entry.manifest.versions.target_application,
        bundle_app_bundle_id,
        bundle_app_version
      ));
    }
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
    skill_catalog
      .resolve_recipe_id(&member.recipe_id)
      .map_err(|error| {
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
      let source = project_root.join(evidence_ref);
      if !source.exists() {
        continue;
      }
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

fn read_json_file(path: &Path) -> Result<serde_json::Value, String> {
  let raw = fs::read_to_string(path)
    .map_err(|error| format!("failed to read JSON file {}: {error}", path.display()))?;
  serde_json::from_str(&raw)
    .map_err(|error| format!("failed to parse JSON file {}: {error}", path.display()))
}

fn render_bundle_package_manifest(
  entry: &SkillBundleCatalogEntry,
  project_root: &Path,
  package_member_dirs: &[String],
) -> String {
  let members = entry
    .manifest
    .members
    .iter()
    .zip(package_member_dirs.iter())
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
        "coverageReport": bundle_member_coverage_relative_path(&member.recipe_id),
        "coverageSummary": {
          "activationStatus": member.coverage_summary.activation_status,
          "semanticSelectionStatus": member.coverage_summary.semantic_selection_status,
          "validatedClaims": member.coverage_summary.validated_claims,
          "boundaryClaims": member.coverage_summary.boundary_claims,
        },
      })
    })
    .collect::<Vec<_>>();

  let value = serde_json::json!({
    "bundleId": entry.manifest.metadata.id,
    "bundleName": entry.manifest.metadata.name,
    "bundleVersion": entry.manifest.metadata.version,
    "bundleStatus": entry.manifest.metadata.status,
    "versions": {
      "auv": entry.manifest.versions.auv,
      "targetApplication": entry.manifest.versions.target_application,
    },
    "sourceManifest": entry.path.display().to_string(),
    "projectRoot": project_root.display().to_string(),
    "coverageReport": "coverage.md",
    "members": members,
    "verification": {
      "expectedSignals": entry.manifest.verification.expected_signals,
      "successCriteria": entry.manifest.verification.success_criteria,
      "nonGoals": entry.manifest.verification.non_goals,
    },
    "knownLimits": entry.manifest.known_limits,
  });

  serde_json::to_string_pretty(&value).unwrap_or_else(|error| {
    format!("{{\"error\":\"failed to render bundle package manifest: {error}\"}}\n")
  })
}

fn bundle_member_relative_dir(recipe_id: &str) -> String {
  format!("members/{}", sanitized_bundle_package_name(recipe_id))
}

fn bundle_member_recipe_relative_path(recipe_id: &str) -> String {
  format!("{}/recipe.json", bundle_member_relative_dir(recipe_id))
}

fn bundle_member_cases_relative_path(recipe_id: &str) -> String {
  format!("{}/cases.json", bundle_member_relative_dir(recipe_id))
}

fn bundle_member_evidence_relative_dir(recipe_id: &str) -> String {
  format!("{}/evidence", bundle_member_relative_dir(recipe_id))
}

fn bundle_member_evidence_relative_path(recipe_id: &str) -> String {
  format!("{}/evidence.txt", bundle_member_relative_dir(recipe_id))
}

fn bundle_member_summary_relative_path(recipe_id: &str) -> String {
  format!("{}/summary.txt", bundle_member_relative_dir(recipe_id))
}

fn bundle_member_coverage_relative_path(recipe_id: &str) -> String {
  format!("{}/coverage.md", bundle_member_relative_dir(recipe_id))
}

fn render_bundle_index_member_line(member: &SkillBundleMember) -> String {
  format!(
    "  - recipeId={} caseMatrixId={} role={} contract={} memberDir={}",
    member.recipe_id,
    member.case_matrix_id,
    member.role,
    member.contract,
    bundle_member_relative_dir(&member.recipe_id)
  )
}

fn render_bundle_member_evidence(member: &SkillBundleMember) -> String {
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
  if !member.coverage_summary.activation_status.is_empty() {
    lines.push(format!(
      "activationStatus={}",
      member.coverage_summary.activation_status
    ));
  }
  if !member.coverage_summary.semantic_selection_status.is_empty() {
    lines.push(format!(
      "semanticSelectionStatus={}",
      member.coverage_summary.semantic_selection_status
    ));
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
  if !member.coverage_summary.validated_claims.is_empty() {
    lines.push("validatedClaims=".to_string());
    for claim in &member.coverage_summary.validated_claims {
      lines.push(format!("  - {}", claim));
    }
  }
  if !member.coverage_summary.boundary_claims.is_empty() {
    lines.push("boundaryClaims=".to_string());
    for claim in &member.coverage_summary.boundary_claims {
      lines.push(format!("  - {}", claim));
    }
  }
  lines.join("\n") + "\n"
}

fn render_bundle_member_summary(
  member: &SkillBundleMember,
  member_relative_dir: &str,
  recipe_path: &Path,
  case_matrix_path: &Path,
) -> String {
  let evidence = if member.evidence_refs.is_empty() {
    "none".to_string()
  } else {
    member.evidence_refs.join(", ")
  };
  format!(
    "`{}` -> `{}` ({})\n  - dir: `{}`\n  - recipe: `{}`\n  - case matrix: `{}`\n  - evidence: `{}`\n  - source recipe: `{}`\n  - source case matrix: `{}`\n  - activation status: `{}`\n  - semantic selection status: `{}`\n  - evidence refs: {}",
    member.recipe_id,
    member.case_matrix_id,
    member.contract,
    member_relative_dir,
    bundle_member_recipe_relative_path(&member.recipe_id),
    bundle_member_cases_relative_path(&member.recipe_id),
    bundle_member_evidence_relative_path(&member.recipe_id),
    recipe_path.display(),
    case_matrix_path.display(),
    if member.coverage_summary.activation_status.is_empty() {
      "unspecified"
    } else {
      member.coverage_summary.activation_status.as_str()
    },
    if member.coverage_summary.semantic_selection_status.is_empty() {
      "unspecified"
    } else {
      member.coverage_summary.semantic_selection_status.as_str()
    },
    evidence
  )
}

fn render_bundle_package_member(
  member: &SkillBundleMember,
  member_relative_dir: &str,
  recipe_path: &Path,
  case_matrix_path: &Path,
) -> String {
  format!(
    "{} -> {} [{}] dir={} recipe={} cases={}",
    member.recipe_id,
    member.case_matrix_id,
    member.contract,
    member_relative_dir,
    recipe_path.display(),
    case_matrix_path.display()
  )
}

fn render_bundle_package_coverage(
  entry: &SkillBundleCatalogEntry,
  skill_catalog: &SkillCatalog,
  case_matrix_catalog: &SkillCaseMatrixCatalog,
  project_root: &Path,
) -> Result<String, String> {
  let mut output = String::new();
  output.push_str(&format!(
    "# Bundle Coverage: {}\n\n",
    entry.manifest.metadata.id
  ));
  output.push_str(&format!(
    "- bundle name: `{}`\n",
    entry.manifest.metadata.name
  ));
  output.push_str(&format!(
    "- bundle version: `{}`\n",
    entry.manifest.metadata.version
  ));
  output.push_str(&format!(
    "- bundle status: `{}`\n",
    entry.manifest.metadata.status
  ));
  output.push_str(&format!(
    "- target family: `{}` on `{}`\n",
    entry.manifest.target.application_family, entry.manifest.target.platform
  ));
  output.push_str(&format!(
    "- member count: `{}`\n\n",
    entry.manifest.members.len()
  ));

  output.push_str("## Known Limits\n\n");
  if entry.manifest.known_limits.is_empty() {
    output.push_str("- none declared\n");
  } else {
    for limit in &entry.manifest.known_limits {
      output.push_str(&format!("- {}\n", limit));
    }
  }

  output.push_str("\n## Member Coverage\n\n");
  for member in &entry.manifest.members {
    let _recipe_entry = skill_catalog.resolve_recipe_id(&member.recipe_id)?;
    let _case_matrix_entry = case_matrix_catalog.resolve(project_root, &member.case_matrix_id)?;
    output.push_str(&format!("### {}\n\n", member.recipe_id));
    output.push_str(&format!("- role: `{}`\n", member.role));
    output.push_str(&format!("- contract: `{}`\n", member.contract));
    if !member.coverage_summary.activation_status.is_empty() {
      output.push_str(&format!(
        "- activation status: `{}`\n",
        member.coverage_summary.activation_status
      ));
    }
    if !member.coverage_summary.semantic_selection_status.is_empty() {
      output.push_str(&format!(
        "- semantic selection status: `{}`\n",
        member.coverage_summary.semantic_selection_status
      ));
    }
    if !member.app_bundle_id.is_empty() {
      output.push_str(&format!("- app bundle id: `{}`\n", member.app_bundle_id));
    }
    if !member.target_application.is_empty() {
      output.push_str(&format!(
        "- target application: `{}`\n",
        member.target_application
      ));
    }
    output.push_str(&format!(
      "- coverage report: `{}`\n",
      bundle_member_coverage_relative_path(&member.recipe_id)
    ));
    output.push_str(&format!(
      "- validated cases: `{}`\n",
      member.validated_case_ids.len()
    ));
    output.push_str(&format!(
      "- candidate cases: `{}`\n",
      member.candidate_case_ids.len()
    ));
    output.push_str(&format!(
      "- recipe path: `{}`\n",
      bundle_member_recipe_relative_path(&member.recipe_id)
    ));
    output.push_str(&format!(
      "- case matrix path: `{}`\n",
      bundle_member_cases_relative_path(&member.recipe_id)
    ));
    if !member.coverage_summary.validated_claims.is_empty() {
      output.push_str("- validated claims:\n");
      for claim in &member.coverage_summary.validated_claims {
        output.push_str(&format!("  - {}\n", claim));
      }
    }
    if !member.coverage_summary.boundary_claims.is_empty() {
      output.push_str("- boundary claims:\n");
      for claim in &member.coverage_summary.boundary_claims {
        output.push_str(&format!("  - {}\n", claim));
      }
    }
    output.push('\n');
  }

  Ok(output)
}

fn render_bundle_standalone_coverage(
  bundle_manifest: &SkillBundleManifest,
  package_manifest: &ExportedBundlePackageManifest,
) -> String {
  let mut output = String::new();
  output.push_str(&format!(
    "# Bundle Coverage: {}\n\n",
    bundle_manifest.metadata.id
  ));
  output.push_str(&format!(
    "- bundle name: `{}`\n",
    bundle_manifest.metadata.name
  ));
  output.push_str(&format!(
    "- bundle version: `{}`\n",
    bundle_manifest.metadata.version
  ));
  output.push_str(&format!(
    "- bundle status: `{}`\n",
    bundle_manifest.metadata.status
  ));
  output.push_str(&format!(
    "- target family: `{}` on `{}`\n",
    bundle_manifest.target.application_family, bundle_manifest.target.platform
  ));
  output.push_str(&format!(
    "- member count: `{}`\n\n",
    bundle_manifest.members.len()
  ));

  output.push_str("## Known Limits\n\n");
  if bundle_manifest.known_limits.is_empty() {
    output.push_str("- none declared\n");
  } else {
    for limit in &bundle_manifest.known_limits {
      output.push_str(&format!("- {}\n", limit));
    }
  }

  output.push_str("\n## Member Coverage\n\n");
  for member in &bundle_manifest.members {
    let package_member = package_manifest
      .members
      .iter()
      .find(|candidate| candidate.recipe_id == member.recipe_id)
      .expect("package member should exist for standalone coverage render");
    output.push_str(&format!("### {}\n\n", member.recipe_id));
    output.push_str(&format!("- role: `{}`\n", member.role));
    output.push_str(&format!("- contract: `{}`\n", member.contract));
    if !member.coverage_summary.activation_status.is_empty() {
      output.push_str(&format!(
        "- activation status: `{}`\n",
        member.coverage_summary.activation_status
      ));
    }
    if !member.coverage_summary.semantic_selection_status.is_empty() {
      output.push_str(&format!(
        "- semantic selection status: `{}`\n",
        member.coverage_summary.semantic_selection_status
      ));
    }
    if !member.app_bundle_id.is_empty() {
      output.push_str(&format!("- app bundle id: `{}`\n", member.app_bundle_id));
    }
    if !member.target_application.is_empty() {
      output.push_str(&format!(
        "- target application: `{}`\n",
        member.target_application
      ));
    }
    output.push_str(&format!(
      "- coverage report: `{}`\n",
      package_member.coverage_report
    ));
    output.push_str(&format!(
      "- validated cases: `{}`\n",
      member.validated_case_ids.len()
    ));
    output.push_str(&format!(
      "- candidate cases: `{}`\n",
      member.candidate_case_ids.len()
    ));
    output.push_str(&format!(
      "- recipe path: `{}`\n",
      bundle_member_recipe_relative_path(&member.recipe_id)
    ));
    output.push_str(&format!(
      "- case matrix path: `{}`\n",
      bundle_member_cases_relative_path(&member.recipe_id)
    ));
    if !member.coverage_summary.validated_claims.is_empty() {
      output.push_str("- validated claims:\n");
      for claim in &member.coverage_summary.validated_claims {
        output.push_str(&format!("  - {}\n", claim));
      }
    }
    if !member.coverage_summary.boundary_claims.is_empty() {
      output.push_str("- boundary claims:\n");
      for claim in &member.coverage_summary.boundary_claims {
        output.push_str(&format!("  - {}\n", claim));
      }
    }
    output.push('\n');
  }

  output
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

fn verify_bundle_metadata_version(entry: &SkillBundleCatalogEntry) -> Result<(), String> {
  semver::Version::parse(&entry.manifest.metadata.version).map_err(|error| {
    format!(
      "bundle {} has invalid metadata.version {}: {error}",
      entry.manifest.metadata.id, entry.manifest.metadata.version
    )
  })?;
  Ok(())
}

fn validate_bundle_target_application_scope(
  entry: &SkillBundleCatalogEntry,
) -> Result<Option<(String, semver::VersionReq)>, String> {
  let raw = entry.manifest.versions.target_application.trim();
  if raw.is_empty() {
    return Ok(None);
  }

  let target_req = semver::VersionReq::parse(raw).map_err(|error| {
    format!(
      "bundle {} has invalid versions.targetApplication {}: {error}",
      entry.manifest.metadata.id, entry.manifest.versions.target_application
    )
  })?;

  let mut app_bundle_ids = std::collections::BTreeSet::new();
  for member in &entry.manifest.members {
    let app_bundle_id = member.app_bundle_id.trim();
    if !app_bundle_id.is_empty() {
      app_bundle_ids.insert(app_bundle_id.to_string());
    }
  }

  if app_bundle_ids.is_empty() {
    return Err(format!(
      "bundle {} declares versions.targetApplication {} but no member declares appBundleId",
      entry.manifest.metadata.id, entry.manifest.versions.target_application
    ));
  }
  if app_bundle_ids.len() > 1 {
    let bundle_ids = app_bundle_ids.into_iter().collect::<Vec<_>>().join(", ");
    return Err(format!(
      "bundle {} declares versions.targetApplication {} but spans multiple appBundleId values: {}",
      entry.manifest.metadata.id, entry.manifest.versions.target_application, bundle_ids
    ));
  }

  let app_bundle_id = app_bundle_ids.into_iter().next().unwrap();
  Ok(Some((app_bundle_id, target_req)))
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
  resolve_installed_app_version(app_bundle_id)
}

fn resolve_installed_app_version(app_bundle_id: &str) -> Result<semver::Version, String> {
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
  let Some(path) = stdout.lines().map(str::trim).find(|line| !line.is_empty()) else {
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
    .args([
      "-c",
      &format!("Print {key}"),
      &info_plist.display().to_string(),
    ])
    .output()
    .map_err(|error| {
      format!(
        "failed to read {key} from {}: {error}",
        info_plist.display()
      )
    })?;
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

#[cfg(test)]
mod tests {
  use super::*;
  use std::env;
  use std::fs;

  use crate::model::now_millis;
  use crate::skill::SkillCaseMatrixCatalog;

  #[test]
  fn bundle_catalog_discovers_bundle_manifests_only() {
    let root = env::temp_dir().join(format!("auv-bundle-catalog-{}", now_millis()));
    fs::create_dir_all(root.join("bundles/test")).expect("temp bundles dir should exist");
    fs::write(
      root.join("bundles/test/example.v0.json"),
      r#"{
        "apiVersion": "auv.ai/v1alpha1",
        "kind": "SkillBundle",
        "metadata": { "id": "test.bundle.v0", "name": "Test Bundle", "version": "0.1.0" },
        "members": []
      }"#,
    )
    .expect("bundle manifest should write");
    fs::write(root.join("bundles/test/readme.txt"), "ignored").expect("non-json file should write");

    let catalog = SkillBundleCatalog::discover(&root).expect("catalog should load");
    assert_eq!(catalog.entries().len(), 1);
    assert_eq!(catalog.entries()[0].manifest.metadata.id, "test.bundle.v0");

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn bundle_catalog_resolves_by_id() {
    let root = env::temp_dir().join(format!("auv-bundle-resolve-{}", now_millis()));
    fs::create_dir_all(root.join("bundles/test")).expect("temp bundles dir should exist");
    fs::write(
      root.join("bundles/test/example.v0.json"),
      r#"{
        "apiVersion": "auv.ai/v1alpha1",
        "kind": "SkillBundle",
        "metadata": { "id": "test.bundle.v0", "name": "Test Bundle", "version": "0.1.0" },
        "members": []
      }"#,
    )
    .expect("bundle manifest should write");

    let catalog = SkillBundleCatalog::discover(&root).expect("catalog should load");
    let entry = catalog
      .resolve(&root, "test.bundle.v0")
      .expect("bundle should resolve");
    assert_eq!(entry.manifest.metadata.name, "Test Bundle");

    let _ = fs::remove_dir_all(root);
  }

  fn bundle_entry_with(
    metadata_version: &str,
    target_application: &str,
    member_app_bundle_ids: &[&str],
  ) -> SkillBundleCatalogEntry {
    SkillBundleCatalogEntry {
      manifest: SkillBundleManifest {
        api_version: "auv.ai/v1alpha1".to_string(),
        kind: "SkillBundle".to_string(),
        metadata: SkillBundleMetadata {
          id: "test.bundle.v0".to_string(),
          name: "Test Bundle".to_string(),
          version: metadata_version.to_string(),
          status: "working".to_string(),
        },
        target: SkillBundleTarget {
          application_family: "native-macos-apps".to_string(),
          platform: "macOS".to_string(),
        },
        versions: SkillBundleVersions {
          auv: ">=0.0.1, <0.1.0".to_string(),
          target_application: target_application.to_string(),
        },
        members: member_app_bundle_ids
          .iter()
          .enumerate()
          .map(|(index, app_bundle_id)| SkillBundleMember {
            recipe_id: format!("test.recipe.{index}"),
            case_matrix_id: format!("test.recipe.{index}.cases"),
            role: "sample".to_string(),
            validated_case_ids: vec!["baseline".to_string()],
            candidate_case_ids: Vec::new(),
            contract: "verifyAxText".to_string(),
            evidence_refs: Vec::new(),
            app_bundle_id: (*app_bundle_id).to_string(),
            target_application: String::new(),
            coverage_summary: SkillBundleMemberCoverageSummary::default(),
          })
          .collect(),
        verification: SkillBundleVerification {
          expected_signals: vec!["signal".to_string()],
          success_criteria: vec!["criteria".to_string()],
          non_goals: Vec::new(),
        },
        known_limits: Vec::new(),
      },
      path: PathBuf::from("/tmp/test-bundle.json"),
    }
  }

  #[test]
  fn bundle_metadata_version_must_be_semver() {
    let entry = bundle_entry_with("not-a-version", "", &["com.apple.Notes"]);
    let error = verify_bundle_metadata_version(&entry).expect_err("metadata version should fail");
    assert!(error.contains("metadata.version"));
  }

  #[test]
  fn bundle_target_application_scope_rejects_multi_app_bundle() {
    let entry = bundle_entry_with(
      "0.1.0",
      ">=1.0.0, <2.0.0",
      &["com.apple.Notes", "com.apple.TextEdit"],
    );
    let error = validate_bundle_target_application_scope(&entry)
      .expect_err("multi-app bundle targetApplication should fail");
    assert!(error.contains("spans multiple appBundleId values"));
  }

  #[test]
  fn bundle_target_application_scope_allows_empty_for_multi_app_bundle() {
    let entry = bundle_entry_with("0.1.0", "", &["com.apple.Notes", "com.apple.TextEdit"]);
    let scope = validate_bundle_target_application_scope(&entry)
      .expect("empty targetApplication should be allowed");
    assert!(scope.is_none());
  }

  #[test]
  fn bundle_target_application_scope_allows_single_app_bundle() {
    let entry = bundle_entry_with("0.1.0", ">=1.0.0, <2.0.0", &["com.apple.Notes"]);
    let scope = validate_bundle_target_application_scope(&entry)
      .expect("single-app targetApplication should be allowed")
      .expect("scope should exist");
    assert_eq!(scope.0, "com.apple.Notes");
    assert!(scope.1.matches(&semver::Version::parse("1.4.0").unwrap()));
  }

  #[test]
  fn export_bundle_writes_self_consistent_package() {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let skill_catalog = SkillCatalog::discover(&project_root).expect("skill catalog should load");
    let case_matrix_catalog =
      SkillCaseMatrixCatalog::discover(&project_root).expect("case matrix catalog should load");
    let bundle_catalog =
      SkillBundleCatalog::discover(&project_root).expect("bundle catalog should load");
    let entry = bundle_catalog
      .resolve(&project_root, "native.app.skill-tree.v0")
      .expect("bundle should resolve");

    let output_dir = env::temp_dir().join(format!("auv-bundle-export-verify-{}", now_millis()));
    export_bundle(
      &project_root,
      &skill_catalog,
      &case_matrix_catalog,
      entry,
      output_dir.clone(),
    )
    .expect("bundle export should succeed");

    let package_root = output_dir.join(sanitized_bundle_package_name(&entry.manifest.metadata.id));
    verify_exported_bundle_package(
      &project_root,
      &skill_catalog,
      &case_matrix_catalog,
      entry,
      &package_root,
    )
    .expect("exported bundle package should self-verify");
    let bundle_id = verify_exported_bundle_package_standalone(&package_root)
      .expect("exported bundle package should standalone-verify");
    assert_eq!(bundle_id, entry.manifest.metadata.id);

    let package_manifest: ExportedBundlePackageManifest = serde_json::from_value(
      read_json_file(&package_root.join("package.json")).expect("package manifest should read"),
    )
    .expect("package manifest should parse");
    for member in &package_manifest.members {
      assert!(member.package_dir.starts_with("members/"));
      assert!(!member.package_dir.contains(" -> "));
    }

    let _ = fs::remove_dir_all(output_dir);
  }
}
