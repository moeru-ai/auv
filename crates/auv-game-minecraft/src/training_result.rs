use std::collections::BTreeSet;
use std::fs;
use std::io::{BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::training_job::{TrainingLaunchJobManifest, TrainingLaunchJobStatus};

pub type TrainingResultCollectionResult<T> = Result<T, String>;

pub const TRAINING_RESULT_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const TRAINING_RESULT_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
const JOB_ENDPOINT_ENV: &str = "AUV_MINECRAFT_TRAINING_JOB_ENDPOINT";
const JOB_TOKEN_ENV: &str = "AUV_MINECRAFT_TRAINING_JOB_TOKEN";
const JOB_STATUS_COMMAND_ENV: &str = "AUV_MINECRAFT_TRAINING_JOB_STATUS_COMMAND";
const STATUS_SNAPSHOT_FILE: &str = "job_status.json";
const RESULT_CONFIG_FILE: &str = "config.yml";
const RESULT_MODELS_DIR: &str = "nerfstudio_models";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrainingResultEnvironment {
  endpoint: Option<String>,
  token: Option<String>,
  status_command: Option<String>,
}

impl TrainingResultEnvironment {
  fn from_process() -> Self {
    Self {
      endpoint: std::env::var(JOB_ENDPOINT_ENV).ok(),
      token: std::env::var(JOB_TOKEN_ENV).ok(),
      status_command: std::env::var(JOB_STATUS_COMMAND_ENV).ok(),
    }
  }

  pub fn with_values(
    endpoint: Option<String>,
    token: Option<String>,
    status_command: Option<String>,
  ) -> Self {
    Self {
      endpoint,
      token,
      status_command,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingResultInputs {
  pub training_job_manifest_path: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingResultRequest {
  pub job_backend: String,
  pub trainer_backend: String,
  pub endpoint: String,
  pub status_command: String,
  pub status_command_explicit: bool,
  pub token_present: bool,
  pub job_token: Option<String>,
  pub job_id: String,
  pub result_dir: String,
  pub job_url: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TrainingResultProbeResponse {
  pub status: TrainingResultStatus,
  pub message: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingResultOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub runbook_path: PathBuf,
  pub manifest: TrainingResultManifest,
  pub inspect_report: TrainingResultInspectReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingResultManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub job_submission_endpoint: String,
  pub source_job_status: TrainingLaunchJobStatus,
  pub status: TrainingResultStatus,
  #[serde(default)]
  pub status_message: Option<String>,
  pub job_id: String,
  #[serde(default)]
  pub job_url: Option<String>,
  pub result_dir: String,
  pub exported_frame_count: usize,
  pub skipped_frame_count: usize,
  pub result_artifacts: Vec<TrainingResultArtifactRecord>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingResultArtifactRecord {
  pub relative_path: String,
  pub absolute_path: String,
  pub readable: bool,
  #[serde(default)]
  pub byte_size: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingResultInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub job_submission_endpoint: String,
  pub source_job_status: TrainingLaunchJobStatus,
  pub status: TrainingResultStatus,
  #[serde(default)]
  pub status_message: Option<String>,
  #[serde(default)]
  pub status_reason: Option<TrainingResultReason>,
  pub job_id: String,
  #[serde(default)]
  pub job_url: Option<String>,
  pub result_dir: String,
  pub result_dir_exists: bool,
  pub key_result_artifacts_present: bool,
  pub result_artifact_count: usize,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingResultStatus {
  Queued,
  Submitted,
  Blocked,
  Failed,
  #[default]
  Succeeded,
}

impl TrainingResultStatus {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Queued => "queued",
      Self::Submitted => "submitted",
      Self::Blocked => "blocked",
      Self::Failed => "failed",
      Self::Succeeded => "succeeded",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingResultReason {
  MissingConfiguration,
  MissingAuthentication,
  LaunchBlocked,
  RemoteStatusUnavailable,
  ProviderReportedFailed,
  ResultDirectoryMissing,
  ResultArtifactsMissing,
}

impl TrainingResultReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::MissingConfiguration => "missing_configuration",
      Self::MissingAuthentication => "missing_authentication",
      Self::LaunchBlocked => "launch_blocked",
      Self::RemoteStatusUnavailable => "remote_status_unavailable",
      Self::ProviderReportedFailed => "provider_reported_failed",
      Self::ResultDirectoryMissing => "result_directory_missing",
      Self::ResultArtifactsMissing => "result_artifacts_missing",
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct TrainingResultStatusSnapshot {
  pub status: TrainingResultStatus,
  #[serde(default)]
  pub message: Option<String>,
}

pub fn collect_3dgs_training_job_result(
  inputs: TrainingResultInputs,
) -> TrainingResultCollectionResult<TrainingResultOutput> {
  collect_3dgs_training_job_result_with_probe_and_env(
    inputs,
    default_probe_training_result,
    TrainingResultEnvironment::from_process(),
  )
}

pub fn collect_3dgs_training_job_result_with_environment(
  inputs: TrainingResultInputs,
  env: TrainingResultEnvironment,
) -> TrainingResultCollectionResult<TrainingResultOutput> {
  collect_3dgs_training_job_result_with_probe_and_env(inputs, default_probe_training_result, env)
}

fn collect_3dgs_training_job_result_with_probe_and_env<F>(
  inputs: TrainingResultInputs,
  probe: F,
  env: TrainingResultEnvironment,
) -> TrainingResultCollectionResult<TrainingResultOutput>
where
  F: FnOnce(&TrainingResultRequest) -> TrainingResultCollectionResult<TrainingResultProbeResponse>,
{
  let job_manifest = read_json_file::<TrainingLaunchJobManifest>(
    &inputs.training_job_manifest_path,
    "MC-7 D6 training job manifest",
  )?;
  let launch_blocked = job_manifest.status == TrainingLaunchJobStatus::Blocked;
  let job_id = match job_manifest.job_id.clone() {
    Some(job_id) => job_id,
    None if launch_blocked => String::new(),
    None => {
      return Err(format!(
        "MC-7 D6 training job manifest {} is missing job_id",
        inputs.training_job_manifest_path.display()
      ));
    }
  };

  let endpoint = env.endpoint;
  let token = env.token;
  let result_dir = PathBuf::from(&job_manifest.suggested_output_dir);
  let status_command_explicit = env.status_command.is_some();
  let request = TrainingResultRequest {
    job_backend: job_manifest.job_backend.clone(),
    trainer_backend: job_manifest.trainer_backend.clone(),
    endpoint: endpoint
      .clone()
      .unwrap_or_else(|| job_manifest.job_submission_endpoint.clone()),
    status_command: env.status_command.unwrap_or_else(|| {
      format!(
        "cat {}",
        shell_quote_path(&result_dir.join(STATUS_SNAPSHOT_FILE))
      )
    }),
    status_command_explicit,
    token_present: token.is_some(),
    job_token: token.clone(),
    job_id: job_id.clone(),
    result_dir: result_dir.to_string_lossy().into_owned(),
    job_url: job_manifest.job_url.clone(),
  };

  let mut warnings = BTreeSet::new();
  let mut known_limits = BTreeSet::new();
  known_limits.extend(job_manifest.known_limits.iter().cloned());
  known_limits.insert(
    "MC-9 D3 closes real provider status evidence only; it does not grade model quality, splat usefulness, or artifact fetch completeness"
      .to_string(),
  );
  known_limits.insert(
    "D7 records provider status truth; local result_dir and key artifacts are observation-only and belong to D11 fetch/completeness"
      .to_string(),
  );

  let (status, status_reason, status_message) = if launch_blocked {
    (
      TrainingResultStatus::Blocked,
      Some(TrainingResultReason::LaunchBlocked),
      None,
    )
  } else if endpoint.is_none() {
    (
      TrainingResultStatus::Blocked,
      Some(TrainingResultReason::MissingConfiguration),
      None,
    )
  } else if token.is_none() {
    (
      TrainingResultStatus::Blocked,
      Some(TrainingResultReason::MissingAuthentication),
      None,
    )
  } else {
    match probe(&request) {
      Ok(response) => {
        let reason = (response.status == TrainingResultStatus::Failed)
          .then_some(TrainingResultReason::ProviderReportedFailed);
        (response.status, reason, response.message)
      }
      Err(error) => {
        warnings.insert(error);
        (
          TrainingResultStatus::Blocked,
          Some(TrainingResultReason::RemoteStatusUnavailable),
          None,
        )
      }
    }
  };

  let result_dir_exists = result_dir.is_dir();
  let (result_artifacts, key_result_artifacts_present) = collect_result_artifacts(&result_dir);
  if status == TrainingResultStatus::Succeeded && !result_dir_exists {
    warnings.insert(
      "provider status is succeeded but local result_dir is not present yet; D11 owns fetch/normalize"
        .to_string(),
    );
    known_limits.insert(
      "local result_dir absence is not a D7 provider failure; use D11 artifact fetch for completeness"
        .to_string(),
    );
  } else if status == TrainingResultStatus::Succeeded && !key_result_artifacts_present {
    warnings.insert(
      "provider status is succeeded but key local result artifacts are not present yet; D11 owns required-artifact completeness"
        .to_string(),
    );
    known_limits.insert(
      "local key artifact absence is not a D7 provider failure; use D11 artifact fetch for completeness"
        .to_string(),
    );
  }

  let generated_at_millis = auv_tracing_driver::now_millis();
  let manifest_path = inputs
    .output_dir
    .join("minecraft-3dgs-training-result.json");
  let inspect_report_path = inputs
    .output_dir
    .join("minecraft-3dgs-training-result-inspect.json");
  let runbook_path = inputs.output_dir.join("mc7-training-result-runbook.md");

  let manifest = TrainingResultManifest {
    schema_version: TRAINING_RESULT_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    source_training_job_manifest_path: inputs
      .training_job_manifest_path
      .to_string_lossy()
      .into_owned(),
    source_training_launch_plan_path: job_manifest.source_training_launch_plan_path.clone(),
    source_training_package_manifest_path: job_manifest
      .source_training_package_manifest_path
      .clone(),
    source_scene_packet_manifest_path: job_manifest.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: job_manifest.source_bundle_manifest_paths.clone(),
    source_run_ids: job_manifest.source_run_ids.clone(),
    trainer_backend: job_manifest.trainer_backend.clone(),
    job_backend: job_manifest.job_backend.clone(),
    job_submission_endpoint: job_manifest.job_submission_endpoint.clone(),
    source_job_status: job_manifest.status,
    status,
    status_message: status_message.clone(),
    job_id: job_id.clone(),
    job_url: job_manifest.job_url.clone(),
    result_dir: result_dir.to_string_lossy().into_owned(),
    exported_frame_count: job_manifest.counts.compatibility_exported_frames,
    skipped_frame_count: job_manifest.counts.compatibility_skipped_frames,
    result_artifacts: result_artifacts.clone(),
    known_limits: known_limits.iter().cloned().collect(),
  };
  write_json(&manifest_path, &manifest, "MC-7 D7 training result JSON")?;

  let inspect_report = TrainingResultInspectReport {
    schema_version: TRAINING_RESULT_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    training_result_manifest_path: manifest_path.to_string_lossy().into_owned(),
    source_training_job_manifest_path: inputs
      .training_job_manifest_path
      .to_string_lossy()
      .into_owned(),
    source_training_launch_plan_path: job_manifest.source_training_launch_plan_path.clone(),
    source_scene_packet_manifest_path: job_manifest.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: job_manifest.source_bundle_manifest_paths.clone(),
    source_run_ids: job_manifest.source_run_ids.clone(),
    trainer_backend: job_manifest.trainer_backend.clone(),
    job_backend: job_manifest.job_backend.clone(),
    job_submission_endpoint: job_manifest.job_submission_endpoint.clone(),
    source_job_status: job_manifest.status,
    status,
    status_message,
    status_reason,
    job_id,
    job_url: job_manifest.job_url.clone(),
    result_dir: result_dir.to_string_lossy().into_owned(),
    result_dir_exists,
    key_result_artifacts_present,
    result_artifact_count: result_artifacts.len(),
    warnings: warnings.iter().cloned().collect(),
    known_limits: known_limits.iter().cloned().collect(),
  };
  if launch_blocked && inspect_report.job_id.is_empty() {
    warnings.insert(
      "launch_blocked leaves job_id empty by design; result evidence still records the blocked terminal branch"
        .to_string(),
    );
  }
  write_json(
    &inspect_report_path,
    &TrainingResultInspectReport {
      warnings: warnings.iter().cloned().collect(),
      ..inspect_report.clone()
    },
    "MC-7 D7 training result inspect JSON",
  )?;

  fs::create_dir_all(&inputs.output_dir).map_err(|error| {
    format!(
      "failed to create MC-7 D7 training result output directory {}: {error}",
      inputs.output_dir.display()
    )
  })?;
  fs::write(
    &runbook_path,
    render_runbook(&manifest, &inspect_report).as_bytes(),
  )
  .map_err(|error| {
    format!(
      "failed to write MC-7 D7 training result runbook {}: {error}",
      runbook_path.display()
    )
  })?;

  Ok(TrainingResultOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    inspect_report_path,
    runbook_path,
    manifest,
    inspect_report: TrainingResultInspectReport {
      warnings: warnings.iter().cloned().collect(),
      ..inspect_report
    },
  })
}

// NOTICE(mc7-d7-result-status-snapshot): D7 currently reads a backend-neutral
// `job_status.json` file from the trainer output directory. Replace this with a
// backend-specific status adapter only in a follow-up that keeps D7 scoped to
// result collection rather than training-quality evaluation.
fn default_probe_training_result(
  request: &TrainingResultRequest,
) -> TrainingResultCollectionResult<TrainingResultProbeResponse> {
  let status_path = PathBuf::from(&request.result_dir).join(STATUS_SNAPSHOT_FILE);
  let snapshot = if !request.status_command_explicit && status_path.is_file() {
    read_json_file::<TrainingResultStatusSnapshot>(&status_path, "MC-8 D2 remote status snapshot")?
  } else {
    run_status_command(request)?
  };
  Ok(TrainingResultProbeResponse {
    status: snapshot.status,
    message: snapshot.message,
  })
}

fn run_status_command(
  request: &TrainingResultRequest,
) -> TrainingResultCollectionResult<TrainingResultStatusSnapshot> {
  #[derive(Serialize)]
  struct StatusCommandRequest<'a> {
    job_id: &'a str,
    job_url: Option<&'a str>,
    endpoint: &'a str,
    token_present: bool,
    job_token: Option<&'a str>,
    job_backend: &'a str,
    trainer_backend: &'a str,
    result_dir: &'a str,
  }
  let stdin_payload = serde_json::to_vec(&StatusCommandRequest {
    job_id: &request.job_id,
    job_url: request.job_url.as_deref(),
    endpoint: &request.endpoint,
    token_present: request.token_present,
    job_token: request.job_token.as_deref(),
    job_backend: &request.job_backend,
    trainer_backend: &request.trainer_backend,
    result_dir: &request.result_dir,
  })
  .map_err(|error| format!("failed to serialize MC-9 D3 status command request: {error}"))?;

  let mut command = Command::new("sh");
  command
    .arg("-lc")
    .arg(&request.status_command)
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());

  let mut child = command.spawn().map_err(|error| {
    format!(
      "failed to run MC-9 D3 provider status command `{}`: {error}",
      request.status_command
    )
  })?;
  child
    .stdin
    .take()
    .ok_or_else(|| "failed to open stdin for MC-9 D3 provider status command".to_string())?
    .write_all(&stdin_payload)
    .map_err(|error| format!("failed to write MC-9 D3 status command request: {error}"))?;

  let output = child.wait_with_output().map_err(|error| {
    format!(
      "failed to wait for MC-9 D3 provider status command `{}`: {error}",
      request.status_command
    )
  })?;
  if !output.status.success() {
    return Err(format!(
      "MC-9 D3 provider status command `{}` failed with status {}: {}",
      request.status_command,
      output.status,
      String::from_utf8_lossy(&output.stderr).trim()
    ));
  }

  serde_json::from_slice::<TrainingResultStatusSnapshot>(&output.stdout).map_err(|error| {
    format!(
      "failed to parse MC-9 D3 provider status command output for job {}: {error}",
      request.job_id
    )
  })
}

fn shell_quote_path(path: &Path) -> String {
  let raw = path.to_string_lossy();
  format!("'{}'", raw.replace('"', "\\\"").replace('\'', "'\\''"))
}

fn collect_result_artifacts(result_dir: &Path) -> (Vec<TrainingResultArtifactRecord>, bool) {
  let mut artifacts = Vec::new();
  let config_path = result_dir.join(RESULT_CONFIG_FILE);
  let models_path = result_dir.join(RESULT_MODELS_DIR);
  let status_path = result_dir.join(STATUS_SNAPSHOT_FILE);
  artifacts.push(result_artifact_record(result_dir, &config_path));
  artifacts.push(result_artifact_record(result_dir, &models_path));
  if status_path.exists() {
    artifacts.push(result_artifact_record(result_dir, &status_path));
  }
  let key_result_artifacts_present =
    path_is_non_symlink_file(&config_path) && path_is_non_symlink_dir(&models_path);
  (artifacts, key_result_artifacts_present)
}

fn result_artifact_record(result_dir: &Path, path: &Path) -> TrainingResultArtifactRecord {
  let metadata = fs::symlink_metadata(path).ok();
  let relative_path = path
    .strip_prefix(result_dir)
    .map(|value| value.to_string_lossy().into_owned())
    .unwrap_or_else(|_| path.to_string_lossy().into_owned());
  let readable = metadata
    .as_ref()
    .is_some_and(|metadata| !metadata.file_type().is_symlink());
  TrainingResultArtifactRecord {
    relative_path,
    absolute_path: path.to_string_lossy().into_owned(),
    readable,
    byte_size: metadata.and_then(|metadata| {
      (!metadata.file_type().is_symlink() && metadata.is_file()).then_some(metadata.len())
    }),
  }
}

fn path_is_non_symlink_file(path: &Path) -> bool {
  fs::symlink_metadata(path)
    .map(|metadata| !metadata.file_type().is_symlink() && metadata.is_file())
    .unwrap_or(false)
}

fn path_is_non_symlink_dir(path: &Path) -> bool {
  fs::symlink_metadata(path)
    .map(|metadata| !metadata.file_type().is_symlink() && metadata.is_dir())
    .unwrap_or(false)
}

fn render_runbook(
  manifest: &TrainingResultManifest,
  inspect_report: &TrainingResultInspectReport,
) -> String {
  let mut output = String::new();
  output.push_str("# MC-9 D3 training result runbook\n\n");
  output.push_str("This slice closes real provider status evidence for D7 only. It does not grade model quality, fetch normalized artifacts, or claim required-artifact completeness.\n\n");
  output.push_str(&format!(
    "- trainer backend: `{}`\n- job backend: `{}`\n- source job status: `{}`\n- provider status: `{}`\n- endpoint: `{}`\n- result dir: `{}`\n\n",
    manifest.trainer_backend,
    manifest.job_backend,
    manifest.source_job_status.as_str(),
    manifest.status.as_str(),
    manifest.job_submission_endpoint,
    manifest.result_dir,
  ));
  if let Some(message) = &manifest.status_message {
    output.push_str(&format!("- provider status message: `{}`\n", message));
  }
  if let Some(reason) = inspect_report.status_reason {
    output.push_str(&format!("- status reason: `{}`\n", reason.as_str()));
  }
  output.push_str(&format!(
    "- local result dir exists: `{}`\n- key local result artifacts present: `{}`\n- exported frames: `{}`\n- skipped frames: `{}`\n- local result artifacts observed: `{}`\n\n",
    inspect_report.result_dir_exists,
    inspect_report.key_result_artifacts_present,
    manifest.exported_frame_count,
    manifest.skipped_frame_count,
    inspect_report.result_artifact_count
  ));
  output.push_str("Observed artifacts:\n");
  for artifact in &manifest.result_artifacts {
    output.push_str(&format!(
      "- `{}` readable={} bytes={}\n",
      artifact.relative_path,
      if artifact.readable { "true" } else { "false" },
      artifact
        .byte_size
        .map(|value| value.to_string())
        .unwrap_or_else(|| "n/a".to_string())
    ));
  }
  output.push_str("\nNotes:\n");
  output.push_str(
    "- D7 records provider/status-command truth; local result_dir and key artifacts are observation-only.\n",
  );
  output.push_str(
    "- If provider status is blocked or failed, fix the remote job contract and rerun D7.\n",
  );
  output.push_str(
    "- Artifact fetch, normalization, and required-artifact completeness belong to D11, not D7.\n",
  );
  output
}

fn write_json(
  path: &Path,
  value: &impl Serialize,
  label: &str,
) -> TrainingResultCollectionResult<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create {label} directory {}: {error}",
        parent.display()
      )
    })?;
  }
  let json = serde_json::to_string_pretty(value)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize {label}: {error}"))?;
  fs::write(path, json.as_bytes())
    .map_err(|error| format!("failed to write {label} {}: {error}", path.display()))
}

fn read_json_file<T: DeserializeOwned>(
  path: &Path,
  label: &str,
) -> TrainingResultCollectionResult<T> {
  let file = fs::File::open(path)
    .map_err(|error| format!("failed to open {label} {}: {error}", path.display()))?;
  serde_json::from_reader(BufReader::new(file))
    .map_err(|error| format!("failed to parse {label} {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::training_job::TrainingLaunchJobBlocker;
  use tempfile::TempDir;

  fn write_job_manifest_fixture(
    temp: &TempDir,
    source_job_status: TrainingLaunchJobStatus,
    with_job_id: bool,
    remote_status: Option<TrainingResultStatus>,
    create_result_dir: bool,
    include_key_artifacts: bool,
  ) -> PathBuf {
    let result_dir = temp.path().join("trainer-output/nerfstudio-splatfacto");
    if create_result_dir {
      fs::create_dir_all(&result_dir).expect("result dir");
    }
    if include_key_artifacts {
      fs::write(
        result_dir.join(RESULT_CONFIG_FILE),
        b"trainer: splatfacto\n",
      )
      .expect("config");
      fs::create_dir_all(result_dir.join(RESULT_MODELS_DIR)).expect("models dir");
    }
    if let Some(remote_status) = remote_status {
      fs::write(
        result_dir.join(STATUS_SNAPSHOT_FILE),
        serde_json::to_vec_pretty(&TrainingResultStatusSnapshot {
          status: remote_status,
          message: Some("remote status snapshot".to_string()),
        })
        .expect("status snapshot"),
      )
      .expect("write status snapshot");
    }

    let manifest_path = temp.path().join("minecraft-3dgs-training-job.json");
    let manifest = TrainingLaunchJobManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_launch_plan_path: "/tmp/launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/training-package/run.json".to_string(),
      source_training_package_inspect_report_path: "/tmp/training-package/inspect_report.json"
        .to_string(),
      source_scene_packet_manifest_path: "/tmp/scene/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle/run.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      counts: crate::training_job::TrainingLaunchJobCounts {
        frames: 2,
        images: 2,
        compatibility_exported_frames: 2,
        compatibility_skipped_frames: 0,
      },
      compatibility_view_name: "nerfstudio".to_string(),
      provider_backend: "remote-command-provider".to_string(),
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      job_backend: "remote".to_string(),
      job_submission_endpoint: "https://jobs.example.test/v1".to_string(),
      job_submission_command: "remote-submit --plan launch.json".to_string(),
      submission_recorded_at_millis: (with_job_id
        && source_job_status != TrainingLaunchJobStatus::Blocked)
        .then_some(1),
      accepted_by_provider: with_job_id && source_job_status != TrainingLaunchJobStatus::Blocked,
      training_data_dir: "/tmp/training-package/compat/nerfstudio".to_string(),
      transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
      export_report_path: "/tmp/training-package/compat/nerfstudio/export_report.json".to_string(),
      suggested_output_dir: result_dir.display().to_string(),
      launch_command: "ns-train splatfacto --data compat/nerfstudio --output-dir out".to_string(),
      status: source_job_status,
      job_id: with_job_id.then_some("job-123".to_string()),
      job_url: Some("https://jobs.example.test/jobs/job-123".to_string()),
      readiness_blocker: (source_job_status == TrainingLaunchJobStatus::Blocked)
        .then_some(TrainingLaunchJobBlocker::SubmissionFailed),
      known_limits: vec!["limit-a".to_string()],
    };
    fs::write(
      &manifest_path,
      serde_json::to_vec_pretty(&manifest).expect("serialize manifest"),
    )
    .expect("write manifest");
    manifest_path
  }

  #[test]
  fn collect_training_result_happy_path_writes_outputs() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      Some(TrainingResultStatus::Succeeded),
      true,
      true,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: None,
      },
    )
    .expect("result collection should succeed");

    assert_eq!(
      output.inspect_report.status,
      TrainingResultStatus::Succeeded
    );
    assert!(output.manifest_path.is_file());
    assert!(output.inspect_report_path.is_file());
    assert!(output.runbook_path.is_file());
    assert_eq!(output.manifest.job_id, "job-123");
  }

  #[test]
  fn collect_training_result_fails_when_job_id_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      false,
      Some(TrainingResultStatus::Succeeded),
      true,
      true,
    );

    let error = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: None,
      },
    )
    .expect_err("missing job id should hard fail");

    assert!(error.contains("missing job_id"));
  }

  #[test]
  fn collect_training_result_blocks_when_remote_status_unavailable() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      None,
      true,
      true,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: None,
      },
    )
    .expect("blocked result should still write outputs");

    assert_eq!(output.inspect_report.status, TrainingResultStatus::Blocked);
    assert_eq!(
      output.inspect_report.status_reason,
      Some(TrainingResultReason::RemoteStatusUnavailable)
    );
  }

  #[test]
  fn collect_training_result_keeps_provider_succeeded_when_key_artifacts_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      Some(TrainingResultStatus::Succeeded),
      true,
      false,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: None,
      },
    )
    .expect("provider succeeded with missing local artifacts should still write outputs");

    assert_eq!(
      output.inspect_report.status,
      TrainingResultStatus::Succeeded
    );
    assert_eq!(output.inspect_report.status_reason, None);
    assert!(!output.inspect_report.key_result_artifacts_present);
    assert!(
      output
        .inspect_report
        .warnings
        .iter()
        .any(|warning| warning.contains("key local result artifacts"))
    );
  }

  #[test]
  fn collect_training_result_keeps_provider_succeeded_when_result_dir_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      None,
      false,
      false,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: Some(
          "python3 -c \"import json,sys; json.dump({'status':'succeeded','message':'provider-done'}, sys.stdout)\""
            .to_string(),
        ),
      },
    )
    .expect("provider succeeded without local result dir should still write outputs");

    assert_eq!(
      output.inspect_report.status,
      TrainingResultStatus::Succeeded
    );
    assert_eq!(
      output.inspect_report.status_message.as_deref(),
      Some("provider-done")
    );
    assert!(!output.inspect_report.result_dir_exists);
    assert!(
      output
        .inspect_report
        .warnings
        .iter()
        .any(|warning| warning.contains("local result_dir is not present yet"))
    );
  }

  #[test]
  fn collect_training_result_explicit_command_failed_sets_provider_reported_failed() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      Some(TrainingResultStatus::Succeeded),
      true,
      true,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: Some(
          "python3 -c \"import json,sys; json.dump({'status':'failed','message':'provider-failed'}, sys.stdout)\""
            .to_string(),
        ),
      },
    )
    .expect("provider failed should still write outputs");

    assert_eq!(output.inspect_report.status, TrainingResultStatus::Failed);
    assert_eq!(
      output.inspect_report.status_reason,
      Some(TrainingResultReason::ProviderReportedFailed)
    );
    assert_eq!(
      output.inspect_report.status_message.as_deref(),
      Some("provider-failed")
    );
  }

  #[test]
  fn collect_training_result_explicit_command_submitted_not_downgraded_by_missing_result_dir() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      None,
      false,
      false,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: Some(
          "python3 -c \"import json,sys; json.dump({'status':'submitted'}, sys.stdout)\""
            .to_string(),
        ),
      },
    )
    .expect("provider submitted should still write outputs");

    assert_eq!(
      output.inspect_report.status,
      TrainingResultStatus::Submitted
    );
    assert!(!output.inspect_report.result_dir_exists);
  }

  #[test]
  fn collect_training_result_explicit_command_queued_not_downgraded_by_missing_result_dir() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      None,
      false,
      false,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: Some(
          "python3 -c \"import json,sys; json.dump({'status':'queued'}, sys.stdout)\"".to_string(),
        ),
      },
    )
    .expect("provider queued should still write outputs");

    assert_eq!(output.inspect_report.status, TrainingResultStatus::Queued);
    assert!(!output.inspect_report.result_dir_exists);
  }

  #[test]
  fn collect_training_result_explicit_command_receives_job_token_on_stdin() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      None,
      true,
      true,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret-token".to_string()),
        status_command: Some(
          "python3 -c \"import json,sys; req=json.load(sys.stdin); json.dump({'status':'succeeded','message':'job_token='+str(req.get('job_token'))}, sys.stdout)\""
            .to_string(),
        ),
      },
    )
    .expect("command with job_token on stdin should succeed");

    assert_eq!(
      output.inspect_report.status_message.as_deref(),
      Some("job_token=secret-token")
    );
  }

  #[test]
  fn collect_training_result_deserializes_legacy_result_reason_codes() {
    let parsed: TrainingResultReason = serde_json::from_str("\"result_directory_missing\"")
      .expect("legacy result_directory_missing should deserialize");
    assert_eq!(parsed, TrainingResultReason::ResultDirectoryMissing);
    let parsed: TrainingResultReason = serde_json::from_str("\"result_artifacts_missing\"")
      .expect("legacy result_artifacts_missing should deserialize");
    assert_eq!(parsed, TrainingResultReason::ResultArtifactsMissing);
  }

  #[test]
  fn collect_training_result_marks_launch_blocked_without_remote_probe() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Blocked,
      true,
      Some(TrainingResultStatus::Succeeded),
      true,
      true,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: None,
      },
    )
    .expect("blocked launch should still write outputs");

    assert_eq!(output.inspect_report.status, TrainingResultStatus::Blocked);
    assert_eq!(
      output.inspect_report.status_reason,
      Some(TrainingResultReason::LaunchBlocked)
    );
  }

  #[test]
  fn collect_training_result_marks_launch_blocked_even_when_job_id_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Blocked,
      false,
      Some(TrainingResultStatus::Succeeded),
      true,
      true,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: None,
      },
    )
    .expect("blocked launch without a submitted job id should still write outputs");

    assert_eq!(output.inspect_report.status, TrainingResultStatus::Blocked);
    assert_eq!(
      output.inspect_report.status_reason,
      Some(TrainingResultReason::LaunchBlocked)
    );
    assert!(
      output
        .inspect_report
        .warnings
        .iter()
        .any(|warning| warning.contains("job_id empty by design"))
    );
    assert_eq!(output.manifest.job_id, "");
  }

  #[test]
  fn collect_training_result_reads_status_from_explicit_command_when_snapshot_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      None,
      true,
      true,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: Some(
          "python3 -c \"import json,sys; json.dump({'status':'succeeded','message':'command-status-bridge'}, sys.stdout)\"".to_string(),
        ),
      },
    )
    .expect("command-based probe should still write outputs");

    assert_eq!(
      output.inspect_report.status,
      TrainingResultStatus::Succeeded
    );
    assert_eq!(
      output.inspect_report.status_message.as_deref(),
      Some("command-status-bridge")
    );
  }

  #[test]
  fn collect_training_result_blocks_when_explicit_status_command_output_is_malformed() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      None,
      true,
      true,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: Some("python3 -c \"import sys; sys.stdout.write('not-json')\"".to_string()),
      },
    )
    .expect("malformed command output should still write outputs");

    assert_eq!(output.inspect_report.status, TrainingResultStatus::Blocked);
    assert_eq!(
      output.inspect_report.status_reason,
      Some(TrainingResultReason::RemoteStatusUnavailable)
    );
    assert!(
      output
        .inspect_report
        .warnings
        .iter()
        .any(|warning| warning.contains("failed to parse MC-9 D3 provider status command output"))
    );
  }

  #[test]
  fn collect_training_result_explicit_command_wins_over_local_snapshot() {
    let temp = tempfile::tempdir().expect("temp dir");
    // create fixture WITH a local succeeded snapshot; explicit command returns failed
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      Some(TrainingResultStatus::Succeeded),
      true,
      true,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: Some(
          "python3 -c \"import json,sys; json.dump({'status':'failed','message':'explicit-wins'}, sys.stdout)\""
            .to_string(),
        ),
      },
    )
    .expect("explicit command should win over local snapshot");

    assert_eq!(output.inspect_report.status, TrainingResultStatus::Failed);
    assert_eq!(
      output.inspect_report.status_reason,
      Some(TrainingResultReason::ProviderReportedFailed)
    );
    assert_eq!(
      output.inspect_report.status_message.as_deref(),
      Some("explicit-wins")
    );
  }

  #[test]
  fn collect_training_result_explicit_command_receives_job_id_on_stdin() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      None,
      true,
      true,
    );

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: Some(
          "python3 -c \"import json,sys; req=json.load(sys.stdin); json.dump({'status':'succeeded','message':'job_id='+req['job_id']}, sys.stdout)\""
            .to_string(),
        ),
      },
    )
    .expect("command with stdin should succeed");

    assert_eq!(
      output.inspect_report.status,
      TrainingResultStatus::Succeeded
    );
    assert_eq!(
      output.inspect_report.status_message.as_deref(),
      Some("job_id=job-123")
    );
  }

  #[cfg(unix)]
  #[test]
  fn collect_training_result_marks_symlinked_config_unreadable() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_job_manifest_fixture(
      &temp,
      TrainingLaunchJobStatus::Submitted,
      true,
      Some(TrainingResultStatus::Succeeded),
      true,
      true,
    );
    let result_dir = temp.path().join("trainer-output/nerfstudio-splatfacto");
    let real_config = temp.path().join("real-config.yml");
    fs::write(&real_config, b"trainer: splatfacto\n").expect("real config");
    fs::remove_file(result_dir.join(RESULT_CONFIG_FILE)).expect("remove config");
    symlink(&real_config, result_dir.join(RESULT_CONFIG_FILE)).expect("symlink config");

    let output = collect_3dgs_training_job_result_with_probe_and_env(
      TrainingResultInputs {
        training_job_manifest_path: manifest_path,
        output_dir: temp.path().join("result"),
      },
      default_probe_training_result,
      TrainingResultEnvironment {
        endpoint: Some("https://jobs.example.test/v1".to_string()),
        token: Some("secret".to_string()),
        status_command: None,
      },
    )
    .expect("result collection should still write outputs");

    assert!(!output.inspect_report.key_result_artifacts_present);
    assert!(
      output
        .manifest
        .result_artifacts
        .iter()
        .any(|artifact| { artifact.relative_path == RESULT_CONFIG_FILE && !artifact.readable })
    );
  }
}
