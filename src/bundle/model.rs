use std::path::PathBuf;

use serde::Deserialize;

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

#[derive(Debug, Deserialize)]
pub(crate) struct ExportedBundlePackageManifest {
  #[serde(rename = "bundleId")]
  pub(crate) bundle_id: String,
  #[serde(rename = "bundleName")]
  pub(crate) bundle_name: String,
  #[serde(rename = "bundleVersion")]
  pub(crate) bundle_version: String,
  #[serde(rename = "bundleStatus")]
  pub(crate) bundle_status: String,
  #[serde(rename = "sourceManifest")]
  pub(crate) source_manifest: String,
  #[serde(rename = "projectRoot")]
  pub(crate) project_root: String,
  #[serde(rename = "coverageReport")]
  pub(crate) coverage_report: String,
  pub(crate) versions: ExportedBundlePackageVersions,
  pub(crate) members: Vec<ExportedBundlePackageMember>,
  pub(crate) verification: ExportedBundlePackageVerification,
  #[serde(default, rename = "knownLimits")]
  pub(crate) known_limits: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExportedBundlePackageVersions {
  pub(crate) auv: String,
  #[serde(default, rename = "targetApplication")]
  pub(crate) target_application: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExportedBundlePackageMember {
  #[serde(rename = "recipeId")]
  pub(crate) recipe_id: String,
  #[serde(rename = "caseMatrixId")]
  pub(crate) case_matrix_id: String,
  pub(crate) role: String,
  pub(crate) contract: String,
  #[serde(default, rename = "appBundleId")]
  pub(crate) app_bundle_id: String,
  #[serde(default, rename = "targetApplication")]
  pub(crate) target_application: String,
  #[serde(default, rename = "validatedCaseIds")]
  pub(crate) validated_case_ids: Vec<String>,
  #[serde(default, rename = "candidateCaseIds")]
  pub(crate) candidate_case_ids: Vec<String>,
  #[serde(default, rename = "evidenceRefs")]
  pub(crate) evidence_refs: Vec<String>,
  #[serde(rename = "packageDir")]
  pub(crate) package_dir: String,
  #[serde(rename = "coverageReport")]
  pub(crate) coverage_report: String,
  #[serde(default, rename = "coverageSummary")]
  pub(crate) coverage_summary: SkillBundleMemberCoverageSummary,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ExportedBundlePackageVerification {
  #[serde(default, rename = "expectedSignals")]
  pub(crate) expected_signals: Vec<String>,
  #[serde(default, rename = "successCriteria")]
  pub(crate) success_criteria: Vec<String>,
  #[serde(default, rename = "nonGoals")]
  pub(crate) non_goals: Vec<String>,
}
