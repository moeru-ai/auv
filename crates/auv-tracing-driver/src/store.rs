// File: src/store.rs
//! Local run store (canonical snapshots + artifact files).
//!
//! The store persists a run as `run.json` plus `spans.jsonl`, `events.jsonl`,
//! and `artifacts.jsonl` under `runs/<run_id>/`, and manages the associated
//! `artifacts/` directory.
//!
//! Boundary: storage only. Viewer/server code lives in `inspect_server`, and
//! execution/orchestration lives in `runtime`/`recording`.

use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::artifact::{ArtifactBytesSource, ArtifactFileSource, ProducedArtifact};
use crate::error::AuvResult;
use crate::time::now_millis;
use crate::trace::{
  ARTIFACT_API_VERSION, ArtifactId, ArtifactRecordV1Alpha1, EVENT_API_VERSION, EventId,
  EventRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, SPAN_API_VERSION, SpanId,
  SpanRecordV1Alpha1,
};

#[derive(Clone, Debug, serde::Serialize)]
pub struct CanonicalRun {
  pub run: RunRecordV1Alpha1,
  pub spans: Vec<SpanRecordV1Alpha1>,
  pub events: Vec<EventRecordV1Alpha1>,
  pub artifacts: Vec<ArtifactRecordV1Alpha1>,
}

#[derive(Clone)]
pub struct LocalStore {
  root: PathBuf,
}

pub fn sanitized_artifact_name(raw: &str) -> String {
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

pub(crate) fn copy_file(source: &PathBuf, destination: &PathBuf) -> AuvResult<()> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create artifact directory {}: {error}",
        parent.display()
      )
    })?;
  }

  fs::copy(source, destination).map_err(|error| {
    let detail = match error.kind() {
      ErrorKind::NotFound => "source file not found".to_string(),
      _ => error.to_string(),
    };
    format!(
      "failed to copy artifact from {} to {}: {detail}",
      source.display(),
      destination.display()
    )
  })?;

  Ok(())
}

pub(crate) fn publish_bytes_to_path(destination: &Path, bytes: &[u8]) -> AuvResult<()> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create artifact directory {}: {error}",
        parent.display()
      )
    })?;
  }

  let file_name = destination
    .file_name()
    .and_then(|file_name| file_name.to_str())
    .unwrap_or("artifact");
  let parent = destination
    .parent()
    .ok_or_else(|| format!("invalid artifact destination {}", destination.display()))?;
  let temp_path = parent.join(format!(".{file_name}.upload-{}.tmp", now_millis()));
  fs::write(&temp_path, bytes).map_err(|error| {
    format!(
      "failed to write staged artifact {}: {error}",
      temp_path.display()
    )
  })?;
  fs::rename(&temp_path, destination).map_err(|error| {
    let _ = fs::remove_file(&temp_path);
    format!(
      "failed to publish staged artifact {}: {error}",
      destination.display()
    )
  })?;

  Ok(())
}

impl LocalStore {
  pub fn new(root: PathBuf) -> AuvResult<Self> {
    fs::create_dir_all(root.join("runs"))
      .map_err(|error| format!("failed to create run store root: {error}"))?;
    Ok(Self { root })
  }

  pub fn root(&self) -> &Path {
    &self.root
  }

  pub fn run_dir(&self, run_id: impl AsRef<str>) -> AuvResult<PathBuf> {
    Ok(
      self
        .root
        .join("runs")
        .join(validate_run_id(run_id.as_ref())?),
    )
  }

  pub fn write_run_snapshot(&self, snapshot: &CanonicalRun) -> AuvResult<()> {
    let run_id = validate_run_id(snapshot.run.run_id.as_str())?;
    let runs_root = self.root.join("runs");
    let run_directory = self.run_dir(run_id)?;
    let write_directory = if run_directory.exists() {
      validate_unpublished_run_directory(&run_directory)?;
      run_directory.clone()
    } else {
      let staging_directory = runs_root.join(format!(".{run_id}-tmp-{}", now_millis()));
      fs::create_dir(&staging_directory).map_err(|error| {
        format!(
          "failed to create staging run directory {}: {error}",
          staging_directory.display()
        )
      })?;
      staging_directory
    };
    let using_staging_directory = write_directory != run_directory;

    let write_result = (|| {
      fs::create_dir_all(write_directory.join("artifacts")).map_err(|error| {
        format!(
          "failed to create canonical run directory {}: {error}",
          write_directory.display()
        )
      })?;
      write_jsonl_atomic(
        &write_directory.join("spans.jsonl"),
        &snapshot.spans,
        "span records",
      )?;
      write_jsonl_atomic(
        &write_directory.join("events.jsonl"),
        &snapshot.events,
        "event records",
      )?;
      write_jsonl_atomic(
        &write_directory.join("artifacts.jsonl"),
        &snapshot.artifacts,
        "artifact records",
      )?;
      write_json_atomic(
        &write_directory.join("run.json"),
        &snapshot.run,
        "run metadata",
      )?;
      Ok(())
    })();

    if let Err(error) = write_result {
      if using_staging_directory {
        let _ = fs::remove_dir_all(&write_directory);
      } else {
        cleanup_run_record_files(&write_directory);
      }
      return Err(error);
    }

    if using_staging_directory {
      if run_directory.exists() {
        let _ = fs::remove_dir_all(&write_directory);
        return Err(format!(
          "run directory {} already exists",
          run_directory.display()
        ));
      }

      fs::rename(&write_directory, &run_directory).map_err(|error| {
        let _ = fs::remove_dir_all(&write_directory);
        format!(
          "failed to publish run directory {} from {}: {error}",
          run_directory.display(),
          write_directory.display()
        )
      })?;
    }

    Ok(())
  }

  pub fn replace_run_snapshot(&self, snapshot: &CanonicalRun) -> AuvResult<()> {
    let run_id = validate_run_id(snapshot.run.run_id.as_str())?;
    let run_directory = self.run_dir(run_id)?;
    if !run_directory.exists() {
      return self.write_run_snapshot(snapshot);
    }
    validate_replaceable_run_directory(&run_directory)?;
    write_snapshot_files(&run_directory, snapshot)
  }

  pub fn stage_artifact(
    &self,
    run_id: &RunId,
    index: usize,
    artifact: ProducedArtifact,
    span_id: &SpanId,
    event_id: Option<EventId>,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    self.stage_artifact_file(
      run_id,
      index,
      span_id,
      event_id,
      ArtifactFileSource {
        role: artifact.kind,
        source_path: artifact.source_path,
        preferred_name: artifact.preferred_name,
        summary: artifact.note,
      },
    )
  }

  pub fn stage_artifact_file(
    &self,
    run_id: &RunId,
    index: usize,
    span_id: &SpanId,
    event_id: Option<EventId>,
    artifact: ArtifactFileSource,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    let extension = artifact
      .source_path
      .extension()
      .and_then(|extension| extension.to_str())
      .unwrap_or("bin");
    let (record, destination) = self.plan_staged_artifact(
      run_id,
      index,
      span_id,
      event_id,
      artifact.role,
      &artifact.preferred_name,
      extension,
      artifact.summary,
    )?;

    copy_file(&artifact.source_path, &destination)?;

    Ok(record)
  }

  pub fn stage_artifact_bytes(
    &self,
    run_id: &RunId,
    index: usize,
    span_id: &SpanId,
    event_id: Option<EventId>,
    artifact: ArtifactBytesSource,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    let extension = Path::new(&artifact.preferred_name)
      .extension()
      .and_then(|extension| extension.to_str())
      .unwrap_or("bin");
    let (record, destination) = self.plan_staged_artifact(
      run_id,
      index,
      span_id,
      event_id,
      artifact.role,
      &artifact.preferred_name,
      extension,
      artifact.summary,
    )?;

    publish_bytes_to_path(&destination, &artifact.bytes)?;

    Ok(record)
  }

  fn plan_staged_artifact(
    &self,
    run_id: &RunId,
    index: usize,
    span_id: &SpanId,
    event_id: Option<EventId>,
    role: String,
    preferred_name: &str,
    extension: &str,
    summary: Option<String>,
  ) -> AuvResult<(ArtifactRecordV1Alpha1, PathBuf)> {
    let artifact_id = ArtifactId::new(format!("artifact_{:04}", index + 1));
    let base_name =
      sanitized_artifact_name(preferred_name.trim_end_matches(&format!(".{extension}")));
    let relative_path =
      PathBuf::from("artifacts").join(format!("{}_{base_name}.{extension}", artifact_id.as_str()));
    let destination = self.run_dir(run_id)?.join(&relative_path);

    Ok((
      ArtifactRecordV1Alpha1 {
        api_version: ARTIFACT_API_VERSION.to_string(),
        artifact_id,
        span_id: span_id.clone(),
        event_id,
        role,
        mime_type: mime_type_for_extension(extension).to_string(),
        path: relative_path.to_string_lossy().into_owned(),
        sha256: None,
        attributes: Default::default(),
        summary,
      },
      destination,
    ))
  }

  pub fn read_run(&self, run_id: &str) -> AuvResult<CanonicalRun> {
    let run_directory = self.run_dir(run_id)?;
    let run: RunRecordV1Alpha1 = read_versioned_json(
      &run_directory.join("run.json"),
      RUN_API_VERSION,
      "run metadata",
    )?;
    let spans: Vec<SpanRecordV1Alpha1> = read_versioned_jsonl(
      &run_directory.join("spans.jsonl"),
      SPAN_API_VERSION,
      "span records",
    )?;
    let events: Vec<EventRecordV1Alpha1> = read_versioned_jsonl(
      &run_directory.join("events.jsonl"),
      EVENT_API_VERSION,
      "event records",
    )?;
    let artifacts: Vec<ArtifactRecordV1Alpha1> = read_versioned_jsonl(
      &run_directory.join("artifacts.jsonl"),
      ARTIFACT_API_VERSION,
      "artifact records",
    )?;

    Ok(CanonicalRun {
      run,
      spans,
      events,
      artifacts,
    })
  }

  pub fn list_runs(&self) -> AuvResult<Vec<RunRecordV1Alpha1>> {
    let runs_root = self.root.join("runs");
    let mut runs = Vec::new();
    for entry in fs::read_dir(&runs_root)
      .map_err(|error| format!("failed to read runs root {}: {error}", runs_root.display()))?
    {
      let entry = entry.map_err(|error| format!("failed to enumerate runs: {error}"))?;
      if !entry.path().is_dir() {
        continue;
      }
      let run_path = entry.path().join("run.json");
      if !run_path.exists() {
        continue;
      }
      let value = read_json_value(&run_path)?;
      match api_version_from_value(&value, &run_path.to_string_lossy()) {
        Ok(RUN_API_VERSION) => {
          let run: RunRecordV1Alpha1 = serde_json::from_value(value)
            .map_err(|error| format!("failed to parse {}: {error}", run_path.display()))?;
          runs.push(run);
        }
        Ok(_) | Err(_) => continue,
      }
    }
    runs.sort_by_key(|run| run.started_at_millis);
    Ok(runs)
  }

  pub fn artifact_file(
    &self,
    run_id: &str,
    artifact_id: &str,
  ) -> AuvResult<(ArtifactRecordV1Alpha1, PathBuf)> {
    self.artifact_file_scoped(run_id, artifact_id, None)
  }

  pub fn artifact_file_scoped(
    &self,
    run_id: &str,
    artifact_id: &str,
    span_id: Option<&str>,
  ) -> AuvResult<(ArtifactRecordV1Alpha1, PathBuf)> {
    let (artifact, candidate_path) = self.artifact_path(run_id, artifact_id, span_id)?;
    let run_directory = self.run_dir(run_id)?;
    let canonical_run_directory = fs::canonicalize(&run_directory).map_err(|error| {
      format!(
        "failed to resolve run directory {}: {error}",
        run_directory.display()
      )
    })?;
    let canonical_artifact_path = fs::canonicalize(&candidate_path).map_err(|error| {
      format!(
        "failed to resolve artifact file {}: {error}",
        candidate_path.display()
      )
    })?;
    if !canonical_artifact_path.starts_with(&canonical_run_directory) {
      return Err(format!(
        "artifact path {} escapes run directory {}",
        canonical_artifact_path.display(),
        canonical_run_directory.display()
      ));
    }
    Ok((artifact, canonical_artifact_path))
  }

  pub fn write_artifact_bytes(
    &self,
    run_id: &str,
    artifact_id: &str,
    bytes: &[u8],
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    self.write_artifact_bytes_scoped(run_id, artifact_id, None, bytes)
  }

  pub fn write_artifact_bytes_scoped(
    &self,
    run_id: &str,
    artifact_id: &str,
    span_id: Option<&str>,
    bytes: &[u8],
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    let (artifact, candidate_path) = self.artifact_path(run_id, artifact_id, span_id)?;
    let run_directory = self.run_dir(run_id)?;
    let canonical_run_directory = fs::canonicalize(&run_directory).map_err(|error| {
      format!(
        "failed to resolve run directory {}: {error}",
        run_directory.display()
      )
    })?;
    let parent = candidate_path
      .parent()
      .ok_or_else(|| format!("invalid artifact path {:?} in run {run_id}", artifact.path))?;
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create artifact directory {}: {error}",
        parent.display()
      )
    })?;
    let canonical_parent = fs::canonicalize(parent).map_err(|error| {
      format!(
        "failed to resolve artifact directory {}: {error}",
        parent.display()
      )
    })?;
    if !canonical_parent.starts_with(&canonical_run_directory) {
      return Err(format!(
        "artifact directory {} escapes run directory {}",
        canonical_parent.display(),
        canonical_run_directory.display()
      ));
    }
    if let Ok(metadata) = fs::symlink_metadata(&candidate_path)
      && metadata.file_type().is_symlink()
    {
      return Err(format!(
        "refusing to overwrite symlink artifact path {}",
        candidate_path.display()
      ));
    }

    let file_name = candidate_path
      .file_name()
      .and_then(|file_name| file_name.to_str())
      .unwrap_or("artifact");
    let temp_path = parent.join(format!(".{file_name}.upload-{}.tmp", now_millis()));
    fs::write(&temp_path, bytes).map_err(|error| {
      format!(
        "failed to write artifact upload {}: {error}",
        temp_path.display()
      )
    })?;
    fs::rename(&temp_path, &candidate_path).map_err(|error| {
      let _ = fs::remove_file(&temp_path);
      format!(
        "failed to publish artifact upload {}: {error}",
        candidate_path.display()
      )
    })?;
    Ok(artifact)
  }

  fn artifact_path(
    &self,
    run_id: &str,
    artifact_id: &str,
    span_id: Option<&str>,
  ) -> AuvResult<(ArtifactRecordV1Alpha1, PathBuf)> {
    let canonical = self.read_run(run_id)?;
    let matches = canonical
      .artifacts
      .into_iter()
      .filter(|artifact| artifact.artifact_id.as_str() == artifact_id)
      .collect::<Vec<_>>();
    let artifact = match span_id {
      Some(span_id) => matches
        .into_iter()
        .find(|artifact| artifact.span_id.as_str() == span_id)
        .ok_or_else(|| {
          format!("artifact {artifact_id} with span_id {span_id} not found in run {run_id}")
        })?,
      None => match matches.as_slice() {
        [] => return Err(format!("artifact {artifact_id} not found in run {run_id}")),
        [artifact] => artifact.clone(),
        _ => {
          return Err(format!(
            "artifact {artifact_id} is ambiguous in run {run_id}; specify span_id"
          ));
        }
      },
    };
    let artifact_path = artifact.path.clone();
    let relative_path = Path::new(&artifact_path);
    if relative_path.is_absolute()
      || relative_path
        .components()
        .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
      return Err(format!(
        "invalid artifact path {:?} in run {run_id}",
        artifact.path
      ));
    }
    let run_directory = self.run_dir(run_id)?;
    Ok((artifact, run_directory.join(relative_path)))
  }
}

fn validate_run_id(run_id: &str) -> AuvResult<&str> {
  if run_id.is_empty() || run_id == "." || run_id == ".." {
    return Err(format!(
      "invalid run id {run_id:?}: expected a safe path component"
    ));
  }
  if !run_id
    .bytes()
    .all(|byte| matches!(byte, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.'))
  {
    return Err(format!(
      "invalid run id {run_id:?}: expected a safe path component"
    ));
  }
  let path = Path::new(run_id);
  if path.components().count() != 1
    || path.file_name().and_then(|name| name.to_str()) != Some(run_id)
  {
    return Err(format!(
      "invalid run id {run_id:?}: expected a safe path component"
    ));
  }
  Ok(run_id)
}

fn validate_unpublished_run_directory(run_directory: &Path) -> AuvResult<()> {
  if run_directory.join("run.json").exists() {
    return Err(format!(
      "run directory {} already exists",
      run_directory.display()
    ));
  }
  for file_name in ["spans.jsonl", "events.jsonl", "artifacts.jsonl"] {
    if run_directory.join(file_name).exists() {
      return Err(format!(
        "run directory {} contains incomplete canonical records",
        run_directory.display()
      ));
    }
  }

  let artifacts_directory = run_directory.join("artifacts");
  if !artifacts_directory.is_dir() {
    return Err(format!(
      "run directory {} already exists without staged artifacts",
      run_directory.display()
    ));
  }

  for entry in fs::read_dir(run_directory).map_err(|error| {
    format!(
      "failed to read run directory {}: {error}",
      run_directory.display()
    )
  })? {
    let entry = entry.map_err(|error| {
      format!(
        "failed to enumerate run directory {}: {error}",
        run_directory.display()
      )
    })?;
    if entry.file_name() != "artifacts" {
      return Err(format!(
        "run directory {} already contains non-artifact data",
        run_directory.display()
      ));
    }
  }

  Ok(())
}

fn validate_replaceable_run_directory(run_directory: &Path) -> AuvResult<()> {
  if run_directory.join("run.json").exists() {
    return Ok(());
  }
  validate_unpublished_run_directory(run_directory)
}

fn cleanup_run_record_files(run_directory: &Path) {
  for file_name in ["run.json", "spans.jsonl", "events.jsonl", "artifacts.jsonl"] {
    let path = run_directory.join(file_name);
    let _ = fs::remove_file(&path);
    let _ = fs::remove_file(path.with_extension("tmp"));
  }
}

fn write_snapshot_files(run_directory: &Path, snapshot: &CanonicalRun) -> AuvResult<()> {
  fs::create_dir_all(run_directory.join("artifacts")).map_err(|error| {
    format!(
      "failed to create canonical run directory {}: {error}",
      run_directory.display()
    )
  })?;
  let write_result = (|| {
    write_jsonl_atomic(
      &run_directory.join("spans.jsonl"),
      &snapshot.spans,
      "span records",
    )?;
    write_jsonl_atomic(
      &run_directory.join("events.jsonl"),
      &snapshot.events,
      "event records",
    )?;
    write_jsonl_atomic(
      &run_directory.join("artifacts.jsonl"),
      &snapshot.artifacts,
      "artifact records",
    )?;
    write_json_atomic(
      &run_directory.join("run.json"),
      &snapshot.run,
      "run metadata",
    )?;
    Ok(())
  })();

  if let Err(error) = write_result {
    cleanup_run_record_files(run_directory);
    return Err(error);
  }
  Ok(())
}

fn write_json_atomic<T: serde::Serialize>(path: &Path, value: &T, label: &str) -> AuvResult<()> {
  let tmp = path.with_extension("tmp");
  let bytes = serde_json::to_vec_pretty(value)
    .map_err(|error| format!("failed to encode {label} {}: {error}", path.display()))?;
  fs::write(&tmp, bytes)
    .map_err(|error| format!("failed to write {label} {}: {error}", tmp.display()))?;
  fs::rename(&tmp, path)
    .map_err(|error| format!("failed to publish {label} {}: {error}", path.display()))
}

fn write_jsonl_atomic<T: serde::Serialize>(
  path: &Path,
  values: &[T],
  label: &str,
) -> AuvResult<()> {
  let tmp = path.with_extension("tmp");
  let mut file = fs::File::create(&tmp)
    .map_err(|error| format!("failed to create {label} {}: {error}", tmp.display()))?;
  for value in values {
    serde_json::to_writer(&mut file, value)
      .map_err(|error| format!("failed to encode {label} {}: {error}", tmp.display()))?;
    file
      .write_all(b"\n")
      .map_err(|error| format!("failed to write {label} {}: {error}", tmp.display()))?;
  }
  drop(file);
  fs::rename(&tmp, path)
    .map_err(|error| format!("failed to publish {label} {}: {error}", path.display()))
}

fn read_json_value(path: &Path) -> AuvResult<serde_json::Value> {
  let raw = fs::read_to_string(path)
    .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  serde_json::from_str(&raw).map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn read_versioned_json<T: serde::de::DeserializeOwned>(
  path: &Path,
  expected_api_version: &str,
  label: &str,
) -> AuvResult<T> {
  let value = read_json_value(path)?;
  require_api_version(
    &value,
    expected_api_version,
    &format!("{label} {}", path.display()),
  )?;
  serde_json::from_value(value)
    .map_err(|error| format!("failed to parse {label} {}: {error}", path.display()))
}

fn read_versioned_jsonl<T: serde::de::DeserializeOwned>(
  path: &Path,
  expected_api_version: &str,
  label: &str,
) -> AuvResult<Vec<T>> {
  let raw = fs::read_to_string(path)
    .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  let mut records = Vec::new();
  for (index, line) in raw.lines().enumerate() {
    if line.trim().is_empty() {
      continue;
    }
    let value: serde_json::Value = serde_json::from_str(line).map_err(|error| {
      format!(
        "failed to parse {} line {}: {error}",
        path.display(),
        index + 1
      )
    })?;
    require_api_version(
      &value,
      expected_api_version,
      &format!("{label} {} line {}", path.display(), index + 1),
    )?;
    let record = serde_json::from_value(value).map_err(|error| {
      format!(
        "failed to parse {} line {}: {error}",
        path.display(),
        index + 1
      )
    })?;
    records.push(record);
  }
  Ok(records)
}

fn require_api_version(
  value: &serde_json::Value,
  expected_api_version: &str,
  label: &str,
) -> AuvResult<()> {
  let api_version = api_version_from_value(value, label)?;
  if api_version != expected_api_version {
    return Err(format!(
      "unsupported_run_format: expected {expected_api_version}, found {api_version}"
    ));
  }
  Ok(())
}

fn api_version_from_value<'a>(value: &'a serde_json::Value, label: &str) -> AuvResult<&'a str> {
  value
    .get("api_version")
    .and_then(|api_version| api_version.as_str())
    .ok_or_else(|| format!("invalid_run_format: missing api_version in {label}"))
}

fn mime_type_for_extension(extension: &str) -> &'static str {
  match extension {
    "json" => "application/json",
    "png" => "image/png",
    "txt" | "log" | "md" => "text/plain",
    _ => "application/octet-stream",
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::trace::{
    ArtifactRecordV1Alpha1, EventRecordV1Alpha1, RUN_API_VERSION, RunRecordV1Alpha1, RunType,
    SpanRecordV1Alpha1, TraceState, TraceStatusCode,
  };
  use std::collections::BTreeMap;
  use std::env;

  #[test]
  fn local_store_persists_canonical_run_files() {
    let root = temp_dir("store-canonical");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    let run = dummy_run("run_store_test");
    let span = dummy_span(&run.root_span_id);
    let event = dummy_event(&span.span_id);
    let artifact = dummy_artifact(&span.span_id);

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: vec![event],
        artifacts: vec![artifact],
      })
      .expect("should persist canonical run");

    let run_dir = root.join("runs").join("run_store_test");
    assert!(run_dir.join("run.json").exists());
    assert!(run_dir.join("spans.jsonl").exists());
    assert!(run_dir.join("events.jsonl").exists());
    assert!(run_dir.join("artifacts.jsonl").exists());
    assert!(!run_dir.join("inspect.txt").exists());
    assert!(!run_dir.join("meta.txt").exists());

    let loaded = store.read_run("run_store_test").expect("run should read");
    assert_eq!(loaded.run.api_version, RUN_API_VERSION);
    assert_eq!(loaded.spans.len(), 1);
    assert_eq!(loaded.events.len(), 1);
    assert_eq!(loaded.artifacts.len(), 1);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_ignores_directories_without_run_json() {
    let root = temp_dir("store-list");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    fs::create_dir_all(root.join("runs").join("old_run_without_run_json")).expect("old run dir");

    let runs = store.list_runs().expect("runs should list");
    assert!(runs.is_empty());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_rejects_unsafe_run_ids() {
    let root = temp_dir("store-unsafe-run-id");
    let store = LocalStore::new(root.clone()).expect("should initialize");

    for run_id in ["", ".", "..", "../escape", "nested/run", "nested\\run"] {
      let error = store
        .run_dir(run_id)
        .expect_err("run id should be rejected");
      assert!(error.contains("invalid run id"));
    }

    assert!(store.run_dir("run_SAFE-1.2").is_ok());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_ignores_incomplete_staging_directories() {
    let root = temp_dir("store-incomplete-staging");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    let staging_dir = root.join("runs").join(".run_store_test-tmp-100");
    fs::create_dir_all(&staging_dir).expect("staging dir");
    fs::write(staging_dir.join("spans.jsonl"), "").expect("staging span file");

    let runs = store.list_runs().expect("runs should list");
    assert!(runs.is_empty());

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_persists_run_after_staging_artifact() {
    let root = temp_dir("store-staged-artifact");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    let run = dummy_run("run_staged_artifact");
    let span = dummy_span(&run.root_span_id);
    let source_path = root.join("source-output.txt");
    fs::write(&source_path, "artifact body").expect("source artifact");

    let artifact = store
      .stage_artifact_file(
        &run.run_id,
        0,
        &span.span_id,
        None,
        ArtifactFileSource {
          role: "driver.output".to_string(),
          source_path: source_path.clone(),
          preferred_name: "output.txt".to_string(),
          summary: Some("output".to_string()),
        },
      )
      .expect("artifact should stage");

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts: vec![artifact.clone()],
      })
      .expect("run should persist after artifact staging");

    let artifact_path = root
      .join("runs")
      .join("run_staged_artifact")
      .join(&artifact.path);
    assert_eq!(
      fs::read_to_string(&artifact_path).expect("staged artifact should remain"),
      "artifact body"
    );
    assert!(source_path.exists());

    let loaded = store
      .read_run("run_staged_artifact")
      .expect("persisted run should read");
    assert_eq!(loaded.artifacts.len(), 1);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_stages_artifact_bytes_without_external_source_file() {
    let root = temp_dir("store-staged-artifact-bytes");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    let run = dummy_run("run_staged_artifact_bytes");
    let span = dummy_span(&run.root_span_id);
    let body = br#"{"hello":"world"}"#;

    let artifact = store
      .stage_artifact_bytes(
        &run.run_id,
        0,
        &span.span_id,
        None,
        ArtifactBytesSource {
          role: "driver.output".to_string(),
          bytes: body.to_vec(),
          preferred_name: "output.json".to_string(),
          summary: Some("output".to_string()),
        },
      )
      .expect("artifact should stage");

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts: vec![artifact.clone()],
      })
      .expect("run should persist after artifact staging");

    let artifact_path = root
      .join("runs")
      .join("run_staged_artifact_bytes")
      .join(&artifact.path);
    assert_eq!(
      fs::read(&artifact_path).expect("staged artifact should remain"),
      body
    );
    assert_eq!(artifact.role, "driver.output");
    assert_eq!(artifact.mime_type, "application/json");

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_stages_view_memory_artifact_role() {
    use auv_view::ViewBounds;
    use auv_view::memory::{
      VIEW_MEMORY_ARTIFACT_ROLE, VIEW_MEMORY_SCHEMA_VERSION, ViewMemory, ViewMemoryScopeSnapshot,
      serialize_memory_bytes,
    };

    let root = temp_dir("store-view-memory-role");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    let run = dummy_run("run_view_memory_role");
    let span = dummy_span(&run.root_span_id);
    let memory = ViewMemory {
      schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
      memory_id: "com.example.app:playlist_sidebar".into(),
      app_bundle_id: "com.example.app".into(),
      scope_id: "playlist_sidebar".into(),
      last_reconstructed_at_millis: 1,
      source_run_id: run.run_id.as_str().to_string(),
      source_reconstruction_ref: "run_id=run_view_memory_role artifact_id=artifact_0001".into(),
      anchors: Vec::new(),
      landmarks: Vec::new(),
      node_snapshots: Default::default(),
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: "playlist_sidebar".into(),
        region_bounds_window_local: ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        baseline_width: 240,
        schema_version_view_ir: "view-ir-v0".into(),
      },
      diagnostics: Vec::new(),
    };
    let bytes = serialize_memory_bytes(&memory).expect("serialize view memory");

    let artifact = store
      .stage_artifact_bytes(
        &run.run_id,
        0,
        &span.span_id,
        None,
        ArtifactBytesSource {
          role: VIEW_MEMORY_ARTIFACT_ROLE.to_string(),
          bytes,
          preferred_name: "view-memory-playlist_sidebar.json".to_string(),
          summary: Some("view memory".to_string()),
        },
      )
      .expect("artifact should stage");

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts: vec![artifact.clone()],
      })
      .expect("run should persist after artifact staging");

    let loaded = store
      .read_run("run_view_memory_role")
      .expect("persisted run should read");
    assert_eq!(loaded.artifacts.len(), 1);
    assert_eq!(loaded.artifacts[0].role, VIEW_MEMORY_ARTIFACT_ROLE);

    let artifact_path = root
      .join("runs")
      .join("run_view_memory_role")
      .join(&artifact.path);
    let decoded: ViewMemory =
      serde_json::from_slice(&fs::read(&artifact_path).expect("read artifact")).expect("decode");
    assert_eq!(decoded.memory_id, memory.memory_id);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_requires_span_id_for_duplicate_artifact_ids() {
    let root = temp_dir("store-duplicate-artifact-id");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    let run = dummy_run("run_duplicate_artifact_id");
    let span_a = dummy_span(&run.root_span_id);
    let span_b_id = SpanId::new("0000000000000002");
    let span_b = dummy_span(&span_b_id);
    let artifact_a = dummy_artifact(&span_a.span_id);
    let artifact_b = ArtifactRecordV1Alpha1 {
      span_id: span_b.span_id.clone(),
      path: "artifacts/artifact_0001_other-output.txt".to_string(),
      ..dummy_artifact(&span_b.span_id)
    };

    store
      .write_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span_a.clone(), span_b.clone()],
        events: vec![dummy_event(&span_a.span_id), dummy_event(&span_b.span_id)],
        artifacts: vec![artifact_a.clone(), artifact_b.clone()],
      })
      .expect("run should persist");
    let run_dir = root
      .join("runs")
      .join("run_duplicate_artifact_id")
      .join("artifacts");
    fs::create_dir_all(&run_dir).expect("artifact dir should create");
    fs::write(run_dir.join("artifact_0001_output.txt"), "first output")
      .expect("first artifact should write");
    fs::write(
      run_dir.join("artifact_0001_other-output.txt"),
      "second output",
    )
    .expect("second artifact should write");

    let error = store
      .artifact_file("run_duplicate_artifact_id", "artifact_0001")
      .expect_err("duplicate artifact ids should require span scoping");
    assert!(error.contains("specify span_id"));

    let (resolved, _) = store
      .artifact_file_scoped(
        "run_duplicate_artifact_id",
        "artifact_0001",
        Some(span_b.span_id.as_str()),
      )
      .expect("scoped artifact lookup should succeed");
    assert_eq!(resolved.span_id, span_b.span_id);
    assert_eq!(resolved.path, artifact_b.path);

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_replaces_existing_run_snapshot_without_removing_artifacts() {
    let root = temp_dir("store-replace-run");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    let run = dummy_run("run_replace_snapshot");
    let span = dummy_span(&run.root_span_id);
    let source_path = root.join("source-output.txt");
    fs::write(&source_path, "artifact body").expect("source artifact");
    let artifact = store
      .stage_artifact_file(
        &run.run_id,
        0,
        &span.span_id,
        None,
        ArtifactFileSource {
          role: "driver.output".to_string(),
          source_path,
          preferred_name: "output.txt".to_string(),
          summary: Some("output".to_string()),
        },
      )
      .expect("artifact should stage");

    store
      .write_run_snapshot(&CanonicalRun {
        run: run.clone(),
        spans: Vec::new(),
        events: Vec::new(),
        artifacts: Vec::new(),
      })
      .expect("initial run should persist");
    store
      .replace_run_snapshot(&CanonicalRun {
        run,
        spans: vec![span],
        events: Vec::new(),
        artifacts: vec![artifact.clone()],
      })
      .expect("existing run should replace");

    let loaded = store
      .read_run("run_replace_snapshot")
      .expect("replaced run should read");
    assert_eq!(loaded.spans.len(), 1);
    assert_eq!(loaded.artifacts.len(), 1);
    assert!(
      root
        .join("runs")
        .join("run_replace_snapshot")
        .join(&artifact.path)
        .exists()
    );

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn cleanup_removes_canonical_record_temp_files() {
    let root = temp_dir("store-cleanup-temp");
    let run_dir = root.join("runs").join("run_cleanup_temp");
    fs::create_dir_all(run_dir.join("artifacts")).expect("run dir");

    for file_name in ["run.json", "spans.jsonl", "events.jsonl", "artifacts.jsonl"] {
      let path = run_dir.join(file_name);
      fs::write(&path, "final").expect("final file");
      fs::write(path.with_extension("tmp"), "temp").expect("temp file");
    }

    cleanup_run_record_files(&run_dir);

    for file_name in ["run.json", "spans.jsonl", "events.jsonl", "artifacts.jsonl"] {
      let path = run_dir.join(file_name);
      assert!(!path.exists());
      assert!(!path.with_extension("tmp").exists());
    }
    assert!(run_dir.join("artifacts").exists());

    let _ = fs::remove_dir_all(root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  fn dummy_run(run_id: &str) -> RunRecordV1Alpha1 {
    let root_span_id = SpanId::new("0000000000000001");
    RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: RunId::new(run_id),
      trace_id: crate::trace::TraceId::new("00000000000000000000000000000001"),
      run_type: RunType::Command,
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(200),
      root_span_id,
      attributes: BTreeMap::new(),
      summary: Some("ok".to_string()),
      failure: None,
    }
  }

  fn dummy_span(span_id: &SpanId) -> SpanRecordV1Alpha1 {
    SpanRecordV1Alpha1 {
      api_version: SPAN_API_VERSION.to_string(),
      span_id: span_id.clone(),
      parent_span_id: None,
      name: "auv.command".to_string(),
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(200),
      attributes: BTreeMap::new(),
      summary: Some("ok".to_string()),
      failure: None,
    }
  }

  fn dummy_event(span_id: &SpanId) -> EventRecordV1Alpha1 {
    EventRecordV1Alpha1 {
      api_version: EVENT_API_VERSION.to_string(),
      event_id: EventId::new("event_1"),
      span_id: span_id.clone(),
      name: "command.resolved".to_string(),
      timestamp_millis: 100,
      attributes: BTreeMap::new(),
      message: Some("resolved".to_string()),
      artifact_ids: Vec::new(),
    }
  }

  fn dummy_artifact(span_id: &SpanId) -> ArtifactRecordV1Alpha1 {
    ArtifactRecordV1Alpha1 {
      api_version: ARTIFACT_API_VERSION.to_string(),
      artifact_id: ArtifactId::new("artifact_0001"),
      span_id: span_id.clone(),
      event_id: None,
      role: "driver.output".to_string(),
      mime_type: "text/plain".to_string(),
      path: "artifacts/artifact_0001_output.txt".to_string(),
      sha256: None,
      attributes: BTreeMap::new(),
      summary: Some("output".to_string()),
    }
  }
}
