use clap::{Args, Subcommand};

/// Expose core AUV capabilities through MCP.
#[derive(Clone, Debug, Args)]
pub struct McpArgs {
  #[command(subcommand)]
  pub command: McpCommand,
}

#[derive(Clone, Debug, Subcommand)]
pub enum McpCommand {
  /// Serve MCP over standard input and output.
  Serve,
}

pub async fn run(args: McpArgs, project_root: &std::path::Path) -> Result<i32, String> {
  match args.command {
    McpCommand::Serve => frontend::serve_stdio(project_root.to_path_buf()).await?,
  }
  Ok(0)
}

pub use frontend::{McpInvokeAdapter, McpInvokeInput, McpInvokeSuccess, McpServer, core_invoke_adapters, serve_stdio_with_registry};

mod frontend {
  use std::collections::{BTreeMap, BTreeSet};
  use std::path::PathBuf;
  use std::pin::Pin;
  use std::sync::Arc;

  use rmcp::{
    ErrorData as McpError, ServerHandler, ServiceExt,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, JsonObject, ListToolsResult, PaginatedRequestParam, ServerCapabilities, ServerInfo},
    service::{RequestContext, RoleServer},
    tool, tool_router,
    transport::stdio,
  };
  use schemars::JsonSchema;
  use serde::Deserialize;
  use serde::Serialize;
  use serde_json::Value;

  use auv_cli_invoke::{ExecutionTarget, InvokeCancellation, InvokeCommand, InvokeCommandInput, InvokeRegistry, default_registry};

  tokio::task_local! {
    static MCP_REQUEST_CANCELLATION: InvokeCancellation;
  }

  type McpInvokeFuture = Pin<Box<dyn Future<Output = Result<McpInvokeSuccess, auv_cli_invoke::InvokeFailure>> + Send + 'static>>;
  type InvokeDispatch = Arc<dyn Fn(Option<String>) -> Result<McpFrontendAuthority, String> + Send + Sync>;

  #[derive(Clone, Debug)]
  pub struct McpInvokeInput {
    pub target: Option<ExecutionTarget>,
    pub inputs: BTreeMap<String, String>,
    pub dry_run: bool,
    pub cancellation: InvokeCancellation,
  }

  #[derive(Clone)]
  pub struct McpInvokeAdapter {
    command_id: &'static str,
    handler: Arc<dyn Fn(McpInvokeInput) -> McpInvokeFuture + Send + Sync>,
  }

  impl McpInvokeAdapter {
    pub fn new<F, Fut>(command_id: &'static str, handler: F) -> Self
    where
      F: Fn(McpInvokeInput) -> Fut + Send + Sync + 'static,
      Fut: Future<Output = Result<McpInvokeSuccess, String>> + Send + 'static,
    {
      Self {
        command_id,
        handler: Arc::new(move |input| {
          let future = handler(input);
          Box::pin(async move { future.await.map_err(Into::into) })
        }),
      }
    }

    fn typed<F, Fut>(command_id: &'static str, handler: F) -> Self
    where
      F: Fn(McpInvokeInput) -> Fut + Send + Sync + 'static,
      Fut: Future<Output = Result<McpInvokeSuccess, auv_cli_invoke::InvokeFailure>> + Send + 'static,
    {
      Self {
        command_id,
        handler: Arc::new(move |input| Box::pin(handler(input))),
      }
    }

    fn invoke(&self, input: McpInvokeInput) -> McpInvokeFuture {
      if let Err(error) = input.cancellation.check() {
        return Box::pin(async move { Err(error.to_string().into()) });
      }
      (self.handler)(input)
    }
  }

  #[derive(Clone, Debug)]
  pub struct McpInvokeSuccess {
    result: Value,
  }

  impl McpInvokeSuccess {
    fn from_value(result: Value) -> Self {
      Self { result }
    }

    pub fn empty() -> Self {
      Self::from_value(Value::Null)
    }

    pub fn from_result<T>(result: &T) -> Result<Self, String>
    where
      T: Serialize + ?Sized,
    {
      serde_json::to_value(result).map(Self::from_value).map_err(|error| format!("failed to serialize MCP invoke result: {error}"))
    }
  }

  #[derive(Clone)]
  pub struct McpServer {
    tool_router: ToolRouter<Self>,
    /// Read-only command metadata used to build the MCP tool schema.
    invoke_registry: Arc<InvokeRegistry>,
    invoke_adapters: Arc<BTreeMap<&'static str, McpInvokeAdapter>>,
    invoke_dispatch: InvokeDispatch,
  }

  impl McpServer {
    /// Builds the core-only MCP server.
    pub fn new(project_root: PathBuf) -> Result<Self, String> {
      Self::with_registry(project_root, Arc::new(default_registry()), core_invoke_adapters())
    }

    pub fn with_registry(
      project_root: PathBuf,
      invoke_registry: Arc<InvokeRegistry>,
      invoke_adapters: Vec<McpInvokeAdapter>,
    ) -> Result<Self, String> {
      let dispatch_project_root = project_root.clone();
      Self::with_invoke_dispatch(
        project_root,
        invoke_registry,
        invoke_adapters,
        Arc::new(move |store_root| build_mcp_authority(dispatch_project_root.clone(), store_root)),
      )
    }

    fn with_invoke_dispatch(
      _project_root: PathBuf,
      invoke_registry: Arc<InvokeRegistry>,
      invoke_adapters: Vec<McpInvokeAdapter>,
      invoke_dispatch: InvokeDispatch,
    ) -> Result<Self, String> {
      let invoke_adapters = validated_adapter_catalog(invoke_registry.as_ref(), invoke_adapters)?;
      Ok(Self {
        tool_router: Self::tool_router(),
        invoke_registry,
        invoke_adapters: Arc::new(invoke_adapters),
        invoke_dispatch,
      })
    }

    pub fn invoke_registry(&self) -> &Arc<InvokeRegistry> {
      &self.invoke_registry
    }
  }

  #[derive(Serialize)]
  #[serde(tag = "status", rename_all = "snake_case")]
  enum McpInvokePresentation {
    Completed {
      run_id: auv_tracing::RunId,
      result: Value,
      #[serde(skip_serializing_if = "Option::is_none")]
      recording_failure: Option<String>,
    },
    Failed {
      run_id: auv_tracing::RunId,
      failure: String,
      failure_details: auv_cli_invoke::InvokeFailure,
      command_id: String,
      #[serde(skip_serializing_if = "Option::is_none")]
      recording_failure: Option<String>,
    },
  }

  fn validated_adapter_catalog(
    registry: &InvokeRegistry,
    adapters: Vec<McpInvokeAdapter>,
  ) -> Result<BTreeMap<&'static str, McpInvokeAdapter>, String> {
    let mut catalog = BTreeMap::new();
    for adapter in adapters {
      let command_id = adapter.command_id;
      if catalog.insert(command_id, adapter).is_some() {
        return Err(format!("duplicate MCP invoke adapter id: {command_id}"));
      }
    }

    let metadata_ids = registry.all().iter().map(|command| command.id).collect::<BTreeSet<_>>();
    let adapter_ids = catalog.keys().copied().collect::<BTreeSet<_>>();
    let missing = metadata_ids.difference(&adapter_ids).copied().collect::<Vec<_>>();
    if !missing.is_empty() {
      return Err(format!("missing MCP invoke adapter ids: {}", missing.join(", ")));
    }
    let extra = adapter_ids.difference(&metadata_ids).copied().collect::<Vec<_>>();
    if !extra.is_empty() {
      return Err(format!("extra MCP invoke adapter ids: {}", extra.join(", ")));
    }
    Ok(catalog)
  }

  #[derive(Clone)]
  struct McpFrontendAuthority {
    dispatch: auv_tracing::Dispatch,
  }

  fn build_mcp_authority(project_root: PathBuf, store_root: Option<String>) -> Result<McpFrontendAuthority, String> {
    let explicit_store_root = store_root.map(PathBuf::from);
    let root = explicit_store_root.unwrap_or_else(|| project_root.join(".auv").join("store"));
    let store = auv_tracing::FileTracingStore::open(&root)
      .map(|store| Arc::new(store) as Arc<dyn auv_tracing::TracingStore>)
      .map_err(|error| format!("failed to open MCP tracing store {}: {error}", root.display()))?;
    let dispatch = auv_tracing::configure().tracing_store(store).build().map_err(|error| error.to_string())?;
    Ok(McpFrontendAuthority { dispatch })
  }

  #[derive(Serialize)]
  struct McpFrontendLifecycle {
    frontend: &'static str,
  }

  impl auv_tracing::EventPayload for McpFrontendLifecycle {
    const NAME: &'static str = "auv.frontend.lifecycle";
    const VERSION: u32 = 1;
  }

  #[derive(Serialize)]
  struct McpFrontendCancellation {
    frontend: &'static str,
    reason: &'static str,
  }

  impl auv_tracing::EventPayload for McpFrontendCancellation {
    const NAME: &'static str = "auv.frontend.cancelled";
    const VERSION: u32 = 1;
  }

  fn command_adapter(command: InvokeCommand) -> McpInvokeAdapter {
    let command_id = command.id;
    McpInvokeAdapter::typed(command_id, move |input| {
      let command = command.clone();
      async move {
        let inputs = mcp_command_inputs(command.namespace, input.inputs);
        let output = command
          .invoke(InvokeCommandInput {
            command_id: command_id.to_string(),
            target: input.target,
            inputs,
            typed_args: None,
            dry_run: input.dry_run,
            cancellation: input.cancellation,
          })
          .await?;
        Ok(McpInvokeSuccess::from_value(output.result().cloned().unwrap_or(Value::Null)))
      }
    })
  }

  fn mcp_command_inputs(namespace: auv_cli_invoke::InvokeNamespace, mut inputs: BTreeMap<String, String>) -> BTreeMap<String, String> {
    // MCP consumes the shared direct operation result but does not opt into
    // incidental CLI live presentation. Explicit overlay.* operations remain
    // enabled because their visual effect is the operation itself.
    if namespace != auv_cli_invoke::InvokeNamespace::Overlay {
      inputs.insert("overlay".to_string(), "false".to_string());
    }
    inputs
  }

  pub fn core_invoke_adapters() -> Vec<McpInvokeAdapter> {
    default_registry().all().iter().cloned().map(command_adapter).collect()
  }

  #[tool_router(router = tool_router)]
  impl McpServer {
    #[tool(
      description = "Invoke one explicit cataloged AUV command id through its MCP typed adapter. See input_schema.x-auv-commands for available command metadata.",
      input_schema = invoke_tool_input_schema()
    )]
    /// Executes the registry command selected by one MCP `invoke` tool call.
    ///
    /// Triggering workflow:
    /// `ServerHandler::call_tool` -> `ToolRouter::call` -> `McpServer::invoke`
    /// -> `McpInvokeAdapter::invoke` -> `InvokeCommand::invoke` -> tracing flush.
    async fn invoke(&self, Parameters(req): Parameters<InvokeToolRequest>) -> Result<CallToolResult, McpError> {
      let adapter = self
        .invoke_adapters
        .get(req.command_id.as_str())
        .cloned()
        .ok_or_else(|| invalid_params(format!("unknown invoke command: {}", req.command_id)))?;
      let authority = (self.invoke_dispatch)(req.store_root).map_err(invalid_params)?;
      let cancellation = MCP_REQUEST_CANCELLATION.try_with(Clone::clone).unwrap_or_default();
      let input = McpInvokeInput {
        target: req.target.into_execution_target().map_err(invalid_params)?,
        inputs: req.inputs,
        dry_run: req.dry_run,
        cancellation: cancellation.clone(),
      };
      let run_id = auv_tracing::RunId::new();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let command_future = root.in_scope(|| {
        auv_tracing::emit_event!(McpFrontendLifecycle { frontend: "mcp" });
        adapter.invoke(input)
      });
      let cancellable_future = async move {
        tokio::pin!(command_future);
        // TODO(invoke-driver-cancellation): request cancellation drops the
        // command future between polls, but cannot interrupt one synchronous
        // driver call already in progress. Add deeper cancellation only after
        // the owning driver exposes an owner-approved cancellable call API.
        tokio::select! {
          biased;
          _ = cancellation.cancelled() => {
            auv_tracing::emit_event!(McpFrontendCancellation {
              frontend: "mcp",
              reason: "request_cancelled",
            });
            Err("invoke cancelled".to_string().into())
          }
          result = &mut command_future => result,
        }
      };
      let direct_result = root.instrument(cancellable_future).await;
      let recording_failure = authority.dispatch.flush().await.err().map(|error| error.to_string());
      let (failed, presentation) = match direct_result {
        Ok(success) => (
          false,
          McpInvokePresentation::Completed {
            run_id,
            result: success.result,
            recording_failure,
          },
        ),
        Err(failure) => (
          true,
          McpInvokePresentation::Failed {
            run_id,
            command_id: req.command_id,
            failure: failure.to_string(),
            failure_details: failure,
            recording_failure,
          },
        ),
      };
      let value = serde_json::to_value(presentation).map_err(invalid_params)?;
      Ok(if failed {
        CallToolResult::structured_error(value)
      } else {
        CallToolResult::structured(value)
      })
    }
  }

  impl ServerHandler for McpServer {
    /// Dispatches an MCP tool request while propagating request cancellation.
    ///
    /// Triggering workflow:
    /// rmcp transport -> `McpServer::call_tool` -> `ToolRouter::call`
    /// -> `McpServer::invoke` -> `InvokeCommand::invoke`.
    async fn call_tool(
      &self,
      request: rmcp::model::CallToolRequestParam,
      context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
      let cancellation = InvokeCancellation::from_token(context.ct.clone());
      let tcc = rmcp::handler::server::tool::ToolCallContext::new(self, request, context);
      MCP_REQUEST_CANCELLATION.scope(cancellation, self.tool_router.call(tcc)).await
    }

    async fn list_tools(
      &self,
      _request: Option<PaginatedRequestParam>,
      _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
      let mut tools = self.tool_router.list_all();
      if let Some(invoke_tool) = tools.iter_mut().find(|tool| tool.name == "invoke") {
        invoke_tool.input_schema = invoke_tool_input_schema_for_registry(self.invoke_registry.as_ref());
      }
      Ok(ListToolsResult::with_all_items(tools))
    }

    fn get_info(&self) -> ServerInfo {
      ServerInfo {
        instructions: Some(
          "MCP exposes explicit AUV tools backed by the registered typed invoke commands; no planner or NL parsing is present.".into(),
        ),
        capabilities: ServerCapabilities::builder().enable_tools().build(),
        ..Default::default()
      }
    }
  }

  fn invoke_tool_input_schema() -> Arc<JsonObject> {
    // Static schema uses the core registry; injected registries rewrite via list_tools
    // with the explicitly injected registry.
    invoke_tool_input_schema_for_registry(&default_registry())
  }

  fn invoke_tool_input_schema_for_registry(registry: &InvokeRegistry) -> Arc<JsonObject> {
    let mut schema = rmcp::handler::server::common::cached_schema_for_type::<InvokeToolRequest>().as_ref().clone();
    let command_ids = registry.all().iter().map(|command| Value::String(command.id.to_string())).collect::<Vec<_>>();

    if let Some(command_id_schema) = schema
      .get_mut("properties")
      .and_then(Value::as_object_mut)
      .and_then(|properties| properties.get_mut("command_id"))
      .and_then(Value::as_object_mut)
    {
      command_id_schema.insert(
        "description".to_string(),
        Value::String("Registry command id. See x-auv-commands on this schema for descriptions and argument metadata.".to_string()),
      );
      command_id_schema.insert("enum".to_string(), Value::Array(command_ids));
    }

    schema.insert("x-auv-commands".to_string(), Value::Array(registry.all().iter().map(invoke_command_metadata).collect::<Vec<_>>()));
    Arc::new(schema)
  }

  fn invoke_command_metadata(command: &InvokeCommand) -> Value {
    let clap_command = command.clap_command();
    serde_json::json!({
      "id": command.id,
      "namespace": command.namespace.as_str(),
      "description": command.description,
      "target_policy": command.target,
      "target_help": command.target.help(),
      "target": { "required": command.target.required(), "accepted_types": command.target.accepted_types() },
      "arguments": clap_command
        .get_arguments()
        .filter(|argument| argument.get_id() != "help")
        .map(|argument| serde_json::json!({
          "flag": argument.get_long().map(|long| format!("--{long}")),
          "input_key": argument.get_long().unwrap_or_else(|| argument.get_id().as_str()),
          "value_name": argument.get_value_names().and_then(|names| names.first()).map(|name| name.as_str()),
          "required": argument.is_required_set(),
          "help": argument.get_help().map(ToString::to_string),
        }))
        .collect::<Vec<_>>(),
    })
  }

  #[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
  #[serde(deny_unknown_fields)]
  struct McpInvokeTarget {
    application_id: Option<String>,
    window_id: Option<String>,
    display_id: Option<String>,
  }

  impl McpInvokeTarget {
    fn into_execution_target(self) -> Result<Option<ExecutionTarget>, String> {
      let targets = [
        self.application_id.map(|id| ("application_id", ExecutionTarget::Application { id })),
        self.window_id.map(|id| ("window_id", ExecutionTarget::Window { id })),
        self.display_id.map(|id| ("display_id", ExecutionTarget::Display { id })),
      ];
      let mut targets = targets.into_iter().flatten();
      let target = targets.next();
      if targets.next().is_some() {
        return Err("target must set at most one of application_id, window_id, or display_id".to_string());
      }
      let Some((field, target)) = target else {
        return Ok(None);
      };
      let id = match &target {
        ExecutionTarget::Application { id } | ExecutionTarget::Window { id } | ExecutionTarget::Display { id } => id,
      };
      if id.trim().is_empty() {
        return Err(format!("target.{field} cannot be empty"));
      }
      Ok(Some(target))
    }
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
    store_root: Option<String>,
  }

  fn invalid_params(message: impl ToString) -> McpError {
    McpError::invalid_params(message.to_string(), None::<Value>)
  }

  pub async fn serve_stdio(project_root: PathBuf) -> Result<(), String> {
    serve_stdio_with_registry(project_root, Arc::new(default_registry()), core_invoke_adapters()).await
  }

  /// Serve MCP stdio with explicit invoke metadata and shared typed commands.
  pub async fn serve_stdio_with_registry(
    project_root: PathBuf,
    invoke_registry: Arc<InvokeRegistry>,
    invoke_adapters: Vec<McpInvokeAdapter>,
  ) -> Result<(), String> {
    let service = McpServer::with_registry(project_root, invoke_registry, invoke_adapters)?
      .serve(stdio())
      .await
      .map_err(|error| format!("failed to serve MCP stdio transport: {error}"))?;
    service.waiting().await.map(|_| ()).map_err(|error| format!("mcp stdio server exited with error: {error}"))
  }

  #[cfg(test)]
  mod tests {
    include!("mcp_test.rs");
  }
}
