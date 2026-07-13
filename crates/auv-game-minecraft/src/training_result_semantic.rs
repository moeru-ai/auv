use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use auv_file::{
  JsonFileReadError, JsonFileWriteError, JsonWriteOptions, read_json_file as read_json_file_helper,
  write_json_file as write_json_file_helper,
};
use auv_stage_status::StageStatus;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::training_result::TrainingResultStatus;
use crate::training_result_artifact::TrainingResultArtifactFetchManifest;

pub type TrainingResultSemanticValidationResult<T> = Result<T, String>;

pub const TRAINING_RESULT_SEMANTIC_MANIFEST_SCHEMA_VERSION: u32 = 1;
pub const TRAINING_RESULT_SEMANTIC_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;

const RESULT_CONFIG_FILE: &str = "config.yml";
const RESULT_MODELS_DIR: &str = "nerfstudio_models";
const STATUS_SNAPSHOT_FILE: &str = "job_status.json";
const SEMANTIC_MANIFEST_FILE: &str = "minecraft-3dgs-training-result-semantic.json";
const SEMANTIC_INSPECT_FILE: &str = "minecraft-3dgs-training-result-semantic-inspect.json";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingResultSemanticValidationInputs {
  pub training_result_artifact_manifest_path: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingResultSemanticValidationOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub manifest: TrainingResultSemanticManifest,
  pub inspect_report: TrainingResultSemanticInspectReport,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingResultSemanticManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub source_result_status: TrainingResultStatus,
  pub normalized_result_dir: String,
  pub semantic_status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub semantic_reason: Option<TrainingResultSemanticReason>,
  pub config_path: String,
  pub models_dir_path: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub status_snapshot_path: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_trainer: Option<String>,
  pub checkpoint_files: Vec<TrainingResultSemanticCheckpointRecord>,
  pub checkpoint_count: usize,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingResultSemanticInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub training_result_semantic_manifest_path: String,
  pub source_training_result_artifact_manifest_path: String,
  pub source_training_result_manifest_path: String,
  pub source_training_job_manifest_path: String,
  pub source_training_launch_plan_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub trainer_backend: String,
  pub job_backend: String,
  pub source_result_status: TrainingResultStatus,
  pub normalized_result_dir: String,
  pub semantic_status: StageStatus,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub semantic_reason: Option<TrainingResultSemanticReason>,
  pub config_yaml_parsed: bool,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub config_trainer: Option<String>,
  pub config_backend_matches: bool,
  pub models_dir_readable: bool,
  pub status_snapshot_present: bool,
  pub checkpoint_count: usize,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrainingResultSemanticCheckpointRecord {
  pub relative_path: String,
  pub byte_size: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingResultSemanticReason {
  NormalizedConfigMissing,
  NormalizedModelsDirectoryMissing,
  NormalizedPathsInvalid,
  ConfigYamlParseFailed,
  ConfigTrainerMissing,
  TrainerBackendMismatch,
  CheckpointMissing,
}

impl TrainingResultSemanticReason {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::NormalizedConfigMissing => "normalized_config_missing",
      Self::NormalizedModelsDirectoryMissing => "normalized_models_directory_missing",
      Self::NormalizedPathsInvalid => "normalized_paths_invalid",
      Self::ConfigYamlParseFailed => "config_yaml_parse_failed",
      Self::ConfigTrainerMissing => "config_trainer_missing",
      Self::TrainerBackendMismatch => "trainer_backend_mismatch",
      Self::CheckpointMissing => "checkpoint_missing",
    }
  }
}

pub fn validate_3dgs_training_result(
  inputs: TrainingResultSemanticValidationInputs,
) -> TrainingResultSemanticValidationResult<TrainingResultSemanticValidationOutput> {
  let artifact_manifest = read_json_file::<TrainingResultArtifactFetchManifest>(
    &inputs.training_result_artifact_manifest_path,
    "MC-9 D11 training result artifact manifest",
  )?;

  fs::create_dir_all(&inputs.output_dir).map_err(|error| format!("failed to create output dir {}: {error}", inputs.output_dir.display()))?;

  let generated_at_millis = auv_tracing_driver::now_millis();
  let normalized_result_dir = PathBuf::from(&artifact_manifest.normalized_result_dir);
  let config_path = normalized_result_dir.join(RESULT_CONFIG_FILE);
  let models_dir_path = normalized_result_dir.join(RESULT_MODELS_DIR);
  let status_snapshot_path = normalized_result_dir.join(STATUS_SNAPSHOT_FILE);

  let mut known_limits = BTreeSet::new();
  known_limits.extend(artifact_manifest.known_limits.iter().cloned());
  known_limits.insert(
        "MC-10 closes normalized training-result semantic inspect evidence only; it does not grade model quality or claim downstream splat usability"
            .to_string(),
    );
  known_limits.insert("MC-10 does not inspect checkpoint internal semantics or run render preview".to_string());
  known_limits.insert("MC-10 does not add dedicated read-side summary consumption; that belongs to MC-11".to_string());

  let mut warnings = BTreeSet::new();
  let status_snapshot_present = is_real_file(&status_snapshot_path);
  if !status_snapshot_present {
    warnings
      .insert("job_status.json is absent or unreadable; MC-10 records the snapshot observation only and does not gate on it".to_string());
  }

  let (semantic_status, semantic_reason, config_yaml_parsed, config_trainer, config_backend_matches, models_dir_readable, checkpoint_files) =
    evaluate_semantic_gate(&normalized_result_dir, &config_path, &models_dir_path, &artifact_manifest.trainer_backend, &mut warnings);

  let checkpoint_count = checkpoint_files.len();
  let manifest_path = inputs.output_dir.join(SEMANTIC_MANIFEST_FILE);
  let inspect_report_path = inputs.output_dir.join(SEMANTIC_INSPECT_FILE);

  let manifest = TrainingResultSemanticManifest {
    schema_version: TRAINING_RESULT_SEMANTIC_MANIFEST_SCHEMA_VERSION,
    generated_at_millis,
    source_training_result_artifact_manifest_path: inputs.training_result_artifact_manifest_path.to_string_lossy().into_owned(),
    source_training_result_manifest_path: artifact_manifest.source_training_result_manifest_path.clone(),
    source_training_job_manifest_path: artifact_manifest.source_training_job_manifest_path.clone(),
    source_training_launch_plan_path: artifact_manifest.source_training_launch_plan_path.clone(),
    source_training_package_manifest_path: artifact_manifest.source_training_package_manifest_path.clone(),
    source_scene_packet_manifest_path: artifact_manifest.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: artifact_manifest.source_bundle_manifest_paths.clone(),
    source_run_ids: artifact_manifest.source_run_ids.clone(),
    trainer_backend: artifact_manifest.trainer_backend.clone(),
    job_backend: artifact_manifest.job_backend.clone(),
    source_result_status: artifact_manifest.source_result_status,
    normalized_result_dir: artifact_manifest.normalized_result_dir.clone(),
    semantic_status,
    semantic_reason,
    config_path: config_path.to_string_lossy().into_owned(),
    models_dir_path: models_dir_path.to_string_lossy().into_owned(),
    status_snapshot_path: status_snapshot_present.then(|| status_snapshot_path.to_string_lossy().into_owned()),
    config_trainer: config_trainer.clone(),
    checkpoint_files: checkpoint_files.clone(),
    checkpoint_count,
    known_limits: known_limits.iter().cloned().collect(),
  };

  let inspect_report = TrainingResultSemanticInspectReport {
    schema_version: TRAINING_RESULT_SEMANTIC_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    training_result_semantic_manifest_path: manifest_path.to_string_lossy().into_owned(),
    source_training_result_artifact_manifest_path: manifest.source_training_result_artifact_manifest_path.clone(),
    source_training_result_manifest_path: manifest.source_training_result_manifest_path.clone(),
    source_training_job_manifest_path: manifest.source_training_job_manifest_path.clone(),
    source_training_launch_plan_path: manifest.source_training_launch_plan_path.clone(),
    source_training_package_manifest_path: manifest.source_training_package_manifest_path.clone(),
    source_scene_packet_manifest_path: manifest.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: manifest.source_bundle_manifest_paths.clone(),
    source_run_ids: manifest.source_run_ids.clone(),
    trainer_backend: manifest.trainer_backend.clone(),
    job_backend: manifest.job_backend.clone(),
    source_result_status: manifest.source_result_status,
    normalized_result_dir: manifest.normalized_result_dir.clone(),
    semantic_status,
    semantic_reason,
    config_yaml_parsed,
    config_trainer,
    config_backend_matches,
    models_dir_readable,
    status_snapshot_present,
    checkpoint_count,
    warnings: warnings.into_iter().collect(),
    known_limits: manifest.known_limits.clone(),
  };

  write_json_file(&manifest_path, &manifest, "MC-10 training result semantic manifest")?;
  write_json_file(&inspect_report_path, &inspect_report, "MC-10 training result semantic inspect report")?;

  Ok(TrainingResultSemanticValidationOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    inspect_report_path,
    manifest,
    inspect_report,
  })
}

fn evaluate_semantic_gate(
  normalized_result_dir: &Path,
  config_path: &Path,
  models_dir_path: &Path,
  trainer_backend: &str,
  warnings: &mut BTreeSet<String>,
) -> (StageStatus, Option<TrainingResultSemanticReason>, bool, Option<String>, bool, bool, Vec<TrainingResultSemanticCheckpointRecord>) {
  if path_is_symlink(normalized_result_dir) || path_is_symlink(config_path) || path_is_symlink(models_dir_path) {
    return blocked_gate(TrainingResultSemanticReason::NormalizedPathsInvalid, false, None, false, false, Vec::new());
  }

  if !is_real_file(config_path) {
    return blocked_gate(TrainingResultSemanticReason::NormalizedConfigMissing, false, None, false, false, Vec::new());
  }

  if !is_real_dir(models_dir_path) {
    return blocked_gate(TrainingResultSemanticReason::NormalizedModelsDirectoryMissing, false, None, false, false, Vec::new());
  }

  let models_dir_readable = true;

  let config_contents = match fs::read_to_string(config_path) {
    Ok(contents) => contents,
    Err(error) => {
      warnings.insert(format!("failed to read config.yml at {}: {error}", config_path.display()));
      return failed_gate(TrainingResultSemanticReason::ConfigYamlParseFailed, false, None, false, models_dir_readable, Vec::new());
    }
  };

  let parsed_value = match serde_yaml::from_str::<Value>(&config_contents) {
    Ok(value) => value,
    Err(error) => {
      warnings.insert(format!("config.yml YAML parse failed: {error}"));
      return failed_gate(TrainingResultSemanticReason::ConfigYamlParseFailed, false, None, false, models_dir_readable, Vec::new());
    }
  };

  let config_trainer = match extract_top_level_trainer(&parsed_value) {
    Ok(Some(trainer)) => Some(trainer),
    Ok(None) => {
      return failed_gate(TrainingResultSemanticReason::ConfigTrainerMissing, true, None, false, models_dir_readable, Vec::new());
    }
    Err(error) => {
      warnings.insert(error);
      return failed_gate(TrainingResultSemanticReason::ConfigYamlParseFailed, true, None, false, models_dir_readable, Vec::new());
    }
  };

  let config_backend_matches = config_trainer.as_deref() == Some(trainer_backend);
  if !config_backend_matches {
    return failed_gate(TrainingResultSemanticReason::TrainerBackendMismatch, true, config_trainer, false, models_dir_readable, Vec::new());
  }

  let checkpoint_files = match collect_checkpoint_files(models_dir_path) {
    Ok(files) => files,
    Err(error) => {
      warnings.insert(format!(
        "failed to scan checkpoint files under {} after discovering {} checkpoint file(s): {}",
        models_dir_path.display(),
        error.partial_files.len(),
        error.cause
      ));
      return failed_gate(TrainingResultSemanticReason::CheckpointMissing, true, config_trainer.clone(), true, false, error.partial_files);
    }
  };

  if checkpoint_files.is_empty() {
    return failed_gate(TrainingResultSemanticReason::CheckpointMissing, true, config_trainer, true, models_dir_readable, Vec::new());
  }

  (StageStatus::Ready, None, true, config_trainer, true, true, checkpoint_files)
}

fn blocked_gate(
  reason: TrainingResultSemanticReason,
  config_yaml_parsed: bool,
  config_trainer: Option<String>,
  config_backend_matches: bool,
  models_dir_readable: bool,
  checkpoint_files: Vec<TrainingResultSemanticCheckpointRecord>,
) -> (StageStatus, Option<TrainingResultSemanticReason>, bool, Option<String>, bool, bool, Vec<TrainingResultSemanticCheckpointRecord>) {
  (StageStatus::Blocked, Some(reason), config_yaml_parsed, config_trainer, config_backend_matches, models_dir_readable, checkpoint_files)
}

fn failed_gate(
  reason: TrainingResultSemanticReason,
  config_yaml_parsed: bool,
  config_trainer: Option<String>,
  config_backend_matches: bool,
  models_dir_readable: bool,
  checkpoint_files: Vec<TrainingResultSemanticCheckpointRecord>,
) -> (StageStatus, Option<TrainingResultSemanticReason>, bool, Option<String>, bool, bool, Vec<TrainingResultSemanticCheckpointRecord>) {
  (StageStatus::Failed, Some(reason), config_yaml_parsed, config_trainer, config_backend_matches, models_dir_readable, checkpoint_files)
}

fn extract_top_level_trainer(value: &Value) -> Result<Option<String>, String> {
  let mapping = value.as_mapping().ok_or_else(|| "config.yml root must be a YAML mapping".to_string())?;
  let trainer_value = match mapping.get(&Value::from("trainer")) {
    Some(value) => value,
    None => return Ok(None),
  };
  trainer_value
    .as_str()
    .map(|trainer| Some(trainer.to_string()))
    .ok_or_else(|| "config.yml top-level trainer must be a scalar string".to_string())
}

pub(crate) fn collect_checkpoint_files(models_dir_path: &Path) -> Result<Vec<TrainingResultSemanticCheckpointRecord>, CheckpointScanError> {
  let mut checkpoints = Vec::new();
  if let Err(cause) = visit_checkpoint_files(models_dir_path, models_dir_path, &mut checkpoints) {
    checkpoints.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    return Err(CheckpointScanError {
      partial_files: checkpoints,
      cause,
    });
  }
  checkpoints.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
  Ok(checkpoints)
}

#[derive(Debug)]
pub(crate) struct CheckpointScanError {
  pub(crate) partial_files: Vec<TrainingResultSemanticCheckpointRecord>,
  pub(crate) cause: std::io::Error,
}

fn visit_checkpoint_files(
  root: &Path,
  current: &Path,
  checkpoints: &mut Vec<TrainingResultSemanticCheckpointRecord>,
) -> std::io::Result<()> {
  let mut entries = fs::read_dir(current)?.collect::<Result<Vec<_>, _>>()?;
  entries.sort_by_key(|entry| entry.file_name());
  for entry in entries {
    let path = entry.path();
    let file_type = entry.file_type()?;
    if file_type.is_symlink() {
      continue;
    }
    if file_type.is_dir() {
      visit_checkpoint_files(root, &path, checkpoints)?;
      continue;
    }
    if file_type.is_file() && path.extension().is_some_and(|extension| extension == "ckpt") {
      let relative_path = path
        .strip_prefix(root)
        .map(|relative| relative.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.file_name().unwrap().to_string_lossy().into_owned());
      let byte_size = fs::metadata(&path)?.len();
      checkpoints.push(TrainingResultSemanticCheckpointRecord {
        relative_path,
        byte_size,
      });
    }
  }
  Ok(())
}

fn path_is_symlink(path: &Path) -> bool {
  path.symlink_metadata().map(|metadata| metadata.file_type().is_symlink()).unwrap_or(false)
}

fn is_real_file(path: &Path) -> bool {
  path.symlink_metadata().map(|metadata| metadata.is_file() && !metadata.file_type().is_symlink()).unwrap_or(false)
}

fn is_real_dir(path: &Path) -> bool {
  path.symlink_metadata().map(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink()).unwrap_or(false)
}

fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> TrainingResultSemanticValidationResult<T> {
  read_json_file_helper(path).map_err(|error| match error {
    JsonFileReadError::Open(error) => {
      format!("failed to open {label} {}: {error}", path.display())
    }
    JsonFileReadError::Parse(error) => {
      format!("failed to parse {label} {}: {error}", path.display())
    }
  })
}

fn write_json_file<T: Serialize>(path: &Path, value: &T, label: &str) -> TrainingResultSemanticValidationResult<()> {
  write_json_file_helper(path, value, JsonWriteOptions::default()).map_err(|error| match error {
    JsonFileWriteError::CreateParent(error) | JsonFileWriteError::Write(error) => {
      format!("failed to write {label} {}: {error}", path.display())
    }
    JsonFileWriteError::Serialize(error) => format!("failed to serialize {label}: {error}"),
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  #[test]
  fn stage_status_preserves_wire_labels() {
    for (status, wire) in [
      (StageStatus::Ready, "\"ready\""),
      (StageStatus::Blocked, "\"blocked\""),
      (StageStatus::Failed, "\"failed\""),
    ] {
      assert_eq!(serde_json::to_string(&status).expect("serialize"), wire);
      let decoded: StageStatus = serde_json::from_str(wire).expect("deserialize");
      assert_eq!(decoded, status);
    }
  }

  use crate::training_result_artifact::{
    TrainingResultArtifactFetchManifest, TrainingResultNormalizedArtifactKind, TrainingResultNormalizedArtifactRecord,
  };
  use tempfile::TempDir;

  fn write_d11_manifest_fixture(temp: &TempDir, normalized_result_dir: &Path, trainer_backend: &str) -> PathBuf {
    let manifest = TrainingResultArtifactFetchManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_training_result_manifest_path: "/tmp/result.json".to_string(),
      source_training_job_manifest_path: "/tmp/job.json".to_string(),
      source_training_launch_plan_path: "/tmp/launch-plan.json".to_string(),
      source_training_package_manifest_path: "/tmp/package.json".to_string(),
      source_scene_packet_manifest_path: "/tmp/scene.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/bundle.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      trainer_backend: trainer_backend.to_string(),
      job_backend: "remote".to_string(),
      source_job_status: TrainingResultStatus::Succeeded,
      source_result_status: TrainingResultStatus::Succeeded,
      source_result_status_reason: None,
      source_result_dir: "/tmp/trainer-output".to_string(),
      normalized_result_dir: normalized_result_dir.to_string_lossy().into_owned(),
      normalized_artifacts: vec![
        TrainingResultNormalizedArtifactRecord {
          kind: TrainingResultNormalizedArtifactKind::Config,
          relative_path: RESULT_CONFIG_FILE.to_string(),
          absolute_path: normalized_result_dir.join(RESULT_CONFIG_FILE).display().to_string(),
          readable: true,
          byte_size: Some(32),
        },
        TrainingResultNormalizedArtifactRecord {
          kind: TrainingResultNormalizedArtifactKind::ModelsDirectory,
          relative_path: RESULT_MODELS_DIR.to_string(),
          absolute_path: normalized_result_dir.join(RESULT_MODELS_DIR).display().to_string(),
          readable: true,
          byte_size: None,
        },
      ],
      known_limits: vec!["d11-limit".to_string()],
    };
    let manifest_path = temp.path().join("minecraft-3dgs-training-result-artifact-manifest.json");
    write_json_file(&manifest_path, &manifest, "fixture d11 manifest").expect("write d11 manifest");
    manifest_path
  }

  fn write_ready_normalized_result(normalized_result_dir: &Path, trainer: &str, with_checkpoint: bool) {
    fs::create_dir_all(normalized_result_dir.join(RESULT_MODELS_DIR)).expect("models dir");
    fs::write(normalized_result_dir.join(RESULT_CONFIG_FILE), format!("trainer: {trainer}\n")).expect("config");
    if with_checkpoint {
      fs::write(normalized_result_dir.join(RESULT_MODELS_DIR).join("step-000001.ckpt"), b"checkpoint").expect("checkpoint");
    }
  }

  #[test]
  fn validate_training_result_semantic_happy_path() {
    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    write_ready_normalized_result(&normalized_result_dir, "nerfstudio.splatfacto", true);
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should succeed");

    assert_eq!(output.inspect_report.semantic_status, StageStatus::Ready);
    assert_eq!(output.inspect_report.semantic_reason, None);
    assert_eq!(output.inspect_report.checkpoint_count, 1);
    assert_eq!(output.inspect_report.config_trainer.as_deref(), Some("nerfstudio.splatfacto"));
    assert!(output.manifest_path.is_file());
    assert!(output.inspect_report_path.is_file());
  }

  #[test]
  fn validate_training_result_semantic_blocks_when_config_missing() {
    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    fs::create_dir_all(normalized_result_dir.join(RESULT_MODELS_DIR)).expect("models dir");
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should write blocked inspect");

    assert_eq!(output.inspect_report.semantic_status, StageStatus::Blocked);
    assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::NormalizedConfigMissing));
  }

  #[test]
  fn validate_training_result_semantic_blocks_when_models_directory_missing() {
    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    fs::create_dir_all(&normalized_result_dir).expect("normalized dir");
    fs::write(normalized_result_dir.join(RESULT_CONFIG_FILE), "trainer: nerfstudio.splatfacto\n").expect("config");
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should write blocked inspect");

    assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::NormalizedModelsDirectoryMissing));
  }

  #[test]
  fn validate_training_result_semantic_blocks_when_path_is_symlink() {
    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    fs::create_dir_all(&normalized_result_dir).expect("normalized dir");
    let real_config = temp.path().join("real-config.yml");
    fs::write(&real_config, "trainer: nerfstudio.splatfacto\n").expect("real config");
    #[cfg(unix)]
    {
      std::os::unix::fs::symlink(&real_config, normalized_result_dir.join(RESULT_CONFIG_FILE)).expect("symlink config");
      fs::create_dir_all(normalized_result_dir.join(RESULT_MODELS_DIR)).expect("models dir");
      let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

      let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
        training_result_artifact_manifest_path: d11_manifest_path,
        output_dir: temp.path().join("semantic"),
      })
      .expect("semantic validation should write blocked inspect");

      assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::NormalizedPathsInvalid));
    }
  }

  #[test]
  fn validate_training_result_semantic_fails_when_yaml_invalid() {
    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    fs::create_dir_all(normalized_result_dir.join(RESULT_MODELS_DIR)).expect("models dir");
    fs::write(normalized_result_dir.join(RESULT_CONFIG_FILE), "trainer: [broken\n").expect("broken config");
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should write failed inspect");

    assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::ConfigYamlParseFailed));
  }

  #[test]
  fn validate_training_result_semantic_fails_when_trainer_missing() {
    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    write_ready_normalized_result(&normalized_result_dir, "nerfstudio.splatfacto", true);
    fs::write(normalized_result_dir.join(RESULT_CONFIG_FILE), "method: splatfacto\n").expect("config without trainer");
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should write failed inspect");

    assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::ConfigTrainerMissing));
  }

  #[test]
  fn validate_training_result_semantic_fails_when_trainer_backend_mismatches() {
    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    write_ready_normalized_result(&normalized_result_dir, "other.backend", true);
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should write failed inspect");

    assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::TrainerBackendMismatch));
    assert!(!output.inspect_report.config_backend_matches);
    assert!(output.inspect_report.models_dir_readable);
  }

  #[test]
  fn validate_training_result_semantic_failed_inspect_fields_reflect_models_dir_presence() {
    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    fs::create_dir_all(normalized_result_dir.join("nerfstudio_models")).expect("models dir");
    fs::write(normalized_result_dir.join("config.yml"), "trainer: [broken\n").expect("broken config");
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should write failed inspect");

    assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::ConfigYamlParseFailed));
    assert!(!output.inspect_report.config_backend_matches);
    assert!(output.inspect_report.models_dir_readable);
  }

  #[test]
  fn validate_training_result_semantic_fails_when_checkpoint_missing() {
    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    write_ready_normalized_result(&normalized_result_dir, "nerfstudio.splatfacto", false);
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should write failed inspect");

    assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::CheckpointMissing));
  }

  #[test]
  fn validate_training_result_semantic_does_not_fail_when_job_status_missing() {
    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    write_ready_normalized_result(&normalized_result_dir, "nerfstudio.splatfacto", true);
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should succeed without job_status.json");

    assert_eq!(output.inspect_report.semantic_status, StageStatus::Ready);
    assert!(!output.inspect_report.status_snapshot_present);
    assert!(output.inspect_report.warnings.iter().any(|warning| warning.contains("job_status.json")));
  }

  #[cfg(unix)]
  #[test]
  fn validate_training_result_semantic_marks_models_dir_unreadable_when_checkpoint_scan_fails() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    write_ready_normalized_result(&normalized_result_dir, "nerfstudio.splatfacto", true);
    let models_dir = normalized_result_dir.join(RESULT_MODELS_DIR);
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let original_permissions = fs::metadata(&models_dir).expect("models metadata").permissions();
    let mut unreadable_permissions = original_permissions.clone();
    unreadable_permissions.set_mode(0o000);
    fs::set_permissions(&models_dir, unreadable_permissions).expect("set unreadable permissions");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should write failed inspect");

    fs::set_permissions(&models_dir, original_permissions).expect("restore permissions");

    assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::CheckpointMissing));
    assert!(!output.inspect_report.models_dir_readable);
  }

  #[cfg(unix)]
  #[test]
  fn validate_training_result_semantic_blocks_when_normalized_result_dir_is_symlink() {
    let temp = TempDir::new().expect("temp dir");
    let real_normalized_result_dir = temp.path().join("real-normalized-result");
    write_ready_normalized_result(&real_normalized_result_dir, "nerfstudio.splatfacto", true);
    let symlink_normalized_result_dir = temp.path().join("normalized-result");
    std::os::unix::fs::symlink(&real_normalized_result_dir, &symlink_normalized_result_dir).expect("symlink normalized result dir");
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &symlink_normalized_result_dir, "nerfstudio.splatfacto");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should write blocked inspect");

    assert_eq!(output.inspect_report.semantic_status, StageStatus::Blocked);
    assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::NormalizedPathsInvalid));
  }

  #[cfg(unix)]
  #[test]
  fn validate_training_result_semantic_preserves_partial_checkpoint_evidence_on_scan_failure() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().expect("temp dir");
    let normalized_result_dir = temp.path().join("normalized-result");
    write_ready_normalized_result(&normalized_result_dir, "nerfstudio.splatfacto", false);
    let models_dir = normalized_result_dir.join(RESULT_MODELS_DIR);
    fs::write(models_dir.join("a-step-000001.ckpt"), b"checkpoint").expect("first checkpoint");
    let blocked_dir = models_dir.join("z-blocked-subdir");
    fs::create_dir_all(&blocked_dir).expect("blocked dir");
    fs::write(blocked_dir.join("step-000002.ckpt"), b"checkpoint").expect("nested checkpoint");
    let d11_manifest_path = write_d11_manifest_fixture(&temp, &normalized_result_dir, "nerfstudio.splatfacto");

    let original_permissions = fs::metadata(&blocked_dir).expect("blocked dir metadata").permissions();
    let mut unreadable_permissions = original_permissions.clone();
    unreadable_permissions.set_mode(0o000);
    fs::set_permissions(&blocked_dir, unreadable_permissions).expect("set unreadable permissions");

    let output = validate_3dgs_training_result(TrainingResultSemanticValidationInputs {
      training_result_artifact_manifest_path: d11_manifest_path,
      output_dir: temp.path().join("semantic"),
    })
    .expect("semantic validation should write failed inspect");

    fs::set_permissions(&blocked_dir, original_permissions).expect("restore permissions");

    assert_eq!(output.inspect_report.semantic_status, StageStatus::Failed);
    assert_eq!(output.inspect_report.semantic_reason, Some(TrainingResultSemanticReason::CheckpointMissing));
    assert_eq!(output.inspect_report.checkpoint_count, 1);
    assert_eq!(output.manifest.checkpoint_count, 1);
    assert_eq!(output.manifest.checkpoint_files.len(), 1);
    assert_eq!(output.manifest.checkpoint_files[0].relative_path, "a-step-000001.ckpt");
    assert!(output.inspect_report.warnings.iter().any(|warning| warning.contains("after discovering 1 checkpoint file(s)")));
  }
}
