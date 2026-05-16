use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::model::AuvResult;

#[derive(Clone, Debug, Deserialize)]
pub struct SkillBundleManifest {
  #[serde(rename = "apiVersion")]
  pub api_version: String,
  pub kind: String,
  pub metadata: SkillBundleMetadata,
  #[serde(default)]
  pub target: SkillBundleTarget,
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

#[cfg(test)]
mod tests {
  use super::SkillBundleCatalog;
  use std::env;
  use std::fs;

  use crate::model::now_millis;

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
}
