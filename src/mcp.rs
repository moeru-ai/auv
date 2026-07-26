use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
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

use auv_cli_invoke::{ArgSpec, InvokeCancellation, InvokeCommand, InvokeCommandInput, InvokeRegistry, default_registry};

tokio::task_local! {
  static MCP_REQUEST_CANCELLATION: InvokeCancellation;
}

type McpInvokeFuture = Pin<Box<dyn Future<Output = Result<McpInvokeSuccess, String>> + Send + 'static>>;
type InvokeDispatch = Arc<dyn Fn(Option<String>) -> Result<McpFrontendAuthority, String> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct McpInvokeInput {
  pub target_application_id: Option<String>,
  pub inputs: BTreeMap<String, String>,
  pub dry_run: bool,
  pub cancellation: InvokeCancellation,
}

impl McpInvokeInput {
  fn required_input(&self, command_id: &str, name: &str) -> Result<&str, String> {
    self
      .inputs
      .get(name)
      .map(String::as_str)
      .filter(|value| !value.trim().is_empty())
      .ok_or_else(|| format!("{command_id} requires --{name}"))
  }
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
      handler: Arc::new(move |input| Box::pin(handler(input))),
    }
  }

  fn invoke(&self, input: McpInvokeInput) -> McpInvokeFuture {
    if let Err(error) = input.cancellation.check() {
      return Box::pin(async move { Err(error.to_string()) });
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
  let root = store_root.map(PathBuf::from).unwrap_or_else(|| crate::default_project_store_root(project_root));
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

fn completed(result: Value) -> McpInvokeSuccess {
  McpInvokeSuccess::from_value(result)
}

fn reject_target_activation(input: &McpInvokeInput, command_id: &str) -> Result<(), String> {
  if input.target_application_id.is_some() {
    return Err(format!("{command_id} cannot use --target until typed input target activation is available"));
  }
  Ok(())
}

fn window_selector(input: &McpInvokeInput) -> auv_driver::WindowSelector {
  let mut selector = auv_driver::WindowSelector {
    main_visible: true,
    ..auv_driver::WindowSelector::default()
  };
  if let Some(target) = input
    .target_application_id
    .as_deref()
    .or_else(|| input.inputs.get("target").map(String::as_str))
    .filter(|value| !value.trim().is_empty())
  {
    selector.app = Some(auv_driver::App::bundle_id(target));
  }
  if let Some(title) = input.inputs.get("title").filter(|value| !value.trim().is_empty()) {
    selector.title = Some(auv_driver::TextMatcher::Contains(title.clone()));
  }
  selector
}

macro_rules! deferred_adapter {
  ($id:literal) => {
    McpInvokeAdapter::new($id, |_input| async move { unimplemented!($id) })
  };
}

fn click_window_point_adapter() -> McpInvokeAdapter {
  click_window_point_adapter_with(auv_cli_invoke::commands::input::click_window_point_domain)
}

fn media_control_adapter(id: &'static str, command: auv_media_macos::MediaCommand) -> McpInvokeAdapter {
  McpInvokeAdapter::new(id, move |_input| async move {
    let result = auv_cli_invoke::commands::media_control::control_media(command).await?;
    McpInvokeSuccess::from_result(&result)
  })
}

fn focus_text_adapter(id: &'static str) -> McpInvokeAdapter {
  McpInvokeAdapter::new(id, move |input| async move {
    if input.dry_run {
      return Ok(completed(Value::Null));
    }
    let app = input
      .target_application_id
      .as_deref()
      .or_else(|| input.inputs.get("target").map(String::as_str))
      .filter(|value| !value.trim().is_empty())
      .ok_or_else(|| format!("{id} requires --target"))?
      .to_string();
    let query = input.inputs.get("query").cloned().unwrap_or_default();
    let candidate = input.inputs.get("candidate").cloned().unwrap_or_default();
    let result = auv_cli_invoke::commands::input::focus_text(app, query, candidate).await?;
    McpInvokeSuccess::from_result(&result)
  })
}

fn click_window_point_adapter_with<F, Fut>(execute: F) -> McpInvokeAdapter
where
  F: Fn(InvokeCommandInput) -> Fut + Clone + Send + Sync + 'static,
  Fut: Future<Output = Result<auv_cli_invoke::commands::input::WindowPointClickOutcome, String>> + Send + 'static,
{
  McpInvokeAdapter::new("input.clickWindowPoint", move |input| {
    let execute = execute.clone();
    async move {
      let dry_run = input.dry_run;
      let outcome = execute(InvokeCommandInput {
        command_id: "input.clickWindowPoint".to_string(),
        target_application_id: input.target_application_id,
        inputs: input.inputs,
        dry_run,
        cancellation: input.cancellation,
      })
      .await?;
      McpInvokeSuccess::from_result(&outcome.into_result())
    }
  })
}

pub fn core_invoke_adapters() -> Vec<McpInvokeAdapter> {
  let mut adapters = vec![
    McpInvokeAdapter::new("app.probePermissions", |input| async move {
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let permissions = auv_cli_invoke::commands::app::read_permissions().await?;
      McpInvokeSuccess::from_result(&permissions)
    }),
    McpInvokeAdapter::new("app.activate", |input| async move {
      auv_cli_invoke::commands::app::activate_application(input.target_application_id).await?;
      Ok(completed(Value::Null))
    }),
    McpInvokeAdapter::new("scan.frame", |input| async move {
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let fixture_dir = input.required_input("scan.frame", "fixture-dir")?.to_string();
      let frame = auv_cli_invoke::commands::scan::produce_scan_frame(PathBuf::from(&fixture_dir)).await?;
      McpInvokeSuccess::from_result(&frame)
    }),
    McpInvokeAdapter::new("scan.coverage", |input| async move {
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let fixture_dir = input.required_input("scan.coverage", "fixture-dir")?.to_string();
      let coverage = auv_cli_invoke::commands::scan::produce_scan_coverage(PathBuf::from(&fixture_dir)).await?;
      McpInvokeSuccess::from_result(&coverage)
    }),
    McpInvokeAdapter::new("display.capture", |input| async move {
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let result = auv_cli_invoke::commands::display::capture_primary_display().await?;
      McpInvokeSuccess::from_result(&auv_cli_invoke::commands::display_capture_result(&result.display, &result.capture))
    }),
    McpInvokeAdapter::new("display.list", |input| async move {
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let displays = auv_cli_invoke::commands::display::observe_displays().await?;
      McpInvokeSuccess::from_result(&displays)
    }),
    McpInvokeAdapter::new("input.typeText", |input| async move {
      reject_target_activation(&input, "input.typeText")?;
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let text = input.required_input("input.typeText", "text")?.to_string();
      let result = auv_cli_invoke::commands::input::type_text_into_active_control(text).await?;
      McpInvokeSuccess::from_result(&result)
    }),
    McpInvokeAdapter::new("input.pasteText", |input| async move {
      reject_target_activation(&input, "input.pasteText")?;
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let text = input.required_input("input.pasteText", "text")?.to_string();
      let result = auv_cli_invoke::commands::input::paste_text_into_active_control(text).await?;
      McpInvokeSuccess::from_result(&result)
    }),
    McpInvokeAdapter::new("input.key", |input| async move {
      reject_target_activation(&input, "input.key")?;
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let key = input.required_input("input.key", "key")?.to_string();
      let result = auv_cli_invoke::commands::input::press_key_in_active_app(key).await?;
      McpInvokeSuccess::from_result(&result)
    }),
    click_window_point_adapter(),
    McpInvokeAdapter::new("screen.captureRegion", |input| async move {
      reject_target_activation(&input, "screen.captureRegion")?;
      let region = auv_cli_invoke::commands::screen::Region::parse(&input.inputs, "screen.captureRegion")?.into_rect();
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let result = auv_cli_invoke::commands::screen::capture_screen_region(region).await?;
      McpInvokeSuccess::from_result(&auv_cli_invoke::commands::display_capture_result(&result.display, &result.capture))
    }),
    McpInvokeAdapter::new("screen.findText", |input| async move {
      reject_target_activation(&input, "screen.findText")?;
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let query = input.required_input("screen.findText", "query")?.to_string();
      let matches = auv_cli_invoke::commands::screen::recognize_screen_text(query, false).await?;
      McpInvokeSuccess::from_result(&matches)
    }),
    McpInvokeAdapter::new("screen.waitForText", |input| async move {
      reject_target_activation(&input, "screen.waitForText")?;
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let query = input.required_input("screen.waitForText", "query")?.to_string();
      let matches = auv_cli_invoke::commands::screen::recognize_screen_text(query, true).await?;
      McpInvokeSuccess::from_result(&matches)
    }),
    McpInvokeAdapter::new("screen.clickText", |input| async move {
      reject_target_activation(&input, "screen.clickText")?;
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let query = input.required_input("screen.clickText", "query")?.to_string();
      let result = auv_cli_invoke::commands::screen::click_recognized_screen_text(query).await?;
      McpInvokeSuccess::from_result(&result)
    }),
    McpInvokeAdapter::new("window.list", |input| async move {
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let windows = auv_cli_invoke::commands::window::observe_windows().await?;
      McpInvokeSuccess::from_result(&windows)
    }),
    McpInvokeAdapter::new("window.capture", |input| async move {
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let result = auv_cli_invoke::commands::window::capture_selected_window(window_selector(&input)).await?;
      McpInvokeSuccess::from_result(&auv_cli_invoke::commands::window::window_capture_result(&result))
    }),
    McpInvokeAdapter::new("window.findText", |input| async move {
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let query = input.required_input("window.findText", "query")?.to_string();
      let result = auv_cli_invoke::commands::window::recognize_window_text(window_selector(&input), query, false).await?;
      McpInvokeSuccess::from_result(&result)
    }),
    McpInvokeAdapter::new("window.waitForText", |input| async move {
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let query = input.required_input("window.waitForText", "query")?.to_string();
      let result = auv_cli_invoke::commands::window::recognize_window_text(window_selector(&input), query, true).await?;
      McpInvokeSuccess::from_result(&result)
    }),
    McpInvokeAdapter::new("window.clickText", |input| async move {
      if input.dry_run {
        return Ok(completed(Value::Null));
      }
      let query = input.required_input("window.clickText", "query")?.to_string();
      let result = auv_cli_invoke::commands::window::click_recognized_window_text(window_selector(&input), query).await?;
      McpInvokeSuccess::from_result(&result)
    }),
  ];

  adapters.extend([
    deferred_adapter!("display.projectScreenshotPoint"),
    deferred_adapter!("display.identifyPoint"),
    focus_text_adapter("input.focusText"),
    deferred_adapter!("input.pressButton"),
    deferred_adapter!("input.axPressButton"),
    focus_text_adapter("input.axFocusText"),
    deferred_adapter!("input.axClickWindowText"),
    deferred_adapter!("input.smartPress"),
    deferred_adapter!("input.clickPoint"),
    deferred_adapter!("input.teachClick"),
    deferred_adapter!("input.scrollPoint"),
    deferred_adapter!("screen.findRows"),
    deferred_adapter!("screen.waitForRows"),
    deferred_adapter!("screen.findImageText"),
    deferred_adapter!("screen.clickRow"),
    deferred_adapter!("window.captureAxTree"),
    deferred_adapter!("window.findRows"),
    deferred_adapter!("window.waitForRows"),
    deferred_adapter!("window.observeRegion"),
    deferred_adapter!("window.findIconMatch"),
    deferred_adapter!("window.scrollRegion"),
    deferred_adapter!("window.verifyText"),
    deferred_adapter!("window.clickRow"),
    deferred_adapter!("overlay.clickPoint"),
    deferred_adapter!("overlay.showCursor"),
    deferred_adapter!("overlay.showDualCursor"),
    deferred_adapter!("overlay.applyCursorBatch"),
    deferred_adapter!("overlay.setCursor"),
    deferred_adapter!("overlay.moveCursor"),
    deferred_adapter!("overlay.moveCursorById"),
    deferred_adapter!("overlay.flashCursor"),
    deferred_adapter!("overlay.flashCursorById"),
    deferred_adapter!("overlay.hideCursorId"),
    deferred_adapter!("overlay.hideCursor"),
    deferred_adapter!("overlay.shutdown"),
    McpInvokeAdapter::new("mediaControl.nowPlaying", |_input| async move {
      let result = auv_cli_invoke::commands::media_control::read_now_playing().await?;
      McpInvokeSuccess::from_result(&result)
    }),
    media_control_adapter("mediaControl.play", auv_media_macos::MediaCommand::Play),
    media_control_adapter("mediaControl.pause", auv_media_macos::MediaCommand::Pause),
    media_control_adapter("mediaControl.togglePlayPause", auv_media_macos::MediaCommand::TogglePlayPause),
    media_control_adapter("mediaControl.next", auv_media_macos::MediaCommand::NextTrack),
    media_control_adapter("mediaControl.previous", auv_media_macos::MediaCommand::PreviousTrack),
  ]);
  adapters
}

#[tool_router(router = tool_router)]
impl McpServer {
  #[tool(
    description = "Invoke one explicit cataloged AUV command id through its MCP typed adapter. See input_schema.x-auv-commands for available command metadata.",
    input_schema = invoke_tool_input_schema()
  )]
  async fn invoke(&self, Parameters(req): Parameters<InvokeToolRequest>) -> Result<CallToolResult, McpError> {
    let adapter = self
      .invoke_adapters
      .get(req.command_id.as_str())
      .cloned()
      .ok_or_else(|| invalid_params(format!("unknown invoke command: {}", req.command_id)))?;
    let authority = (self.invoke_dispatch)(req.store_root).map_err(invalid_params)?;
    let cancellation = MCP_REQUEST_CANCELLATION.try_with(Clone::clone).unwrap_or_default();
    let input = McpInvokeInput {
      target_application_id: req.target.application_id,
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
          Err("invoke cancelled".to_string())
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
          failure,
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
        "MCP exposes explicit AUV tools with catalog metadata and MCP-owned typed invoke adapters; no planner or NL parsing is present."
          .into(),
      ),
      capabilities: ServerCapabilities::builder().enable_tools().build(),
      ..Default::default()
    }
  }
}

fn invoke_tool_input_schema() -> Arc<JsonObject> {
  // Static schema uses core registry; product servers rewrite via list_tools
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
  serde_json::json!({
    "id": command.id,
    "namespace": command.namespace.as_str(),
    "description": command.description,
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
#[serde(deny_unknown_fields)]
struct McpInvokeTarget {
  application_id: Option<String>,
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

/// Serve MCP stdio with explicit invoke metadata and MCP-owned command adapters.
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
