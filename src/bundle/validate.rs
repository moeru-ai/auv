// File: src/bundle/validate.rs
use crate::skill::{SkillCaseMatrixCatalog, SkillCatalog};

use super::model::SkillBundleCatalogEntry;

pub(crate) fn verify_bundle_metadata_version(
  entry: &SkillBundleCatalogEntry,
) -> Result<(), String> {
  semver::Version::parse(&entry.manifest.metadata.version).map_err(|error| {
    format!(
      "bundle {} has invalid metadata.version {}: {error}",
      entry.manifest.metadata.id, entry.manifest.metadata.version
    )
  })?;
  Ok(())
}

pub(crate) fn validate_bundle_target_application_scope(
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

pub fn verify_bundle(
  project_root: &std::path::Path,
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
  let _bundle_target_application = validate_bundle_target_application_scope(entry)?;
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
  for command in &entry.manifest.commands {
    if command.id.trim().is_empty() {
      return Err(format!(
        "bundle {} has a command with empty id",
        entry.manifest.metadata.id
      ));
    }
    if command.recipe_id.trim().is_empty() {
      return Err(format!(
        "bundle {} command {} must declare recipeId",
        entry.manifest.metadata.id, command.id
      ));
    }
    skill_catalog
      .resolve_recipe_id(&command.recipe_id)
      .map_err(|error| {
        format!(
          "bundle {} command {} references unknown recipe {}: {error}",
          entry.manifest.metadata.id, command.id, command.recipe_id
        )
      })?;
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
    let recipe_entry = skill_catalog
      .resolve_recipe_id(&member.recipe_id)
      .map_err(|error| {
        format!(
          "bundle {} references unknown recipe {}: {error}",
          entry.manifest.metadata.id, member.recipe_id
        )
      })?;
    if recipe_entry.manifest.strategy.verification_contract != member.contract {
      return Err(format!(
        "bundle {} member {} declares contract {} but recipe strategy.verificationContract is {}",
        entry.manifest.metadata.id,
        member.recipe_id,
        member.contract,
        recipe_entry.manifest.strategy.verification_contract
      ));
    }
    if !member.target_application.trim().is_empty() {
      semver::VersionReq::parse(&member.target_application).map_err(|error| {
        format!(
          "bundle {} member {} has invalid targetApplication {}: {error}",
          entry.manifest.metadata.id, member.recipe_id, member.target_application
        )
      })?;
      if member.app_bundle_id.trim().is_empty() {
        return Err(format!(
          "bundle {} member {} declares targetApplication {} but no appBundleId",
          entry.manifest.metadata.id, member.recipe_id, member.target_application
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
