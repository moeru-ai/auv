use clap::Args;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use auv_cli_invoke::{ExecutionTarget, InvokeCliParse, InvokeRequest};

/// Invoke one core computer-use capability and record its run.
#[derive(Clone, Debug, Args)]
#[command(disable_help_flag = true)]
pub struct InvokeArgs {
  /// Command id and command-specific arguments, parsed by the invoke registry.
  // TODO(invoke-os-string-argv): non-UTF-8 invoke paths are deferred because
  // the current recorded InvokeRequest protocol stores inputs as String; reopen
  // when that owner-approved wire/storage contract can preserve OS path bytes.
  #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
  pub arguments: Vec<String>,
}

// TODO(invoke-mcp-ownership): `auv-cli-invoke` remains the accepted invoke
// owner for this migration; reopen its interface boundary when `auv-cli-mcp`
// is designed, per `2026-08-04-core-cli-command-ownership-design.md`.
pub async fn run(args: InvokeArgs, selection: &auv::selection::RootSelection, project_root: &Path) -> Result<i32, String> {
  let mut arguments = vec!["invoke".to_string()];
  arguments.extend(args.arguments);
  let parsed = auv_cli_invoke::parse_invoke_args(&arguments)?;
  match parsed {
    InvokeCliParse::Help { command_id } => {
      let registry = auv_cli_invoke::default_registry();
      if let Some(command_id) = command_id {
        let command = registry
          .resolve(&command_id)
          .ok_or_else(|| format!("unknown command {command_id}; use `auv invoke --help` to inspect available entries"))?;
        print!("{}", auv_cli_invoke::render_command_help(command));
      } else {
        print!("{}", auv_cli_invoke::render_help_index(&registry));
      }
      Ok(0)
    }
    InvokeCliParse::Invoke {
      command_id,
      target_application_id,
      inputs,
      typed_args,
      store_root,
      dry_run,
      output,
    } => {
      execute(
        InvokeRequest {
          command_id,
          target: ExecutionTarget {
            application_id: target_application_id,
          },
          inputs,
          dry_run,
        },
        typed_args,
        store_root,
        output,
        selection,
        project_root,
      )
      .await
    }
  }
}

async fn execute(
  request: InvokeRequest,
  typed_args: auv_cli_invoke::TypedInvokeArgs,
  store_root: Option<PathBuf>,
  output: auv_cli_invoke::InvokeOutputOptions,
  selection: &auv::selection::RootSelection,
  project_root: &Path,
) -> Result<i32, String> {
  let authority = build_tracing(project_root, store_root.as_ref())?;
  let registry = auv_cli_invoke::default_registry();
  let command = registry.resolve(&request.command_id).cloned().ok_or_else(|| format!("unknown invoke command: {}", request.command_id))?;
  // TODO(selected-invoke-dry-run): validate Device/Run selection without
  // creating a Run once the control plane has a side-effect-free resolve
  // operation. The current dry-run remains local to preserve its no-I/O contract.
  let selected_context = if !selection.is_empty() && !request.dry_run {
    Some(crate::commands::plugin::resolve_invoke_context(selection).await?)
  } else {
    None
  };
  let remote_context = selected_context.as_ref().map(|resolved| resolved.context.clone());
  let input = auv_cli_invoke::InvokeCommandInput {
    command_id: request.command_id.clone(),
    target_application_id: request.target.application_id,
    inputs: request.inputs,
    typed_args: Some(typed_args),
    dry_run: request.dry_run,
    cancellation: auv_cli_invoke::InvokeCancellation::new(),
  };
  let invoked_command = command.clone();
  let run_id = tracing_run_id(selected_context.as_ref())?;
  let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
  let future = root.in_scope(|| async move {
    auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
    match remote_context {
      // The typed remote adapter contains the complete invoke command match.
      // Keep that large async state machine off the Windows main-thread stack.
      Some(context) => Box::pin(auv_cli_invoke::runner::invoke(input, context)).await,
      None => invoked_command.invoke(input).await,
    }
  });
  let mut direct_result = root.instrument(future).await;
  if let Some(context) = selected_context
    && let Err(error) = context.finish(direct_result.is_ok()).await
  {
    if direct_result.is_ok() {
      direct_result = Err(error);
    } else {
      eprintln!("warning: failed to finalize the selected invoke Run: {error}");
    }
  }
  if let Some(failure) = authority.dispatch.flush().await.err().map(|error| error.to_string()) {
    eprintln!("warning: invoke recording failure for run {run_id}: {failure}");
  }
  let artifact_paths = direct_result
    .as_ref()
    .ok()
    .into_iter()
    .flat_map(auv_cli_invoke::InvokeCommandOutput::artifacts)
    .map(|metadata| (metadata.uri().clone(), authority.store.artifact_path(metadata)))
    .collect::<Vec<_>>();
  let result = auv_cli_invoke::InvokeResult::from_command_result(run_id, &command, direct_result).with_artifact_paths(artifact_paths);
  Ok(auv_cli_invoke::render_invoke_result(&result, output)?.exit_code)
}

#[derive(Clone)]
struct CliTracing {
  dispatch: auv_tracing::Dispatch,
  store: Arc<auv_tracing::FileTracingStore>,
}

fn build_tracing(project_root: &Path, explicit: Option<&PathBuf>) -> Result<CliTracing, String> {
  let root = explicit.cloned().unwrap_or_else(|| project_root.join(".auv").join("store"));
  let store = Arc::new(
    auv_tracing::FileTracingStore::open(&root).map_err(|error| format!("failed to open tracing store {}: {error}", root.display()))?,
  );
  let dispatch =
    auv_tracing::configure().tracing_store(store.clone()).build().map_err(|error| format!("failed to configure invoke tracing: {error}"))?;
  Ok(CliTracing { dispatch, store })
}

#[derive(serde::Serialize)]
struct InvokeFrontendLifecycle {
  frontend: &'static str,
}
impl auv_tracing::EventPayload for InvokeFrontendLifecycle {
  const NAME: &'static str = "auv.frontend.lifecycle";
  const VERSION: u32 = 1;
}

fn tracing_run_id(selected: Option<&crate::commands::plugin::ResolvedExecutionContext>) -> Result<auv_tracing::RunId, String> {
  let Some(control_run_id) = selected.and_then(|selected| selected.context.run_id.as_deref()) else {
    return Ok(auv_tracing::RunId::new());
  };
  // TODO(resource-id-migration): Remove the legacy `run_` branch after old
  // control-plane stores no longer expose prefixed UUID Run identities.
  let value = control_run_id.strip_prefix("run_").unwrap_or(control_run_id);
  let uuid = uuid::Uuid::parse_str(value)
    .map_err(|error| format!("selected Run ID {control_run_id:?} cannot be projected into the tracing Run identity: {error}"))?;
  uuid
    .to_string()
    .parse()
    .map_err(|error| format!("selected Run ID {control_run_id:?} cannot be projected into the tracing Run identity: {error}"))
}
