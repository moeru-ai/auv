use std::collections::BTreeMap;
use std::path::PathBuf;

use rmcp::{
  ErrorData as McpError, ServerHandler, ServiceExt,
  handler::server::{router::tool::ToolRouter, wrapper::Parameters},
  model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
  tool, tool_handler, tool_router,
  transport::stdio,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde::Serialize;
use serde_json::Value;

use crate::bundle::SkillBundleCatalog;
use crate::model::{ExecutionTarget, InvokeRequest};
use crate::skill::SkillCatalog;
use crate::{build_default_runtime, build_runtime_with_store_root};

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

  fn runtime(&self, store_root: Option<String>) -> Result<crate::runtime::Runtime, McpError> {
    let runtime = match store_root {
      Some(root) => build_runtime_with_store_root(self.project_root.clone(), PathBuf::from(root)),
      None => build_default_runtime(self.project_root.clone()),
    };
    runtime.map_err(invalid_params)
  }

  fn bundle_catalog(&self) -> Result<SkillBundleCatalog, McpError> {
    SkillBundleCatalog::discover(&self.project_root).map_err(invalid_params)
  }

  fn skill_catalog(&self) -> Result<SkillCatalog, McpError> {
    SkillCatalog::discover(&self.project_root).map_err(invalid_params)
  }
}

#[tool_router(router = tool_router)]
impl McpServer {
  #[tool(description = "List available AUV bundles.")]
  async fn bundle_list(&self) -> Result<CallToolResult, McpError> {
    let catalog = self.bundle_catalog()?;
    let bundles = catalog
      .entries()
      .iter()
      .map(|entry| {
        serde_json::json!({
          "id": entry.manifest.metadata.id,
          "name": entry.manifest.metadata.name,
          "status": entry.manifest.metadata.status,
          "path": entry.path.display().to_string(),
        })
      })
      .collect::<Vec<_>>();
    json_result(serde_json::json!({ "bundles": bundles }))
  }

  #[tool(description = "Show one AUV bundle manifest by id or path.")]
  async fn bundle_show(
    &self,
    Parameters(req): Parameters<BundleShowRequest>,
  ) -> Result<CallToolResult, McpError> {
    let catalog = self.bundle_catalog()?;
    let entry = catalog
      .resolve(&self.project_root, &req.query)
      .map_err(invalid_params)?;
    json_result(serde_json::json!({
      "bundle": read_manifest_value(&entry.path).map_err(internal_error)?
    }))
  }

  #[tool(description = "List available AUV skills.")]
  async fn skill_list(&self) -> Result<CallToolResult, McpError> {
    let catalog = self.skill_catalog()?;
    let mut skills = Vec::new();
    for entry in catalog.entries() {
      let taxonomy_id = entry
        .manifest
        .strategy
        .taxonomy_id()
        .map_err(invalid_params)?;
      skills.push(serde_json::json!({
        "recipe_id": entry.manifest.recipe_id,
        "objective": entry.manifest.objective,
        "status": entry.manifest.status,
        "path": entry.path.display().to_string(),
        "strategy": {
          "family": entry.manifest.strategy.family,
          "grounding": entry.manifest.strategy.grounding,
          "activation": entry.manifest.strategy.activation,
          "verification_contract": entry.manifest.strategy.verification_contract,
          "taxonomy_id": taxonomy_id,
        }
      }));
    }
    json_result(serde_json::json!({ "skills": skills }))
  }

  #[tool(description = "Show one AUV skill manifest by id or path.")]
  async fn skill_show(
    &self,
    Parameters(req): Parameters<SkillShowRequest>,
  ) -> Result<CallToolResult, McpError> {
    let catalog = self.skill_catalog()?;
    let entry = catalog
      .resolve(&self.project_root, &req.query)
      .map_err(invalid_params)?;
    json_result(serde_json::json!({
      "skill": read_manifest_value(&entry.path).map_err(internal_error)?
    }))
  }

  #[tool(description = "Invoke one explicit AUV command id through the shared runtime.")]
  async fn invoke(
    &self,
    Parameters(req): Parameters<InvokeToolRequest>,
  ) -> Result<CallToolResult, McpError> {
    let runtime = self.runtime(req.inspect.store_root.clone())?;
    let result = runtime
      .invoke(InvokeRequest {
        command_id: req.command_id,
        target: ExecutionTarget {
          application_id: req.target.application_id,
        },
        inputs: req.inputs,
        dry_run: req.dry_run,
      })
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
    let runtime = self.runtime(req.store_root.clone())?;
    let text = runtime.inspect(&req.run_id).map_err(invalid_params)?;
    json_result(serde_json::json!({
      "run_id": req.run_id,
      "text": text,
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

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct BundleShowRequest {
  query: String,
}

#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct SkillShowRequest {
  query: String,
}

#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
struct McpInvokeTarget {
  application_id: Option<String>,
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

fn read_manifest_value(path: &PathBuf) -> Result<Value, String> {
  let raw = std::fs::read_to_string(path)
    .map_err(|error| format!("failed to read manifest {}: {error}", path.display()))?;
  serde_json::from_str(&raw)
    .map_err(|error| format!("failed to parse manifest {}: {error}", path.display()))
}

fn invalid_params(message: impl ToString) -> McpError {
  McpError::invalid_params(message.to_string(), None::<Value>)
}

fn internal_error(message: impl ToString) -> McpError {
  McpError::internal_error(message.to_string(), None::<Value>)
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
    let project_root = PathBuf::from("/Users/liuziheng/https-github-com-moeru-ai-auv");
    let (server_transport, client_transport) = tokio::io::duplex(16384);

    let server = McpServer::new(project_root);
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
    assert!(tool_names.contains(&"bundle_list"));
    assert!(tool_names.contains(&"invoke"));
    assert!(tool_names.contains(&"run_inspect"));

    let invoke = client
      .call_tool(CallToolRequestParam {
        name: "invoke".into(),
        arguments: Some(
          serde_json::json!({
            "command_id": "steam.library.list.v0",
            "dry_run": false,
            "inputs": {},
            "target": {},
            "inspect": {}
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
    let output_summary = invoke_json
      .get("output_summary")
      .and_then(|value| value.as_str())
      .expect("summary should exist");
    assert!(output_summary.contains("Listed"));

    let inspect = client
      .call_tool(CallToolRequestParam {
        name: "run_inspect".into(),
        arguments: Some(
          serde_json::json!({
            "run_id": run_id
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
    assert!(inspect_text.contains("steam.library.list.v0"));
    assert!(inspect_text.contains("artifact_0001"));

    client.cancel().await?;
    server_handle.await??;
    Ok(())
  }
}
