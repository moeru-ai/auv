use std::fs;
use std::path::Path;

use crate::model::AuvResult;

use super::model::{SkillBundleCatalogEntry, SkillBundleManifest};

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
