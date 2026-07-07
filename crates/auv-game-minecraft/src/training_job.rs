use std::collections::BTreeSet;
use std::fs;
use std::io::BufReader;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::training_launch::TrainingLaunchPlanManifest;
use crate::training_package::TrainingPackageInspectReport;

pub type TrainingJobResult<T> = Result<T, String>;

pub const TRAINING_JOB_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const TRAINING_JOB_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;
const TRAINER_BACKEND: &str = "nerfstudio.splatfacto";
const JOB_BACKEND: &str = "remote";
const PROVIDER_BACKEND: &str = "remote-command-provider";
const SUBMIT_ENDPOINT_ENV: &str = "AUV_MINECRAFT_TRAINING_JOB_ENDPOINT";
const SUBMIT_TOKEN_ENV: &str = "AUV_MINECRAFT_TRAINING_JOB_TOKEN";
const SUBMIT_COMMAND_ENV: &str = "AUV_MINECRAFT_TRAINING_JOB_SUBMIT_COMMAND";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrainingJobEnvironment {
  submit_endpoint: Option<String>,
  submit_token: Option<String>,
  submit_command: Option<String>,
}

impl TrainingJobEnvironment {
  fn from_process() -> Self {
    Self {
      submit_endpoint: std::env::var(SUBMIT_ENDPOINT_ENV).ok(),
      submit_token: std::env::var(SUBMIT_TOKEN_ENV).ok(),
      submit_command: std::env::var(SUBMIT_COMMAND_ENV).ok(),
    }
  }

  pub fn with_values(submit_endpoint: Option<String>, submit_token: Option<String>, submit_command: Option<String>) -> Self {
    Self {
      submit_endpoint,
      submit_token,
      submit_command,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingLaunchJobInputs {
  pub training_launch_plan_path: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingLaunchJobRequest {
  pub provider_backend: String,
  pub job_backend: String,
  pub trainer_backend: String,
  pub endpoint: String,
  pub submit_command: String,
  pub token_present: bool,
  pub token: Option<String>,
  pub launch_command: String,
  pub training_data_dir: String,
  pub suggested_output_dir: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingLaunchJobSubmission {
  pub status: TrainingLaunchJobStatus,
  pub job_id: Option<String>,
  pub job_url: Option<String>,
  pub blocker: Option<TrainingLaunchJobBlocker>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingLaunchJobOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub runbook_path: PathBuf,
  pub manifest: TrainingLaunchJobManifest,
  pub inspect_report: TrainingLaunchJobInspectReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingLaunchJobManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_training_package_inspect_report_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: TrainingLaunchJobCounts,
  pub compatibility_view_name: String,
  #[serde(default = "default_provider_backend")]
  pub provider_backend: String,
  pub trainer_backend: String,
  pub job_backend: String,
  pub job_submission_endpoint: String,
  pub job_submission_command: String,
  #[serde(default)]
  pub submission_recorded_at_millis: Option<u64>,
  #[serde(default)]
  pub accepted_by_provider: bool,
  pub training_data_dir: String,
  #[serde(default)]
  pub transforms_path: Option<String>,
  pub export_report_path: String,
  pub suggested_output_dir: String,
  pub launch_command: String,
  #[serde(default)]
  pub status: TrainingLaunchJobStatus,
  #[serde(default)]
  pub job_id: Option<String>,
  #[serde(default)]
  pub job_url: Option<String>,
  #[serde(default)]
  pub readiness_blocker: Option<TrainingLaunchJobBlocker>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingLaunchJobCounts {
  pub frames: usize,
  pub images: usize,
  pub compatibility_exported_frames: usize,
  pub compatibility_skipped_frames: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingLaunchJobInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub training_launch_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  #[serde(default = "default_provider_backend")]
  pub provider_backend: String,
  pub job_backend: String,
  pub trainer_backend: String,
  pub job_submission_endpoint: String,
  pub job_submission_command: String,
  #[serde(default)]
  pub submission_recorded_at_millis: Option<u64>,
  #[serde(default)]
  pub accepted_by_provider: bool,
  pub status: TrainingLaunchJobStatus,
  #[serde(default)]
  pub job_id: Option<String>,
  #[serde(default)]
  pub job_url: Option<String>,
  #[serde(default)]
  pub readiness_blocker: Option<TrainingLaunchJobBlocker>,
  pub probe_command: String,
  pub probe_succeeded: bool,
  pub exported_frame_count: usize,
  pub skipped_frame_count: usize,
  pub transforms_present: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingLaunchJobStatus {
  Queued,
  Submitted,
  Blocked,
  Failed,
  Succeeded,
}

impl TrainingLaunchJobStatus {
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

impl Default for TrainingLaunchJobStatus {
  fn default() -> Self {
    Self::Queued
  }
}

fn default_provider_backend() -> String {
  PROVIDER_BACKEND.to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingLaunchJobBlocker {
  MissingConfiguration,
  MissingAuthentication,
  IncompleteLaunchPlan,
  UnsupportedBackend,
  SubmissionFailed,
}

impl TrainingLaunchJobBlocker {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::MissingConfiguration => "missing_configuration",
      Self::MissingAuthentication => "missing_authentication",
      Self::IncompleteLaunchPlan => "incomplete_launch_plan",
      Self::UnsupportedBackend => "unsupported_backend",
      Self::SubmissionFailed => "submission_failed",
    }
  }
}

pub fn launch_3dgs_training_job(inputs: TrainingLaunchJobInputs) -> TrainingJobResult<TrainingLaunchJobOutput> {
  launch_3dgs_training_job_with_submit(inputs, default_submit_job)
}

fn launch_3dgs_training_job_with_submit<F>(inputs: TrainingLaunchJobInputs, submit: F) -> TrainingJobResult<TrainingLaunchJobOutput>
where
  F: FnOnce(&TrainingLaunchJobRequest) -> TrainingLaunchJobSubmission,
{
  launch_3dgs_training_job_with_submit_and_env(inputs, submit, TrainingJobEnvironment::from_process())
}

pub fn launch_3dgs_training_job_with_environment(
  inputs: TrainingLaunchJobInputs,
  env: TrainingJobEnvironment,
) -> TrainingJobResult<TrainingLaunchJobOutput> {
  launch_3dgs_training_job_with_submit_and_env(inputs, default_submit_job, env)
}

fn launch_3dgs_training_job_with_submit_and_env<F>(
  inputs: TrainingLaunchJobInputs,
  submit: F,
  env: TrainingJobEnvironment,
) -> TrainingJobResult<TrainingLaunchJobOutput>
where
  F: FnOnce(&TrainingLaunchJobRequest) -> TrainingLaunchJobSubmission,
{
  let launch_plan = read_json_file::<TrainingLaunchPlanManifest>(&inputs.training_launch_plan_path, "MC-7 D5 training launch plan")?;
  let training_package_manifest_path = PathBuf::from(&launch_plan.source_training_package_manifest_path);
  let training_package_dir = training_package_manifest_path
    .parent()
    .ok_or_else(|| format!("MC-7 D5 training package manifest {} has no parent directory", training_package_manifest_path.display()))?;
  let training_package_inspect_report_path = PathBuf::from(&launch_plan.source_training_package_inspect_report_path);
  let training_package_inspect_report =
    read_json_file::<TrainingPackageInspectReport>(&training_package_inspect_report_path, "MC-7 D5 training package inspect report")?;

  let submit_endpoint = env.submit_endpoint;
  let submit_token = env.submit_token;
  let submit_command = env
    .submit_command
    .unwrap_or_else(|| format!("remote-submit --endpoint <configured> --plan {}", sh_quote(&inputs.training_launch_plan_path)));

  let training_data_dir = training_package_dir.join("compat/nerfstudio");
  let transforms_path = launch_plan.transforms_path.as_ref().map(|path| training_package_dir.join(path));
  let export_report_path = training_data_dir.join("export_report.json");
  let job_submission_endpoint = submit_endpoint.clone().unwrap_or_else(|| "unconfigured".to_string());

  let mut warnings = BTreeSet::new();
  warnings.extend(launch_plan.known_limits.iter().cloned());
  warnings.extend(training_package_inspect_report.warnings.iter().cloned());

  let mut known_limits = BTreeSet::new();
  known_limits.extend(launch_plan.known_limits.iter().cloned());
  known_limits.extend(training_package_inspect_report.known_limits.iter().cloned());
  known_limits
    .insert("MC-7 D6 is a remote job envelope only; it does not execute training locally or consume trained splat outputs".to_string());
  known_limits.insert(
    "MC-9 D1 binds this training job lane to one provider contract; multi-provider expansion is deferred by owner approval".to_string(),
  );

  let blocker = if launch_plan.compatibility_view_name != "nerfstudio" {
    Some(TrainingLaunchJobBlocker::UnsupportedBackend)
  } else if launch_plan.source_run_ids.is_empty()
    || launch_plan.source_scene_packet_manifest_path.is_empty()
    || launch_plan.source_training_package_manifest_path.is_empty()
  {
    Some(TrainingLaunchJobBlocker::IncompleteLaunchPlan)
  } else if submit_endpoint.is_none() {
    Some(TrainingLaunchJobBlocker::MissingConfiguration)
  } else if submit_token.is_none() {
    Some(TrainingLaunchJobBlocker::MissingAuthentication)
  } else if !export_report_path.is_file() || transforms_path.as_ref().is_some_and(|path| !path.is_file()) {
    Some(TrainingLaunchJobBlocker::IncompleteLaunchPlan)
  } else {
    None
  };

  let request = TrainingLaunchJobRequest {
    provider_backend: PROVIDER_BACKEND.to_string(),
    job_backend: JOB_BACKEND.to_string(),
    trainer_backend: TRAINER_BACKEND.to_string(),
    endpoint: job_submission_endpoint.clone(),
    submit_command: submit_command.clone(),
    token_present: submit_token.is_some(),
    token: submit_token.clone(),
    launch_command: launch_plan.launch_command.clone(),
    training_data_dir: training_data_dir.to_string_lossy().into_owned(),
    suggested_output_dir: launch_plan.suggested_output_dir.clone(),
  };
  let submission = blocker
    .map(|blocker| TrainingLaunchJobSubmission {
      status: TrainingLaunchJobStatus::Blocked,
      job_id: None,
      job_url: None,
      blocker: Some(blocker),
    })
    .unwrap_or_else(|| submit(&request));

  let generated_at_millis = auv_tracing_driver::now_millis();
  let accepted_by_provider = submission.status != TrainingLaunchJobStatus::Blocked && submission.job_id.is_some();
  let submission_recorded_at_millis = accepted_by_provider.then_some(generated_at_millis);
  let manifest_path = inputs.output_dir.join("minecraft-3dgs-training-job.json");
  let inspect_report_path = inputs.output_dir.join("minecraft-3dgs-training-job-inspect.json");
  let runbook_path = inputs.output_dir.join("mc7-training-job-runbook.md");

  let manifest = TrainingLaunchJobManifest {
    schema_version: TRAINING_JOB_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    source_training_launch_plan_path: inputs.training_launch_plan_path.to_string_lossy().into_owned(),
    source_training_package_manifest_path: launch_plan.source_training_package_manifest_path.clone(),
    source_training_package_inspect_report_path: launch_plan.source_training_package_inspect_report_path.clone(),
    source_scene_packet_manifest_path: launch_plan.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: launch_plan.source_bundle_manifest_paths.clone(),
    source_run_ids: launch_plan.source_run_ids.clone(),
    counts: TrainingLaunchJobCounts {
      frames: launch_plan.counts.frames,
      images: launch_plan.counts.images,
      compatibility_exported_frames: launch_plan.counts.compatibility_exported_frames,
      compatibility_skipped_frames: launch_plan.counts.compatibility_skipped_frames,
    },
    compatibility_view_name: launch_plan.compatibility_view_name.clone(),
    provider_backend: PROVIDER_BACKEND.to_string(),
    trainer_backend: TRAINER_BACKEND.to_string(),
    job_backend: JOB_BACKEND.to_string(),
    job_submission_endpoint: job_submission_endpoint.clone(),
    job_submission_command: submit_command.clone(),
    submission_recorded_at_millis,
    accepted_by_provider,
    training_data_dir: training_data_dir.to_string_lossy().into_owned(),
    transforms_path: launch_plan.transforms_path.clone(),
    export_report_path: export_report_path.to_string_lossy().into_owned(),
    suggested_output_dir: launch_plan.suggested_output_dir.clone(),
    launch_command: launch_plan.launch_command.clone(),
    status: submission.status,
    job_id: submission.job_id.clone(),
    job_url: submission.job_url.clone(),
    readiness_blocker: submission.blocker,
    known_limits: known_limits.iter().cloned().collect(),
  };
  write_json(&manifest_path, &manifest, "MC-7 D6 training job JSON")?;

  let inspect_report = TrainingLaunchJobInspectReport {
    schema_version: TRAINING_JOB_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    training_launch_manifest_path: manifest_path.to_string_lossy().into_owned(),
    source_training_launch_plan_path: inputs.training_launch_plan_path.to_string_lossy().into_owned(),
    source_training_package_manifest_path: launch_plan.source_training_package_manifest_path.clone(),
    source_scene_packet_manifest_path: launch_plan.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: launch_plan.source_bundle_manifest_paths.clone(),
    source_run_ids: launch_plan.source_run_ids.clone(),
    provider_backend: PROVIDER_BACKEND.to_string(),
    job_backend: JOB_BACKEND.to_string(),
    trainer_backend: TRAINER_BACKEND.to_string(),
    job_submission_endpoint: job_submission_endpoint.clone(),
    job_submission_command: submit_command.clone(),
    submission_recorded_at_millis,
    accepted_by_provider,
    status: submission.status,
    job_id: submission.job_id,
    job_url: submission.job_url,
    readiness_blocker: submission.blocker,
    probe_command: "ns-train --help".to_string(),
    probe_succeeded: Command::new("ns-train").arg("--help").status().map(|status| status.success()).unwrap_or(false),
    exported_frame_count: launch_plan.counts.compatibility_exported_frames,
    skipped_frame_count: launch_plan.counts.compatibility_skipped_frames,
    transforms_present: transforms_path.is_some(),
    warnings: warnings.iter().cloned().collect(),
    known_limits: known_limits.iter().cloned().collect(),
  };
  write_json(&inspect_report_path, &inspect_report, "MC-7 D6 training job inspect JSON")?;

  fs::create_dir_all(&inputs.output_dir)
    .map_err(|error| format!("failed to create MC-7 D6 training job output directory {}: {error}", inputs.output_dir.display()))?;
  fs::write(&runbook_path, render_runbook(&manifest, &inspect_report).as_bytes())
    .map_err(|error| format!("failed to write MC-7 D6 training job runbook {}: {error}", runbook_path.display()))?;

  Ok(TrainingLaunchJobOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    inspect_report_path,
    runbook_path,
    manifest,
    inspect_report,
  })
}

fn default_submit_job(request: &TrainingLaunchJobRequest) -> TrainingLaunchJobSubmission {
  run_submit_command(request).unwrap_or_else(|error| TrainingLaunchJobSubmission {
    status: TrainingLaunchJobStatus::Failed,
    job_id: None,
    job_url: None,
    blocker: Some(error),
  })
}

fn run_submit_command(request: &TrainingLaunchJobRequest) -> Result<TrainingLaunchJobSubmission, TrainingLaunchJobBlocker> {
  let mut command = Command::new("sh");
  command.arg("-lc").arg(&request.submit_command).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());

  let mut child = command.spawn().map_err(|_| TrainingLaunchJobBlocker::SubmissionFailed)?;

  let stdin_payload = serde_json::to_vec(request).map_err(|_| TrainingLaunchJobBlocker::SubmissionFailed)?;
  child
    .stdin
    .take()
    .ok_or(TrainingLaunchJobBlocker::SubmissionFailed)?
    .write_all(&stdin_payload)
    .map_err(|_| TrainingLaunchJobBlocker::SubmissionFailed)?;

  let output = child.wait_with_output().map_err(|_| TrainingLaunchJobBlocker::SubmissionFailed)?;
  if !output.status.success() {
    return Err(TrainingLaunchJobBlocker::SubmissionFailed);
  }

  let mut submission: TrainingLaunchJobSubmission =
    serde_json::from_slice(&output.stdout).map_err(|_| TrainingLaunchJobBlocker::SubmissionFailed)?;
  if submission.status != TrainingLaunchJobStatus::Blocked && submission.job_id.is_none() {
    return Err(TrainingLaunchJobBlocker::SubmissionFailed);
  }
  if submission.status == TrainingLaunchJobStatus::Blocked && submission.blocker.is_none() {
    submission.blocker = Some(TrainingLaunchJobBlocker::SubmissionFailed);
  }
  Ok(submission)
}

fn render_runbook(manifest: &TrainingLaunchJobManifest, inspect_report: &TrainingLaunchJobInspectReport) -> String {
  let mut output = String::new();
  output.push_str("# MC-7 training job runbook\n\n");
  output.push_str("This is a remote job envelope only. It does not run training locally and it does not claim model quality.\n\n");
  output.push_str(&format!(
    "- provider backend: `{}`\n- trainer backend: `{}`\n- job backend: `{}`\n- status: `{}`\n- accepted by provider: `{}`\n- endpoint: `{}`\n\n",
    manifest.provider_backend,
    manifest.trainer_backend,
    manifest.job_backend,
    status_text(inspect_report.status),
    manifest.accepted_by_provider,
    manifest.job_submission_endpoint,
  ));
  if let Some(blocker) = inspect_report.readiness_blocker {
    output.push_str(&format!("- blocker: `{}`\n\n", blocker_text(blocker)));
  }
  output.push_str(&format!(
    "- exported frames: `{}`\n- skipped frames: `{}`\n- probe command: `{}`\n\n",
    inspect_report.exported_frame_count, inspect_report.skipped_frame_count, inspect_report.probe_command
  ));
  output.push_str("Launch request:\n\n```bash\n");
  output.push_str(&manifest.job_submission_command);
  output.push_str("\n```\n\n");
  output.push_str("Notes:\n");
  output.push_str("- D6 only envelopes the launch request; it does not execute training locally.\n");
  output.push_str(
    "- NOTICE: MC-9 D1 binds this path to a single provider contract; do not widen to multiple providers without a new owner-approved slice.\n",
  );
  output.push_str(
    "- D2 records whether the provider actually accepted the submit request; later slices may consume that evidence but do not change the persisted artifact roles here.\n",
  );
  output.push_str("- D7 is the first slice that can consume remote training results.\n");
  output.push_str("- If blocked, fix configuration or the launch plan and rerun D6.\n");
  output
}

fn status_text(status: TrainingLaunchJobStatus) -> &'static str {
  match status {
    TrainingLaunchJobStatus::Queued => "queued",
    TrainingLaunchJobStatus::Submitted => "submitted",
    TrainingLaunchJobStatus::Blocked => "blocked",
    TrainingLaunchJobStatus::Failed => "failed",
    TrainingLaunchJobStatus::Succeeded => "succeeded",
  }
}

fn blocker_text(blocker: TrainingLaunchJobBlocker) -> &'static str {
  match blocker {
    TrainingLaunchJobBlocker::MissingConfiguration => "missing_configuration",
    TrainingLaunchJobBlocker::MissingAuthentication => "missing_authentication",
    TrainingLaunchJobBlocker::IncompleteLaunchPlan => "incomplete_launch_plan",
    TrainingLaunchJobBlocker::UnsupportedBackend => "unsupported_backend",
    TrainingLaunchJobBlocker::SubmissionFailed => "submission_failed",
  }
}

fn write_json(path: &Path, value: &impl Serialize, label: &str) -> TrainingJobResult<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|error| format!("failed to create {label} directory {}: {error}", parent.display()))?;
  }
  let json = serde_json::to_string_pretty(value)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize {label}: {error}"))?;
  fs::write(path, json.as_bytes()).map_err(|error| format!("failed to write {label} {}: {error}", path.display()))
}

fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> TrainingJobResult<T> {
  let file = fs::File::open(path).map_err(|error| format!("failed to open {label} {}: {error}", path.display()))?;
  serde_json::from_reader(BufReader::new(file)).map_err(|error| format!("failed to parse {label} {}: {error}", path.display()))
}

fn sh_quote(path: &Path) -> String {
  format!("\"{}\"", path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::training_package::TrainingPackageCounts;
  use tempfile::TempDir;

  fn write_launch_plan_fixture(
    temp: &TempDir,
    compatibility_view_name: &str,
    exported_frame_count: usize,
    include_transforms: bool,
    with_config: bool,
  ) -> PathBuf {
    let package_dir = temp.path().join("training-package");
    let compat_dir = package_dir.join("compat/nerfstudio");
    fs::create_dir_all(&compat_dir).expect("compat dir");
    fs::write(
      compat_dir.join("export_report.json"),
      b"{}
",
    )
    .expect("export report");
    if include_transforms {
      fs::write(
        compat_dir.join("transforms.json"),
        b"{}
",
      )
      .expect("transforms");
    }

    let training_package_manifest_path = package_dir.join("run.json");
    let training_package_inspect_report_path = package_dir.join("inspect_report.json");
    let launch_plan_path = temp.path().join("minecraft-3dgs-training-launch-plan.json");
    let launch_plan = TrainingLaunchPlanManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_package_manifest_path: training_package_manifest_path.display().to_string(),
      source_training_package_inspect_report_path: training_package_inspect_report_path.display().to_string(),
      source_scene_packet_manifest_path: package_dir.join("scene/run.json").display().to_string(),
      source_bundle_manifest_paths: vec![package_dir.join("bundle/run.json").display().to_string()],
      source_run_ids: vec!["run-1".to_string()],
      counts: TrainingPackageCounts {
        frames: 1,
        images: 1,
        compatibility_exported_frames: exported_frame_count,
        compatibility_skipped_frames: 0,
      },
      compatibility_view_name: compatibility_view_name.to_string(),
      trainer_backend: "nerfstudio.splatfacto".to_string(),
      training_data_dir: compat_dir.display().to_string(),
      transforms_path: include_transforms.then_some("compat/nerfstudio/transforms.json".to_string()),
      export_report_path: "compat/nerfstudio/export_report.json".to_string(),
      suggested_output_dir: temp.path().join("job-output").display().to_string(),
      launch_command: "ns-train splatfacto --data compat/nerfstudio --output-dir out".to_string(),
      known_limits: vec!["limit-a".to_string()],
    };
    fs::write(&launch_plan_path, serde_json::to_vec_pretty(&launch_plan).expect("serialize plan")).expect("write plan");
    let inspect_report = TrainingPackageInspectReport {
      schema_version: 1,
      generated_at_millis: 1,
      training_package_manifest_path: launch_plan.source_training_package_manifest_path.clone(),
      scene_packet_manifest_path: launch_plan.source_scene_packet_manifest_path.clone(),
      source_bundle_manifest_paths: launch_plan.source_bundle_manifest_paths.clone(),
      source_run_ids: launch_plan.source_run_ids.clone(),
      counts: launch_plan.counts.clone(),
      compatibility_views: Vec::new(),
      warnings: vec!["warn-a".to_string()],
      known_limits: vec!["limit-b".to_string()],
    };
    fs::write(&training_package_inspect_report_path, serde_json::to_vec_pretty(&inspect_report).expect("serialize inspect"))
      .expect("write inspect");
    fs::write(
      training_package_manifest_path,
      b"{}
",
    )
    .expect("write run");
    let _ = with_config;
    launch_plan_path
  }

  #[test]
  fn job_launch_happy_path_writes_all_outputs() {
    let temp = tempfile::tempdir().expect("temp dir");
    let plan_path = write_launch_plan_fixture(&temp, "nerfstudio", 2, true, true);
    let output = launch_3dgs_training_job_with_submit_and_env(
      TrainingLaunchJobInputs {
        training_launch_plan_path: plan_path,
        output_dir: temp.path().join("job"),
      },
      |request| TrainingLaunchJobSubmission {
        status: TrainingLaunchJobStatus::Submitted,
        job_id: Some(format!("job-for-{}", request.trainer_backend)),
        job_url: Some("https://jobs.example.test/job/1".to_string()),
        blocker: None,
      },
      TrainingJobEnvironment {
        submit_endpoint: Some("https://jobs.example.test/v1".to_string()),
        submit_token: Some("secret".to_string()),
        submit_command: Some("remote-submit --dry-run".to_string()),
      },
    )
    .expect("job should launch");

    assert_eq!(output.inspect_report.status, TrainingLaunchJobStatus::Submitted);
    assert!(output.manifest_path.is_file());
    assert!(output.inspect_report_path.is_file());
    assert!(output.runbook_path.is_file());
    assert_eq!(output.inspect_report.job_id.as_deref(), Some("job-for-nerfstudio.splatfacto"));
    assert!(output.manifest.accepted_by_provider);
    assert_eq!(output.manifest.submission_recorded_at_millis, Some(output.manifest.generated_at_millis));
  }

  #[test]
  fn job_launch_blocks_when_endpoint_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let plan_path = write_launch_plan_fixture(&temp, "nerfstudio", 2, true, false);
    let output = launch_3dgs_training_job_with_submit_and_env(
      TrainingLaunchJobInputs {
        training_launch_plan_path: plan_path,
        output_dir: temp.path().join("job"),
      },
      |_request| unreachable!("blocked path should not submit"),
      TrainingJobEnvironment {
        submit_endpoint: None,
        submit_token: None,
        submit_command: None,
      },
    )
    .expect("blocked job should still write outputs");

    assert_eq!(output.inspect_report.status, TrainingLaunchJobStatus::Blocked);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchJobBlocker::MissingConfiguration));
    assert!(!output.manifest.accepted_by_provider);
    assert_eq!(output.manifest.submission_recorded_at_millis, None);
  }

  #[test]
  fn job_launch_blocks_when_backend_unsupported() {
    let temp = tempfile::tempdir().expect("temp dir");
    let plan_path = write_launch_plan_fixture(&temp, "other-view", 2, true, true);
    let output = launch_3dgs_training_job_with_submit_and_env(
      TrainingLaunchJobInputs {
        training_launch_plan_path: plan_path,
        output_dir: temp.path().join("job"),
      },
      |_request| unreachable!("blocked path should not submit"),
      TrainingJobEnvironment {
        submit_endpoint: Some("https://jobs.example.test/v1".to_string()),
        submit_token: Some("secret".to_string()),
        submit_command: Some("remote-submit --dry-run".to_string()),
      },
    )
    .expect("blocked job should still write outputs");

    assert_eq!(output.inspect_report.status, TrainingLaunchJobStatus::Blocked);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchJobBlocker::UnsupportedBackend));
  }

  #[test]
  fn job_launch_request_carries_submit_token_into_submitter() {
    let temp = tempfile::tempdir().expect("temp dir");
    let plan_path = write_launch_plan_fixture(&temp, "nerfstudio", 2, true, true);
    let output = launch_3dgs_training_job_with_submit_and_env(
      TrainingLaunchJobInputs {
        training_launch_plan_path: plan_path,
        output_dir: temp.path().join("job"),
      },
      |request| {
        assert_eq!(request.provider_backend, PROVIDER_BACKEND);
        assert_eq!(request.token.as_deref(), Some("secret-token"));
        assert!(request.token_present);
        TrainingLaunchJobSubmission {
          status: TrainingLaunchJobStatus::Submitted,
          job_id: Some("job-with-token".to_string()),
          job_url: Some("https://jobs.example.test/job/token".to_string()),
          blocker: None,
        }
      },
      TrainingJobEnvironment {
        submit_endpoint: Some("https://jobs.example.test/v1".to_string()),
        submit_token: Some("secret-token".to_string()),
        submit_command: Some("remote-submit --dry-run".to_string()),
      },
    )
    .expect("job should launch");

    assert_eq!(output.inspect_report.job_id.as_deref(), Some("job-with-token"));
    assert_eq!(output.manifest.provider_backend, PROVIDER_BACKEND);
    assert_eq!(output.inspect_report.provider_backend, PROVIDER_BACKEND);
    assert!(output.inspect_report.accepted_by_provider);
    assert_eq!(output.inspect_report.submission_recorded_at_millis, output.manifest.submission_recorded_at_millis);
  }

  #[test]
  fn default_submit_job_reads_json_submission_from_command_stdout() {
    let temp = tempfile::tempdir().expect("temp dir");
    let plan_path = write_launch_plan_fixture(&temp, "nerfstudio", 2, true, true);
    let output = launch_3dgs_training_job_with_submit_and_env(
      TrainingLaunchJobInputs {
        training_launch_plan_path: plan_path,
        output_dir: temp.path().join("job"),
      },
      default_submit_job,
      TrainingJobEnvironment {
        submit_endpoint: Some("https://jobs.example.test/v1".to_string()),
        submit_token: Some("secret-token".to_string()),
        submit_command: Some(
          "python3 -c \"import json,sys; req=json.load(sys.stdin); json.dump({'status':'submitted','job_id':'job-from-command','job_url':req['endpoint'].rstrip('/') + '/jobs/job-from-command','blocker':None}, sys.stdout)\"".to_string(),
        ),
      },
    )
      .expect("job should launch through submit command");

    assert_eq!(output.inspect_report.status, TrainingLaunchJobStatus::Submitted);
    assert_eq!(output.inspect_report.job_id.as_deref(), Some("job-from-command"));
    assert_eq!(output.inspect_report.job_url.as_deref(), Some("https://jobs.example.test/v1/jobs/job-from-command"));
    assert_eq!(output.manifest.provider_backend, PROVIDER_BACKEND);
    assert!(output.manifest.accepted_by_provider);
    assert!(output.inspect_report.submission_recorded_at_millis.is_some());
  }

  #[test]
  fn default_submit_job_fails_when_command_returns_missing_job_id() {
    let temp = tempfile::tempdir().expect("temp dir");
    let plan_path = write_launch_plan_fixture(&temp, "nerfstudio", 2, true, true);
    let output = launch_3dgs_training_job_with_submit_and_env(
      TrainingLaunchJobInputs {
        training_launch_plan_path: plan_path,
        output_dir: temp.path().join("job"),
      },
      default_submit_job,
      TrainingJobEnvironment {
        submit_endpoint: Some("https://jobs.example.test/v1".to_string()),
        submit_token: Some("secret-token".to_string()),
        submit_command: Some(
          "python3 -c \"import json,sys; json.dump({'status':'submitted','job_id':None,'job_url':'https://jobs.example.test/v1/jobs/missing','blocker':None}, sys.stdout)\"".to_string(),
        ),
      },
    )
      .expect("failed submission should still write outputs");

    assert_eq!(output.inspect_report.status, TrainingLaunchJobStatus::Failed);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchJobBlocker::SubmissionFailed));
  }

  #[test]
  fn default_submit_job_fails_when_queued_status_has_no_job_id() {
    let temp = tempfile::tempdir().expect("temp dir");
    let plan_path = write_launch_plan_fixture(&temp, "nerfstudio", 2, true, true);
    let output = launch_3dgs_training_job_with_submit_and_env(
      TrainingLaunchJobInputs {
        training_launch_plan_path: plan_path,
        output_dir: temp.path().join("job"),
      },
      default_submit_job,
      TrainingJobEnvironment {
        submit_endpoint: Some("https://jobs.example.test/v1".to_string()),
        submit_token: Some("secret-token".to_string()),
        submit_command: Some(
          "python3 -c \"import json,sys; json.dump({'status':'queued','job_id':None,'job_url':None,'blocker':None}, sys.stdout)\""
            .to_string(),
        ),
      },
    )
    .expect("failed submission should still write outputs");
    assert_eq!(output.inspect_report.status, TrainingLaunchJobStatus::Failed);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchJobBlocker::SubmissionFailed));
  }

  #[test]
  fn default_submit_job_fails_when_failed_status_has_no_job_id() {
    let temp = tempfile::tempdir().expect("temp dir");
    let plan_path = write_launch_plan_fixture(&temp, "nerfstudio", 2, true, true);
    let output = launch_3dgs_training_job_with_submit_and_env(
      TrainingLaunchJobInputs {
        training_launch_plan_path: plan_path,
        output_dir: temp.path().join("job"),
      },
      default_submit_job,
      TrainingJobEnvironment {
        submit_endpoint: Some("https://jobs.example.test/v1".to_string()),
        submit_token: Some("secret-token".to_string()),
        submit_command: Some(
          "python3 -c \"import json,sys; json.dump({'status':'failed','job_id':None,'job_url':None,'blocker':None}, sys.stdout)\""
            .to_string(),
        ),
      },
    )
    .expect("failed submission should still write outputs");
    assert_eq!(output.inspect_report.status, TrainingLaunchJobStatus::Failed);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchJobBlocker::SubmissionFailed));
    assert!(!output.inspect_report.accepted_by_provider);
    assert_eq!(output.inspect_report.submission_recorded_at_millis, None);
  }

  #[test]
  fn default_submit_job_records_provider_acceptance_for_failed_status_with_job_id() {
    let temp = tempfile::tempdir().expect("temp dir");
    let plan_path = write_launch_plan_fixture(&temp, "nerfstudio", 2, true, true);
    let output = launch_3dgs_training_job_with_submit_and_env(
      TrainingLaunchJobInputs {
        training_launch_plan_path: plan_path,
        output_dir: temp.path().join("job"),
      },
      |_request| TrainingLaunchJobSubmission {
        status: TrainingLaunchJobStatus::Failed,
        job_id: Some("job-failed".to_string()),
        job_url: Some("https://jobs.example.test/v1/jobs/job-failed".to_string()),
        blocker: None,
      },
      TrainingJobEnvironment {
        submit_endpoint: Some("https://jobs.example.test/v1".to_string()),
        submit_token: Some("secret-token".to_string()),
        submit_command: Some("remote-submit --dry-run".to_string()),
      },
    )
    .expect("failed status with job id should still write outputs");

    assert_eq!(output.inspect_report.status, TrainingLaunchJobStatus::Failed);
    assert_eq!(output.inspect_report.job_id.as_deref(), Some("job-failed"));
    assert!(output.manifest.accepted_by_provider);
    assert!(output.inspect_report.accepted_by_provider);
    assert_eq!(output.manifest.submission_recorded_at_millis, Some(output.manifest.generated_at_millis));
    assert_eq!(output.inspect_report.submission_recorded_at_millis, Some(output.inspect_report.generated_at_millis));
  }

  #[test]
  fn real_submit_manifest_records_provider_acceptance_fields() {
    let temp = tempfile::tempdir().expect("temp dir");
    let plan_path = write_launch_plan_fixture(&temp, "nerfstudio", 2, true, true);
    let output = launch_3dgs_training_job_with_submit_and_env(
      TrainingLaunchJobInputs {
        training_launch_plan_path: plan_path,
        output_dir: temp.path().join("job"),
      },
      |_request| TrainingLaunchJobSubmission {
        status: TrainingLaunchJobStatus::Submitted,
        job_id: Some("provider-job-42".to_string()),
        job_url: Some("https://provider.example/jobs/provider-job-42".to_string()),
        blocker: None,
      },
      TrainingJobEnvironment {
        submit_endpoint: Some("https://provider.example/api".to_string()),
        submit_token: Some("secret-token".to_string()),
        submit_command: Some("provider-submit --json".to_string()),
      },
    )
    .expect("real submit should write outputs");

    assert!(output.manifest.accepted_by_provider);
    assert_eq!(output.manifest.submission_recorded_at_millis, Some(output.manifest.generated_at_millis));
    assert!(output.inspect_report.accepted_by_provider);
    assert_eq!(output.inspect_report.submission_recorded_at_millis, Some(output.inspect_report.generated_at_millis));
    assert_eq!(output.manifest.job_id.as_deref(), Some("provider-job-42"));
    assert_eq!(output.inspect_report.job_url.as_deref(), Some("https://provider.example/jobs/provider-job-42"));
  }
  #[test]
  fn training_job_manifest_backfills_provider_backend_from_legacy_json() {
    let legacy_json = r#"
{
  "schema_version": 1,
  "generated_at_millis": 1,
  "source_training_launch_plan_path": "/tmp/launch.json",
  "source_training_package_manifest_path": "/tmp/package/run.json",
  "source_training_package_inspect_report_path": "/tmp/package/inspect_report.json",
  "source_scene_packet_manifest_path": "/tmp/scene/run.json",
  "source_bundle_manifest_paths": ["/tmp/bundle/run.json"],
  "source_run_ids": ["run-1"],
  "counts": {
    "frames": 1,
    "images": 1,
    "compatibility_exported_frames": 1,
    "compatibility_skipped_frames": 0
  },
  "compatibility_view_name": "nerfstudio",
  "trainer_backend": "nerfstudio.splatfacto",
  "job_backend": "remote",
  "job_submission_endpoint": "https://jobs.example.test/v1",
  "job_submission_command": "remote-submit --dry-run",
  "training_data_dir": "/tmp/package/compat/nerfstudio",
  "transforms_path": "compat/nerfstudio/transforms.json",
  "export_report_path": "/tmp/package/compat/nerfstudio/export_report.json",
  "suggested_output_dir": "/tmp/job-output",
  "launch_command": "ns-train splatfacto --data /tmp/package/compat/nerfstudio --output-dir /tmp/job-output",
  "status": "submitted",
  "job_id": "job-1",
  "job_url": "https://jobs.example.test/v1/jobs/job-1",
  "readiness_blocker": null,
  "known_limits": ["legacy artifact"]
}
"#;
    let manifest: TrainingLaunchJobManifest = serde_json::from_str(legacy_json).expect("legacy manifest should parse");
    assert_eq!(manifest.provider_backend, PROVIDER_BACKEND);
    assert!(!manifest.accepted_by_provider);
    assert_eq!(manifest.submission_recorded_at_millis, None);
  }

  #[test]
  fn training_job_inspect_backfills_provider_backend_from_legacy_json() {
    let legacy_json = r#"
{
  "schema_version": 1,
  "generated_at_millis": 1,
  "training_launch_manifest_path": "/tmp/job.json",
  "source_training_launch_plan_path": "/tmp/launch.json",
  "source_training_package_manifest_path": "/tmp/package/run.json",
  "source_scene_packet_manifest_path": "/tmp/scene/run.json",
  "source_bundle_manifest_paths": ["/tmp/bundle/run.json"],
  "source_run_ids": ["run-1"],
  "job_backend": "remote",
  "trainer_backend": "nerfstudio.splatfacto",
  "job_submission_endpoint": "https://jobs.example.test/v1",
  "job_submission_command": "remote-submit --dry-run",
  "status": "submitted",
  "job_id": "job-1",
  "job_url": "https://jobs.example.test/v1/jobs/job-1",
  "readiness_blocker": null,
  "probe_command": "ns-train --help",
  "probe_succeeded": true,
  "exported_frame_count": 1,
  "skipped_frame_count": 0,
  "transforms_present": true,
  "warnings": [],
  "known_limits": ["legacy artifact"]
}
"#;
    let report: TrainingLaunchJobInspectReport = serde_json::from_str(legacy_json).expect("legacy inspect should parse");
    assert_eq!(report.provider_backend, PROVIDER_BACKEND);
    assert!(!report.accepted_by_provider);
    assert_eq!(report.submission_recorded_at_millis, None);
  }
}
