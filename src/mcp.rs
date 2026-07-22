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

use auv_cli_invoke::{ArgSpec, InvokeCancellation, InvokeCommand, InvokeRegistry, default_registry};

tokio::task_local! {
  static MCP_REQUEST_CANCELLATION: InvokeCancellation;
}

type McpInvokeFuture = Pin<Box<dyn Future<Output = Result<McpInvokeOutcome, String>> + Send + 'static>>;
type InvokeDispatch = Arc<dyn Fn(Option<String>) -> Result<McpFrontendAuthority, String> + Send + Sync>;

#[derive(Clone, Debug)]
pub struct McpInvokeInput {
  pub target_application_id: Option<String>,
  pub target_label: Option<String>,
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
    Fut: Future<Output = Result<McpInvokeOutcome, String>> + Send + 'static,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum McpInvokeStatus {
  Completed,
  Failed,
}

impl McpInvokeStatus {
  fn as_str(self) -> &'static str {
    match self {
      Self::Completed => "completed",
      Self::Failed => "failed",
    }
  }
}

#[derive(Clone, Debug)]
pub struct McpInvokeOutcome {
  status: McpInvokeStatus,
  output_summary: String,
  signals: BTreeMap<String, Value>,
  details: BTreeMap<String, Value>,
  artifact_failures: Vec<auv_cli_invoke::ArtifactInstrumentationFailure>,
  failure_message: Option<String>,
}

impl McpInvokeOutcome {
  pub fn completed(summary: impl Into<String>, details: Value) -> Self {
    Self {
      status: McpInvokeStatus::Completed,
      output_summary: summary.into(),
      signals: BTreeMap::new(),
      details: object_fields(details),
      artifact_failures: Vec::new(),
      failure_message: None,
    }
  }

  pub fn failed(summary: impl Into<String>, failure: impl Into<String>, details: Value) -> Self {
    Self {
      status: McpInvokeStatus::Failed,
      output_summary: summary.into(),
      signals: BTreeMap::new(),
      details: object_fields(details),
      artifact_failures: Vec::new(),
      failure_message: Some(failure.into()),
    }
  }

  pub fn insert_signal(&mut self, name: impl Into<String>, value: impl Into<Value>) {
    self.signals.insert(name.into(), value.into());
  }

  pub fn mark_failed(&mut self, summary: impl Into<String>, failure: impl Into<String>) {
    self.status = McpInvokeStatus::Failed;
    self.output_summary = summary.into();
    self.failure_message = Some(failure.into());
  }

  pub fn with_artifact_instrumentation(mut self, receipt: auv_cli_invoke::ArtifactInstrumentationReceipt) -> Self {
    self.artifact_failures = receipt.into_failures();
    self
  }
}

fn object_fields(value: Value) -> BTreeMap<String, Value> {
  match value {
    Value::Object(fields) => fields.into_iter().collect(),
    Value::Null => BTreeMap::new(),
    other => BTreeMap::from([("value".to_string(), other)]),
  }
}

#[derive(Clone)]
pub struct McpServer {
  project_root: PathBuf,
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
      Arc::new(move |store_root| build_invoke_dispatch(dispatch_project_root.clone(), store_root)),
    )
  }

  fn with_invoke_dispatch(
    project_root: PathBuf,
    invoke_registry: Arc<InvokeRegistry>,
    invoke_adapters: Vec<McpInvokeAdapter>,
    invoke_dispatch: InvokeDispatch,
  ) -> Result<Self, String> {
    let invoke_adapters = validated_adapter_catalog(invoke_registry.as_ref(), invoke_adapters)?;
    Ok(Self {
      project_root,
      tool_router: Self::tool_router(),
      invoke_registry,
      invoke_adapters: Arc::new(invoke_adapters),
      invoke_dispatch,
    })
  }

  pub fn invoke_registry(&self) -> &Arc<InvokeRegistry> {
    &self.invoke_registry
  }

  fn authority_store(&self, store_root: Option<String>) -> Result<Arc<dyn auv_tracing::RunStore>, McpError> {
    let root = store_root.map(PathBuf::from).unwrap_or_else(|| crate::default_project_store_root(self.project_root.clone()));
    auv_tracing::FileRunStore::open(&root)
      .map(|store| Arc::new(store) as Arc<dyn auv_tracing::RunStore>)
      .map_err(|error| invalid_params(format!("failed to open MCP run authority {}: {error}", root.display())))
  }
}

#[derive(Serialize)]
struct McpInvokePresentation {
  run_id: String,
  status: &'static str,
  output_summary: String,
  signals: BTreeMap<String, Value>,
  artifacts: Vec<auv_tracing::ArtifactMetadata>,
  artifact_failures: Vec<auv_cli_invoke::ArtifactInstrumentationFailure>,
  failure_message: Option<String>,
  tracing_failure: Option<String>,
  result: BTreeMap<String, Value>,
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

fn build_invoke_dispatch(project_root: PathBuf, store_root: Option<String>) -> Result<McpFrontendAuthority, String> {
  let root = store_root.map(PathBuf::from).unwrap_or_else(|| crate::default_project_store_root(project_root));
  let store = auv_tracing::FileRunStore::open(&root)
    .map(|store| Arc::new(store) as Arc<dyn auv_tracing::RunStore>)
    .map_err(|error| format!("failed to open MCP run authority {}: {error}", root.display()))?;
  let dispatch = auv_tracing::configure().run_store(store.clone()).build().map_err(|error| error.to_string())?;
  Ok(McpFrontendAuthority { dispatch })
}

struct McpFrontendExecution {
  run_id: auv_tracing::RunId,
  direct_result: Result<McpInvokeOutcome, String>,
  tracing_failure: Option<String>,
  canonical_artifacts: Vec<auv_tracing::ArtifactMetadata>,
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

async fn execute_mcp_frontend<F, Fut>(
  authority: &McpFrontendAuthority,
  cancellation: InvokeCancellation,
  call: F,
) -> Result<McpFrontendExecution, String>
where
  F: FnOnce() -> Fut + Send + 'static,
  Fut: Future<Output = Result<McpInvokeOutcome, String>> + Send + 'static,
{
  let recorded = authority
    .dispatch
    .record(|| {
      auv_tracing::emit_event!(McpFrontendLifecycle { frontend: "mcp" });
      let future = call();
      async move {
        tokio::pin!(future);
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
          result = &mut future => result,
        }
      }
    })
    .await
    .map_err(|error| error.to_string())?;
  let (run_id, direct_result, tracing_failure, snapshot) = recorded.into_parts();
  let canonical_artifacts = snapshot.artifacts().values().map(|artifact| artifact.metadata().clone()).collect();
  Ok(McpFrontendExecution {
    run_id,
    direct_result,
    tracing_failure: tracing_failure.map(|error| error.to_string()),
    canonical_artifacts,
  })
}

fn completed(summary: impl Into<String>, fields: Value) -> McpInvokeOutcome {
  McpInvokeOutcome::completed(summary, fields)
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

fn ocr_fields(matches: &auv_driver::OcrMatches) -> Value {
  serde_json::json!({
    "match_count": matches.matches.len(),
    "best_text": matches.matches.first().map(|matched| matched.text.as_str()),
  })
}

macro_rules! deferred_adapter {
  ($id:literal, $call:expr, $summary:literal) => {
    McpInvokeAdapter::new($id, |_input| async move {
      $call.await?;
      Ok(completed($summary, serde_json::json!({})))
    })
  };
}

pub fn core_invoke_adapters() -> Vec<McpInvokeAdapter> {
  let mut adapters = vec![
    McpInvokeAdapter::new("app.probePermissions", |input| async move {
      if input.dry_run {
        return Ok(completed("dry run: app.probePermissions would probe macOS permissions", serde_json::json!({})));
      }
      let permissions = auv_cli_invoke::commands::app::read_permissions().await?;
      Ok(completed(
        "macOS permissions probed",
        serde_json::json!({
          "permissions": {
            "screen_recording": permissions.screen_recording.as_str(),
            "screen_capture_kit": permissions.screen_capture_kit.as_str(),
            "accessibility": permissions.accessibility.as_str(),
            "automation_to_system_events": permissions.automation_to_system_events.as_str(),
          }
        }),
      ))
    }),
    McpInvokeAdapter::new("app.activate", |input| async move {
      auv_cli_invoke::commands::app::activate_application(input.target_application_id).await?;
      Ok(completed("activated target app", serde_json::json!({})))
    }),
    McpInvokeAdapter::new("scan.frame", |input| async move {
      if input.dry_run {
        return Ok(completed("scan.frame dry-run", serde_json::json!({})));
      }
      let fixture_dir = input.required_input("scan.frame", "fixture-dir")?.to_string();
      let (frame, instrumentation) = auv_cli_invoke::commands::scan::produce_scan_frame(PathBuf::from(&fixture_dir)).await?.into_parts();
      Ok(
        completed(format!("scan frame produced from fixture {fixture_dir}"), serde_json::json!({ "frame": frame }))
          .with_artifact_instrumentation(instrumentation),
      )
    }),
    McpInvokeAdapter::new("scan.coverage", |input| async move {
      if input.dry_run {
        return Ok(completed("scan.coverage dry-run", serde_json::json!({})));
      }
      let fixture_dir = input.required_input("scan.coverage", "fixture-dir")?.to_string();
      let (coverage, instrumentation) =
        auv_cli_invoke::commands::scan::produce_scan_coverage(PathBuf::from(&fixture_dir)).await?.into_parts();
      Ok(
        completed(format!("scan coverage produced from fixture {fixture_dir}"), serde_json::json!({ "coverage": coverage }))
          .with_artifact_instrumentation(instrumentation),
      )
    }),
    McpInvokeAdapter::new("display.capture", |input| async move {
      if input.dry_run {
        return Ok(completed("dry run: display.capture would capture the primary display", serde_json::json!({})));
      }
      let (result, instrumentation) = auv_cli_invoke::commands::display::capture_primary_display().await?.into_parts();
      Ok(
        completed(
          "display captured",
          serde_json::json!({
            "display_id": result.display.id,
            "backend": result.capture.backend,
            "pixel_width": result.capture.image.width(),
            "pixel_height": result.capture.image.height(),
          }),
        )
        .with_artifact_instrumentation(instrumentation),
      )
    }),
    McpInvokeAdapter::new("display.list", |input| async move {
      if input.dry_run {
        return Ok(completed("dry run: display.list would enumerate connected displays", serde_json::json!({})));
      }
      let displays = auv_cli_invoke::commands::display::observe_displays().await?;
      Ok(completed(
        format!("listed {} display(s)", displays.displays.len()),
        serde_json::json!({ "display_count": displays.displays.len() }),
      ))
    }),
    McpInvokeAdapter::new("input.typeText", |input| async move {
      reject_target_activation(&input, "input.typeText")?;
      if input.dry_run {
        return Ok(completed("dry run: input.typeText", serde_json::json!({})));
      }
      let text = input.required_input("input.typeText", "text")?.to_string();
      let (result, instrumentation) = auv_cli_invoke::commands::input::type_text_into_active_control(text).await?.into_parts();
      Ok(
        completed("typed text into active control", serde_json::json!({ "selected_path": result.selected_path.as_str() }))
          .with_artifact_instrumentation(instrumentation),
      )
    }),
    McpInvokeAdapter::new("input.pasteText", |input| async move {
      reject_target_activation(&input, "input.pasteText")?;
      if input.dry_run {
        return Ok(completed("dry run: input.pasteText", serde_json::json!({})));
      }
      let text = input.required_input("input.pasteText", "text")?.to_string();
      auv_cli_invoke::commands::input::paste_text_into_active_control(text).await?;
      Ok(completed("pasted text into active control", serde_json::json!({ "clipboard_disturbance": "temporary" })))
    }),
    McpInvokeAdapter::new("input.key", |input| async move {
      reject_target_activation(&input, "input.key")?;
      if input.dry_run {
        return Ok(completed("dry run: input.key", serde_json::json!({})));
      }
      let key = input.required_input("input.key", "key")?.to_string();
      let (result, instrumentation) = auv_cli_invoke::commands::input::press_key_in_active_app(key.clone()).await?.into_parts();
      Ok(
        completed("pressed key in active app", serde_json::json!({ "key": key, "selected_path": result.selected_path.as_str() }))
          .with_artifact_instrumentation(instrumentation),
      )
    }),
    McpInvokeAdapter::new("input.clickWindowPoint", |input| async move {
      let point = auv_cli_invoke::commands::input::WindowPointInput::parse(&input.inputs, "input.clickWindowPoint")?;
      if input.dry_run {
        return Ok(completed("dry run: input.clickWindowPoint", serde_json::json!({})));
      }
      let (result, instrumentation) =
        auv_cli_invoke::commands::input::click_point_in_window(window_selector(&input), point).await?.into_parts();
      Ok(
        completed(
          "clicked window point",
          serde_json::json!({
            "window_id": result.window.reference.id,
            "window_x": result.point.point().x,
            "window_y": result.point.point().y,
            "selected_path": result.action.selected_path.as_str(),
          }),
        )
        .with_artifact_instrumentation(instrumentation),
      )
    }),
    McpInvokeAdapter::new("screen.captureRegion", |input| async move {
      reject_target_activation(&input, "screen.captureRegion")?;
      let region = auv_cli_invoke::commands::screen::Region::parse(&input.inputs, "screen.captureRegion")?.into_rect();
      if input.dry_run {
        return Ok(completed("dry run: screen.captureRegion", serde_json::json!({})));
      }
      let (result, instrumentation) = auv_cli_invoke::commands::screen::capture_screen_region(region).await?.into_parts();
      Ok(
        completed(
          "screen region captured",
          serde_json::json!({
            "display_id": result.display.id,
            "pixel_width": result.capture.image.width(),
            "pixel_height": result.capture.image.height(),
          }),
        )
        .with_artifact_instrumentation(instrumentation),
      )
    }),
    McpInvokeAdapter::new("screen.findText", |input| async move {
      reject_target_activation(&input, "screen.findText")?;
      if input.dry_run {
        return Ok(completed("dry run: screen.findText", serde_json::json!({})));
      }
      let query = input.required_input("screen.findText", "query")?.to_string();
      let (matches, instrumentation) = auv_cli_invoke::commands::screen::recognize_screen_text(query, false).await?.into_parts();
      Ok(completed("screen text recognized", ocr_fields(&matches)).with_artifact_instrumentation(instrumentation))
    }),
    McpInvokeAdapter::new("screen.waitForText", |input| async move {
      reject_target_activation(&input, "screen.waitForText")?;
      if input.dry_run {
        return Ok(completed("dry run: screen.waitForText", serde_json::json!({})));
      }
      let query = input.required_input("screen.waitForText", "query")?.to_string();
      let (matches, instrumentation) = auv_cli_invoke::commands::screen::recognize_screen_text(query, true).await?.into_parts();
      Ok(completed("screen text recognized after waiting", ocr_fields(&matches)).with_artifact_instrumentation(instrumentation))
    }),
    McpInvokeAdapter::new("screen.clickText", |input| async move {
      reject_target_activation(&input, "screen.clickText")?;
      if input.dry_run {
        return Ok(completed("dry run: screen.clickText", serde_json::json!({})));
      }
      let query = input.required_input("screen.clickText", "query")?.to_string();
      let (result, instrumentation) = auv_cli_invoke::commands::screen::click_recognized_screen_text(query).await?.into_parts();
      Ok(
        completed(
          "clicked recognized screen text",
          serde_json::json!({
            "match_count": result.matches.matches.len(),
            "click_x": result.point.x,
            "click_y": result.point.y,
          }),
        )
        .with_artifact_instrumentation(instrumentation),
      )
    }),
    McpInvokeAdapter::new("window.list", |input| async move {
      if input.dry_run {
        return Ok(completed("dry run: window.list", serde_json::json!({})));
      }
      let windows = auv_cli_invoke::commands::window::observe_windows().await?;
      Ok(completed(format!("listed {} window(s)", windows.len()), serde_json::json!({ "window_count": windows.len() })))
    }),
    McpInvokeAdapter::new("window.capture", |input| async move {
      if input.dry_run {
        return Ok(completed("dry run: window.capture", serde_json::json!({})));
      }
      let (result, instrumentation) = auv_cli_invoke::commands::window::capture_selected_window(window_selector(&input)).await?.into_parts();
      Ok(
        completed(
          "window captured",
          serde_json::json!({
            "window_id": result.window.reference.id,
            "pixel_width": result.capture.image.width(),
            "pixel_height": result.capture.image.height(),
          }),
        )
        .with_artifact_instrumentation(instrumentation),
      )
    }),
    McpInvokeAdapter::new("window.findText", |input| async move {
      if input.dry_run {
        return Ok(completed("dry run: window.findText", serde_json::json!({})));
      }
      let query = input.required_input("window.findText", "query")?.to_string();
      let (result, instrumentation) =
        auv_cli_invoke::commands::window::recognize_window_text(window_selector(&input), query, false).await?.into_parts();
      let mut fields = ocr_fields(&result.matches);
      fields.as_object_mut().expect("OCR fields are an object").insert("window_id".to_string(), Value::String(result.window.reference.id));
      Ok(completed("window text recognized", fields).with_artifact_instrumentation(instrumentation))
    }),
    McpInvokeAdapter::new("window.waitForText", |input| async move {
      if input.dry_run {
        return Ok(completed("dry run: window.waitForText", serde_json::json!({})));
      }
      let query = input.required_input("window.waitForText", "query")?.to_string();
      let (result, instrumentation) =
        auv_cli_invoke::commands::window::recognize_window_text(window_selector(&input), query, true).await?.into_parts();
      let mut fields = ocr_fields(&result.matches);
      fields.as_object_mut().expect("OCR fields are an object").insert("window_id".to_string(), Value::String(result.window.reference.id));
      Ok(completed("window text recognized after waiting", fields).with_artifact_instrumentation(instrumentation))
    }),
    McpInvokeAdapter::new("window.clickText", |input| async move {
      if input.dry_run {
        return Ok(completed("dry run: window.clickText", serde_json::json!({})));
      }
      let query = input.required_input("window.clickText", "query")?.to_string();
      let (result, instrumentation) =
        auv_cli_invoke::commands::window::click_recognized_window_text(window_selector(&input), query).await?.into_parts();
      Ok(
        completed(
          "clicked recognized window text",
          serde_json::json!({
            "window_id": result.window.reference.id,
            "match_count": result.matches.matches.len(),
            "window_x": result.point.point().x,
            "window_y": result.point.point().y,
            "selected_path": result.action.selected_path.as_str(),
          }),
        )
        .with_artifact_instrumentation(instrumentation),
      )
    }),
  ];

  adapters.extend([
    deferred_adapter!(
      "display.projectScreenshotPoint",
      auv_cli_invoke::commands::display::project_primary_screenshot_point(),
      "projected screenshot point"
    ),
    deferred_adapter!("display.identifyPoint", auv_cli_invoke::commands::display::identify_display_point(), "identified display point"),
    deferred_adapter!("input.focusText", auv_cli_invoke::commands::input::focus_text(), "focused text input"),
    deferred_adapter!("input.pressButton", auv_cli_invoke::commands::input::press_button_by_query(), "pressed button"),
    deferred_adapter!("input.axPressButton", auv_cli_invoke::commands::input::press_button_with_ax(), "pressed button through AX"),
    deferred_adapter!("input.axFocusText", auv_cli_invoke::commands::input::focus_text_with_ax(), "focused text through AX"),
    deferred_adapter!(
      "input.axClickWindowText",
      auv_cli_invoke::commands::input::click_window_text_with_ax(),
      "clicked window text through AX"
    ),
    deferred_adapter!("input.smartPress", auv_cli_invoke::commands::input::resolve_and_press(), "resolved and pressed target"),
    deferred_adapter!("input.clickPoint", auv_cli_invoke::commands::input::click_global_point(), "clicked global point"),
    deferred_adapter!("input.teachClick", auv_cli_invoke::commands::input::teach_click_workflow(), "recorded taught click"),
    deferred_adapter!("input.scrollPoint", auv_cli_invoke::commands::input::scroll_global_point(), "scrolled global point"),
    deferred_adapter!("screen.findRows", auv_cli_invoke::commands::screen::find_screen_rows_domain(), "found screen rows"),
    deferred_adapter!(
      "screen.waitForRows",
      auv_cli_invoke::commands::screen::wait_for_screen_rows_domain(),
      "found screen rows after waiting"
    ),
    deferred_adapter!("screen.findImageText", auv_cli_invoke::commands::screen::recognize_image_text(), "recognized image text"),
    deferred_adapter!("screen.clickRow", auv_cli_invoke::commands::screen::click_screen_row_domain(), "clicked screen row"),
    deferred_adapter!("window.captureAxTree", auv_cli_invoke::commands::window::capture_ax_tree_snapshot(), "captured AX tree"),
    deferred_adapter!("window.findRows", auv_cli_invoke::commands::window::find_window_rows_domain(), "found window rows"),
    deferred_adapter!(
      "window.waitForRows",
      auv_cli_invoke::commands::window::wait_for_window_rows_domain(),
      "found window rows after waiting"
    ),
    deferred_adapter!("window.observeRegion", auv_cli_invoke::commands::window::observe_window_region_domain(), "observed window region"),
    deferred_adapter!("window.findIconMatch", auv_cli_invoke::commands::window::find_window_icon_match(), "found window icon match"),
    deferred_adapter!("window.scrollRegion", auv_cli_invoke::commands::window::scroll_window_region_domain(), "scrolled window region"),
    deferred_adapter!("window.verifyText", auv_cli_invoke::commands::window::verify_window_ax_text(), "verified window AX text"),
    deferred_adapter!("window.clickRow", auv_cli_invoke::commands::window::click_window_row_domain(), "clicked window row"),
    deferred_adapter!("overlay.clickPoint", auv_cli_invoke::commands::overlay::click_point(), "clicked overlay point"),
    deferred_adapter!("overlay.showCursor", auv_cli_invoke::commands::overlay::show_cursor(), "showed overlay cursor"),
    deferred_adapter!("overlay.showDualCursor", auv_cli_invoke::commands::overlay::show_dual_cursor(), "showed dual overlay cursors"),
    deferred_adapter!("overlay.applyCursorBatch", auv_cli_invoke::commands::overlay::apply_cursor_batch(), "applied overlay cursor batch"),
    deferred_adapter!("overlay.setCursor", auv_cli_invoke::commands::overlay::set_cursor(), "set overlay cursor"),
    deferred_adapter!("overlay.moveCursor", auv_cli_invoke::commands::overlay::move_cursor(), "moved overlay cursor"),
    deferred_adapter!("overlay.moveCursorById", auv_cli_invoke::commands::overlay::move_cursor_by_id(), "moved overlay cursor by id"),
    deferred_adapter!("overlay.flashCursor", auv_cli_invoke::commands::overlay::flash_cursor(), "flashed overlay cursor"),
    deferred_adapter!("overlay.flashCursorById", auv_cli_invoke::commands::overlay::flash_cursor_by_id(), "flashed overlay cursor by id"),
    deferred_adapter!("overlay.hideCursorId", auv_cli_invoke::commands::overlay::hide_cursor_by_id(), "hid overlay cursor by id"),
    deferred_adapter!("overlay.hideCursor", auv_cli_invoke::commands::overlay::hide_cursor(), "hid overlay cursor"),
    deferred_adapter!("overlay.shutdown", auv_cli_invoke::commands::overlay::shutdown(), "shut down overlay"),
    deferred_adapter!("mediaControl.nowPlaying", auv_cli_invoke::commands::media_control::read_now_playing(), "read now-playing state"),
    deferred_adapter!("mediaControl.play", auv_cli_invoke::commands::media_control::play_media(), "played media"),
    deferred_adapter!("mediaControl.pause", auv_cli_invoke::commands::media_control::pause_media(), "paused media"),
    deferred_adapter!(
      "mediaControl.togglePlayPause",
      auv_cli_invoke::commands::media_control::toggle_play_pause(),
      "toggled media playback"
    ),
    deferred_adapter!("mediaControl.next", auv_cli_invoke::commands::media_control::next_track(), "advanced to next track"),
    deferred_adapter!("mediaControl.previous", auv_cli_invoke::commands::media_control::previous_track(), "returned to previous track"),
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
    let authority = (self.invoke_dispatch)(req.inspect.store_root).map_err(invalid_params)?;
    let cancellation = MCP_REQUEST_CANCELLATION.try_with(Clone::clone).unwrap_or_default();
    let input = McpInvokeInput {
      target_application_id: req.target.application_id,
      target_label: req.target.target_label,
      inputs: req.inputs,
      dry_run: req.dry_run,
      cancellation: cancellation.clone(),
    };
    let execution = execute_mcp_frontend(&authority, cancellation, move || adapter.invoke(input)).await.map_err(invalid_params)?;
    let outcome = match execution.direct_result {
      Ok(outcome) => outcome,
      Err(error) => McpInvokeOutcome::failed(error.clone(), error, Value::Null),
    };
    let failed = outcome.status == McpInvokeStatus::Failed;
    let value = serde_json::to_value(McpInvokePresentation {
      run_id: execution.run_id.to_string(),
      status: outcome.status.as_str(),
      output_summary: outcome.output_summary,
      signals: outcome.signals,
      artifacts: execution.canonical_artifacts,
      artifact_failures: outcome.artifact_failures,
      failure_message: outcome.failure_message,
      tracing_failure: execution.tracing_failure,
      result: outcome.details,
    })
    .map_err(invalid_params)?;
    Ok(if failed {
      CallToolResult::structured_error(value)
    } else {
      CallToolResult::structured(value)
    })
  }

  #[tool(description = "Inspect one existing AUV run id.")]
  async fn run_inspect(&self, Parameters(req): Parameters<RunInspectRequest>) -> Result<CallToolResult, McpError> {
    let store = self.authority_store(req.store_root)?;
    let run_id = req.run_id.parse::<auv_tracing::RunId>().map_err(invalid_params)?;
    let snapshot =
      store.load_snapshot(run_id).await.map_err(invalid_params)?.ok_or_else(|| invalid_params(format!("run not found: {run_id}")))?;
    Ok(CallToolResult::structured(serde_json::to_value(auv_inspect_model::InspectDocument::from(&snapshot)).map_err(invalid_params)?))
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

#[cfg(test)]
mod tests {
  use std::future::Future;
  use std::sync::Mutex;
  use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

  use super::*;
  use crate::model::now_millis;
  use auv_tracing::{
    AuthorityId, BoxFuture, ErrorCode, EventPayload, MemoryRunStore, RunStore, TelemetryError, TelemetryItem, TelemetryProjector,
    TelemetryRoutePolicy,
  };
  use rmcp::{
    ClientHandler, ServiceExt,
    model::{CallToolRequestParam, ClientInfo, ClientRequest, Request},
    service::PeerRequestOptions,
  };

  #[derive(Debug, Clone, Default)]
  struct DummyClientHandler;

  impl ClientHandler for DummyClientHandler {
    fn get_info(&self) -> ClientInfo {
      ClientInfo::default()
    }
  }

  #[derive(Clone, Default)]
  struct CountingCall {
    calls: Arc<AtomicUsize>,
  }

  impl CountingCall {
    fn call_count(&self) -> usize {
      self.calls.load(Ordering::SeqCst)
    }

    fn call(&self) -> impl Future<Output = Result<u32, String>> + Send + 'static + use<> {
      auv_tracing::emit_event!(McpCallEvent {
        phase: "constructed"
      });
      let calls = self.calls.clone();
      async move {
        calls.fetch_add(1, Ordering::SeqCst);
        auv_tracing::emit_event!(McpCallEvent { phase: "polled" });
        Ok(7)
      }
    }
  }

  #[derive(serde::Serialize)]
  struct McpCallEvent {
    phase: &'static str,
  }

  impl EventPayload for McpCallEvent {
    const NAME: &'static str = "auv.test.mcp_frontend_call";
    const VERSION: u32 = 1;
  }

  #[derive(Clone, Copy)]
  struct FailingProjector;

  impl TelemetryProjector for FailingProjector {
    fn project(&self, _item: TelemetryItem) -> BoxFuture<'_, Result<(), TelemetryError>> {
      Box::pin(async { Err(TelemetryError::new(ErrorCode::parse("auv.test.telemetry_error").unwrap())) })
    }

    fn flush(&self) -> BoxFuture<'_, Result<(), TelemetryError>> {
      Box::pin(async { Ok(()) })
    }
  }

  #[tokio::test]
  async fn mcp_tool_scopes_typed_call_and_does_not_retry_on_telemetry_error() -> anyhow::Result<()> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let configure_store = store.clone();
    let call = CountingCall::default();
    let adapter_call = call.clone();
    let adapter = McpInvokeAdapter::new("scan.frame", move |_input| {
      let future = adapter_call.call();
      async move {
        let value = future.await?;
        Ok(completed(
          "counted fixture call",
          serde_json::json!({
            "value": value,
            "status": "adapter-status",
            "run_id": "adapter-run",
            "artifacts": ["adapter-artifact"]
          }),
        ))
      }
    });
    let command = default_registry().resolve("scan.frame").expect("scan metadata").clone();
    let registry = InvokeRegistry::from_groups(vec![auv_cli_invoke::CommandGroup::new("test", "TEST").command(command)]);
    let server = McpServer::with_invoke_dispatch(
      project_root,
      Arc::new(registry),
      vec![adapter],
      Arc::new(move |_store_root| {
        let dispatch = auv_tracing::configure()
          .run_store(configure_store.clone())
          .project_telemetry(Arc::new(FailingProjector), TelemetryRoutePolicy::fixed_fields_only())
          .build()
          .map_err(|error| error.to_string())?;
        Ok(McpFrontendAuthority { dispatch })
      }),
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(16384);
    let server_handle = tokio::spawn(async move {
      let service = server.serve(server_transport).await?;
      service.waiting().await?;
      anyhow::Ok(())
    });
    let client = DummyClientHandler.serve(client_transport).await?;

    let response = client
      .call_tool(CallToolRequestParam {
        name: "invoke".into(),
        arguments: Some(serde_json::json!({ "command_id": "scan.frame" }).as_object().unwrap().clone()),
      })
      .await?;
    let value: Value =
      serde_json::from_str(&response.content.first().and_then(|content| content.raw.as_text()).expect("invoke response").text)?;
    let run_id = value["run_id"].as_str().expect("run id").parse::<auv_tracing::RunId>()?;

    assert_eq!(value["result"]["value"], 7);
    assert_eq!(value["status"], "completed");
    assert_ne!(value["run_id"], "adapter-run");
    assert_eq!(value["artifacts"], serde_json::json!([]));
    assert_eq!(value["result"]["status"], "adapter-status");
    assert_eq!(call.call_count(), 1);
    assert!(value["tracing_failure"].as_str().is_some());
    let snapshot = store.load_snapshot(run_id).await?.expect("recorded run");
    assert_eq!(snapshot.run_id(), run_id);
    assert_eq!(
      snapshot.events().iter().map(|event| event.schema().name().as_str()).collect::<Vec<_>>(),
      vec![
        "auv.frontend.lifecycle",
        "auv.test.mcp_frontend_call",
        "auv.test.mcp_frontend_call"
      ]
    );
    assert_eq!(serde_json::from_str::<Value>(snapshot.events()[0].payload().get())?, serde_json::json!({ "frontend": "mcp" }));
    assert_eq!(serde_json::from_str::<Value>(snapshot.events()[1].payload().get())?, serde_json::json!({ "phase": "constructed" }));
    assert_eq!(serde_json::from_str::<Value>(snapshot.events()[2].payload().get())?, serde_json::json!({ "phase": "polled" }));

    client.cancel().await?;
    server_handle.await??;
    Ok(())
  }

  #[tokio::test]
  async fn click_window_point_invalid_dry_runs_fail_without_ui_work() -> anyhow::Result<()> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let store_root = temp_dir("mcp-click-window-point-validation");
    let store_root_arg = store_root.display().to_string();
    let (server_transport, client_transport) = tokio::io::duplex(16384);
    let server = McpServer::new(project_root).map_err(anyhow::Error::msg)?;
    let server_handle = tokio::spawn(async move {
      let service = server.serve(server_transport).await?;
      service.waiting().await?;
      anyhow::Ok(())
    });
    let client = DummyClientHandler.serve(client_transport).await?;
    let invalid_cases = [
      ("missing coordinate mode", serde_json::json!({}), "requires --offset_x/--offset_y or --relative_x/--relative_y"),
      (
        "mixed coordinate modes",
        serde_json::json!({
          "offset_x": "10",
          "offset_y": "20",
          "relative_x": "0.5",
          "relative_y": "0.5"
        }),
        "not both",
      ),
      ("incomplete offset pair", serde_json::json!({ "offset_x": "10" }), "requires both --offset_x and --offset_y"),
      ("incomplete relative pair", serde_json::json!({ "relative_y": "0.5" }), "requires both --relative_x and --relative_y"),
      ("relative coordinate below zero", serde_json::json!({ "relative_x": "-0.01", "relative_y": "0.5" }), "0..=1"),
      ("relative coordinate above one", serde_json::json!({ "relative_x": "1.01", "relative_y": "0.5" }), "0..=1"),
    ];

    // ROOT CAUSE:
    //
    // If an MCP window-point invocation was a dry-run, malformed coordinates
    // completed because the adapter returned before shared CLI validation.
    //
    // Before the fix, every case below returned `completed`.
    // The fix validates first, while keeping dry-runs free of UI work.
    for (case, inputs, expected_error) in invalid_cases {
      let response = client
        .call_tool(CallToolRequestParam {
          name: "invoke".into(),
          arguments: Some(
            serde_json::json!({
              "command_id": "input.clickWindowPoint",
              "dry_run": true,
              "inputs": inputs,
              "inspect": { "store_root": store_root_arg }
            })
            .as_object()
            .expect("invoke arguments")
            .clone(),
          ),
        })
        .await?;
      let value: Value =
        serde_json::from_str(&response.content.first().and_then(|content| content.raw.as_text()).expect("invoke response").text)?;
      let run_id = value["run_id"].as_str().expect("run id").parse::<auv_tracing::RunId>()?;
      let store = auv_tracing::FileRunStore::open(&store_root)?;
      let snapshot = store.load_snapshot(run_id).await?.expect("invalid dry-run V1 snapshot");

      assert_ne!(value["status"], "completed", "{case}");
      assert_eq!(value["status"], "failed", "{case}");
      assert!(value["failure_message"].as_str().is_some_and(|message| message.contains(expected_error)), "{case}: {value}");
      assert_eq!(value["artifacts"], serde_json::json!([]), "{case}");
      assert!(snapshot.artifacts().is_empty(), "{case}: invalid dry-run must not execute artifact-producing UI work");
      assert_eq!(snapshot.events().len(), 1, "{case}: invalid dry-run must record only the MCP frontend lifecycle");
    }

    let response = client
      .call_tool(CallToolRequestParam {
        name: "invoke".into(),
        arguments: Some(
          serde_json::json!({
            "command_id": "input.clickWindowPoint",
            "dry_run": true,
            "inputs": { "offset_x": "10", "offset_y": "20" },
            "inspect": { "store_root": store_root_arg }
          })
          .as_object()
          .expect("invoke arguments")
          .clone(),
        ),
      })
      .await?;
    let value: Value =
      serde_json::from_str(&response.content.first().and_then(|content| content.raw.as_text()).expect("invoke response").text)?;
    let run_id = value["run_id"].as_str().expect("run id").parse::<auv_tracing::RunId>()?;
    let store = auv_tracing::FileRunStore::open(&store_root)?;
    let snapshot = store.load_snapshot(run_id).await?.expect("valid dry-run V1 snapshot");

    assert_eq!(value["status"], "completed");
    assert_eq!(value["output_summary"], "dry run: input.clickWindowPoint");
    assert_eq!(snapshot.run_id(), run_id);
    assert!(snapshot.artifacts().is_empty());

    client.cancel().await?;
    server_handle.await??;
    let _ = std::fs::remove_dir_all(store_root);
    Ok(())
  }

  #[tokio::test]
  async fn click_window_point_production_adapter_rejects_non_finite_dry_runs() {
    let adapter = core_invoke_adapters()
      .into_iter()
      .find(|adapter| adapter.command_id == "input.clickWindowPoint")
      .expect("production input.clickWindowPoint adapter");

    for value in ["NaN", "inf", "-inf"] {
      let input = McpInvokeInput {
        target_application_id: None,
        target_label: None,
        inputs: BTreeMap::from([
          ("offset_x".to_string(), value.to_string()),
          ("offset_y".to_string(), "20".to_string()),
        ]),
        dry_run: true,
        cancellation: InvokeCancellation::new(),
      };

      let error = adapter.invoke(input).await.expect_err("non-finite dry-run must fail validation");
      assert!(error.contains("finite"), "{value}: {error}");
    }
  }

  #[tokio::test]
  async fn capture_region_production_adapter_uses_shared_validation_before_dry_run() {
    let adapter = core_invoke_adapters()
      .into_iter()
      .find(|adapter| adapter.command_id == "screen.captureRegion")
      .expect("production screen.captureRegion adapter");
    let invalid = [
      (BTreeMap::new(), "screen.captureRegion requires --x"),
      (BTreeMap::from([("x".to_string(), "1".to_string())]), "screen.captureRegion requires --y"),
      (
        BTreeMap::from([
          ("x".to_string(), "NaN".to_string()),
          ("y".to_string(), "2".to_string()),
          ("width".to_string(), "3".to_string()),
          ("height".to_string(), "4".to_string()),
        ]),
        "screen.captureRegion requires finite --x",
      ),
      (
        BTreeMap::from([
          ("x".to_string(), "1".to_string()),
          ("y".to_string(), "inf".to_string()),
          ("width".to_string(), "3".to_string()),
          ("height".to_string(), "4".to_string()),
        ]),
        "screen.captureRegion requires finite --y",
      ),
      (
        BTreeMap::from([
          ("x".to_string(), "1".to_string()),
          ("y".to_string(), "2".to_string()),
          ("width".to_string(), "0".to_string()),
          ("height".to_string(), "4".to_string()),
        ]),
        "screen.captureRegion requires --width greater than zero",
      ),
      (
        BTreeMap::from([
          ("x".to_string(), "1".to_string()),
          ("y".to_string(), "2".to_string()),
          ("width".to_string(), "-3".to_string()),
          ("height".to_string(), "4".to_string()),
        ]),
        "screen.captureRegion requires --width greater than zero",
      ),
      (
        BTreeMap::from([
          ("x".to_string(), "1".to_string()),
          ("y".to_string(), "2".to_string()),
          ("width".to_string(), "3".to_string()),
          ("height".to_string(), "0".to_string()),
        ]),
        "screen.captureRegion requires --height greater than zero",
      ),
      (
        BTreeMap::from([
          ("x".to_string(), "1".to_string()),
          ("y".to_string(), "2".to_string()),
          ("width".to_string(), "3".to_string()),
          ("height".to_string(), "-4".to_string()),
        ]),
        "screen.captureRegion requires --height greater than zero",
      ),
    ];

    for (inputs, expected) in invalid {
      let error = adapter
        .invoke(McpInvokeInput {
          target_application_id: None,
          target_label: None,
          inputs,
          dry_run: true,
          cancellation: InvokeCancellation::new(),
        })
        .await
        .expect_err("invalid region must fail before the dry-run branch");
      assert_eq!(error, expected);
    }

    let valid = adapter
      .invoke(McpInvokeInput {
        target_application_id: None,
        target_label: None,
        inputs: BTreeMap::from([
          ("x".to_string(), "-1".to_string()),
          ("y".to_string(), "2".to_string()),
          ("width".to_string(), "3".to_string()),
          ("height".to_string(), "4".to_string()),
        ]),
        dry_run: true,
        cancellation: InvokeCancellation::new(),
      })
      .await
      .expect("valid dry-run region");
    assert_eq!(valid.status, McpInvokeStatus::Completed);
  }

  struct ResourceCleanup(Arc<AtomicBool>);

  impl Drop for ResourceCleanup {
    fn drop(&mut self) {
      self.0.store(true, Ordering::SeqCst);
    }
  }

  #[tokio::test]
  async fn mcp_request_cancellation_drops_the_polled_call_before_later_side_effects() -> anyhow::Result<()> {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let configured_store = store.clone();
    let acquired = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let cleaned = Arc::new(AtomicBool::new(false));
    let later_side_effect = Arc::new(AtomicBool::new(false));
    let (run_id_sender, run_id_receiver) = tokio::sync::oneshot::channel();
    let run_id_sender = Arc::new(Mutex::new(Some(run_id_sender)));
    let adapter = McpInvokeAdapter::new("scan.frame", {
      let acquired = acquired.clone();
      let release = release.clone();
      let cleaned = cleaned.clone();
      let later_side_effect = later_side_effect.clone();
      move |_input| {
        let acquired = acquired.clone();
        let release = release.clone();
        let cleanup = ResourceCleanup(cleaned.clone());
        let later_side_effect = later_side_effect.clone();
        let run_id_sender = run_id_sender.clone();
        async move {
          let _cleanup = cleanup;
          let run_id = *auv_tracing::Context::current().run_id().expect("request run context");
          if let Some(sender) = run_id_sender.lock().unwrap().take() {
            let _ = sender.send(run_id);
          }
          acquired.notify_one();
          release.notified().await;
          later_side_effect.store(true, Ordering::SeqCst);
          Ok(completed("released", serde_json::json!({})))
        }
      }
    });
    let command = default_registry().resolve("scan.frame").expect("scan metadata").clone();
    let registry = InvokeRegistry::from_groups(vec![auv_cli_invoke::CommandGroup::new("test", "TEST").command(command)]);
    let server = McpServer::with_invoke_dispatch(
      PathBuf::from(env!("CARGO_MANIFEST_DIR")),
      Arc::new(registry),
      vec![adapter],
      Arc::new(move |_store_root| {
        let dispatch = auv_tracing::configure().run_store(configured_store.clone()).build().map_err(|error| error.to_string())?;
        Ok(McpFrontendAuthority { dispatch })
      }),
    )
    .map_err(anyhow::Error::msg)?;
    let (server_transport, client_transport) = tokio::io::duplex(16384);
    let server_handle = tokio::spawn(async move {
      let service = server.serve(server_transport).await?;
      service.waiting().await?;
      anyhow::Ok(())
    });
    let client = DummyClientHandler.serve(client_transport).await?;
    let request = client
      .send_cancellable_request(
        ClientRequest::CallToolRequest(Request::new(CallToolRequestParam {
          name: "invoke".into(),
          arguments: Some(serde_json::json!({ "command_id": "scan.frame" }).as_object().unwrap().clone()),
        })),
        PeerRequestOptions::no_options(),
      )
      .await?;
    acquired.notified().await;
    let run_id = run_id_receiver.await?;

    request.cancel(Some("test cancellation".to_string())).await?;
    tokio::time::timeout(std::time::Duration::from_secs(1), async {
      while !cleaned.load(Ordering::SeqCst) {
        tokio::task::yield_now().await;
      }
    })
    .await?;
    let cleaned_before_release = cleaned.load(Ordering::SeqCst);
    release.notify_waiters();
    tokio::task::yield_now().await;

    assert!(cleaned_before_release, "request cancellation must drop acquired resources without manually releasing the call");
    assert!(!later_side_effect.load(Ordering::SeqCst), "no later side effect may run after cancellation");
    let snapshot = tokio::time::timeout(std::time::Duration::from_secs(1), async {
      loop {
        if let Some(snapshot) = store.load_snapshot(run_id).await.expect("snapshot read")
          && snapshot.events().iter().any(|event| event.schema().name().as_str() == "auv.frontend.cancelled")
        {
          break snapshot;
        }
        tokio::task::yield_now().await;
      }
    })
    .await?;
    assert_eq!(snapshot.run_id(), run_id);

    client.cancel().await?;
    server_handle.await??;
    Ok(())
  }

  #[tokio::test]
  async fn mcp_server_lists_catalog_and_maps_direct_invoke_values() -> anyhow::Result<()> {
    let project_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let store_root = temp_dir("mcp-shared-runtime-store");
    let (server_transport, client_transport) = tokio::io::duplex(16384);

    let server = McpServer::new(project_root.clone()).map_err(anyhow::Error::msg)?;
    let server_handle = tokio::spawn(async move {
      let service = server.serve(server_transport).await?;
      service.waiting().await?;
      anyhow::Ok(())
    });

    let client = DummyClientHandler.serve(client_transport).await?;

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
    assert!(invoke_description.contains("typed adapter"));
    let command_id_schema = invoke_tool
      .input_schema
      .get("properties")
      .and_then(|properties| properties.get("command_id"))
      .expect("invoke schema should describe command_id");
    let command_ids =
      command_id_schema.get("enum").and_then(|value| value.as_array()).expect("command_id schema should enumerate registry command ids");
    assert!(!command_ids.iter().any(|id| id == "fixture.observe"));
    assert!(command_ids.iter().any(|id| id == "scan.coverage"));
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
            "command_id": "scan.coverage",
            "dry_run": true,
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
    assert_eq!(invoke.is_error, Some(false));
    assert_eq!(invoke.structured_content.as_ref(), Some(&invoke_json));
    let run_id = invoke_json.get("run_id").and_then(|value| value.as_str()).expect("run_id should exist").to_string();
    assert_eq!(invoke_json.get("output_summary").and_then(|value| value.as_str()), Some("scan.coverage dry-run"));
    assert_eq!(invoke_json.get("status").and_then(|value| value.as_str()), Some("completed"));
    assert_eq!(invoke_json.get("signals"), Some(&Value::Object(Default::default())));
    assert_eq!(invoke_json.get("artifacts").and_then(|value| value.as_array()).map(Vec::len), Some(0));
    assert!(invoke_json.get("tracing_failure").is_some_and(Value::is_null));

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
    assert_eq!(failed_invoke.is_error, Some(true));
    assert_eq!(failed_invoke.structured_content.as_ref(), Some(&failed_invoke_json));
    let failed_run_id = failed_invoke_json.get("run_id").and_then(|value| value.as_str()).expect("failed run_id should exist");
    assert_ne!(failed_run_id, run_id);
    assert_eq!(failed_invoke_json.get("status").and_then(|value| value.as_str()), Some("failed"));
    assert!(
      failed_invoke_json
        .get("failure_message")
        .and_then(|value| value.as_str())
        .is_some_and(|message| message.contains("typed app activation API"))
    );

    assert!(failed_invoke_json.get("tracing_failure").is_some_and(Value::is_null));

    client.cancel().await?;
    server_handle.await??;
    let _ = std::fs::remove_dir_all(store_root);
    Ok(())
  }

  #[test]
  fn every_core_catalog_command_has_an_mcp_owned_adapter() {
    let registry = default_registry();
    let adapters = core_invoke_adapters();

    assert_eq!(adapters.len(), registry.all().len());
    for command in registry.all() {
      assert!(adapters.iter().any(|adapter| adapter.command_id == command.id), "missing MCP adapter for {}", command.id);
    }
  }

  #[test]
  fn mcp_server_rejects_duplicate_adapter_ids() {
    let registry = Arc::new(default_registry());
    let mut adapters = core_invoke_adapters();
    adapters.push(adapters[0].clone());

    let result = McpServer::with_registry(PathBuf::from(env!("CARGO_MANIFEST_DIR")), registry, adapters);

    assert!(result.is_err());
    assert!(result.err().is_some_and(|error| error.contains("duplicate")));
  }

  #[test]
  fn mcp_server_rejects_missing_adapter_ids() {
    let registry = Arc::new(default_registry());
    let mut adapters = core_invoke_adapters();
    let missing = adapters.pop().expect("core adapter").command_id;

    let result = McpServer::with_registry(PathBuf::from(env!("CARGO_MANIFEST_DIR")), registry, adapters);

    assert!(result.is_err());
    assert!(result.err().is_some_and(|error| error.contains("missing") && error.contains(missing)));
  }

  #[test]
  fn mcp_server_rejects_extra_adapter_ids() {
    let registry = Arc::new(default_registry());
    let mut adapters = core_invoke_adapters();
    adapters.push(McpInvokeAdapter::new("test.hidden", |_input| async move { Ok(completed("hidden", serde_json::json!({}))) }));

    let result = McpServer::with_registry(PathBuf::from(env!("CARGO_MANIFEST_DIR")), registry, adapters);

    assert!(result.is_err());
    assert!(result.err().is_some_and(|error| error.contains("extra") && error.contains("test.hidden")));
  }

  fn temp_dir(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!("auv-{}-{}", label, now_millis()))
  }
}
