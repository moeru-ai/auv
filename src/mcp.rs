use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
  ErrorData as McpError, ServerHandler, ServiceExt,
  handler::server::{router::tool::ToolRouter, wrapper::Parameters},
  model::{CallToolResult, Content, JsonObject, ServerCapabilities, ServerInfo},
  tool, tool_handler, tool_router,
  transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::candidate_action_command::CandidateActionCommandRequest;
use crate::candidate_action_decision::CandidateActionKind;
use crate::model::{ExecutionTarget, InvokeRequest};
use crate::{
  build_default_runtime, build_default_store, build_runtime_with_store_root, model::now_millis,
};
use auv_cli_invoke::{ArgSpec, InvokeCommand, default_registry};

#[derive(Clone)]
pub struct McpServer {
  project_root: PathBuf,
  tool_router: ToolRouter<Self>,
}

impl McpServer {
  pub fn new(project_root: PathBuf) -> Self {
    Self {
      project_root,
      tool_router: Self::tool_router(),
    }
  }

  fn store(
    &self,
    store_root: Option<String>,
  ) -> Result<auv_tracing_driver::store::LocalStore, McpError> {
    let store = match store_root {
      Some(root) => auv_tracing_driver::store::LocalStore::new(PathBuf::from(root)),
      None => build_default_store(self.project_root.clone()),
    };
    store.map_err(invalid_params)
  }

  fn runtime(&self, store_root: Option<String>) -> Result<crate::runtime::Runtime, McpError> {
    let runtime = match store_root {
      Some(root) => build_runtime_with_store_root(self.project_root.clone(), PathBuf::from(root)),
      None => build_default_runtime(self.project_root.clone()),
    };
    runtime.map_err(invalid_params)
  }
}

#[tool_router(router = tool_router)]
impl McpServer {
  #[tool(
    description = "Invoke one explicit registry-backed AUV command id through the shared invoke wrapper. See input_schema.x-auv-commands for available command metadata.",
    input_schema = invoke_tool_input_schema()
  )]
  async fn invoke(
    &self,
    Parameters(req): Parameters<InvokeToolRequest>,
  ) -> Result<CallToolResult, McpError> {
    let store = self.store(req.inspect.store_root.clone())?;
    let recording = auv_tracing_driver::RunRecordingBackend::new(
      store,
      Arc::new(auv_tracing_driver::MemoryRunRecorder::new()),
    );
    let registry = default_registry();
    let result = auv_cli_invoke::invoke_recorded(
      &recording,
      &registry,
      InvokeRequest {
        command_id: req.command_id,
        target: ExecutionTarget {
          application_id: req.target.application_id,
          target_label: req.target.target_label,
        },
        inputs: req.inputs,
        dry_run: req.dry_run,
      },
    )
    .map_err(invalid_params)?;

    let artifacts = result
      .artifacts
      .iter()
      .map(|artifact| {
        serde_json::json!({
          "artifact_id": artifact.artifact_id.as_str(),
          "role": artifact.role,
          "path": artifact.path,
        })
      })
      .collect::<Vec<_>>();
    let artifact_paths = result
      .artifact_paths
      .iter()
      .map(|path| path.display().to_string())
      .collect::<Vec<_>>();

    json_result(serde_json::json!({
      "run_id": result.run_id,
      "status": result.status.as_str(),
      "output_summary": result.output_summary,
      "signals": result.signals,
      "artifacts": artifacts,
      "artifact_paths": artifact_paths,
      "failure_message": result.failure_message,
    }))
  }

  #[tool(description = "Inspect one existing AUV run id.")]
  async fn run_inspect(
    &self,
    Parameters(req): Parameters<RunInspectRequest>,
  ) -> Result<CallToolResult, McpError> {
    let store = self.store(req.store_root.clone())?;
    let text = crate::inspect::inspect_run(&store, &req.run_id).map_err(invalid_params)?;
    json_result(serde_json::json!({
      "run_id": req.run_id,
      "text": text,
    }))
  }

  #[tool(
    description = "Run the archived consent-gated candidate-action command through the shared runtime. M0 evidence tool only: direct query/role target, no planner, no model proposer, no consent minting by MCP."
  )]
  async fn candidate_action_run(
    &self,
    Parameters(req): Parameters<CandidateActionRunRequest>,
  ) -> Result<CallToolResult, McpError> {
    let runtime = self.runtime(req.inspect.store_root.clone())?;
    let request = req.into_command_request().map_err(invalid_params)?;
    let output = runtime
      .run_candidate_action_command(request)
      .map_err(invalid_params)?;

    json_result(serde_json::json!({
      "run_id": output.run_id.as_str(),
      "run_dir": output.run_dir.display().to_string(),
      "status": output.value.status.as_str(),
      "proposal_artifact_id": output.value.proposal_artifact_id,
      "promotion_artifact_id": output.value.promotion_artifact_id,
      "decision_artifact_id": output.value.decision_artifact_id,
      "execution_artifact_id": output.value.execution_artifact_id,
      "promotion_refusals": output.value.promotion_refusals,
    }))
  }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for McpServer {
  fn get_info(&self) -> ServerInfo {
    ServerInfo {
      instructions: Some(
        "Thin MCP frontend over AUV runtime. Call explicit tools with explicit command ids; no planner or NL parsing is present.".into(),
      ),
      capabilities: ServerCapabilities::builder().enable_tools().build(),
      ..Default::default()
    }
  }
}

fn invoke_tool_input_schema() -> Arc<JsonObject> {
  let mut schema = rmcp::handler::server::common::cached_schema_for_type::<InvokeToolRequest>()
    .as_ref()
    .clone();
  let registry = default_registry();
  let command_ids = registry
    .all()
    .iter()
    .map(|command| Value::String(command.id.to_string()))
    .collect::<Vec<_>>();

  if let Some(command_id_schema) = schema
    .get_mut("properties")
    .and_then(Value::as_object_mut)
    .and_then(|properties| properties.get_mut("command_id"))
    .and_then(Value::as_object_mut)
  {
    command_id_schema.insert(
      "description".to_string(),
      Value::String(
        "Registry command id. See x-auv-commands on this schema for summaries and argument metadata."
          .to_string(),
      ),
    );
    command_id_schema.insert("enum".to_string(), Value::Array(command_ids));
  }

  schema.insert(
    "x-auv-commands".to_string(),
    Value::Array(
      registry
        .all()
        .iter()
        .map(invoke_command_metadata)
        .collect::<Vec<_>>(),
    ),
  );
  Arc::new(schema)
}

fn invoke_command_metadata(command: &InvokeCommand) -> Value {
  serde_json::json!({
    "id": command.id,
    "namespace": command.namespace.as_str(),
    "summary": command.summary,
    "arguments": command
      .args
      .iter()
      .map(invoke_arg_metadata)
      .collect::<Vec<_>>(),
  })
}

fn invoke_arg_metadata(arg: &ArgSpec) -> Value {
  serde_json::json!({
    "flag": arg.flag,
    "input_key": invoke_arg_input_key(arg.flag),
    "value_name": arg.value_name,
    "required": arg.required,
    "help": arg.help,
  })
}

fn invoke_arg_input_key(flag: &str) -> String {
  match flag {
    "--target" => "target.application_id".to_string(),
    other => other.trim_start_matches("--").to_string(),
  }
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
struct McpInvokeTarget {
  application_id: Option<String>,
  target_label: Option<String>,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
struct McpInspectOptions {
  store_root: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct InvokeToolRequest {
  command_id: String,
  #[serde(default)]
  target: McpInvokeTarget,
  #[serde(default)]
  inputs: BTreeMap<String, String>,
  #[serde(default)]
  dry_run: bool,
  #[serde(default)]
  inspect: McpInspectOptions,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct RunInspectRequest {
  run_id: String,
  #[serde(default)]
  store_root: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct CandidateActionRunRequest {
  target_app: String,
  query: String,
  role: String,
  #[serde(default = "default_candidate_action")]
  action: String,
  #[serde(default)]
  text: Option<String>,
  #[serde(default)]
  dev_self_minted_consent: bool,
  #[serde(default)]
  human_gesture_consent: bool,
  #[serde(default = "default_human_gesture_timeout_ms")]
  human_gesture_timeout_ms: u64,
  #[serde(default)]
  granted_by: String,
  #[serde(default)]
  reveal_shortcut: Option<String>,
  #[serde(default = "default_reveal_settle_ms")]
  reveal_settle_ms: u64,
  #[serde(default = "default_stable_frames")]
  stable_frames: u32,
  #[serde(default)]
  stable_frame_delay_ms: u64,
  #[serde(default = "default_max_centroid_drift_px")]
  max_centroid_drift_px: f64,
  #[serde(default = "default_require_stable_text")]
  require_stable_text: bool,
  #[serde(default)]
  inspect: McpInspectOptions,
}

impl CandidateActionRunRequest {
  fn into_command_request(self) -> Result<CandidateActionCommandRequest, String> {
    let action = parse_candidate_action(&self.action, self.text.as_deref())?;
    let suffix = format!("mcp-m0-{}", now_millis());
    let request = CandidateActionCommandRequest {
      app_bundle_id: self.target_app,
      query: Some(self.query),
      role: Some(self.role),
      action: Some(action),
      intent: None,
      proposer_model: None,
      proposer_base_url: None,
      reveal_shortcut: self.reveal_shortcut,
      reveal_settle_ms: self.reveal_settle_ms,
      stable_frames: self.stable_frames,
      stable_frame_delay_ms: self.stable_frame_delay_ms,
      max_centroid_drift_px: self.max_centroid_drift_px,
      require_stable_text: self.require_stable_text,
      dev_self_minted_consent: self.dev_self_minted_consent,
      human_gesture_consent: self.human_gesture_consent,
      human_gesture_timeout_ms: self.human_gesture_timeout_ms,
      proposal_id: format!("{suffix}-proposal"),
      promotion_id: format!("{suffix}-promotion"),
      decision_id: format!("{suffix}-decision"),
      execution_id: format!("{suffix}-execution"),
      granted_by: self.granted_by,
      promotion_scope_note: "M0 MCP consent/refusal evidence: candidate promotion only".to_string(),
      promotion_evidence_note: "M0 MCP caller supplied consent state; MCP did not mint consent"
        .to_string(),
      execution_scope_note: "M0 MCP consent/refusal evidence: execute one resolved action"
        .to_string(),
      execution_evidence_note: "M0 MCP caller supplied consent state; MCP did not mint consent"
        .to_string(),
    };
    request.validate()?;
    Ok(request)
  }
}

fn parse_candidate_action(action: &str, text: Option<&str>) -> Result<CandidateActionKind, String> {
  match action.trim() {
    "" | "click" => Ok(CandidateActionKind::Click),
    "type_text" | "type-text" => {
      let text = text
        .ok_or_else(|| "text is required when action is type_text".to_string())?
        .to_string();
      Ok(CandidateActionKind::TypeText { text })
    }
    other => Err(format!(
      "unsupported candidate action {other}; expected click or type_text"
    )),
  }
}

fn default_candidate_action() -> String {
  "click".to_string()
}

fn default_human_gesture_timeout_ms() -> u64 {
  15_000
}

fn default_reveal_settle_ms() -> u64 {
  250
}

fn default_stable_frames() -> u32 {
  1
}

fn default_max_centroid_drift_px() -> f64 {
  2.0
}

fn default_require_stable_text() -> bool {
  true
}

fn json_result(value: Value) -> Result<CallToolResult, McpError> {
  Ok(CallToolResult::success(vec![Content::json(value)?]))
}

fn invalid_params(message: impl ToString) -> McpError {
  McpError::invalid_params(message.to_string(), None::<Value>)
}

pub async fn serve_stdio(project_root: PathBuf) -> Result<(), String> {
  let service = McpServer::new(project_root)
    .serve(stdio())
    .await
    .map_err(|error| format!("failed to serve MCP stdio transport: {error}"))?;
  service
    .waiting()
    .await
    .map(|_| ())
    .map_err(|error| format!("mcp stdio server exited with error: {error}"))
}

#[cfg(test)]
mod tests {
  use super::*;
  use rmcp::{
    ClientHandler, ServiceExt,
    model::{CallToolRequestParam, ClientInfo},
  };

  #[derive(Debug, Clone, Default)]
  struct DummyClientHandler;

  impl ClientHandler for DummyClientHandler {
    fn get_info(&self) -> ClientInfo {
      ClientInfo::default()
    }
  }

  #[tokio::test]
  async fn mcp_server_lists_and_invokes_shared_runtime() -> anyhow::Result<()> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let store_root = temp_dir("mcp-shared-runtime-store");
    let (server_transport, client_transport) = tokio::io::duplex(16384);

    let server = McpServer::new(project_root.clone());
    let server_handle = tokio::spawn(async move {
      let service = server.serve(server_transport).await?;
      service.waiting().await?;
      anyhow::Ok(())
    });

    let client = DummyClientHandler::default()
      .serve(client_transport)
      .await?;

    let tools = client.list_tools(Default::default()).await?;
    let tool_names = tools
      .tools
      .iter()
      .map(|tool| tool.name.as_ref())
      .collect::<Vec<_>>();
    assert!(!tool_names.contains(&"bundle_list"));
    assert!(!tool_names.contains(&"bundle_show"));
    assert!(!tool_names.contains(&"skill_list"));
    assert!(!tool_names.contains(&"skill_show"));
    assert!(tool_names.contains(&"invoke"));
    assert!(tool_names.contains(&"run_inspect"));
    assert!(tool_names.contains(&"candidate_action_run"));

    let invoke_tool = tools
      .tools
      .iter()
      .find(|tool| tool.name.as_ref() == "invoke")
      .expect("invoke tool should be listed");
    let invoke_description = invoke_tool
      .description
      .as_ref()
      .expect("invoke tool should have a description");
    assert!(invoke_description.contains("registry"));
    let command_id_schema = invoke_tool
      .input_schema
      .get("properties")
      .and_then(|properties| properties.get("command_id"))
      .expect("invoke schema should describe command_id");
    let command_ids = command_id_schema
      .get("enum")
      .and_then(|value| value.as_array())
      .expect("command_id schema should enumerate registry command ids");
    assert!(command_ids.iter().any(|id| id == "fixture.observe"));
    assert!(command_ids.iter().any(|id| id == "input.pressButton"));
    assert!(!command_ids.iter().any(|id| id == "steam.library.list.v0"));
    assert!(!command_ids.iter().any(|id| id == "debug.captureWindow"));
    assert!(!command_ids.iter().any(|id| id == "verify.axText"));
    assert!(!command_ids.iter().any(|id| id == "music.result.play"));

    let command_metadata = invoke_tool
      .input_schema
      .get("x-auv-commands")
      .and_then(|value| value.as_array())
      .expect("invoke schema should expose registry command metadata");
    let metadata_ids = command_metadata
      .iter()
      .filter_map(|command| command.get("id").and_then(|value| value.as_str()))
      .collect::<Vec<_>>();
    assert!(!metadata_ids.iter().any(|id| id.starts_with("debug.")));
    assert!(!metadata_ids.iter().any(|id| id.starts_with("verify.")));
    assert!(!metadata_ids.iter().any(|id| id.starts_with("music.")));
    assert!(!metadata_ids.iter().any(|id| id.starts_with("steam.")));
    let press_button_metadata = command_metadata
      .iter()
      .find(|command| {
        command.get("id").and_then(|value| value.as_str()) == Some("input.pressButton")
      })
      .expect("input.pressButton metadata should be listed");
    assert_eq!(
      press_button_metadata
        .get("namespace")
        .and_then(|value| value.as_str()),
      Some("input")
    );
    assert!(
      press_button_metadata
        .get("summary")
        .and_then(|value| value.as_str())
        .is_some_and(|summary| summary.contains("query"))
    );
    let press_button_args = press_button_metadata
      .get("arguments")
      .and_then(|value| value.as_array())
      .expect("command metadata should expose argument specs");
    assert!(press_button_args.iter().any(|arg| {
      arg.get("flag").and_then(|value| value.as_str()) == Some("--query")
        && arg.get("required").and_then(|value| value.as_bool()) == Some(true)
    }));
    let now_playing_metadata = command_metadata
      .iter()
      .find(|command| {
        command.get("id").and_then(|value| value.as_str()) == Some("mediaControl.nowPlaying")
      })
      .expect("mediaControl.nowPlaying metadata should be listed");
    assert_eq!(
      now_playing_metadata
        .get("namespace")
        .and_then(|value| value.as_str()),
      Some("mediaControl")
    );

    let invoke = client
      .call_tool(CallToolRequestParam {
        name: "invoke".into(),
        arguments: Some(
          serde_json::json!({
            "command_id": "fixture.observe",
            "dry_run": false,
            "inputs": {},
            "target": {},
            "inspect": {
              "store_root": store_root.display().to_string()
            }
          })
          .as_object()
          .unwrap()
          .clone(),
        ),
      })
      .await?;
    let invoke_json: serde_json::Value = serde_json::from_str(
      &invoke
        .content
        .first()
        .and_then(|content| content.raw.as_text())
        .expect("invoke should return text content")
        .text,
    )
    .expect("invoke text should decode as json");
    let run_id = invoke_json
      .get("run_id")
      .and_then(|value| value.as_str())
      .expect("run_id should exist")
      .to_string();
    assert_eq!(
      invoke_json
        .get("output_summary")
        .and_then(|value| value.as_str()),
      Some("fixture observed")
    );
    assert_eq!(
      invoke_json.get("status").and_then(|value| value.as_str()),
      Some("completed")
    );
    assert_eq!(
      invoke_json.get("signals"),
      Some(&serde_json::Value::Object(Default::default()))
    );
    assert_eq!(
      invoke_json
        .get("artifacts")
        .and_then(|value| value.as_array())
        .map(Vec::len),
      Some(0)
    );

    let inspect = client
      .call_tool(CallToolRequestParam {
        name: "run_inspect".into(),
        arguments: Some(
          serde_json::json!({
            "run_id": run_id,
            "store_root": store_root.display().to_string()
          })
          .as_object()
          .unwrap()
          .clone(),
        ),
      })
      .await?;
    let inspect_json: serde_json::Value = serde_json::from_str(
      &inspect
        .content
        .first()
        .and_then(|content| content.raw.as_text())
        .expect("inspect should return text content")
        .text,
    )
    .expect("inspect text should decode as json");
    let inspect_text = inspect_json
      .get("text")
      .and_then(|value| value.as_str())
      .expect("inspect text should exist");
    assert!(inspect_text.contains("Summary: fixture observed"));
    assert!(inspect_text.contains("name=auv.command.invoke"));
    assert!(inspect_text.contains("resolved fixture.observe"));

    let failed_invoke = client
      .call_tool(CallToolRequestParam {
        name: "invoke".into(),
        arguments: Some(
          serde_json::json!({
            "command_id": "app.activate",
            "dry_run": false,
            "inputs": {},
            "target": {},
            "inspect": {
              "store_root": store_root.display().to_string()
            }
          })
          .as_object()
          .unwrap()
          .clone(),
        ),
      })
      .await?;
    let failed_invoke_json: serde_json::Value = serde_json::from_str(
      &failed_invoke
        .content
        .first()
        .and_then(|content| content.raw.as_text())
        .expect("failed invoke should return text content")
        .text,
    )
    .expect("failed invoke text should decode as json");
    let failed_run_id = failed_invoke_json
      .get("run_id")
      .and_then(|value| value.as_str())
      .expect("failed run_id should exist")
      .to_string();
    assert_eq!(
      failed_invoke_json
        .get("status")
        .and_then(|value| value.as_str()),
      Some("failed")
    );
    assert!(
      failed_invoke_json
        .get("failure_message")
        .and_then(|value| value.as_str())
        .is_some_and(|message| message.contains("typed app activation API"))
    );

    let failed_inspect = client
      .call_tool(CallToolRequestParam {
        name: "run_inspect".into(),
        arguments: Some(
          serde_json::json!({
            "run_id": failed_run_id,
            "store_root": store_root.display().to_string()
          })
          .as_object()
          .unwrap()
          .clone(),
        ),
      })
      .await?;
    let failed_inspect_json: serde_json::Value = serde_json::from_str(
      &failed_inspect
        .content
        .first()
        .and_then(|content| content.raw.as_text())
        .expect("failed inspect should return text content")
        .text,
    )
    .expect("failed inspect text should decode as json");
    let failed_inspect_text = failed_inspect_json
      .get("text")
      .and_then(|value| value.as_str())
      .expect("failed inspect text should exist");
    assert!(failed_inspect_text.contains("Status: error"));
    assert!(failed_inspect_text.contains("command.failed"));
    assert!(failed_inspect_text.contains("typed app activation API"));

    client.cancel().await?;
    server_handle.await??;
    let _ = std::fs::remove_dir_all(store_root);
    Ok(())
  }

  #[test]
  fn candidate_action_run_request_preserves_refusal_first_defaults() {
    let request = CandidateActionRunRequest {
      target_app: "com.apple.TextEdit".to_string(),
      query: "body".to_string(),
      role: "AXTextArea".to_string(),
      action: "click".to_string(),
      text: None,
      dev_self_minted_consent: false,
      human_gesture_consent: false,
      human_gesture_timeout_ms: default_human_gesture_timeout_ms(),
      granted_by: String::new(),
      reveal_shortcut: None,
      reveal_settle_ms: default_reveal_settle_ms(),
      stable_frames: default_stable_frames(),
      stable_frame_delay_ms: 0,
      max_centroid_drift_px: default_max_centroid_drift_px(),
      require_stable_text: default_require_stable_text(),
      inspect: McpInspectOptions::default(),
    };

    let command = request
      .into_command_request()
      .expect("no-consent request should validate and later refuse at runtime");

    assert_eq!(command.app_bundle_id, "com.apple.TextEdit");
    assert!(!command.dev_self_minted_consent);
    assert!(!command.human_gesture_consent);
    assert!(matches!(command.action, Some(CandidateActionKind::Click)));
  }

  #[test]
  fn candidate_action_run_request_does_not_self_mint_without_granted_by() {
    let request = CandidateActionRunRequest {
      target_app: "com.apple.TextEdit".to_string(),
      query: "body".to_string(),
      role: "AXTextArea".to_string(),
      action: "type_text".to_string(),
      text: Some("hello".to_string()),
      dev_self_minted_consent: true,
      human_gesture_consent: false,
      human_gesture_timeout_ms: default_human_gesture_timeout_ms(),
      granted_by: String::new(),
      reveal_shortcut: None,
      reveal_settle_ms: default_reveal_settle_ms(),
      stable_frames: default_stable_frames(),
      stable_frame_delay_ms: 0,
      max_centroid_drift_px: default_max_centroid_drift_px(),
      require_stable_text: default_require_stable_text(),
      inspect: McpInspectOptions::default(),
    };

    let error = request
      .into_command_request()
      .expect_err("dev self-minted consent still requires explicit caller identity");

    assert!(error.contains("--granted-by is required"));
  }

  fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()))
  }
}
