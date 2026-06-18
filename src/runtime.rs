// File: src/runtime.rs
//! Runtime execution engine.
//!
//! `Runtime` is a shrinking compatibility facade for legacy recorded operations
//! and recording access while invoke execution has moved to `auv-cli-invoke`.
//!
//! Boundary: this layer executes *given* requests. It is not a planner/LLM
//! agent, and it does not choose strategies beyond what the request/cmd
//! specifies.

use std::io::{BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const TELEMETRY_SAMPLE_ARTIFACT_ROLE: &str = "telemetry-sample";
pub const TELEMETRY_SAMPLE_MAX_BYTES: u64 = 128 * 1024;
pub const MINECRAFT_PROJECTION_ARTIFACT_ROLE: &str = "minecraft-projection";

use crate::contract::ArtifactRef;
use crate::model::AuvResult;
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::trace::RunType;
use auv_tracing_driver::{MemoryRunRecorder, RunRecorder, RunRecordingBackend};

pub struct Runtime {
  project_root: PathBuf,
  recording: RunRecordingBackend,
}

// NOTICE(mc2-live-telemetry-tail-artifact): live telemetry.jsonl can grow to
// multi-GB files during long Minecraft sessions. The current artifact staging
// path still copies and server-uploads the artifact as a whole, so recording
// the full live file would amplify disk and memory use badly. For the current
// MC-2 slice we only persist a capped tail sample as durable evidence. If a
// future slice needs full-session archival, that path must stream copy/upload
// instead of routing the live file through the existing artifact staging seam.
fn prepare_telemetry_sample_artifact(path: &Path) -> AuvResult<Option<PathBuf>> {
  let metadata = std::fs::metadata(path).map_err(|error| {
    format!(
      "failed to stat telemetry sample artifact {}: {error}",
      path.display()
    )
  })?;
  if metadata.len() <= TELEMETRY_SAMPLE_MAX_BYTES {
    return Ok(None);
  }

  let file = std::fs::File::open(path).map_err(|error| {
    format!(
      "failed to open telemetry sample artifact {}: {error}",
      path.display()
    )
  })?;
  let mut reader = BufReader::new(file);
  let start = metadata.len().saturating_sub(TELEMETRY_SAMPLE_MAX_BYTES);
  reader.seek(SeekFrom::Start(start)).map_err(|error| {
    format!(
      "failed to seek telemetry sample artifact {}: {error}",
      path.display()
    )
  })?;

  if start > 0 {
    let mut discarded = String::new();
    reader.read_line(&mut discarded).map_err(|error| {
      format!(
        "failed to align telemetry sample artifact {} to next line: {error}",
        path.display()
      )
    })?;
  }

  let temp_path = std::env::temp_dir().join(format!(
    "auv-telemetry-tail-{}-{}.jsonl",
    std::process::id(),
    crate::model::now_millis()
  ));
  let mut temp = std::fs::File::create(&temp_path).map_err(|error| {
    format!(
      "failed to create trimmed telemetry sample artifact {}: {error}",
      temp_path.display()
    )
  })?;
  std::io::copy(&mut reader, &mut temp).map_err(|error| {
    format!(
      "failed to trim telemetry sample artifact {}: {error}",
      path.display()
    )
  })?;
  temp.flush().map_err(|error| {
    format!(
      "failed to flush trimmed telemetry sample artifact {}: {error}",
      temp_path.display()
    )
  })?;
  drop(temp);

  Ok(Some(temp_path))
}

impl Runtime {
  pub fn new(project_root: PathBuf, store: LocalStore) -> Self {
    Self {
      project_root,
      recording: RunRecordingBackend::new(store, Arc::new(MemoryRunRecorder::new())),
    }
  }

  pub fn project_root(&self) -> &Path {
    &self.project_root
  }

  pub fn inspect(&self, run_id: &str) -> AuvResult<String> {
    crate::inspect::inspect_run(self.recording.store(), run_id)
  }

  pub fn read_run(&self, run_id: &str) -> AuvResult<auv_tracing_driver::store::CanonicalRun> {
    self.recording.read_run(run_id)
  }

  pub fn list_verifications(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::contract::VerificationResult>> {
    crate::run_read::list_verifications(self.recording.store(), run_id)
  }

  pub fn list_observation_snapshots(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::contract::ObservationSnapshot>> {
    crate::run_read::list_observation_snapshots(self.recording.store(), run_id)
  }

  pub fn list_detector_recognition_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::DetectorRecognitionLineage>> {
    crate::run_read::list_detector_recognition_lineage(self.recording.store(), run_id)
  }

  pub fn list_candidate_promotion_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::CandidatePromotionLineage>> {
    crate::run_read::list_candidate_promotion_lineage(self.recording.store(), run_id)
  }

  pub fn list_candidate_action_decision_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::CandidateActionDecisionLineage>> {
    crate::run_read::list_candidate_action_decision_lineage(self.recording.store(), run_id)
  }

  pub fn run_recorded_operation<T, E, F>(
    &self,
    spec: auv_tracing_driver::run_builder::RunSpec,
    operation_label: impl Into<String>,
    operation: F,
  ) -> AuvResult<auv_tracing_driver::recorded_operation::RecordedOperationOutput<T>>
  where
    E: std::fmt::Display,
    F: FnOnce(
      &mut auv_tracing_driver::recorded_operation::RecordedOperationContext<'_>,
    ) -> Result<T, E>,
  {
    self
      .recording
      .handle()
      .run_recorded_operation(spec, operation_label, operation)
  }

  pub fn record_candidate_action_decision(
    &self,
    promotion: &crate::candidate_promotion_recording::CandidatePromotionArtifact,
    request: crate::candidate_action_decision::CandidateActionDecisionRequest,
  ) -> AuvResult<
    auv_tracing_driver::recorded_operation::RecordedOperationOutput<(
      ArtifactRef,
      crate::candidate_action_decision::CandidateActionDecisionArtifact,
    )>,
  > {
    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.candidate.action.decide_only",
      ),
      "Candidate action decide-only artifact recording",
      |context| {
        crate::candidate_action_decision::record_candidate_action_decision_artifact(
          context, promotion, &request,
        )
      },
    )
  }

  pub fn record_telemetry_sample_artifact(
    &self,
    sample_path: impl Into<PathBuf>,
  ) -> AuvResult<auv_tracing_driver::recorded_operation::RecordedOperationOutput<ArtifactRef>> {
    let sample_path = sample_path.into();
    let preferred_name = sample_path
      .file_name()
      .and_then(|name| name.to_str())
      .ok_or_else(|| {
        format!(
          "telemetry sample path {:?} has no valid file name",
          sample_path
        )
      })?
      .to_string();

    if !sample_path.is_file() {
      return Err(format!(
        "telemetry sample path {:?} is not a readable file",
        sample_path
      ));
    }

    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.minecraft.telemetry.sample",
      ),
      "Minecraft telemetry sample artifact recording",
      |context| {
        let artifact_source =
          prepare_telemetry_sample_artifact(&sample_path)?.unwrap_or_else(|| sample_path.clone());
        let stage_result = context.stage_artifact_file_with_ref(
          TELEMETRY_SAMPLE_ARTIFACT_ROLE,
          &artifact_source,
          &preferred_name,
          Some("durable minecraft telemetry sample".to_string()),
        );
        if artifact_source != sample_path {
          let _ = std::fs::remove_file(&artifact_source);
        }
        let (_, artifact_ref) = stage_result?;
        Ok::<_, String>(artifact_ref)
      },
    )
  }

  pub fn record_minecraft_projection_artifact(
    &self,
    projection_artifact: auv_game_minecraft::MinecraftProjectionArtifact,
  ) -> AuvResult<auv_tracing_driver::recorded_operation::RecordedOperationOutput<ArtifactRef>> {
    projection_artifact.validate()?;
    let artifact_json = serde_json::to_string_pretty(&projection_artifact)
      .map_err(|error| format!("failed to serialize minecraft projection artifact: {error}"))?;

    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.minecraft.projection.artifact",
      ),
      "Minecraft projection artifact recording",
      |context| {
        let temp_root = std::env::temp_dir();
        let artifact_path = temp_root.join(format!(
          "auv-minecraft-projection-{}-{}.json",
          context.run_id(),
          crate::model::now_millis()
        ));
        std::fs::write(&artifact_path, artifact_json.as_bytes())
          .map_err(|error| format!("failed to write minecraft projection artifact: {error}"))?;
        let (_, artifact_ref) = context.stage_artifact_file_with_ref(
          MINECRAFT_PROJECTION_ARTIFACT_ROLE,
          &artifact_path,
          "projection-artifact.json",
          Some("durable minecraft projection artifact".to_string()),
        )?;
        let _ = std::fs::remove_file(&artifact_path);
        Ok::<_, String>(artifact_ref)
      },
    )
  }

  pub fn list_candidate_action_execution_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::CandidateActionExecutionLineage>> {
    crate::run_read::list_candidate_action_execution_lineage(self.recording.store(), run_id)
  }

  pub fn run_candidate_action_command(
    &self,
    request: crate::candidate_action_command::CandidateActionCommandRequest,
  ) -> AuvResult<
    auv_tracing_driver::recorded_operation::RecordedOperationOutput<
      crate::candidate_action_command::CandidateActionCommandOutput,
    >,
  > {
    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.candidate.action.command",
      ),
      "Consent-gated candidate action command",
      |context| {
        crate::candidate_action_command::execute_candidate_action_command(context, &request)
      },
    )
  }

  pub fn record_candidate_action_execution(
    &self,
    promotion: &crate::candidate_promotion_recording::CandidatePromotionArtifact,
    decision: &crate::candidate_action_decision::CandidateActionDecisionArtifact,
    request: crate::candidate_action_decision::CandidateActionExecutionRequest,
    input_action_result: auv_driver::InputActionResult,
  ) -> AuvResult<
    auv_tracing_driver::recorded_operation::RecordedOperationOutput<(
      ArtifactRef,
      crate::candidate_action_decision::CandidateActionExecutionArtifact,
    )>,
  > {
    self.run_recorded_operation(
      auv_tracing_driver::run_builder::RunSpec::new(
        RunType::Execute,
        "auv.candidate.action.execute_single",
      ),
      "Candidate action execution artifact recording",
      |context| {
        crate::candidate_action_decision::record_candidate_action_execution_artifact(
          context,
          promotion,
          decision,
          &request,
          input_action_result,
        )
      },
    )
  }

  pub fn run_dir(&self, run_id: impl AsRef<str>) -> AuvResult<PathBuf> {
    self.recording.run_dir(run_id)
  }

  pub fn recorder(&self) -> Arc<dyn RunRecorder> {
    self.recording.recorder()
  }

  #[cfg(test)]
  pub(crate) fn recording_backend(&self) -> &RunRecordingBackend {
    &self.recording
  }

  pub fn recording(&self) -> &RunRecordingBackend {
    &self.recording
  }

  pub fn with_recording(mut self, recording: RunRecordingBackend) -> Self {
    self.recording = recording;
    self
  }

  pub fn with_recorder(mut self, recorder: Arc<dyn RunRecorder>) -> Self {
    let store = self.recording.store().clone();
    self.recording = RunRecordingBackend::new(store, recorder);
    self
  }
}

#[cfg(test)]
mod tests {
  use serde_json::json;
  use std::env;
  use std::fs;
  use std::path::PathBuf;

  use super::{
    MINECRAFT_PROJECTION_ARTIFACT_ROLE, Runtime, TELEMETRY_SAMPLE_ARTIFACT_ROLE,
    TELEMETRY_SAMPLE_MAX_BYTES,
  };
  use crate::model::now_millis;
  use auv_tracing_driver::store::LocalStore;
  #[test]
  fn record_telemetry_sample_artifact_persists_sample_for_inspect() {
    let project_root = temp_dir("runtime-telemetry-project");
    let store_root = temp_dir("runtime-telemetry-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let source_path = project_root.join("telemetry.jsonl");
    fs::write(&source_path, "{\"sample\":true}\n").expect("telemetry sample should write");

    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let output = runtime
      .record_telemetry_sample_artifact(source_path.clone())
      .expect("telemetry sample recording should succeed");

    assert_eq!(output.value.run_id.as_str(), output.run_id.as_str());
    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    assert_eq!(run.artifacts.len(), 1);
    assert_eq!(run.artifacts[0].role, TELEMETRY_SAMPLE_ARTIFACT_ROLE);
    assert_eq!(
      run.artifacts[0].path,
      "artifacts/artifact_0001_telemetry.jsonl"
    );

    let inspect_text = runtime
      .inspect(output.run_id.as_str())
      .expect("inspect should render run");
    assert!(inspect_text.contains("Artifacts:"));
    assert!(inspect_text.contains("role=telemetry-sample"));
    assert!(inspect_text.contains("path=artifacts/artifact_0001_telemetry.jsonl"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn record_telemetry_sample_artifact_rejects_missing_file() {
    let project_root = temp_dir("runtime-telemetry-missing-project");
    let store_root = temp_dir("runtime-telemetry-missing-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let source_path = project_root.join("missing-telemetry.jsonl");

    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let error = runtime
      .record_telemetry_sample_artifact(source_path.clone())
      .expect_err("missing telemetry sample should fail");

    assert!(error.contains("is not a readable file"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn record_telemetry_sample_artifact_keeps_large_source_file_intact() {
    let project_root = temp_dir("runtime-telemetry-large-project");
    let store_root = temp_dir("runtime-telemetry-large-store");
    fs::create_dir_all(&project_root).expect("project root should exist");
    let source_path = project_root.join("telemetry.jsonl");
    let original_body = (0..6000)
      .map(|index| {
        format!(
          "{{\"sample\":{index},\"payload\":\"{}\"}}\n",
          "x".repeat(32)
        )
      })
      .collect::<String>();
    fs::write(&source_path, &original_body).expect("large telemetry sample should write");
    let original_size = fs::metadata(&source_path).expect("source metadata").len();
    assert!(
      original_size > TELEMETRY_SAMPLE_MAX_BYTES,
      "fixture must exceed trimming threshold"
    );

    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let output = runtime
      .record_telemetry_sample_artifact(source_path.clone())
      .expect("telemetry sample recording should succeed");

    let persisted_source = fs::read_to_string(&source_path).expect("source file should remain");
    assert_eq!(persisted_source, original_body);

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    let staged_path = store_root
      .join("runs")
      .join(output.run_id.as_str())
      .join(&run.artifacts[0].path);
    let staged_size = fs::metadata(&staged_path)
      .expect("staged artifact metadata")
      .len();
    assert!(staged_size <= TELEMETRY_SAMPLE_MAX_BYTES);

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn record_minecraft_projection_artifact_persists_artifact_for_inspect() {
    let project_root = temp_dir("runtime-minecraft-projection-project");
    let store_root = temp_dir("runtime-minecraft-projection-store");
    fs::create_dir_all(&project_root).expect("project root should exist");

    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());
    let projection_artifact = auv_game_minecraft::MinecraftProjectionArtifact {
      spatial_frame_id: "frame-1".to_string(),
      world_tick: 42,
      monotonic_timestamp_ms: 1_000,
      screenshot_artifact_ref: Some("artifact://screenshot-1".to_string()),
      mc_capture_skew_ms: Some(180),
      viewport_bounds: auv_game_minecraft::ProjectionViewportBounds {
        x: 0.0,
        y: 0.0,
        width: 800.0,
        height: 600.0,
      },
      projected_point: Some(auv_game_minecraft::MinecraftProjectedPoint {
        screen_point: Some(auv_driver::geometry::Point::new(320.0, 240.0)),
        visibility: auv_game_minecraft::ProjectionVisibility::Visible,
        match_radius_px: 12.0,
        basis_frame_id: "frame-1".to_string(),
        confidence: 1.0,
      }),
      visibility: auv_game_minecraft::ProjectionVisibility::Visible,
      raycast_block_id: Some("minecraft:stone".to_string()),
      screen_state: Some("menu".to_string()),
      mismatch_refusal_reason: Some(
        auv_game_minecraft::verify::MismatchRefusalReason::MenuLoadingScreen,
      ),
      verification_reference: Some("verification-1".to_string()),
    };

    let output = runtime
      .record_minecraft_projection_artifact(projection_artifact)
      .expect("minecraft projection artifact recording should succeed");

    let run = runtime
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    assert_eq!(run.artifacts.len(), 1);
    assert_eq!(run.artifacts[0].role, MINECRAFT_PROJECTION_ARTIFACT_ROLE);
    assert_eq!(
      run.artifacts[0].path,
      "artifacts/artifact_0001_projection-artifact.json"
    );

    let inspect_text = runtime
      .inspect(output.run_id.as_str())
      .expect("inspect should render run");
    assert!(inspect_text.contains("MC-2 Projection Artifacts:"));
    assert!(inspect_text.contains("frame=frame-1"));
    assert!(inspect_text.contains("screenshot_artifact_ref=artifact://screenshot-1"));
    assert!(inspect_text.contains("capture_skew_ms=180"));
    assert!(inspect_text.contains("screen_state=menu"));
    assert!(inspect_text.contains("refusal_reason=MenuLoadingScreen"));
    assert!(inspect_text.contains("verification_reference=verification-1"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  #[test]
  fn start_run_with_default_spec_stamps_local_default_attributes() {
    let project_root = temp_dir("runtime-default-device-project");
    let store_root = temp_dir("runtime-default-device-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let run = runtime
      .recording()
      .handle()
      .start_run(auv_tracing_driver::run_builder::RunSpec::new(
        auv_tracing_driver::trace::RunType::Command,
        "auv.command",
      ))
      .expect("default-spec run should start");
    assert_eq!(run.device_id().as_str(), "local");
    assert_eq!(run.session_id().as_str(), "default");

    runtime
      .recording()
      .handle()
      .finish_run(
        run,
        auv_tracing_driver::run_builder::RunFinish {
          status_code: auv_tracing_driver::trace::TraceStatusCode::Ok,
          summary: Some("default".to_string()),
          failure: None,
        },
      )
      .expect("default-spec run should finish");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn start_run_with_explicit_device_session_overrides_defaults() {
    let project_root = temp_dir("runtime-explicit-device-project");
    let store_root = temp_dir("runtime-explicit-device-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let spec = auv_tracing_driver::run_builder::RunSpec::new(
      auv_tracing_driver::trace::RunType::Command,
      "auv.command",
    )
    .with_device(auv_tracing_driver::trace::DeviceId::new("remote-mac"))
    .with_session(auv_tracing_driver::trace::SessionId::new("music"));
    let run = runtime
      .recording()
      .handle()
      .start_run(spec)
      .expect("explicit-device run should start");
    assert_eq!(run.device_id().as_str(), "remote-mac");
    assert_eq!(run.session_id().as_str(), "music");

    runtime
      .recording()
      .handle()
      .finish_run(
        run,
        auv_tracing_driver::run_builder::RunFinish {
          status_code: auv_tracing_driver::trace::TraceStatusCode::Ok,
          summary: Some("explicit".to_string()),
          failure: None,
        },
      )
      .expect("explicit-device run should finish");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn run_snapshot_stores_device_session_in_attributes() {
    let project_root = temp_dir("runtime-attr-roundtrip-project");
    let store_root = temp_dir("runtime-attr-roundtrip-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let spec = auv_tracing_driver::run_builder::RunSpec::new(
      auv_tracing_driver::trace::RunType::Command,
      "auv.command",
    )
    .with_device(auv_tracing_driver::trace::DeviceId::new("local"))
    .with_session(auv_tracing_driver::trace::SessionId::new("scan"));
    let run = runtime
      .recording()
      .handle()
      .start_run(spec)
      .expect("run should start");
    let run_id = run.id().as_str().to_string();
    runtime
      .recording()
      .handle()
      .finish_run(
        run,
        auv_tracing_driver::run_builder::RunFinish {
          status_code: auv_tracing_driver::trace::TraceStatusCode::Ok,
          summary: Some("attr".to_string()),
          failure: None,
        },
      )
      .expect("run should finish");

    let canonical = runtime.read_run(&run_id).expect("run snapshot should read");
    let attrs = &canonical.run.attributes;
    assert_eq!(
      attrs.get(auv_tracing_driver::trace::RUN_ATTR_DEVICE_ID),
      Some(&json!("local"))
    );
    assert_eq!(
      attrs.get(auv_tracing_driver::trace::RUN_ATTR_SESSION_ID),
      Some(&json!("scan"))
    );

    // Old on-disk layout invariant: `.auv/runs/{run_id}/` directory, no
    // per-device or per-session subdir inserted.
    let run_dir = store_root.join("runs").join(&run_id);
    assert!(run_dir.exists(), "run dir must remain at runs/{{run_id}}");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn runtime_with_success_driver(project_root: PathBuf, store_root: PathBuf) -> Runtime {
    Runtime::new(
      project_root,
      LocalStore::new(store_root).expect("store should initialize"),
    )
  }
}
