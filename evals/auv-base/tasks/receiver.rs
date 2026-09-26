//! Receipt API behind the Vite test page. Reset never synthesizes keys.

use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use axum::{
  Json, Router,
  extract::State,
  http::{StatusCode, header},
  response::IntoResponse,
  routing::{get, post},
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout};

use crate::support::{now_ms, objects, pause};

struct Receipts {
  command: Value,
  receipt: Value,
  last_state: Value,
  log: Option<std::fs::File>,
  error: Option<String>,
}

#[derive(Clone)]
struct ReceiverState {
  receipts: Arc<Mutex<Receipts>>,
}

pub struct Receiver {
  state: ReceiverState,
  url: url::Url,
  api_task: JoinHandle<()>,
  vite: Child,
}

impl Receiver {
  pub async fn start(log: Option<&Path>) -> Result<Self> {
    let state = ReceiverState {
      receipts: Arc::new(Mutex::new(Receipts {
        command: json!({ "caseId": "startup", "initial": "" }),
        receipt: json!({}),
        last_state: Value::Null,
        log: log.map(std::fs::File::create).transpose()?,
        error: None,
      })),
    };

    let routes = Router::new().route("/command", get(command)).route("/receipt", post(receipt)).with_state(state.clone());

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let api_origin = format!("http://127.0.0.1:{}", listener.local_addr()?.port());

    // Vite owns the page and TS transform. Its proxy keeps the browser's
    // command and receipt requests on one origin with this Rust API.
    let mut vite = Command::new(objects().join("node_modules/.bin/vite"))
      .args(["--config", "vite.config.ts"])
      .current_dir(objects())
      .env("AUV_EVAL_API_ORIGIN", api_origin)
      .env("FORCE_COLOR", "0")
      .stdout(std::process::Stdio::piped())
      .stderr(std::process::Stdio::inherit())
      .kill_on_drop(true)
      .spawn()
      .context("start Vite from test-objects/node_modules; run just eval setup first")?;

    let stdout = vite.stdout.take().context("Vite stdout unavailable")?;
    let mut lines = BufReader::new(stdout).lines();
    let url = timeout(Duration::from_secs(20), async {
      while let Some(line) = lines.next_line().await? {
        if let Some(start) = line.find("http://127.0.0.1:") {
          let address = line[start..].split_whitespace().next().unwrap();
          return Ok::<_, anyhow::Error>(url::Url::parse(address)?);
        }
      }

      anyhow::bail!("Vite exited before reporting its local URL")
    })
    .await
    .context("Vite did not become ready within 20 seconds")??;

    // Keep draining logs so the child cannot block on a full pipe.
    tokio::spawn(async move { while lines.next_line().await.ok().flatten().is_some() {} });

    let errors = state.receipts.clone();
    let api_task = tokio::spawn(async move {
      if let Err(error) = axum::serve(listener, routes).await {
        errors.lock().unwrap().error = Some(error.to_string());
      }
    });

    Ok(Self {
      state,
      url,
      api_task,
      vite,
    })
  }

  pub fn url(&self, title: &str, observe_focus: bool) -> String {
    let mut url = self.url.clone();
    url.query_pairs_mut().append_pair("title", title);

    if observe_focus {
      url.query_pairs_mut().append_pair("observeFocus", "true");
    }

    url.into()
  }

  pub fn snapshot(&self) -> Result<Value> {
    let state = self.state.receipts.lock().unwrap();
    ensure!(state.error.is_none(), "receiver failed: {:?}", state.error);

    Ok(state.receipt.clone())
  }

  pub async fn reset(&self, id: &str, initial: &str) -> Result<()> {
    self.state.receipts.lock().unwrap().command = json!({ "caseId": id, "initial": initial });
    let end = Instant::now() + Duration::from_secs(10);

    loop {
      let receipt = self.snapshot()?;
      if receipt["caseId"] == id {
        ensure!(receipt["value"] == initial && receipt["inputSelected"] == true, "receiver reset state mismatch");
        ensure!(receipt["events"].as_array().is_some_and(Vec::is_empty), "reset generated input events");

        return Ok(());
      }

      ensure!(Instant::now() < end, "receiver reset timed out");
      pause(50).await?;
    }
  }
}

impl Drop for Receiver {
  fn drop(&mut self) {
    self.api_task.abort();
    let _ = self.vite.start_kill();
  }
}

async fn command(State(state): State<ReceiverState>) -> impl IntoResponse {
  ([(header::CACHE_CONTROL, "no-store")], Json(state.receipts.lock().unwrap().command.clone()))
}

async fn receipt(State(state): State<ReceiverState>, Json(value): Json<Value>) -> StatusCode {
  let mut state = state.receipts.lock().unwrap();
  let Some(sequence) = value["sequence"].as_u64() else {
    return StatusCode::BAD_REQUEST;
  };

  if state.receipt["sequence"].as_u64().is_none_or(|previous| sequence > previous) {
    let mut observed = json!({});
    for key in [
      "caseId",
      "value",
      "selection",
      "documentFocused",
      "inputSelected",
    ] {
      observed[key] = value[key].clone();
    }

    if observed != state.last_state {
      state.last_state = observed.clone();
      observed["timeMs"] = json!(now_ms());

      if let Some(log) = &mut state.log
        && let Err(error) = writeln!(log, "{observed}")
      {
        state.error = Some(error.to_string());
      }
    }

    state.receipt = value;
  }

  StatusCode::NO_CONTENT
}
