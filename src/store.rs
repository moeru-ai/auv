use std::fs;
use std::path::{Path, PathBuf};

use crate::driver::{copy_file, sanitized_artifact_name};
use crate::model::{ArtifactRecord, AuvResult, ProducedArtifact, RunRecord, now_millis};

pub struct LocalStore {
  root: PathBuf,
}

impl LocalStore {
  pub fn new(root: PathBuf) -> AuvResult<Self> {
    fs::create_dir_all(root.join("runs"))
      .map_err(|error| format!("failed to create run store root: {error}"))?;
    fs::create_dir_all(root.join("artifacts"))
      .map_err(|error| format!("failed to create artifact store root: {error}"))?;
    Ok(Self { root })
  }

  pub fn stage_artifact(
    &self,
    run_id: &str,
    index: usize,
    artifact: ProducedArtifact,
  ) -> AuvResult<ArtifactRecord> {
    let artifact_id = format!("artifact_{:02}", index + 1);
    let extension = artifact
      .source_path
      .extension()
      .and_then(|extension| extension.to_str())
      .unwrap_or("bin");
    let base_name = sanitized_artifact_name(
      artifact
        .preferred_name
        .trim_end_matches(&format!(".{extension}")),
    );
    let destination = self
      .root
      .join("artifacts")
      .join(run_id)
      .join(format!("{artifact_id}_{base_name}.{extension}"));

    copy_file(&artifact.source_path, &destination)?;
    if artifact.source_path != destination {
      let _ = fs::remove_file(&artifact.source_path);
    }

    Ok(ArtifactRecord {
      id: artifact_id,
      kind: artifact.kind,
      path: destination,
      note: artifact.note,
    })
  }

  pub fn persist_run(&self, run: &RunRecord) -> AuvResult<()> {
    let runs_root = self.root.join("runs");
    let run_directory = runs_root.join(&run.run_id);
    if run_directory.exists() {
      return Err(format!(
        "run directory {} already exists",
        run_directory.display()
      ));
    }

    let staging_directory = runs_root.join(format!(".{}-tmp-{}", run.run_id, now_millis()));
    fs::create_dir_all(&staging_directory).map_err(|error| {
      format!(
        "failed to create staging run directory {}: {error}",
        staging_directory.display()
      )
    })?;

    let write_result = write_run_snapshot(run, &staging_directory);
    if let Err(error) = write_result {
      let _ = fs::remove_dir_all(&staging_directory);
      return Err(error);
    }

    fs::rename(&staging_directory, &run_directory).map_err(|error| {
      let _ = fs::remove_dir_all(&staging_directory);
      format!(
        "failed to publish run directory {} from {}: {error}",
        run_directory.display(),
        staging_directory.display()
      )
    })?;

    Ok(())
  }

  pub fn render_inspection(&self, run_id: &str) -> AuvResult<String> {
    let inspection_path = self.root.join("runs").join(run_id).join("inspect.txt");

    fs::read_to_string(&inspection_path).map_err(|error| {
      format!(
        "failed to read inspect snapshot {}: {error}",
        inspection_path.display()
      )
    })
  }
}

fn write_run_snapshot(run: &RunRecord, directory: &Path) -> AuvResult<()> {
  write_snapshot_file(
    &directory.join("meta.txt"),
    run.render_meta(),
    "run metadata",
  )?;
  write_snapshot_file(
    &directory.join("inputs.txt"),
    run.render_inputs(),
    "run inputs",
  )?;
  write_snapshot_file(
    &directory.join("events.log"),
    run.render_events(),
    "run events",
  )?;
  write_snapshot_file(
    &directory.join("artifacts.txt"),
    run.render_artifacts(),
    "artifact manifest",
  )?;
  write_snapshot_file(
    &directory.join("output.txt"),
    format!("{}\n", run.output_summary),
    "run output",
  )?;
  write_snapshot_file(
    &directory.join("inspect.txt"),
    run.to_string(),
    "inspect snapshot",
  )?;
  Ok(())
}

fn write_snapshot_file(path: &Path, content: String, label: &str) -> AuvResult<()> {
  fs::write(path, content)
    .map_err(|error| format!("failed to write {} {}: {error}", label, path.display()))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::RunStatus;
  use std::collections::BTreeMap;
  use std::env;

  #[test]
  fn local_store_creates_root_directories() {
    let root = temp_dir("store-init");
    LocalStore::new(root.clone()).expect("should initialize");
    assert!(root.join("runs").exists());
    assert!(root.join("artifacts").exists());
    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_persists_and_renders_run() {
    let root = temp_dir("store-persist");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    let run = RunRecord {
      run_id: "test_run".to_string(),
      command_id: "test.cmd".to_string(),
      driver_id: "test.driver".to_string(),
      operation: "test_op".to_string(),
      target_application_id: None,
      runtime_version: "0.0.1".to_string(),
      started_at_millis: 1000,
      finished_at_millis: Some(1100),
      status: RunStatus::Completed,
      inputs: BTreeMap::new(),
      output_summary: "success".to_string(),
      events: Vec::new(),
      artifacts: Vec::new(),
    };

    store.persist_run(&run).expect("should persist");
    let inspection = store.render_inspection("test_run").expect("should render");

    assert!(inspection.contains("Run test_run"));
    assert!(inspection.contains("Status: completed"));
    assert!(inspection.contains("Command: test.cmd"));

    let _ = fs::remove_dir_all(root);
  }

  #[test]
  fn local_store_fails_if_run_already_exists() {
    let root = temp_dir("store-conflict");
    let store = LocalStore::new(root.clone()).expect("should initialize");
    let run = dummy_run("conflict_run");

    store
      .persist_run(&run)
      .expect("first persist should succeed");
    let error = store
      .persist_run(&run)
      .expect_err("second persist should fail");
    assert!(error.contains("already exists"));

    let _ = fs::remove_dir_all(root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  fn dummy_run(run_id: &str) -> RunRecord {
    RunRecord {
      run_id: run_id.to_string(),
      command_id: "test.cmd".to_string(),
      driver_id: "test.driver".to_string(),
      operation: "test_op".to_string(),
      target_application_id: None,
      runtime_version: "0.0.1".to_string(),
      started_at_millis: now_millis(),
      finished_at_millis: None,
      status: RunStatus::Failed,
      inputs: BTreeMap::new(),
      output_summary: String::new(),
      events: Vec::new(),
      artifacts: Vec::new(),
    }
  }
}
