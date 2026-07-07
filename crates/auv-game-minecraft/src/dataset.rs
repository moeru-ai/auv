use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub type DatasetResult<T> = Result<T, String>;

pub const SPATIAL_BUNDLE_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpatialBundleInputs {
  pub output_dir: PathBuf,
  pub source: SourceRunSummary,
  pub artifacts: Vec<SpatialBundleSourceArtifact>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRunSummary {
  pub source_run_id: String,
  pub source_operation: String,
  pub source_run_type: String,
  pub source_status: String,
  pub generated_at_millis: u64,
  #[serde(default)]
  pub auv_git_commit: Option<String>,
  #[serde(default)]
  pub exporter_git_commit: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpatialBundleSourceArtifact {
  pub artifact_id: String,
  pub role: String,
  pub source_path: PathBuf,
  pub source_run_path: String,
  pub summary: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialBundleOutput {
  pub output_dir: PathBuf,
  pub manifest: SpatialBundleManifest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpatialBundleManifest {
  pub schema_version: u32,
  pub source_run: SourceRunSummary,
  pub counts: SpatialBundleCounts,
  pub artifacts: Vec<SpatialBundleArtifactRecord>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpatialBundleCounts {
  pub screenshots: usize,
  pub spatial_frames: usize,
  pub actions: usize,
  pub verification: usize,
  pub overlays: usize,
  pub skipped: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpatialBundleArtifactRecord {
  pub artifact_id: String,
  pub role: String,
  pub source_path: String,
  pub bundle_path: String,
  pub directory: SpatialBundleDirectory,
  pub summary: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpatialBundleDirectory {
  Screenshots,
  SpatialFrames,
  Actions,
  Verification,
  Overlays,
}

impl SpatialBundleDirectory {
  pub fn path_segment(self) -> &'static str {
    match self {
      Self::Screenshots => "screenshots",
      Self::SpatialFrames => "spatial_frames",
      Self::Actions => "actions",
      Self::Verification => "verification",
      Self::Overlays => "overlays",
    }
  }
}

pub fn export_spatial_bundle(inputs: SpatialBundleInputs) -> DatasetResult<SpatialBundleOutput> {
  prepare_bundle_dirs(&inputs.output_dir)?;
  let mut counts = SpatialBundleCounts::default();
  let mut records = Vec::new();

  for source in inputs.artifacts {
    let Some(directory) = directory_for_role(&source.role) else {
      counts.skipped += 1;
      continue;
    };
    increment_count(&mut counts, directory);
    let file_name = bundle_file_name(&source)?;
    let bundle_path = Path::new(directory.path_segment()).join(file_name);
    let destination = inputs.output_dir.join(&bundle_path);
    copy_file(&source.source_path, &destination)?;
    records.push(SpatialBundleArtifactRecord {
      artifact_id: source.artifact_id,
      role: source.role,
      source_path: source.source_run_path,
      bundle_path: bundle_path.to_string_lossy().into_owned(),
      directory,
      summary: source.summary,
    });
  }

  let manifest = SpatialBundleManifest {
    schema_version: SPATIAL_BUNDLE_SCHEMA_VERSION,
    source_run: inputs.source,
    counts,
    artifacts: records,
    known_limits: vec![
      "mc6 bundle v0 copies source run artifacts only; it does not synthesize missing labels".to_string(),
      "actions/ may be empty until Minecraft live-click records a first-class InputActionResult artifact".to_string(),
    ],
  };
  write_manifest(&inputs.output_dir.join("run.json"), &manifest)?;

  Ok(SpatialBundleOutput {
    output_dir: inputs.output_dir,
    manifest,
  })
}

pub fn directory_for_role(role: &str) -> Option<SpatialBundleDirectory> {
  match role {
    "minecraft-screenshot" => Some(SpatialBundleDirectory::Screenshots),
    "minecraft-spatial-frame" | "minecraft-projection" => Some(SpatialBundleDirectory::SpatialFrames),
    "candidate-action-decision" | "candidate-action-execution" | "candidate-promotion" => Some(SpatialBundleDirectory::Actions),
    "operation-result" => Some(SpatialBundleDirectory::Verification),
    "minecraft-overlay" => Some(SpatialBundleDirectory::Overlays),
    _ => None,
  }
}

fn prepare_bundle_dirs(output_dir: &Path) -> DatasetResult<()> {
  for segment in [
    "screenshots",
    "spatial_frames",
    "actions",
    "verification",
    "overlays",
  ] {
    fs::create_dir_all(output_dir.join(segment))
      .map_err(|error| format!("failed to create minecraft spatial bundle directory {segment} under {}: {error}", output_dir.display()))?;
  }
  Ok(())
}

fn increment_count(counts: &mut SpatialBundleCounts, directory: SpatialBundleDirectory) {
  match directory {
    SpatialBundleDirectory::Screenshots => counts.screenshots += 1,
    SpatialBundleDirectory::SpatialFrames => counts.spatial_frames += 1,
    SpatialBundleDirectory::Actions => counts.actions += 1,
    SpatialBundleDirectory::Verification => counts.verification += 1,
    SpatialBundleDirectory::Overlays => counts.overlays += 1,
  }
}

fn bundle_file_name(source: &SpatialBundleSourceArtifact) -> DatasetResult<String> {
  let source_name = source
    .source_path
    .file_name()
    .and_then(|name| name.to_str())
    .ok_or_else(|| format!("minecraft spatial bundle source path {} has no valid file name", source.source_path.display()))?;
  Ok(format!("{}-{}", sanitize_component(&source.artifact_id), source_name))
}

fn sanitize_component(raw: &str) -> String {
  let sanitized = raw
    .chars()
    .map(|character| match character {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
      _ => '-',
    })
    .collect::<String>()
    .trim_matches('-')
    .to_string();
  if sanitized.is_empty() {
    "artifact".to_string()
  } else {
    sanitized
  }
}

fn copy_file(source: &Path, destination: &Path) -> DatasetResult<()> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent)
      .map_err(|error| format!("failed to create minecraft spatial bundle artifact directory {}: {error}", parent.display()))?;
  }
  fs::copy(source, destination).map_err(|error| {
    format!("failed to copy minecraft spatial bundle artifact from {} to {}: {error}", source.display(), destination.display())
  })?;
  Ok(())
}

fn write_manifest(path: &Path, manifest: &SpatialBundleManifest) -> DatasetResult<()> {
  let json = serde_json::to_string_pretty(manifest)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize minecraft spatial bundle manifest: {error}"))?;
  fs::write(path, json.as_bytes()).map_err(|error| format!("failed to write minecraft spatial bundle manifest {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn source_summary() -> SourceRunSummary {
    SourceRunSummary {
      source_run_id: "run_1".to_string(),
      source_operation: "auv.minecraft.bridge".to_string(),
      source_run_type: "execute".to_string(),
      source_status: "ok".to_string(),
      generated_at_millis: 123,
      auv_git_commit: Some("abc123".to_string()),
      exporter_git_commit: Some("abc123".to_string()),
    }
  }

  #[test]
  fn maps_known_roles_to_bundle_directories() {
    assert_eq!(directory_for_role("minecraft-screenshot"), Some(SpatialBundleDirectory::Screenshots));
    assert_eq!(directory_for_role("minecraft-spatial-frame"), Some(SpatialBundleDirectory::SpatialFrames));
    assert_eq!(directory_for_role("minecraft-projection"), Some(SpatialBundleDirectory::SpatialFrames));
    assert_eq!(directory_for_role("operation-result"), Some(SpatialBundleDirectory::Verification));
    assert_eq!(directory_for_role("minecraft-overlay"), Some(SpatialBundleDirectory::Overlays));
    assert_eq!(directory_for_role("telemetry-sample"), None);
  }

  #[test]
  fn exports_bundle_manifest_and_copied_artifacts() {
    let temp = tempfile::tempdir().expect("temp dir");
    let source_root = temp.path().join("source");
    let output_dir = temp.path().join("bundle");
    fs::create_dir_all(&source_root).expect("source dir");
    let screenshot = source_root.join("frame.png");
    let frame = source_root.join("frame.json");
    let operation = source_root.join("operation-result.json");
    fs::write(&screenshot, b"png").expect("screenshot");
    fs::write(&frame, b"{}").expect("frame");
    fs::write(&operation, b"{}").expect("operation");

    let output = export_spatial_bundle(SpatialBundleInputs {
      output_dir: output_dir.clone(),
      source: source_summary(),
      artifacts: vec![
        SpatialBundleSourceArtifact {
          artifact_id: "artifact_0001".to_string(),
          role: "minecraft-screenshot".to_string(),
          source_path: screenshot,
          source_run_path: "artifacts/artifact_0001_frame.png".to_string(),
          summary: None,
        },
        SpatialBundleSourceArtifact {
          artifact_id: "artifact_0002".to_string(),
          role: "minecraft-spatial-frame".to_string(),
          source_path: frame,
          source_run_path: "artifacts/artifact_0002_frame.json".to_string(),
          summary: Some("frame".to_string()),
        },
        SpatialBundleSourceArtifact {
          artifact_id: "artifact_0003".to_string(),
          role: "operation-result".to_string(),
          source_path: operation,
          source_run_path: "artifacts/artifact_0003_operation-result.json".to_string(),
          summary: None,
        },
        SpatialBundleSourceArtifact {
          artifact_id: "artifact_0004".to_string(),
          role: "telemetry-sample".to_string(),
          source_path: source_root.join("telemetry.jsonl"),
          source_run_path: "artifacts/artifact_0004_telemetry.jsonl".to_string(),
          summary: None,
        },
      ],
    })
    .expect("bundle should export");

    assert_eq!(output.manifest.schema_version, 1);
    assert_eq!(output.manifest.counts.screenshots, 1);
    assert_eq!(output.manifest.counts.spatial_frames, 1);
    assert_eq!(output.manifest.counts.verification, 1);
    assert_eq!(output.manifest.counts.skipped, 1);
    assert!(output_dir.join("run.json").is_file());
    assert_eq!(fs::read_dir(output_dir.join("screenshots")).expect("screenshots").count(), 1);
    assert_eq!(fs::read_dir(output_dir.join("spatial_frames")).expect("frames").count(), 1);
    assert_eq!(fs::read_dir(output_dir.join("verification")).expect("verification").count(), 1);
  }
}
