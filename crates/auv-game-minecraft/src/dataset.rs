use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

pub type DatasetResult<T> = Result<T, String>;

pub const SPATIAL_BUNDLE_SCHEMA_VERSION: u32 = 1;
/// Bundle-local routing role written to `SpatialBundleManifest::artifacts`.
///
/// This is not a canonical `auv-tracing` artifact purpose or URI.
pub const SPATIAL_FRAME_BUNDLE_ROLE: &str = "minecraft-spatial-frame";
/// Bundle-local routing role written to `SpatialBundleManifest::artifacts`.
///
/// This is not a canonical `auv-tracing` artifact purpose or URI.
pub const PROJECTION_BUNDLE_ROLE: &str = "minecraft-projection";

const JAVASCRIPT_EXACT_INTEGER_MAX: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SourceAuthorityId(String);

impl SourceAuthorityId {
  pub fn new(value: impl Into<String>) -> DatasetResult<Self> {
    Ok(Self(validate_source_identifier("authority id", value.into())?))
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl fmt::Display for SourceAuthorityId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.0)
  }
}

impl<'de> Deserialize<'de> for SourceAuthorityId {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SourceRunId(String);

impl SourceRunId {
  pub fn new(value: impl Into<String>) -> DatasetResult<Self> {
    Ok(Self(validate_source_identifier("run id", value.into())?))
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl fmt::Display for SourceRunId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.0)
  }
}

impl<'de> Deserialize<'de> for SourceRunId {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SourceRunRevision(u64);

impl SourceRunRevision {
  pub fn new(value: u64) -> DatasetResult<Self> {
    if value > JAVASCRIPT_EXACT_INTEGER_MAX {
      return Err("Minecraft source run revision exceeds the JavaScript exact integer limit".to_string());
    }
    Ok(Self(value))
  }

  pub fn get(self) -> u64 {
    self.0
  }
}

impl<'de> Deserialize<'de> for SourceRunRevision {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    Self::new(u64::deserialize(deserializer)?).map_err(serde::de::Error::custom)
  }
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct SourceArtifactUri(String);

impl SourceArtifactUri {
  pub fn new(value: impl Into<String>) -> DatasetResult<Self> {
    let value = value.into();
    let remainder =
      value.strip_prefix("auv://runs/").ok_or_else(|| format!("Minecraft source artifact URI {value:?} must start with auv://runs/"))?;
    let (run_id, artifact_id) = remainder
      .split_once("/artifacts/")
      .ok_or_else(|| format!("Minecraft source artifact URI {value:?} must identify one run artifact"))?;
    validate_source_identifier("artifact URI run id", run_id.to_string())?;
    validate_source_identifier("artifact URI artifact id", artifact_id.to_string())?;
    Ok(Self(value))
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }

  pub fn run_id(&self) -> &str {
    self
      .0
      .strip_prefix("auv://runs/")
      .and_then(|remainder| remainder.split_once("/artifacts/").map(|(run_id, _)| run_id))
      .expect("SourceArtifactUri constructor preserves canonical run path")
  }
}

impl fmt::Display for SourceArtifactUri {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.0)
  }
}

impl FromStr for SourceArtifactUri {
  type Err = String;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    Self::new(value)
  }
}

impl<'de> Deserialize<'de> for SourceArtifactUri {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
  }
}

#[cfg(feature = "tracing")]
impl From<auv_tracing::AuthorityId> for SourceAuthorityId {
  fn from(value: auv_tracing::AuthorityId) -> Self {
    Self(value.to_string())
  }
}

#[cfg(feature = "tracing")]
impl From<auv_tracing::RunId> for SourceRunId {
  fn from(value: auv_tracing::RunId) -> Self {
    Self(value.to_string())
  }
}

#[cfg(feature = "tracing")]
impl From<auv_tracing::RunRevision> for SourceRunRevision {
  fn from(value: auv_tracing::RunRevision) -> Self {
    Self(value.get())
  }
}

#[cfg(feature = "tracing")]
impl From<auv_tracing::ArtifactUri> for SourceArtifactUri {
  fn from(value: auv_tracing::ArtifactUri) -> Self {
    Self(value.to_string())
  }
}

fn validate_source_identifier(kind: &str, value: String) -> DatasetResult<String> {
  if value.is_empty() {
    return Err(format!("Minecraft source {kind} must not be empty"));
  }
  if value.len() > 128 {
    return Err(format!("Minecraft source {kind} exceeds 128 bytes"));
  }
  if !value.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')) {
    return Err(format!("Minecraft source {kind} {value:?} contains unsupported characters"));
  }
  Ok(value)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpatialBundleInputs {
  pub output_dir: PathBuf,
  pub source_run: SourceRunReference,
  pub exported_at_millis: u64,
  pub artifacts: Vec<SpatialBundleSourceArtifact>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRunReference {
  pub authority_id: SourceAuthorityId,
  pub run_id: SourceRunId,
  pub through_revision: SourceRunRevision,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct BundleArtifactId(String);

impl BundleArtifactId {
  pub fn new(value: impl Into<String>) -> DatasetResult<Self> {
    let value = value.into();
    if value.is_empty() {
      return Err("Minecraft bundle artifact id must not be empty".to_string());
    }
    if !value.chars().all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_')) {
      return Err(format!("Minecraft bundle artifact id {value:?} contains unsupported characters"));
    }
    Ok(Self(value))
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl fmt::Display for BundleArtifactId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.0)
  }
}

impl<'de> Deserialize<'de> for BundleArtifactId {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: serde::Deserializer<'de>,
  {
    Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpatialBundleSourceArtifact {
  pub source_artifact_uri: SourceArtifactUri,
  pub bundle_artifact_id: BundleArtifactId,
  pub role: String,
  pub source_file: PathBuf,
  pub screenshot_bundle_artifact_id: Option<BundleArtifactId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpatialBundleOutput {
  pub output_dir: PathBuf,
  pub manifest: SpatialBundleManifest,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpatialBundleManifest {
  pub schema_version: u32,
  pub source_run: SourceRunReference,
  pub exported_at_millis: u64,
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
  pub source_artifact_uri: SourceArtifactUri,
  pub bundle_artifact_id: BundleArtifactId,
  pub role: String,
  pub bundle_path: PathBuf,
  pub directory: SpatialBundleDirectory,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub screenshot_bundle_artifact_id: Option<BundleArtifactId>,
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
    copy_file(&source.source_file, &destination)?;
    records.push(SpatialBundleArtifactRecord {
      source_artifact_uri: source.source_artifact_uri,
      bundle_artifact_id: source.bundle_artifact_id,
      role: source.role,
      bundle_path,
      directory,
      screenshot_bundle_artifact_id: source.screenshot_bundle_artifact_id,
    });
  }

  let manifest = SpatialBundleManifest {
    schema_version: SPATIAL_BUNDLE_SCHEMA_VERSION,
    source_run: inputs.source_run,
    exported_at_millis: inputs.exported_at_millis,
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
    SPATIAL_FRAME_BUNDLE_ROLE | PROJECTION_BUNDLE_ROLE => Some(SpatialBundleDirectory::SpatialFrames),
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
    .source_file
    .file_name()
    .and_then(|name| name.to_str())
    .ok_or_else(|| format!("minecraft spatial bundle source path {} has no valid file name", source.source_file.display()))?;
  Ok(format!("{}-{source_name}", source.bundle_artifact_id))
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

  fn source_run_reference() -> SourceRunReference {
    SourceRunReference {
      authority_id: SourceAuthorityId::new("authority_1").expect("source authority"),
      run_id: SourceRunId::new("run_1").expect("source run"),
      through_revision: SourceRunRevision::new(7).expect("source revision"),
    }
  }

  fn source_artifact_uri(index: u128) -> SourceArtifactUri {
    SourceArtifactUri::new(format!("auv://runs/00000000-0000-0000-0000-000000000001/artifacts/00000000-0000-0000-0000-{index:012}"))
      .expect("source artifact URI")
  }

  #[test]
  fn maps_known_roles_to_bundle_directories() {
    assert_eq!(directory_for_role("minecraft-screenshot"), Some(SpatialBundleDirectory::Screenshots));
    assert_eq!(directory_for_role(SPATIAL_FRAME_BUNDLE_ROLE), Some(SpatialBundleDirectory::SpatialFrames));
    assert_eq!(directory_for_role(PROJECTION_BUNDLE_ROLE), Some(SpatialBundleDirectory::SpatialFrames));
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
      source_run: source_run_reference(),
      exported_at_millis: 123,
      artifacts: vec![
        SpatialBundleSourceArtifact {
          source_artifact_uri: source_artifact_uri(1),
          bundle_artifact_id: BundleArtifactId::new("bundle-000001").expect("bundle artifact id"),
          role: "minecraft-screenshot".to_string(),
          source_file: screenshot,
          screenshot_bundle_artifact_id: None,
        },
        SpatialBundleSourceArtifact {
          source_artifact_uri: source_artifact_uri(2),
          bundle_artifact_id: BundleArtifactId::new("bundle-000002").expect("bundle artifact id"),
          role: SPATIAL_FRAME_BUNDLE_ROLE.to_string(),
          source_file: frame,
          screenshot_bundle_artifact_id: None,
        },
        SpatialBundleSourceArtifact {
          source_artifact_uri: source_artifact_uri(3),
          bundle_artifact_id: BundleArtifactId::new("bundle-000003").expect("bundle artifact id"),
          role: "operation-result".to_string(),
          source_file: operation,
          screenshot_bundle_artifact_id: None,
        },
        SpatialBundleSourceArtifact {
          source_artifact_uri: source_artifact_uri(4),
          bundle_artifact_id: BundleArtifactId::new("bundle-000004").expect("bundle artifact id"),
          role: "telemetry-sample".to_string(),
          source_file: source_root.join("telemetry.jsonl"),
          screenshot_bundle_artifact_id: None,
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
