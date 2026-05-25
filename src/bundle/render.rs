// File: src/bundle/render.rs
//! Bundle export renderers.
//!
//! Pure rendering helpers for bundle export: JSON manifests plus human-readable
//! summaries and coverage reports.

use std::path::Path;

use crate::skill::{SkillCaseMatrixCatalog, SkillCatalog, SkillStrategy};

use super::model::{
  ExportedBundlePackageManifest, SkillBundleCatalogEntry, SkillBundleManifest, SkillBundleMember,
};
use super::paths::{
  bundle_member_cases_relative_path, bundle_member_coverage_relative_path,
  bundle_member_evidence_relative_path, bundle_member_recipe_relative_path,
  bundle_member_relative_dir,
};

pub(crate) fn render_bundle_package_manifest(
  entry: &SkillBundleCatalogEntry,
  project_root: &Path,
  package_members: &[(String, SkillStrategy)],
) -> String {
  let members = entry
    .manifest
    .members
    .iter()
    .zip(package_members.iter())
    .map(|(member, (member_path, strategy))| {
      let taxonomy_id = strategy.taxonomy_id().unwrap_or_else(|_| {
        format!(
          "{}.{}.{}.{}",
          strategy.family, strategy.grounding, strategy.activation, strategy.verification_contract
        )
      });
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
        "taxonomyId": taxonomy_id,
        "strategy": {
          "family": strategy.family,
          "grounding": strategy.grounding,
          "activation": strategy.activation,
          "verificationContract": strategy.verification_contract,
        },
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

pub(crate) fn render_bundle_index_member_line(member: &SkillBundleMember) -> String {
  format!(
    "  - recipeId={} caseMatrixId={} role={} contract={} memberDir={}",
    member.recipe_id,
    member.case_matrix_id,
    member.role,
    member.contract,
    bundle_member_relative_dir(&member.recipe_id)
  )
}

pub(crate) fn render_bundle_member_evidence(member: &SkillBundleMember) -> String {
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

pub(crate) fn render_bundle_member_summary(
  member: &SkillBundleMember,
  strategy: &SkillStrategy,
  member_relative_dir: &str,
  recipe_path: &Path,
  case_matrix_path: &Path,
) -> String {
  let taxonomy_id = strategy.taxonomy_id().unwrap_or_else(|_| {
    format!(
      "{}.{}.{}.{}",
      strategy.family, strategy.grounding, strategy.activation, strategy.verification_contract
    )
  });
  let evidence = if member.evidence_refs.is_empty() {
    "none".to_string()
  } else {
    member.evidence_refs.join(", ")
  };
  format!(
    "`{}` -> `{}` ({})\n  - dir: `{}`\n  - recipe: `{}`\n  - case matrix: `{}`\n  - evidence: `{}`\n  - source recipe: `{}`\n  - source case matrix: `{}`\n  - strategy: `{}/{}/{} -> {}`\n  - strategy taxonomy: `{}`\n  - activation status: `{}`\n  - semantic selection status: `{}`\n  - evidence refs: {}",
    member.recipe_id,
    member.case_matrix_id,
    member.contract,
    member_relative_dir,
    bundle_member_recipe_relative_path(&member.recipe_id),
    bundle_member_cases_relative_path(&member.recipe_id),
    bundle_member_evidence_relative_path(&member.recipe_id),
    recipe_path.display(),
    case_matrix_path.display(),
    strategy.family,
    strategy.grounding,
    strategy.activation,
    strategy.verification_contract,
    taxonomy_id,
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

pub(crate) fn render_bundle_package_member(
  member: &SkillBundleMember,
  strategy: &SkillStrategy,
  member_relative_dir: &str,
  recipe_path: &Path,
  case_matrix_path: &Path,
) -> String {
  let taxonomy_id = strategy.taxonomy_id().unwrap_or_else(|_| {
    format!(
      "{}.{}.{}.{}",
      strategy.family, strategy.grounding, strategy.activation, strategy.verification_contract
    )
  });
  format!(
    "{} -> {} [{}] strategy={}/{}/{}->{} taxonomy={} dir={} recipe={} cases={}",
    member.recipe_id,
    member.case_matrix_id,
    member.contract,
    strategy.family,
    strategy.grounding,
    strategy.activation,
    strategy.verification_contract,
    taxonomy_id,
    member_relative_dir,
    recipe_path.display(),
    case_matrix_path.display()
  )
}

pub fn render_bundle_package_coverage(
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
    let recipe_entry = skill_catalog.resolve_recipe_id(&member.recipe_id)?;
    let _case_matrix_entry = case_matrix_catalog.resolve(project_root, &member.case_matrix_id)?;
    output.push_str(&format!("### {}\n\n", member.recipe_id));
    output.push_str(&format!("- role: `{}`\n", member.role));
    output.push_str(&format!("- contract: `{}`\n", member.contract));
    output.push_str(&format!(
      "- strategy: `{}/{}/{} -> {}`\n",
      recipe_entry.manifest.strategy.family,
      recipe_entry.manifest.strategy.grounding,
      recipe_entry.manifest.strategy.activation,
      recipe_entry.manifest.strategy.verification_contract
    ));
    if let Ok(taxonomy_id) = recipe_entry.manifest.strategy.taxonomy_id() {
      output.push_str(&format!("- strategy taxonomy: `{}`\n", taxonomy_id));
    }
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

pub(crate) fn render_bundle_standalone_coverage(
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
    output.push_str(&format!(
      "- strategy: `{}/{}/{} -> {}`\n",
      package_member.strategy.family,
      package_member.strategy.grounding,
      package_member.strategy.activation,
      package_member.strategy.verification_contract
    ));
    if !package_member.taxonomy_id.is_empty() {
      output.push_str(&format!(
        "- strategy taxonomy: `{}`\n",
        package_member.taxonomy_id
      ));
    }
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
