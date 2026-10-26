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

pub async fn run(args: McpArgs, selection: &auv::selection::RootSelection, project_root: &std::path::Path) -> Result<i32, String> {
  match args.command {
    McpCommand::Serve => frontend::serve_stdio(project_root.to_path_buf(), selection.clone()).await?,
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
    model::{CallToolResult, Implementation, JsonObject, ListToolsResult, PaginatedRequestParam, ServerCapabilities, ServerInfo},
    service::{RequestContext, RoleServer},
    tool, tool_router,
    transport::stdio,
  };
  use schemars::JsonSchema;
  use serde::Deserialize;
  use serde::Serialize;
  use serde_json::Value;

  use auv_auto_loop::runtime::OperationExecutor;
  use auv_cli_invoke::{
    CommandSupport, ExecutionTarget, InvokeCancellation, InvokeCommand, InvokeCommandInput, InvokeRegistry, RunnerSupport, default_registry,
  };

  use crate::commands::op::{ExecutionModeArg, OpRunArgs, default_executor, run_with_executor_inner};

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
    /// Evidence recorded by the command, presented as references, never pixels.
    artifacts: Vec<auv_tracing::ArtifactMetadata>,
  }

  impl McpInvokeSuccess {
    fn from_value(result: Value) -> Self {
      Self {
        result,
        artifacts: Vec::new(),
      }
    }

    fn from_output(output: &auv_cli_invoke::InvokeCommandOutput) -> Self {
      Self {
        result: output.result().cloned().unwrap_or(Value::Null),
        artifacts: output.artifacts().to_vec(),
      }
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
    /// Executor behind the `op_run` tool. The same platform default as `auv op run`.
    op_executor: Arc<dyn OperationExecutor>,
    /// Root `--device`, `--device-id`, and `--run` for `invoke` calls that select no Device.
    default_selection: auv::selection::RootSelection,
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
        op_executor: default_executor(),
        default_selection: auv::selection::RootSelection::default(),
      })
    }

    /// Routes `invoke` calls without their own Device fields through `selection`.
    pub fn with_default_selection(mut self, selection: auv::selection::RootSelection) -> Self {
      self.default_selection = selection;
      self
    }

    /// Per-call Device fields replace the root Device selection; the root Run is kept.
    fn invoke_selection(&self, device_name: Option<String>, device_id: Option<String>) -> Result<auv::selection::RootSelection, McpError> {
      if device_name.is_none() && device_id.is_none() {
        return Ok(self.default_selection.clone());
      }
      let mut selection = device_selection(device_name, device_id)?;
      selection.run_id = self.default_selection.run_id.clone();
      Ok(selection)
    }

    // TODO(mcp-op-executor-injection): executor injection is test-only for now. Windows defaults to the
    // real production executor, so tests must substitute a fake. Open it to library callers
    // (e.g. a `serve_stdio_with_registry` parameter) only when an embedder needs a non-default executor.
    #[cfg(test)]
    fn with_op_executor(mut self, op_executor: Arc<dyn OperationExecutor>) -> Self {
      self.op_executor = op_executor;
      self
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
      #[serde(skip_serializing_if = "Vec::is_empty")]
      artifacts: Vec<auv_cli_invoke::InvokeArtifactResult>,
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
    store: Arc<auv_tracing::FileTracingStore>,
  }

  fn build_mcp_authority(project_root: PathBuf, store_root: Option<String>) -> Result<McpFrontendAuthority, String> {
    let explicit_store_root = store_root.map(PathBuf::from);
    let root = explicit_store_root.unwrap_or_else(|| project_root.join(".auv").join("store"));
    let store = auv_tracing::FileTracingStore::open(&root)
      .map(Arc::new)
      .map_err(|error| format!("failed to open MCP tracing store {}: {error}", root.display()))?;
    let dispatch = auv_tracing::configure().tracing_store(store.clone()).build().map_err(|error| error.to_string())?;
    Ok(McpFrontendAuthority { dispatch, store })
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
        Ok(McpInvokeSuccess::from_output(&output))
      }
    })
  }

  /// Runs one core command on a selected Device through the CLI's typed remote adapter.
  fn remote_invoke(command_id: String, command: Option<InvokeCommand>, input: McpInvokeInput, context: auv::AuvContext) -> McpInvokeFuture {
    Box::pin(async move {
      let inputs = match &command {
        Some(command) => mcp_command_inputs(command.namespace, input.inputs),
        None => input.inputs,
      };
      let output = auv_cli_invoke::runner::invoke(
        InvokeCommandInput {
          command_id,
          target: input.target,
          inputs,
          typed_args: None,
          dry_run: input.dry_run,
          cancellation: input.cancellation,
        },
        context,
      )
      .await?;
      Ok(McpInvokeSuccess::from_output(&output))
    })
  }

  /// Presents one invoke outcome; artifacts carry store paths so clients can read evidence themselves.
  fn invoke_tool_result(
    run_id: auv_tracing::RunId,
    command_id: String,
    direct_result: Result<McpInvokeSuccess, auv_cli_invoke::InvokeFailure>,
    recording_failure: Option<String>,
    authority: &McpFrontendAuthority,
  ) -> Result<CallToolResult, McpError> {
    let (failed, presentation) = match direct_result {
      Ok(success) => (
        false,
        McpInvokePresentation::Completed {
          run_id,
          result: success.result,
          artifacts: success
            .artifacts
            .into_iter()
            .map(|metadata| {
              let path = authority.store.artifact_path(&metadata);
              auv_cli_invoke::InvokeArtifactResult::new(metadata).with_file_path(path)
            })
            .collect(),
          recording_failure,
        },
      ),
      Err(failure) => (
        true,
        McpInvokePresentation::Failed {
          run_id,
          command_id,
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
      let selection = self.invoke_selection(req.device_name, req.device_id)?;
      let authority = (self.invoke_dispatch)(req.store_root).map_err(invalid_params)?;
      let cancellation = MCP_REQUEST_CANCELLATION.try_with(Clone::clone).unwrap_or_default();
      let command = self.invoke_registry.resolve(&req.command_id).cloned();
      let input = McpInvokeInput {
        target: req.target.into_execution_target().map_err(invalid_params)?,
        inputs: req.inputs,
        dry_run: req.dry_run,
        cancellation: cancellation.clone(),
      };
      // Same selection rule as `auv invoke`: a selected Device always runs the
      // command remotely; only dry runs that need no Device stay local.
      let wants_device = !selection.is_empty()
        && (!input.dry_run
          || command.as_ref().is_some_and(|command| command.target == auv_cli_invoke::command::TargetPolicy::OptionalKeyboard));
      let selected = if wants_device {
        match crate::commands::plugin::resolve_invoke_context(&selection).await {
          Ok(selected) => Some(selected),
          Err(error) => return invoke_tool_result(auv_tracing::RunId::new(), req.command_id, Err(error.into()), None, &authority),
        }
      } else {
        None
      };
      let run_id = crate::commands::invoke::tracing_run_id(selected.as_ref()).map_err(invalid_params)?;
      let remote_context = selected.as_ref().map(|selected| selected.context.clone());
      let command_id = req.command_id.clone();
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let command_future = root.in_scope(|| {
        auv_tracing::emit_event!(McpFrontendLifecycle { frontend: "mcp" });
        match remote_context {
          Some(context) => remote_invoke(command_id, command, input, context),
          None => adapter.invoke(input),
        }
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
      let mut direct_result = root.instrument(cancellable_future).await;
      if let Some(selected) = selected
        && let Err(error) = selected.finish(direct_result.is_ok()).await
        && direct_result.is_ok()
      {
        direct_result = Err(error.into());
      }
      let recording_failure = authority.dispatch.flush().await.err().map(|error| error.to_string());
      invoke_tool_result(run_id, req.command_id, direct_result, recording_failure, &authority)
    }

    /// Triggering workflow: MCP `tools/call` for `device_list_user_sessions` reaches this
    /// handler through `tool_router`, then calls `Devices::list_user_sessions` on
    /// the selected Device client.
    #[tool(
      description = "Request current OS login sessions from one selected AUV Device. Select a configured paired Device by device_name or device_id, or use the local default. Requires a validated native host; unsupported hosts return a typed error."
    )]
    async fn device_list_user_sessions(&self, Parameters(req): Parameters<DeviceSessionsToolRequest>) -> Result<CallToolResult, McpError> {
      let selection = device_selection(req.device_name, req.device_id)?;
      let (client, _) = auv::Client::selected(None, &selection)
        .await
        .map_err(device_selection_error)?
        .ok_or_else(|| McpError::internal_error("no AUV daemon was discovered", None))?;
      // Device entry audit belongs to the target Device. This query creates no
      // caller-owned Run or trace artifact in the MCP frontend.
      match client.devices().list_user_sessions().await {
        Ok(sessions) => Ok(CallToolResult::structured(serde_json::json!({
          "sessions": sessions.iter().map(|session| serde_json::json!({
            "session_selector": session.selector,
            "user": session.user,
            "lock_state": session.lock_state.as_str(),
            "connection_kind": session.connection_kind.as_str(),
            "seat": session.seat,
          })).collect::<Vec<_>>(),
        }))),
        Err(auv::devices::DeviceError::Entry(reason)) => Ok(CallToolResult::structured_error(serde_json::json!({
          "reason": reason.as_str(),
        }))),
        Err(error) => Err(McpError::internal_error(error.to_string(), None)),
      }
    }

    /// Triggering workflow: MCP `tools/call` for `device_get_user_session`
    /// reaches the selected Device through `Devices::get_user_session`.
    #[tool(
      description = "Get one current OS user session by the opaque session_selector returned by device_list_user_sessions. Requires a validated native host; unsupported hosts return a typed error."
    )]
    async fn device_get_user_session(
      &self,
      Parameters(req): Parameters<DeviceGetUserSessionToolRequest>,
    ) -> Result<CallToolResult, McpError> {
      if req.session_selector.trim().is_empty() {
        return Err(invalid_params("session_selector must be nonempty"));
      }

      let selection = device_selection(req.device_name, req.device_id)?;
      let (client, _) = auv::Client::selected(None, &selection)
        .await
        .map_err(device_selection_error)?
        .ok_or_else(|| McpError::internal_error("no AUV daemon was discovered", None))?;

      match client.devices().get_user_session(&req.session_selector).await {
        Ok(session) => Ok(CallToolResult::structured(serde_json::json!({
          "session_selector": session.selector,
          "user": session.user,
          "lock_state": session.lock_state.as_str(),
          "connection_kind": session.connection_kind.as_str(),
          "seat": session.seat,
        }))),
        Err(auv::devices::DeviceError::Entry(reason)) => Ok(CallToolResult::structured_error(serde_json::json!({
          "reason": reason.as_str(),
        }))),
        Err(error) => Err(McpError::internal_error(error.to_string(), None)),
      }
    }

    /// Triggering workflow: MCP `tools/call` for `device_ensure_user_session_unlocked` reaches this
    /// handler through `tool_router`, then calls `Devices::ensure_user_session_unlocked` on the
    /// selected Device client.
    #[tool(
      description = "Request unlock of one existing OS login session on a selected Device. Set exactly one of user or session_selector. A user with no existing session is not signed in. Requires a validated native host; unsupported hosts return a typed error. The credential stays on the target Device."
    )]
    async fn device_ensure_user_session_unlocked(
      &self,
      Parameters(req): Parameters<DeviceUnlockToolRequest>,
    ) -> Result<CallToolResult, McpError> {
      let target = req.target()?;
      let selection = device_selection(req.device_name, req.device_id)?;
      let (client, _) = auv::Client::selected(None, &selection)
        .await
        .map_err(device_selection_error)?
        .ok_or_else(|| McpError::internal_error("no AUV daemon was discovered", None))?;
      // The target owns the audit and verifies the effect; MCP does not record
      // a caller-owned Run containing entry inputs or OS login state.
      match client.devices().ensure_user_session_unlocked(target).await {
        Ok(effect) => Ok(CallToolResult::structured(serde_json::json!({
          "effect": effect.kind.as_str(),
          "user": effect.user,
          "session_selector": effect.session_selector,
        }))),
        Err(auv::devices::DeviceError::Entry(reason)) => Ok(CallToolResult::structured_error(serde_json::json!({
          "reason": reason.as_str(),
        }))),
        Err(error) => Err(McpError::internal_error(error.to_string(), None)),
      }
    }

    /// Triggering workflow: MCP `tools/call` for `device_ensure_user_session_locked`
    /// selects a Device and calls the same typed operation as the CLI.
    #[tool(
      description = "Lock one existing usable OS login session on a selected Device. Set exactly one of user or session_selector. The target verifies the exact session is locked; no enrolled credential is read."
    )]
    async fn device_ensure_user_session_locked(
      &self,
      Parameters(req): Parameters<DeviceUnlockToolRequest>,
    ) -> Result<CallToolResult, McpError> {
      let target = req.target()?;
      let selection = device_selection(req.device_name, req.device_id)?;
      let (client, _) = auv::Client::selected(None, &selection)
        .await
        .map_err(device_selection_error)?
        .ok_or_else(|| McpError::internal_error("no AUV daemon was discovered", None))?;

      match client.devices().ensure_user_session_locked(target).await {
        Ok(effect) => Ok(CallToolResult::structured(serde_json::json!({
          "effect": effect.kind.as_str(),
          "user": effect.user,
          "session_selector": effect.session_selector,
        }))),
        Err(auv::devices::DeviceError::Entry(reason)) => Ok(CallToolResult::structured_error(serde_json::json!({
          "reason": reason.as_str(),
        }))),
        Err(error) => Err(McpError::internal_error(error.to_string(), None)),
      }
    }

    /// Triggering workflow: MCP `tools/call` for `op_run` reaches this handler through
    /// `tool_router`, then `execute_op_run` -> `commands::op::run_with_executor_inner`, the same
    /// path as `auv op run`.
    #[tool(
      description = "Run one compiled mode-aware operation through the same scheduler and executor as `auv op run`. `mode` is required and must match the operation's declared mode: fast dispatches without waiting for verification and is never confirmed; verified waits for the verification gate and is confirmed only when it passes. A mode mismatch is rejected before any driver side effect. The result carries status, execution_mode, confirmed and reason_code."
    )]
    async fn op_run(&self, Parameters(req): Parameters<OpRunToolRequest>) -> Result<CallToolResult, McpError> {
      self.execute_op_run(req).await
    }
  }

  impl McpServer {
    /// Translates one `op_run` request into `OpRunArgs` and maps the shared result back.
    ///
    /// This is deliberately thin: scheduling, mode-conflict rejection, gate handling and output
    /// construction (including "fast is never confirmed") all stay in `run_with_executor_inner`.
    async fn execute_op_run(&self, req: OpRunToolRequest) -> Result<CallToolResult, McpError> {
      let has_custom_file = req.file.is_some();
      let args = OpRunArgs {
        operation: req.operation,
        mode: req.mode,
        file: req.file,
        json: true,
      };
      let executor = Arc::clone(&self.op_executor);
      // The executor is synchronous and a production run drives real UI automation for seconds;
      // keep it off the async worker threads so other MCP requests (and cancellation) stay responsive.
      let outcome = tokio::task::spawn_blocking(move || run_with_executor_inner(&args, executor.as_ref()))
        .await
        .map_err(|error| McpError::internal_error(format!("op_run worker failed: {error}"), None))?;

      match outcome {
        Ok((exit_code, output)) => {
          let value = serde_json::to_value(&output).map_err(|error| McpError::internal_error(error.to_string(), None))?;
          Ok(if exit_code == 0 {
            CallToolResult::structured(value)
          } else {
            CallToolResult::structured_error(value)
          })
        }
        // `Err` is raised only while building the catalog: reading/admitting the caller's `file`, or
        // the compiled-in built-ins. Scheduler, mode and gate failures are `Ok` with a reason_code.
        Err(message) if has_custom_file => Err(invalid_params(message)),
        Err(message) => Err(McpError::internal_error(message, None)),
      }
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
        invoke_tool.description = Some(invoke_tool_description(self.invoke_registry.as_ref()).into());
      }
      Ok(ListToolsResult::with_all_items(tools))
    }

    fn get_info(&self) -> ServerInfo {
      ServerInfo {
        instructions: Some(
          "MCP exposes explicit AUV invoke commands, mode-aware operation runs (op_run) and typed Device unlock tools; no planner or NL parsing is present.".into(),
        ),
        capabilities: ServerCapabilities::builder().enable_tools().build(),
        // The handshake is the only place a client can ask which build answers. Defaulting
        // this field reports the transport crate (rmcp) instead of auv, so a long-running
        // server started before an upgrade cannot be told apart from the current one.
        server_info: Implementation {
          name: "auv".into(),
          version: env!("CARGO_PKG_VERSION").into(),
          ..Default::default()
        },
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

  /// Renders the command catalog into the `invoke` description.
  ///
  /// NOTICE: Model clients render tool definitions from standard JSON Schema
  /// keys; the `x-auv-commands` extension never reached the model (an OSWorld
  /// request cost the same input tokens with or without it), so agents guessed
  /// `inputs` keys. The description is the one field every client shows.
  fn invoke_tool_description(registry: &InvokeRegistry) -> String {
    let mut text = String::from(
      "Invoke one AUV command and record it as a Run. Set `command_id` to one command below and \
       pass its arguments in `inputs` as string values, keyed exactly as listed; list values are JSON array strings. \
       Set `target` only where a command accepts one. `support` names the platforms whose source has a path for the \
       command, and `runner support` states the same for the selected-Runner route when it differs; neither is a \
       claim that the path was run.\n\n\
       Application ids are the ones `window.list` reports on this Device, and they differ by platform: a bundle id \
       on macOS (com.google.Chrome), the executable name on Windows (chrome.exe), and the AT-SPI \
       application id on Linux (org.gnome.TextEditor). Examples below use macOS ids; run `window.list` instead of guessing.\n\n\
       Points: with an app or window `target`, point coordinates are window-local unless `inputs.relative-to` is \
       `screen`. A `display.capture` image is in pixels: divide a pixel by the capture's scale factor and add its \
       capture bounds origin to get a screen point. Send a screen point without `target`, or with `target` and \
       `relative-to: screen`.\n\n\
       Input delivery: `inputs.input-policy` decides how input reaches a target window. Leave it unset unless an \
       action had no visible effect. Delivery never proves the app reacted; check with a capture.\n\
       - background-preferred (default for click, drag and scroll on a window target): send input to the window \
       without raising it or moving the real pointer. An inactive or covered app can ignore it.\n\
       - foreground-preferred (default for keyboard commands): raise the target window first, then send real input. \
       Use it when background delivery had no visible effect.\n\
       - background-only: never raise the window and never fall back; the call fails instead. Use it only when the \
       user's foreground must not change.\n\
       Platforms: Linux focuses the target window and sends all input to the foreground; background-only fails. \
       Windows sends background pointer input as window messages, which some toolkits ignore. On Windows and \
       Linux, keyboard commands type into the focused window only.\n\nCommands:",
    );
    for command in registry.all() {
      text.push_str(&format!("\n\n{}: {}", command.id, command.description));
      match command.role {
        auv_cli_invoke::CommandRole::Desktop => {}
        auv_cli_invoke::CommandRole::Alias { canonical } => text.push_str(&format!("\n  alias of: {canonical}")),
        auv_cli_invoke::CommandRole::Fixture => text.push_str("\n  fixture: processes data you supply; does not control a desktop"),
      }
      text.push_str(&format!("\n  support: {}", platform_support_summary(command)));
      if let Some(runner) = runner_support_summary(command) {
        text.push_str(&format!("\n  runner support: {runner}"));
      }
      for argument in command_arguments(command) {
        let required = if argument.required {
          "required"
        } else {
          "optional"
        };
        text.push_str(&format!("\n  inputs.{} ({required}): {}", argument.input_key, argument.help));
      }
      if command.target != auv_cli_invoke::command::TargetPolicy::Forbidden {
        text.push_str(&format!("\n  target: {}", command.target.help()));
      }
      // A command whose examples do not parse is caught by the registry tests;
      // the description stays usable without them.
      for example in command.examples().unwrap_or_default().iter().take(2) {
        text.push_str(&format!("\n  example: {}", mcp_example_call(command.id, example)));
      }
    }
    text
  }

  /// One line of per-platform support, such as `macOS supported; Linux partial; Windows unsupported`.
  ///
  /// NOTICE: Cell notes record source-audit facts for the support matrix, not caller guidance, and
  /// the same delivery note repeats on every keyboard command. The description explains input
  /// delivery once in its header; notes stay in `x-auv-commands` and `auv invoke <id> --help`.
  fn platform_support_summary(command: &InvokeCommand) -> String {
    support_summary(command.support)
  }

  /// One line of selected-Runner support, shown only where the route differs from the direct
  /// `support` line above it or the command has no Runner route at all.
  fn runner_support_summary(command: &InvokeCommand) -> Option<String> {
    match command.runner_cells() {
      None => Some(auv_cli_invoke::support::NO_RUNNER_ROUTE_NOTE.to_string()),
      Some(cells) if cells == command.support => None,
      Some(cells) => Some(support_summary(cells)),
    }
  }

  fn support_summary(cells: CommandSupport) -> String {
    auv_cli_invoke::Platform::ALL
      .iter()
      .map(|platform| format!("{} {}", platform.label(), cells.on(*platform).state.as_str()))
      .collect::<Vec<_>>()
      .join("; ")
  }

  struct CommandArgument {
    input_key: String,
    required: bool,
    help: String,
  }

  fn command_arguments(command: &InvokeCommand) -> Vec<CommandArgument> {
    command
      .input_arguments()
      .iter()
      .map(|argument| CommandArgument {
        input_key: auv_cli_invoke::command::input_key(argument),
        required: argument.is_required_set(),
        help: argument.get_help().map(ToString::to_string).unwrap_or_default(),
      })
      .collect()
  }

  /// One example as the MCP `invoke` arguments a client sends.
  fn mcp_example_call(command_id: &str, example: &auv_cli_invoke::InvokeExample) -> Value {
    let mut call = serde_json::json!({ "command_id": command_id, "inputs": example.inputs });
    if let Some(target) = &example.target {
      let (field, id) = match target {
        ExecutionTarget::Application { id } => ("application_id", id),
        ExecutionTarget::Window { id } => ("window_id", id),
        ExecutionTarget::Display { id } => ("display_id", id),
      };
      call["target"] = serde_json::json!({ field: id });
    }
    call
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
      "support": command.support,
      "runner_support": runner_support_metadata(command),
      "role": command.role,
      "arguments": clap_command
        .get_arguments()
        .filter(|argument| argument.get_id() != "help")
        .map(|argument| serde_json::json!({
          "flag": argument.get_long().map(|long| format!("--{long}")),
          "input_key": argument.get_long().unwrap_or_else(|| argument.get_id().as_str()),
          "value_name": argument.get_value_names().and_then(|names| names.first()).map(|name| name.as_str()),
          "required": argument.is_required_set(),
          "repeated": matches!(argument.get_action(), clap::ArgAction::Append),
          "help": argument.get_help().map(ToString::to_string),
        }))
        .collect::<Vec<_>>(),
      "examples": command
        .examples()
        .unwrap_or_default()
        .iter()
        .map(|example| serde_json::json!({ "summary": example.summary, "cli": example.cli, "call": mcp_example_call(command.id, example) }))
        .collect::<Vec<_>>(),
    })
  }

  /// The selected-Runner route's support next to the direct `support` cells:
  /// `route` names the declaration shape and `cells` the resolved per-platform
  /// cells. A command with no Runner route states that instead of "unsupported
  /// platforms", because the route is refused before any platform judgment.
  fn runner_support_metadata(command: &InvokeCommand) -> Value {
    let route = match command.runner {
      RunnerSupport::NoRoute => "no_route",
      RunnerSupport::Direct => "direct",
      RunnerSupport::Cells(_) => "cells",
    };
    match command.runner_cells() {
      None => serde_json::json!({ "route": route }),
      Some(cells) => serde_json::json!({ "route": route, "cells": cells }),
    }
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
  #[serde(deny_unknown_fields)]
  struct InvokeToolRequest {
    command_id: String,
    /// Exact configured Device name; omit to use the server's root selection or run locally.
    #[serde(default)]
    device_name: Option<String>,
    /// Canonical Device ID or unambiguous prefix; omit to use the server's root selection or run locally.
    #[serde(default)]
    device_id: Option<String>,
    #[serde(default)]
    target: McpInvokeTarget,
    #[serde(default)]
    inputs: BTreeMap<String, String>,
    #[serde(default)]
    dry_run: bool,
    #[serde(default)]
    store_root: Option<String>,
  }

  #[derive(Debug, Deserialize, JsonSchema)]
  #[serde(deny_unknown_fields)]
  struct OpRunToolRequest {
    /// Operation name, e.g. qqmusic.prepare_playback or qqmusic.prepare_playback_fast.
    operation: String,
    /// Execution mode. Required: there is no default, and a request without it is rejected before
    /// scheduling. Must match the mode the operation declares.
    mode: ExecutionModeArg,
    /// Optional path to a custom compiled operation JSON file, admitted alongside the built-ins.
    file: Option<PathBuf>,
  }

  #[derive(Debug, Deserialize, JsonSchema)]
  #[serde(deny_unknown_fields)]
  struct DeviceSessionsToolRequest {
    /// Exact configured Device name; omit for local discovery.
    device_name: Option<String>,
    /// Canonical Device ID or unambiguous prefix; omit for local discovery.
    device_id: Option<String>,
  }

  #[derive(Debug, Deserialize, JsonSchema)]
  #[serde(deny_unknown_fields)]
  struct DeviceGetUserSessionToolRequest {
    /// Exact configured Device name; omit for local discovery.
    device_name: Option<String>,
    /// Canonical Device ID or unambiguous prefix; omit for local discovery.
    device_id: Option<String>,
    /// Opaque selector returned by device_list_user_sessions.
    session_selector: String,
  }

  #[derive(Debug, Deserialize, JsonSchema)]
  #[serde(deny_unknown_fields)]
  struct DeviceUnlockToolRequest {
    /// Exact configured Device name; omit for local discovery.
    device_name: Option<String>,
    /// Canonical Device ID or unambiguous prefix; omit for local discovery.
    device_id: Option<String>,
    /// OS account on the selected Device.
    user: Option<String>,
    /// Opaque selector returned by device_list_user_sessions.
    session_selector: Option<String>,
  }

  impl DeviceUnlockToolRequest {
    fn target(&self) -> Result<auv::devices::UserSessionTarget, McpError> {
      match (self.user.as_deref(), self.session_selector.as_deref()) {
        (Some(user), None) if !user.trim().is_empty() => Ok(auv::devices::UserSessionTarget::User(user.to_string())),
        (None, Some(selector)) if !selector.trim().is_empty() => Ok(auv::devices::UserSessionTarget::SessionSelector(selector.to_string())),
        _ => Err(invalid_params("set exactly one nonempty user or session_selector")),
      }
    }
  }

  fn device_selection(device_name: Option<String>, device_id: Option<String>) -> Result<auv::selection::RootSelection, McpError> {
    if device_name.as_ref().is_some_and(|value| value.trim().is_empty()) || device_id.as_ref().is_some_and(|value| value.trim().is_empty()) {
      return Err(invalid_params("device_name and device_id must be nonempty when set"));
    }

    Ok(auv::selection::RootSelection {
      credential: None,
      device_name,
      device_id,
      run_id: None,
    })
  }

  fn device_selection_error(error: auv::selection::SelectedClientError) -> McpError {
    use auv::ContextError;
    use auv::client::PlacementError;
    use auv::devices::DeviceError;
    use auv::selection::{SelectedClientError, SelectionError};

    let caller_selection = match &error {
      SelectedClientError::Selection(SelectionError::Device(device)) => matches!(
        device,
        DeviceError::Identity(_) | DeviceError::NotFound | DeviceError::Ambiguous { .. } | DeviceError::SelectionConflict { .. }
      ),
      SelectedClientError::Selection(_) | SelectedClientError::Placement(PlacementError::Selection(_)) => true,
      SelectedClientError::Context(context) | SelectedClientError::Placement(PlacementError::Context(context)) => matches!(
        context,
        ContextError::Identity(_)
          | ContextError::DeviceNotConfigured
          | ContextError::DeviceSelectionAmbiguous { .. }
          | ContextError::CanonicalDeviceMissing(_)
      ),
      _ => false,
    };

    if caller_selection {
      invalid_params(error)
    } else {
      McpError::internal_error(error.to_string(), None)
    }
  }

  fn invalid_params(message: impl ToString) -> McpError {
    McpError::invalid_params(message.to_string(), None::<Value>)
  }

  pub async fn serve_stdio(project_root: PathBuf, selection: auv::selection::RootSelection) -> Result<(), String> {
    serve_server(McpServer::new(project_root)?.with_default_selection(selection)).await
  }

  /// Serve MCP stdio with explicit invoke metadata and shared typed commands.
  pub async fn serve_stdio_with_registry(
    project_root: PathBuf,
    invoke_registry: Arc<InvokeRegistry>,
    invoke_adapters: Vec<McpInvokeAdapter>,
  ) -> Result<(), String> {
    // TODO(mcp-plugin-device-selection): embedders serve without a root
    // Device selection; per-call `device_id` still routes core commands. Add a
    // selection parameter when a plugin frontend needs root `--device-id`.
    serve_server(McpServer::with_registry(project_root, invoke_registry, invoke_adapters)?).await
  }

  async fn serve_server(server: McpServer) -> Result<(), String> {
    let service = server.serve(stdio()).await.map_err(|error| format!("failed to serve MCP stdio transport: {error}"))?;
    service.waiting().await.map(|_| ()).map_err(|error| format!("mcp stdio server exited with error: {error}"))
  }

  #[cfg(test)]
  mod tests {
    include!("mcp_test.rs");
  }
}
