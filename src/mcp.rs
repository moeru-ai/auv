use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use rmcp::{
  ErrorData as McpError, ServerHandler, ServiceExt,
  handler::server::{router::tool::ToolRouter, wrapper::Parameters},
  model::{CallToolResult, Content, JsonObject, ListToolsResult, PaginatedRequestParam, ServerCapabilities, ServerInfo},
  service::{RequestContext, RoleServer},
  tool, tool_router,
  transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::build_default_store;
use crate::model::{ExecutionTarget, InvokeRequest};
use auv_cli_invoke::{ArgSpec, InvokeCommand, default_registry};

#[derive(Clone)]
pub struct McpServer {
  project_root: PathBuf,
  tool_router: ToolRouter<Self>,
  inspect_composer: Arc<auv_inspect_model::InspectComposer>,
}

impl McpServer {
  /// Builds the core-only MCP server.
  ///
  /// Product callers inject their composer through
  /// [`Self::with_inspect_composer`]; this core library does not construct
  /// app-specific inspect sections.
  pub fn new(project_root: PathBuf) -> Self {
    Self::with_inspect_composer(project_root, crate::inspect::build_core_inspect_composer().expect("core inspect composer"))
  }

  pub fn with_inspect_composer(project_root: PathBuf, inspect_composer: Arc<auv_inspect_model::InspectComposer>) -> Self {
    Self {
      project_root,
      tool_router: Self::tool_router(),
      inspect_composer,
    }
  }

  /// Shared composer used by MCP text inspect (same instance as CLI product assembly).
  pub fn inspect_composer(&self) -> &Arc<auv_inspect_model::InspectComposer> {
    &self.inspect_composer
  }

  fn store(&self, store_root: Option<String>) -> Result<auv_tracing_driver::store::LocalStore, McpError> {
    let store = match store_root {
      Some(root) => auv_tracing_driver::store::LocalStore::new(PathBuf::from(root)),
      None => build_default_store(self.project_root.clone()),
    };
    store.map_err(invalid_params)
  }
}

#[tool_router(router = tool_router)]
impl McpServer {
  #[tool(
    description = "Invoke one explicit registry-backed AUV command id through the shared invoke wrapper. See input_schema.x-auv-commands for available command metadata.",
    input_schema = invoke_tool_input_schema()
  )]
  async fn invoke(&self, Parameters(req): Parameters<InvokeToolRequest>) -> Result<CallToolResult, McpError> {
    let store = self.store(req.inspect.store_root.clone())?;
    let recording = auv_tracing_driver::RunRecordingBackend::new(store, Arc::new(auv_tracing_driver::MemoryRunRecorder::new()));
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
    let artifact_paths = result.artifact_paths.iter().map(|path| path.display().to_string()).collect::<Vec<_>>();

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
  async fn run_inspect(&self, Parameters(req): Parameters<RunInspectRequest>) -> Result<CallToolResult, McpError> {
    let store = self.store(req.store_root.clone())?;
    let run = store.read_run(&req.run_id).map_err(invalid_params)?;
    let document = self.inspect_composer.collect_document(&store, &run).map_err(|error| invalid_params(error.to_string()))?;
    let text = document.render_text();
    let sections = document
      .sections
      .iter()
      .map(|section| {
        serde_json::json!({
          "id": section.id,
          "text": section.text,
          "json": section.json,
        })
      })
      .collect::<Vec<_>>();
    json_result(serde_json::json!({
      "run_id": req.run_id,
      "text": text,
      "sections": sections,
    }))
  }
}

impl ServerHandler for McpServer {
  async fn call_tool(
    &self,
    request: rmcp::model::CallToolRequestParam,
    context: RequestContext<RoleServer>,
  ) -> Result<CallToolResult, McpError> {
    let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
    self.tool_router.call(tcc).await
  }

  async fn list_tools(
    &self,
    _request: Option<PaginatedRequestParam>,
    _context: RequestContext<RoleServer>,
  ) -> Result<ListToolsResult, McpError> {
    Ok(ListToolsResult::with_all_items(self.tool_router.list_all()))
  }

  fn get_info(&self) -> ServerInfo {
    ServerInfo {
      instructions: Some(
        "MCP exposes explicit AUV tools, including a registry-backed invoke wrapper for generic commands; no planner or NL parsing is present.".into(),
      ),
      capabilities: ServerCapabilities::builder().enable_tools().build(),
      ..Default::default()
    }
  }
}

fn invoke_tool_input_schema() -> Arc<JsonObject> {
  let mut schema = rmcp::handler::server::common::cached_schema_for_type::<InvokeToolRequest>().as_ref().clone();
  let registry = default_registry();
  let command_ids = registry.all().iter().map(|command| Value::String(command.id.to_string())).collect::<Vec<_>>();

  if let Some(command_id_schema) = schema
    .get_mut("properties")
    .and_then(Value::as_object_mut)
    .and_then(|properties| properties.get_mut("command_id"))
    .and_then(Value::as_object_mut)
  {
    command_id_schema.insert(
      "description".to_string(),
      Value::String("Registry command id. See x-auv-commands on this schema for summaries and argument metadata.".to_string()),
    );
    command_id_schema.insert("enum".to_string(), Value::Array(command_ids));
  }

  schema.insert("x-auv-commands".to_string(), Value::Array(registry.all().iter().map(invoke_command_metadata).collect::<Vec<_>>()));
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

fn json_result(value: Value) -> Result<CallToolResult, McpError> {
  Ok(CallToolResult::success(vec![Content::json(value)?]))
}

fn invalid_params(message: impl ToString) -> McpError {
  McpError::invalid_params(message.to_string(), None::<Value>)
}

pub async fn serve_stdio(project_root: PathBuf) -> Result<(), String> {
  let composer = crate::inspect::build_core_inspect_composer().map_err(|error| error.to_string())?;
  serve_stdio_with_composer(project_root, composer).await
}

/// Serve MCP stdio with an explicit inspect composer (product injects donor sections here).
pub async fn serve_stdio_with_composer(
  project_root: PathBuf,
  inspect_composer: Arc<auv_inspect_model::InspectComposer>,
) -> Result<(), String> {
  let service = McpServer::with_inspect_composer(project_root, inspect_composer)
    .serve(stdio())
    .await
    .map_err(|error| format!("failed to serve MCP stdio transport: {error}"))?;
  service.waiting().await.map(|_| ()).map_err(|error| format!("mcp stdio server exited with error: {error}"))
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::model::now_millis;
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
  async fn mcp_server_lists_and_invokes_shared_invoke_wrapper() -> anyhow::Result<()> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let store_root = temp_dir("mcp-shared-runtime-store");
    let (server_transport, client_transport) = tokio::io::duplex(16384);

    let server = McpServer::new(project_root.clone());
    let server_handle = tokio::spawn(async move {
      let service = server.serve(server_transport).await?;
      service.waiting().await?;
      anyhow::Ok(())
    });

    let client = DummyClientHandler::default().serve(client_transport).await?;

    let tools = client.list_tools(Default::default()).await?;
    let tool_names = tools.tools.iter().map(|tool| tool.name.as_ref()).collect::<Vec<_>>();
    assert!(!tool_names.contains(&"bundle_list"));
    assert!(!tool_names.contains(&"bundle_show"));
    assert!(!tool_names.contains(&"skill_list"));
    assert!(!tool_names.contains(&"skill_show"));
    assert!(tool_names.contains(&"invoke"));
    assert!(tool_names.contains(&"run_inspect"));
    assert!(
      !tool_names.contains(&"candidate_action_run"),
      "candidate_action_run is an archived vertical and must not be exposed through MCP"
    );

    let invoke_tool = tools.tools.iter().find(|tool| tool.name.as_ref() == "invoke").expect("invoke tool should be listed");
    let invoke_description = invoke_tool.description.as_ref().expect("invoke tool should have a description");
    assert!(invoke_description.contains("registry"));
    let command_id_schema = invoke_tool
      .input_schema
      .get("properties")
      .and_then(|properties| properties.get("command_id"))
      .expect("invoke schema should describe command_id");
    let command_ids =
      command_id_schema.get("enum").and_then(|value| value.as_array()).expect("command_id schema should enumerate registry command ids");
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
    let metadata_ids = command_metadata.iter().filter_map(|command| command.get("id").and_then(|value| value.as_str())).collect::<Vec<_>>();
    assert!(!metadata_ids.iter().any(|id| id.starts_with("debug.")));
    assert!(!metadata_ids.iter().any(|id| id.starts_with("verify.")));
    assert!(!metadata_ids.iter().any(|id| id.starts_with("music.")));
    assert!(!metadata_ids.iter().any(|id| id.starts_with("steam.")));
    let press_button_metadata = command_metadata
      .iter()
      .find(|command| command.get("id").and_then(|value| value.as_str()) == Some("input.pressButton"))
      .expect("input.pressButton metadata should be listed");
    assert_eq!(press_button_metadata.get("namespace").and_then(|value| value.as_str()), Some("input"));
    assert!(press_button_metadata.get("summary").and_then(|value| value.as_str()).is_some_and(|summary| summary.contains("query")));
    let press_button_args =
      press_button_metadata.get("arguments").and_then(|value| value.as_array()).expect("command metadata should expose argument specs");
    assert!(press_button_args.iter().any(|arg| {
      arg.get("flag").and_then(|value| value.as_str()) == Some("--query")
        && arg.get("required").and_then(|value| value.as_bool()) == Some(true)
    }));
    let now_playing_metadata = command_metadata
      .iter()
      .find(|command| command.get("id").and_then(|value| value.as_str()) == Some("mediaControl.nowPlaying"))
      .expect("mediaControl.nowPlaying metadata should be listed");
    assert_eq!(now_playing_metadata.get("namespace").and_then(|value| value.as_str()), Some("mediaControl"));

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
    let invoke_json: Value = serde_json::from_str(
      &invoke.content.first().and_then(|content| content.raw.as_text()).expect("invoke should return text content").text,
    )
    .expect("invoke text should decode as json");
    let run_id = invoke_json.get("run_id").and_then(|value| value.as_str()).expect("run_id should exist").to_string();
    assert_eq!(invoke_json.get("output_summary").and_then(|value| value.as_str()), Some("fixture observed"));
    assert_eq!(invoke_json.get("status").and_then(|value| value.as_str()), Some("completed"));
    assert_eq!(invoke_json.get("signals"), Some(&Value::Object(Default::default())));
    assert_eq!(invoke_json.get("artifacts").and_then(|value| value.as_array()).map(Vec::len), Some(0));

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
    let inspect_json: Value = serde_json::from_str(
      &inspect.content.first().and_then(|content| content.raw.as_text()).expect("inspect should return text content").text,
    )
    .expect("inspect text should decode as json");
    let inspect_text = inspect_json.get("text").and_then(|value| value.as_str()).expect("inspect text should exist");
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
    let failed_invoke_json: Value = serde_json::from_str(
      &failed_invoke.content.first().and_then(|content| content.raw.as_text()).expect("failed invoke should return text content").text,
    )
    .expect("failed invoke text should decode as json");
    let failed_run_id = failed_invoke_json.get("run_id").and_then(|value| value.as_str()).expect("failed run_id should exist").to_string();
    assert_eq!(failed_invoke_json.get("status").and_then(|value| value.as_str()), Some("failed"));
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
    let failed_inspect_json: Value = serde_json::from_str(
      &failed_inspect.content.first().and_then(|content| content.raw.as_text()).expect("failed inspect should return text content").text,
    )
    .expect("failed inspect text should decode as json");
    let failed_inspect_text = failed_inspect_json.get("text").and_then(|value| value.as_str()).expect("failed inspect text should exist");
    assert!(failed_inspect_text.contains("Status: error"));
    assert!(failed_inspect_text.contains("command.failed"));
    assert!(failed_inspect_text.contains("typed app activation API"));

    client.cancel().await?;
    server_handle.await??;
    let _ = std::fs::remove_dir_all(store_root);
    Ok(())
  }

  fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv-{}-{}", label, now_millis()))
  }
}
