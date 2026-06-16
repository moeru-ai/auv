//! Run recorder trait and concrete backends.
//!
//! `RunRecorder` defines a sink that accepts `RunUpdate` events plus artifact
//! bytes. Concrete impls fan updates to in-memory buffers, tokio broadcast
//! channels, composite multi-sinks, or the inspect server HTTP write API.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::broadcast;

use crate::error::AuvResult;
use crate::trace::{ArtifactRecordV1Alpha1, RunId};

use super::update::RunUpdate;
use super::wire::WireUpdate;

const INSPECT_SERVER_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

pub trait RunRecorder: Send + Sync {
  fn record(&self, update: RunUpdate) -> AuvResult<()>;

  fn record_artifact_bytes(
    &self,
    _run_id: &RunId,
    _artifact: &ArtifactRecordV1Alpha1,
    _path: &Path,
  ) -> AuvResult<()> {
    Ok(())
  }

  fn requires_successful_delivery(&self) -> bool {
    false
  }
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

  fn requires_successful_delivery(&self) -> bool {
    self
      .recorders
      .iter()
      .any(|recorder| recorder.requires_successful_delivery())
  }

  fn record_artifact_bytes(
    &self,
    run_id: &RunId,
    artifact: &ArtifactRecordV1Alpha1,
    path: &Path,
  ) -> AuvResult<()> {
    let mut failures = Vec::new();
    for recorder in &self.recorders {
      if let Err(error) = recorder.record_artifact_bytes(run_id, artifact, path) {
        failures.push(error);
      }
    }
    if failures.is_empty() {
      Ok(())
    } else {
      Err(format!(
        "{} recorder target(s) failed to write artifact bytes: {}",
        failures.len(),
        failures.join("; ")
      ))
    }
  }
}

#[derive(Clone)]
pub struct InspectServerRunRecorder {
  base_url: String,
  token: Option<String>,
  required: bool,
}

impl InspectServerRunRecorder {
  pub fn new(base_url: String, token: Option<String>, required: bool) -> Self {
    Self {
      base_url: base_url.trim_end_matches('/').to_string(),
      token,
      required,
    }
  }

  fn handle_failure(&self, message: String) -> AuvResult<()> {
    if self.required {
      Err(message)
    } else {
      eprintln!("warning: {message}");
      Ok(())
    }
  }
}

impl RunRecorder for InspectServerRunRecorder {
  fn record(&self, update: RunUpdate) -> AuvResult<()> {
    let base_url = self.base_url.clone();
    let token = self.token.clone();
    let run_id = update.run_id().as_str().to_string();
    let api_update = WireUpdate(update);
    let result = std::thread::spawn(move || {
      let url = format!("{base_url}/write/runs/{run_id}/updates");
      let client = reqwest::blocking::Client::builder()
        .connect_timeout(INSPECT_SERVER_WRITE_TIMEOUT)
        .timeout(INSPECT_SERVER_WRITE_TIMEOUT)
        .build()
        .map_err(|error| format!("inspect server write client setup failed: {error}"))?;
      let mut request = client
        .post(url)
        .json(&serde_json::json!({ "updates": [api_update] }));
      if let Some(token) = token {
        request = request.bearer_auth(token);
      }
      let response = request
        .send()
        .map_err(|error| format!("inspect server write failed: {error}"))?;
      if response.status().is_success() {
        return Ok(());
      }
      let status = response.status();
      let body = response.text().unwrap_or_else(|_| String::new());
      Err(format!(
        "inspect server write rejected with {status}: {body}"
      ))
    })
    .join()
    .unwrap_or_else(|_| Err("inspect server write failed: client thread panicked".to_string()));

    result.or_else(|message| self.handle_failure(message))
  }

  fn requires_successful_delivery(&self) -> bool {
    self.required
  }

  fn record_artifact_bytes(
    &self,
    run_id: &RunId,
    artifact: &ArtifactRecordV1Alpha1,
    path: &Path,
  ) -> AuvResult<()> {
    let base_url = self.base_url.clone();
    let token = self.token.clone();
    let required = self.required;
    let run_id = run_id.as_str().to_string();
    let artifact_id = artifact.artifact_id.as_str().to_string();
    let span_id = artifact.span_id.as_str().to_string();
    let mime_type = artifact.mime_type.clone();
    let path = path.to_path_buf();
    let result = std::thread::spawn(move || {
      let file = std::fs::File::open(&path)
        .map_err(|error| format!("inspect server artifact upload read failed: {error}"))?;
      let metadata = file
        .metadata()
        .map_err(|error| format!("inspect server artifact upload stat failed: {error}"))?;
      let body = reqwest::blocking::Body::sized(file, metadata.len());
      let mut url = reqwest::Url::parse(&format!(
        "{base_url}/write/runs/{run_id}/artifacts/{artifact_id}"
      ))
      .map_err(|error| format!("inspect server artifact upload url build failed: {error}"))?;
      url.query_pairs_mut().append_pair("spanId", &span_id);
      let client = reqwest::blocking::Client::builder()
        .connect_timeout(INSPECT_SERVER_WRITE_TIMEOUT)
        .timeout(INSPECT_SERVER_WRITE_TIMEOUT)
        .build()
        .map_err(|error| format!("inspect server artifact upload client setup failed: {error}"))?;
      let mut request = client
        .post(url)
        .header(reqwest::header::CONTENT_TYPE, mime_type)
        .body(body);
      if let Some(token) = token {
        request = request.bearer_auth(token);
      }
      let response = request
        .send()
        .map_err(|error| format!("inspect server artifact upload failed: {error}"))?;
      if response.status().is_success() {
        return Ok(());
      }
      let status = response.status();
      let body = response.text().unwrap_or_else(|_| String::new());
      Err(format!(
        "inspect server artifact upload rejected with {status}: {body}"
      ))
    })
    .join()
    .unwrap_or_else(|_| {
      Err("inspect server artifact upload failed: client thread panicked".to_string())
    });

    result.or_else(|message| {
      if required {
        Err(message)
      } else {
        eprintln!("warning: {message}");
        Ok(())
      }
    })
  }
}

#[cfg(test)]
fn inspect_server_write_timeout_for_test() -> Duration {
  INSPECT_SERVER_WRITE_TIMEOUT
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
    ARTIFACT_API_VERSION, ArtifactId, ArtifactRecordV1Alpha1, RUN_API_VERSION, RunId,
    RunRecordV1Alpha1, RunType, SpanId, TraceId, TraceState, TraceStatusCode,
  };

  use super::{InspectServerRunRecorder, RunRecorder, RunUpdate};

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
    fn record(&self, update: RunUpdate) -> crate::error::AuvResult<()> {
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

  #[test]
  fn inspect_server_recorder_has_bounded_request_timeout() {
    assert!(super::inspect_server_write_timeout_for_test() <= std::time::Duration::from_secs(10));
  }

  #[tokio::test(flavor = "multi_thread")]
  async fn inspect_server_recorder_posts_update_batch_with_token() {
    use axum::http::{HeaderMap, header::AUTHORIZATION};
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::Value;
    use tokio::net::TcpListener;

    let captured = Arc::new(Mutex::new(None::<(Option<String>, Value)>));
    let captured_route = captured.clone();
    let app = Router::new().route(
      "/write/runs/run_update_test/updates",
      post(move |headers: HeaderMap, Json(value): Json<Value>| {
        let captured = captured_route.clone();
        async move {
          let authorization = headers
            .get(AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(ToOwned::to_owned);
          *captured.lock().expect("capture lock") = Some((authorization, value));
          Json(serde_json::json!({ "accepted": 1 }))
        }
      }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
      .await
      .expect("bind test server");
    let address = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
      axum::serve(listener, app).await.expect("test server");
    });

    let recorder = InspectServerRunRecorder::new(
      format!("http://{address}"),
      Some("secret".to_string()),
      false,
    );
    recorder
      .record(RunUpdate::RunStarted {
        run_id: RunId::new("run_update_test"),
        run: test_run(),
      })
      .expect("server record should succeed");

    let (authorization, body) = captured
      .lock()
      .expect("capture lock")
      .clone()
      .expect("captured request");
    assert_eq!(authorization.as_deref(), Some("Bearer secret"));
    assert_eq!(body["updates"][0]["type"], "runStarted");
    assert_eq!(body["updates"][0]["runId"], "run_update_test");
    assert_eq!(body["updates"][0]["run"]["apiVersion"], "auv.run.v1alpha1");
  }

  #[tokio::test(flavor = "multi_thread")]
  async fn inspect_server_recorder_uploads_artifact_bytes_with_token() {
    use axum::Router;
    use axum::body::{Body, to_bytes};
    use axum::extract::Query;
    use axum::http::{HeaderMap, header::AUTHORIZATION};
    use axum::routing::post;
    use tokio::net::TcpListener;

    #[derive(Clone, Debug, serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct UploadQuery {
      span_id: Option<String>,
    }

    let captured = Arc::new(Mutex::new(
      None::<(Option<String>, String, Option<String>, Vec<u8>)>,
    ));
    let captured_route = captured.clone();
    let app = Router::new().route(
      "/write/runs/run_update_test/artifacts/artifact_0001",
      post(
        move |headers: HeaderMap, Query(query): Query<UploadQuery>, body: Body| {
          let captured = captured_route.clone();
          async move {
            let authorization = headers
              .get(AUTHORIZATION)
              .and_then(|value| value.to_str().ok())
              .map(ToOwned::to_owned);
            let bytes = to_bytes(body, usize::MAX)
              .await
              .expect("body should read")
              .to_vec();
            *captured.lock().expect("capture lock") = Some((
              authorization,
              "artifact_0001".to_string(),
              query.span_id,
              bytes,
            ));
            "ok"
          }
        },
      ),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
      .await
      .expect("bind test server");
    let address = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
      axum::serve(listener, app).await.expect("test server");
    });
    let path = std::env::temp_dir().join(format!(
      "auv-artifact-upload-source-{}.txt",
      crate::time::now_millis()
    ));
    std::fs::write(&path, "artifact body").expect("artifact source should write");

    let recorder = InspectServerRunRecorder::new(
      format!("http://{address}"),
      Some("secret".to_string()),
      true,
    );
    let artifact = ArtifactRecordV1Alpha1 {
      api_version: ARTIFACT_API_VERSION.to_string(),
      artifact_id: ArtifactId::new("artifact_0001"),
      span_id: SpanId::new("0000000000000001"),
      event_id: None,
      role: "driver.output".to_string(),
      mime_type: "text/plain".to_string(),
      path: "artifacts/artifact_0001_output.txt".to_string(),
      sha256: None,
      attributes: Default::default(),
      summary: None,
    };

    recorder
      .record_artifact_bytes(&RunId::new("run_update_test"), &artifact, &path)
      .expect("artifact upload should succeed");

    let (authorization, artifact_id, span_id, bytes) = captured
      .lock()
      .expect("capture lock")
      .clone()
      .expect("captured request");
    assert_eq!(authorization.as_deref(), Some("Bearer secret"));
    assert_eq!(artifact_id, "artifact_0001");
    assert_eq!(span_id.as_deref(), Some("0000000000000001"));
    assert_eq!(bytes, b"artifact body");
    let _ = std::fs::remove_file(path);
  }

  #[tokio::test(flavor = "multi_thread")]
  async fn inspect_server_recorder_only_fails_rejected_writes_when_required() {
    use axum::http::StatusCode;
    use axum::routing::post;
    use axum::{Json, Router};
    use serde_json::Value;
    use tokio::net::TcpListener;

    let app = Router::new().route(
      "/write/runs/run_update_test/updates",
      post(|Json(_value): Json<Value>| async {
        (
          StatusCode::CONFLICT,
          Json(serde_json::json!({
            "error": {
              "code": "runConflict",
              "message": "duplicate run",
              "retryable": false
            }
          })),
        )
      }),
    );
    let listener = TcpListener::bind("127.0.0.1:0")
      .await
      .expect("bind test server");
    let address = listener.local_addr().expect("local addr");
    tokio::spawn(async move {
      axum::serve(listener, app).await.expect("test server");
    });

    let optional = InspectServerRunRecorder::new(format!("http://{address}"), None, false);
    optional
      .record(RunUpdate::RunStarted {
        run_id: RunId::new("run_update_test"),
        run: test_run(),
      })
      .expect("optional server write rejection should warn and continue");

    let required = InspectServerRunRecorder::new(format!("http://{address}"), None, true);
    let error = required
      .record(RunUpdate::RunStarted {
        run_id: RunId::new("run_update_test"),
        run: test_run(),
      })
      .expect_err("required server write rejection should fail");
    assert!(error.contains("inspect server write rejected"));
    assert!(error.contains("409"));
  }
}
