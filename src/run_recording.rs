use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use crate::model::AuvResult;
use crate::store::{ArtifactFileSource, CanonicalRun, LocalStore};
use crate::trace::{
  ArtifactId, ArtifactRecordV1Alpha1, EventRecordV1Alpha1, RunId, RunRecordV1Alpha1, SpanId,
  SpanRecordV1Alpha1,
};

#[derive(Clone, Debug, PartialEq)]
pub enum RunUpdate {
  RunStarted {
    run_id: RunId,
    run: RunRecordV1Alpha1,
  },
  SpanStarted {
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  EventAppended {
    run_id: RunId,
    event: EventRecordV1Alpha1,
  },
  ArtifactCreated {
    run_id: RunId,
    artifact: ArtifactRecordV1Alpha1,
  },
  SpanFinished {
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  RunFinished {
    run_id: RunId,
    run: RunRecordV1Alpha1,
  },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InspectServerSession {
  pub url: String,
  pub store_root: String,
  pub write_enabled: bool,
  pub write_token: Option<String>,
  pub pid: u32,
  pub started_at_millis: u128,
}

pub fn default_session_path() -> std::path::PathBuf {
  std::env::var_os("AUV_INSPECT_SESSION")
    .map(std::path::PathBuf::from)
    .unwrap_or_else(|| std::env::temp_dir().join("auv-inspect-session.json"))
}

pub fn write_inspect_session(session: &InspectServerSession) -> AuvResult<()> {
  let path = default_session_path();
  if let Some(parent) = path.parent()
    && !parent.as_os_str().is_empty()
  {
    std::fs::create_dir_all(parent)
      .map_err(|error| format!("failed to create inspect session directory: {error}"))?;
  }
  let bytes = serde_json::to_vec_pretty(session)
    .map_err(|error| format!("failed to encode inspect session: {error}"))?;
  write_inspect_session_bytes(&path, &bytes)
}

fn write_inspect_session_bytes(path: &Path, bytes: &[u8]) -> AuvResult<()> {
  let temp_path = inspect_session_temp_path(path)?;
  let write_result = (|| {
    let mut file = create_inspect_session_temp_file(&temp_path)?;
    file.write_all(bytes).map_err(|error| {
      format!(
        "failed to write inspect session {}: {error}",
        temp_path.display()
      )
    })?;
    file.sync_all().map_err(|error| {
      format!(
        "failed to sync inspect session {}: {error}",
        temp_path.display()
      )
    })?;
    drop(file);
    replace_inspect_session_file(&temp_path, path)
  })();

  if let Err(error) = write_result {
    let _ = std::fs::remove_file(&temp_path);
    return Err(error);
  }

  Ok(())
}

fn inspect_session_temp_path(path: &Path) -> AuvResult<PathBuf> {
  let parent = path
    .parent()
    .filter(|parent| !parent.as_os_str().is_empty())
    .unwrap_or_else(|| Path::new("."));
  let file_name = path
    .file_name()
    .ok_or_else(|| format!("inspect session path {} has no file name", path.display()))?;
  Ok(parent.join(format!(
    ".{}.{}.{}.tmp",
    file_name.to_string_lossy(),
    std::process::id(),
    crate::model::now_millis()
  )))
}

#[cfg(unix)]
fn create_inspect_session_temp_file(path: &Path) -> AuvResult<std::fs::File> {
  use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

  let file = std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .mode(0o600)
    .open(path)
    .map_err(|error| {
      format!(
        "failed to create inspect session temp file {}: {error}",
        path.display()
      )
    })?;
  std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).map_err(|error| {
    format!(
      "failed to restrict inspect session temp file {}: {error}",
      path.display()
    )
  })?;
  Ok(file)
}

#[cfg(not(unix))]
fn create_inspect_session_temp_file(path: &Path) -> AuvResult<std::fs::File> {
  std::fs::OpenOptions::new()
    .write(true)
    .create_new(true)
    .open(path)
    .map_err(|error| {
      format!(
        "failed to create inspect session temp file {}: {error}",
        path.display()
      )
    })
}

#[cfg(unix)]
fn replace_inspect_session_file(temp_path: &Path, path: &Path) -> AuvResult<()> {
  std::fs::rename(temp_path, path).map_err(|error| {
    format!(
      "failed to replace inspect session {} from {}: {error}",
      path.display(),
      temp_path.display()
    )
  })
}

#[cfg(not(unix))]
fn replace_inspect_session_file(temp_path: &Path, path: &Path) -> AuvResult<()> {
  let _ = std::fs::remove_file(path);
  std::fs::rename(temp_path, path).map_err(|error| {
    format!(
      "failed to replace inspect session {} from {}: {error}",
      path.display(),
      temp_path.display()
    )
  })
}

pub fn read_inspect_session() -> AuvResult<Option<InspectServerSession>> {
  let path = default_session_path();
  if !path.exists() {
    return Ok(None);
  }
  let raw = std::fs::read_to_string(&path)
    .map_err(|error| format!("failed to read inspect session {}: {error}", path.display()))?;
  serde_json::from_str(&raw).map(Some).map_err(|error| {
    format!(
      "failed to parse inspect session {}: {error}",
      path.display()
    )
  })
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRunRecord {
  pub api_version: String,
  pub run_id: RunId,
  pub trace_id: crate::trace::TraceId,
  pub run_type: crate::trace::RunType,
  pub state: crate::trace::TraceState,
  pub status_code: crate::trace::TraceStatusCode,
  pub started_at_millis: u128,
  pub finished_at_millis: Option<u128>,
  pub root_span_id: SpanId,
  pub attributes: crate::recording::Attributes,
  pub summary: Option<String>,
  pub failure: Option<crate::trace::TraceFailure>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSpanRecord {
  pub api_version: String,
  pub span_id: SpanId,
  pub parent_span_id: Option<SpanId>,
  pub name: String,
  pub state: crate::trace::TraceState,
  pub status_code: crate::trace::TraceStatusCode,
  pub started_at_millis: u128,
  pub finished_at_millis: Option<u128>,
  pub attributes: crate::recording::Attributes,
  pub summary: Option<String>,
  pub failure: Option<crate::trace::TraceFailure>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiEventRecord {
  pub api_version: String,
  pub event_id: crate::trace::EventId,
  pub span_id: SpanId,
  pub name: String,
  pub timestamp_millis: u128,
  pub attributes: crate::recording::Attributes,
  pub message: Option<String>,
  pub artifact_ids: Vec<ArtifactId>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiArtifactRecord {
  pub api_version: String,
  pub artifact_id: ArtifactId,
  pub span_id: SpanId,
  pub event_id: Option<crate::trace::EventId>,
  pub role: String,
  pub mime_type: String,
  pub path: String,
  pub sha256: Option<String>,
  pub attributes: crate::recording::Attributes,
  pub summary: Option<String>,
}

impl From<RunRecordV1Alpha1> for ApiRunRecord {
  fn from(record: RunRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      run_id: record.run_id,
      trace_id: record.trace_id,
      run_type: record.run_type,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: record.started_at_millis,
      finished_at_millis: record.finished_at_millis,
      root_span_id: record.root_span_id,
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<ApiRunRecord> for RunRecordV1Alpha1 {
  fn from(record: ApiRunRecord) -> Self {
    Self {
      api_version: record.api_version,
      run_id: record.run_id,
      trace_id: record.trace_id,
      run_type: record.run_type,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: record.started_at_millis,
      finished_at_millis: record.finished_at_millis,
      root_span_id: record.root_span_id,
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<SpanRecordV1Alpha1> for ApiSpanRecord {
  fn from(record: SpanRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      span_id: record.span_id,
      parent_span_id: record.parent_span_id,
      name: record.name,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: record.started_at_millis,
      finished_at_millis: record.finished_at_millis,
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<ApiSpanRecord> for SpanRecordV1Alpha1 {
  fn from(record: ApiSpanRecord) -> Self {
    Self {
      api_version: record.api_version,
      span_id: record.span_id,
      parent_span_id: record.parent_span_id,
      name: record.name,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: record.started_at_millis,
      finished_at_millis: record.finished_at_millis,
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<EventRecordV1Alpha1> for ApiEventRecord {
  fn from(record: EventRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      event_id: record.event_id,
      span_id: record.span_id,
      name: record.name,
      timestamp_millis: record.timestamp_millis,
      attributes: record.attributes,
      message: record.message,
      artifact_ids: record.artifact_ids,
    }
  }
}

impl From<ApiEventRecord> for EventRecordV1Alpha1 {
  fn from(record: ApiEventRecord) -> Self {
    Self {
      api_version: record.api_version,
      event_id: record.event_id,
      span_id: record.span_id,
      name: record.name,
      timestamp_millis: record.timestamp_millis,
      attributes: record.attributes,
      message: record.message,
      artifact_ids: record.artifact_ids,
    }
  }
}

impl From<ArtifactRecordV1Alpha1> for ApiArtifactRecord {
  fn from(record: ArtifactRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      artifact_id: record.artifact_id,
      span_id: record.span_id,
      event_id: record.event_id,
      role: record.role,
      mime_type: record.mime_type,
      path: record.path,
      sha256: record.sha256,
      attributes: record.attributes,
      summary: record.summary,
    }
  }
}

impl From<ApiArtifactRecord> for ArtifactRecordV1Alpha1 {
  fn from(record: ApiArtifactRecord) -> Self {
    Self {
      api_version: record.api_version,
      artifact_id: record.artifact_id,
      span_id: record.span_id,
      event_id: record.event_id,
      role: record.role,
      mime_type: record.mime_type,
      path: record.path,
      sha256: record.sha256,
      attributes: record.attributes,
      summary: record.summary,
    }
  }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ApiRunUpdate {
  RunStarted {
    #[serde(rename = "runId")]
    run_id: RunId,
    run: ApiRunRecord,
  },
  SpanStarted {
    #[serde(rename = "runId")]
    run_id: RunId,
    span: ApiSpanRecord,
  },
  EventAppended {
    #[serde(rename = "runId")]
    run_id: RunId,
    event: ApiEventRecord,
  },
  ArtifactCreated {
    #[serde(rename = "runId")]
    run_id: RunId,
    artifact: ApiArtifactRecord,
  },
  SpanFinished {
    #[serde(rename = "runId")]
    run_id: RunId,
    span: ApiSpanRecord,
  },
  RunFinished {
    #[serde(rename = "runId")]
    run_id: RunId,
    run: ApiRunRecord,
  },
}

impl From<RunUpdate> for ApiRunUpdate {
  fn from(update: RunUpdate) -> Self {
    match update {
      RunUpdate::RunStarted { run_id, run } => Self::RunStarted {
        run_id,
        run: run.into(),
      },
      RunUpdate::SpanStarted { run_id, span } => Self::SpanStarted {
        run_id,
        span: span.into(),
      },
      RunUpdate::EventAppended { run_id, event } => Self::EventAppended {
        run_id,
        event: event.into(),
      },
      RunUpdate::ArtifactCreated { run_id, artifact } => Self::ArtifactCreated {
        run_id,
        artifact: artifact.into(),
      },
      RunUpdate::SpanFinished { run_id, span } => Self::SpanFinished {
        run_id,
        span: span.into(),
      },
      RunUpdate::RunFinished { run_id, run } => Self::RunFinished {
        run_id,
        run: run.into(),
      },
    }
  }
}

impl From<ApiRunUpdate> for RunUpdate {
  fn from(update: ApiRunUpdate) -> Self {
    match update {
      ApiRunUpdate::RunStarted { run_id, run } => Self::RunStarted {
        run_id,
        run: run.into(),
      },
      ApiRunUpdate::SpanStarted { run_id, span } => Self::SpanStarted {
        run_id,
        span: span.into(),
      },
      ApiRunUpdate::EventAppended { run_id, event } => Self::EventAppended {
        run_id,
        event: event.into(),
      },
      ApiRunUpdate::ArtifactCreated { run_id, artifact } => Self::ArtifactCreated {
        run_id,
        artifact: artifact.into(),
      },
      ApiRunUpdate::SpanFinished { run_id, span } => Self::SpanFinished {
        run_id,
        span: span.into(),
      },
      ApiRunUpdate::RunFinished { run_id, run } => Self::RunFinished {
        run_id,
        run: run.into(),
      },
    }
  }
}

impl RunUpdate {
  pub fn run_id(&self) -> &RunId {
    match self {
      Self::RunStarted { run_id, .. }
      | Self::SpanStarted { run_id, .. }
      | Self::EventAppended { run_id, .. }
      | Self::ArtifactCreated { run_id, .. }
      | Self::SpanFinished { run_id, .. }
      | Self::RunFinished { run_id, .. } => run_id,
    }
  }
}

pub trait RunRecorder: Send + Sync {
  fn record(&self, update: RunUpdate) -> AuvResult<()>;
}

pub struct NoopRunRecorder;

impl RunRecorder for NoopRunRecorder {
  fn record(&self, _update: RunUpdate) -> AuvResult<()> {
    Ok(())
  }
}

pub struct CompositeRunRecorder {
  recorders: Vec<Arc<dyn RunRecorder>>,
}

impl CompositeRunRecorder {
  pub fn new(recorders: Vec<Arc<dyn RunRecorder>>) -> Self {
    Self { recorders }
  }
}

impl RunRecorder for CompositeRunRecorder {
  fn record(&self, update: RunUpdate) -> AuvResult<()> {
    let mut failures = Vec::new();
    for recorder in &self.recorders {
      if let Err(error) = recorder.record(update.clone()) {
        failures.push(error);
      }
    }
    if failures.is_empty() {
      Ok(())
    } else {
      Err(format!(
        "{} recorder target(s) failed: {}",
        failures.len(),
        failures.join("; ")
      ))
    }
  }
}

#[derive(Clone)]
pub struct RunRecordingBackend {
  store: LocalStore,
  recorder: Arc<dyn RunRecorder>,
}

impl RunRecordingBackend {
  pub fn new(store: LocalStore, recorder: Arc<dyn RunRecorder>) -> Self {
    Self { store, recorder }
  }

  pub fn local_only(store: LocalStore) -> Self {
    Self {
      store,
      recorder: Arc::new(NoopRunRecorder),
    }
  }

  pub fn store(&self) -> &LocalStore {
    &self.store
  }

  pub fn recorder(&self) -> Arc<dyn RunRecorder> {
    self.recorder.clone()
  }

  pub fn record(&self, update: RunUpdate) -> AuvResult<()> {
    self.recorder.record(update)
  }

  pub fn read_run(&self, run_id: &str) -> AuvResult<CanonicalRun> {
    self.store.read_run(run_id)
  }

  pub fn write_run_snapshot(&self, snapshot: &CanonicalRun) -> AuvResult<()> {
    self.store.write_run_snapshot(snapshot)
  }

  pub fn run_dir(&self, run_id: impl AsRef<str>) -> AuvResult<std::path::PathBuf> {
    self.store.run_dir(run_id)
  }

  pub fn stage_artifact(
    &self,
    run_id: &RunId,
    index: usize,
    artifact: crate::model::ProducedArtifact,
    span_id: &SpanId,
    event_id: Option<crate::trace::EventId>,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    self
      .store
      .stage_artifact(run_id, index, artifact, span_id, event_id)
  }

  pub fn stage_artifact_file(
    &self,
    run_id: &RunId,
    index: usize,
    span_id: &SpanId,
    event_id: Option<crate::trace::EventId>,
    artifact: ArtifactFileSource,
  ) -> AuvResult<ArtifactRecordV1Alpha1> {
    self
      .store
      .stage_artifact_file(run_id, index, span_id, event_id, artifact)
  }
}

#[derive(Clone)]
pub struct MemoryRunRecorder {
  updates: Arc<Mutex<Vec<RunUpdate>>>,
}

impl MemoryRunRecorder {
  pub fn new() -> Self {
    Self {
      updates: Arc::new(Mutex::new(Vec::new())),
    }
  }

  pub fn drain_for_test(&self) -> Vec<RunUpdate> {
    self
      .updates
      .lock()
      .map(|updates| updates.clone())
      .unwrap_or_default()
  }
}

impl Default for MemoryRunRecorder {
  fn default() -> Self {
    Self::new()
  }
}

impl RunRecorder for MemoryRunRecorder {
  fn record(&self, update: RunUpdate) -> AuvResult<()> {
    if let Ok(mut updates) = self.updates.lock() {
      updates.push(update);
    }
    Ok(())
  }
}

#[derive(Clone)]
pub struct BroadcastRunRecorder {
  sender: broadcast::Sender<RunUpdate>,
}

impl BroadcastRunRecorder {
  pub fn new(capacity: usize) -> Self {
    let (sender, _) = broadcast::channel(capacity);
    Self { sender }
  }

  pub fn subscribe(&self) -> broadcast::Receiver<RunUpdate> {
    self.sender.subscribe()
  }
}

impl RunRecorder for BroadcastRunRecorder {
  fn record(&self, update: RunUpdate) -> AuvResult<()> {
    let _ = self.sender.send(update);
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use std::sync::{Arc, Mutex};

  use crate::trace::{
    RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SpanId, TraceId, TraceState,
    TraceStatusCode,
  };

  use super::{ApiRunUpdate, InspectServerSession, RunRecorder, RunUpdate, write_inspect_session};

  static ENV_LOCK: Mutex<()> = Mutex::new(());

  #[derive(Default)]
  struct CapturingRecorder {
    updates: Mutex<Vec<RunUpdate>>,
  }

  impl CapturingRecorder {
    fn updates(&self) -> Vec<RunUpdate> {
      self.updates.lock().expect("updates lock").clone()
    }
  }

  impl RunRecorder for CapturingRecorder {
    fn record(&self, update: RunUpdate) -> crate::model::AuvResult<()> {
      self.updates.lock().expect("updates lock").push(update);
      Ok(())
    }
  }

  fn test_run() -> RunRecordV1Alpha1 {
    RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: RunId::new("run_update_test"),
      trace_id: TraceId::new("00000000000000000000000000000001"),
      run_type: RunType::Execute,
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: 100,
      finished_at_millis: None,
      root_span_id: SpanId::new("0000000000000001"),
      attributes: Default::default(),
      summary: None,
      failure: None,
    }
  }

  #[test]
  fn run_update_serializes_public_shape_as_camel_case() {
    let update = RunUpdate::RunStarted {
      run_id: RunId::new("run_update_test"),
      run: test_run(),
    };

    let value = serde_json::to_value(ApiRunUpdate::from(update)).expect("update should serialize");
    assert_eq!(value["type"], "runStarted");
    assert_eq!(value["runId"], "run_update_test");
    assert_eq!(value["run"]["apiVersion"], "auv.run.v1alpha1");
    assert_eq!(value["run"]["rootSpanId"], "0000000000000001");
    assert!(value["run"].get("root_span_id").is_none());
  }

  #[test]
  fn composite_recorder_fans_out_to_every_target() {
    let first = Arc::new(CapturingRecorder::default());
    let second = Arc::new(CapturingRecorder::default());
    let recorder = super::CompositeRunRecorder::new(vec![first.clone(), second.clone()]);
    let update = RunUpdate::RunStarted {
      run_id: RunId::new("run_update_test"),
      run: test_run(),
    };

    recorder
      .record(update.clone())
      .expect("fanout should succeed");

    assert_eq!(first.updates(), vec![update.clone()]);
    assert_eq!(second.updates(), vec![update]);
  }

  #[cfg(unix)]
  #[test]
  fn write_inspect_session_replaces_file_with_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let _guard = ENV_LOCK.lock().expect("env lock");
    let root = std::env::temp_dir().join(format!(
      "auv-inspect-session-permissions-{}",
      crate::model::now_millis()
    ));
    let path = root.join("session.json");
    std::fs::create_dir_all(&root).expect("session test directory should write");
    std::fs::write(&path, "{}").expect("existing session file should write");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
      .expect("existing session file permissions should change");
    unsafe {
      std::env::set_var("AUV_INSPECT_SESSION", &path);
    }

    write_inspect_session(&InspectServerSession {
      url: "http://127.0.0.1:8765".to_string(),
      store_root: root.display().to_string(),
      write_enabled: true,
      write_token: Some("secret".to_string()),
      pid: 123,
      started_at_millis: 456,
    })
    .expect("session should write");

    let mode = std::fs::metadata(&path)
      .expect("session file should exist")
      .permissions()
      .mode()
      & 0o777;
    unsafe {
      std::env::remove_var("AUV_INSPECT_SESSION");
    }
    let _ = std::fs::remove_dir_all(root);

    assert_eq!(mode, 0o600);
  }
}
