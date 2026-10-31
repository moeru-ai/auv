//! The real stdio MCP client used by isolated keyboard acceptance.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, ensure};
use rmcp::model::{CallToolRequest, CallToolRequestParam, ClientRequest};
use rmcp::service::{PeerRequestOptions, RunningService};
use rmcp::{RoleClient, ServiceExt};
use serde_json::{Value, json};

use super::super::super::super::receiver_process::Receiver;

pub(super) struct Client {
  service: Option<RunningService<RoleClient, ()>>,
  child: tokio::process::Child,
  // The runtime outlives the service and child, including failure-path drops.
  runtime: tokio::runtime::Runtime,
  pid: u32,
}

impl Client {
  pub(super) fn spawn(mut command: Command, log: &Path) -> Result<Self> {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    command.args(["mcp", "serve"]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(std::fs::File::create(log)?);
    let child = {
      let _entered = runtime.enter();
      tokio::process::Command::from(command).kill_on_drop(true).spawn()?
    };
    let pid = child.id().context("MCP process omitted its PID")?;
    Ok(Self {
      service: None,
      child,
      runtime,
      pid,
    })
  }

  /// Admit the actual server and invoke tool before any desktop action.
  pub(super) fn initialize(&mut self) -> Result<Value> {
    let stdout = self.child.stdout.take().context("MCP stdout missing")?;
    let stdin = self.child.stdin.take().context("MCP stdin missing")?;
    let service = self.runtime.block_on(async {
      tokio::time::timeout(Duration::from_secs(10), ().serve((stdout, stdin)))
        .await
        .context("MCP initialization exceeded ten seconds")?
        .context("MCP initialization failed")
    })?;
    self.service = Some(service);
    let service = self.service.as_ref().unwrap();
    let info = service.peer_info().context("MCP initialization returned no server information")?;
    ensure!(info.capabilities.tools.is_some(), "MCP server did not advertise tools");
    let tools = self.runtime.block_on(async {
      tokio::time::timeout(Duration::from_secs(10), service.list_all_tools())
        .await
        .context("MCP tools/list exceeded ten seconds")?
        .context("MCP tools/list failed")
    })?;
    let invoke = tools.iter().find(|tool| tool.name == "invoke").context("MCP server omitted invoke")?;
    Ok(json!({"pid": self.pid, "server": info, "invoke": invoke, "tool_count": tools.len()}))
  }

  pub(super) fn invoke(&self, arguments: Value) -> Result<Value> {
    let service = self.service.as_ref().context("MCP handshake was not admitted")?;
    let request = CallToolRequestParam {
      name: "invoke".into(),
      arguments: Some(arguments.as_object().context("MCP arguments must be an object")?.clone()),
    };
    let result = self.runtime.block_on(async {
      tokio::time::timeout(Duration::from_secs(15), service.call_tool(request))
        .await
        .context("MCP invoke exceeded fifteen seconds")?
        .context("MCP invoke transport failed")
    })?;
    Ok(serde_json::to_value(result)?)
  }

  pub(super) fn cancel_after_first(&self, arguments: Value, receiver: &mut Receiver) -> Result<Value> {
    let service = self.service.as_ref().context("MCP handshake was not admitted")?;
    let params = CallToolRequestParam {
      name: "invoke".into(),
      arguments: Some(arguments.as_object().context("MCP arguments must be an object")?.clone()),
    };
    self.runtime.block_on(async {
      let request = service
        .send_request_with_option(ClientRequest::CallToolRequest(CallToolRequest::new(params)), PeerRequestOptions::no_options())
        .await?;
      let id = request.id.clone();
      let deadline = Instant::now() + Duration::from_secs(5);
      let acknowledged = loop {
        let state = receiver.request(json!({"cmd": "state"}))?;
        let text = state["typing"]["text"].as_str().context("cancel observer omitted text")?.to_owned();
        if !text.is_empty() {
          break text;
        }
        if Instant::now() >= deadline {
          request.cancel(Some("application acknowledgement timed out".into())).await?;
          anyhow::bail!("application never acknowledged the first key");
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
      };
      request.cancel(Some("isolated cancellation after application acknowledgement".into())).await?;
      // This is a finite late-input check beyond the two remaining 400 ms
      // intervals, not proof of an unlimited quiet period or a native receipt.
      tokio::time::sleep(Duration::from_millis(1400)).await;
      Ok(json!({"request_id": id, "acknowledged_text": acknowledged, "cancellation_sent": true,
        "wire_reply_unknown": true, "late_input_window_ms": 1400}))
    })
  }

  pub(super) fn close(&mut self) -> Result<Value> {
    if let Some(service) = self.service.take() {
      self.runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), service.cancel()).await.context("MCP client shutdown exceeded five seconds")??;
        Ok::<_, anyhow::Error>(())
      })?;
    }
    let (status, forced) = self.runtime.block_on(async {
      match tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await {
        Ok(status) => Ok::<_, anyhow::Error>((status?, false)),
        Err(_) => {
          self.child.start_kill()?;
          Ok((tokio::time::timeout(Duration::from_secs(3), self.child.wait()).await.context("MCP process did not exit after kill")??, true))
        }
      }
    })?;
    Ok(json!({"pid": self.pid, "process_exited": true, "exit": status.code(), "forced_kill": forced}))
  }
}

impl Drop for Client {
  fn drop(&mut self) {
    // Explicit close writes the exit receipt; failure-path Drop only terminates
    // this owned server and cannot promote cleanup to a verified outcome.
    if self.child.try_wait().ok().flatten().is_none() {
      let _ = self.child.start_kill();
    }
  }
}
