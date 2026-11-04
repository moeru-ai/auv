use clap::Args;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use auv_cli_invoke::{InvokeCliParse, InvokeRequest, command::TargetPolicy};

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
pub async fn run(args: InvokeArgs, selection: &auv::selection::RootSelection, no_daemon: bool, project_root: &Path) -> Result<i32, String> {
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
      target,
      inputs,
      typed_args,
      store_root,
      dry_run,
      output,
    } => {
      execute(
        InvokeRequest {
          command_id,
          target,
          inputs,
          dry_run,
        },
        typed_args,
        store_root,
        output,
        selection,
        no_daemon,
        project_root,
      )
      .await
    }
  }
}

/// Where one invoke call runs. Every frontend (CLI, MCP) uses
/// [`invoke_route`], so they cannot drift apart.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InvokeRoute {
  /// In this process, on the direct local driver path.
  Direct,
  /// Through the daemon to the Runner of the selected Device or Run.
  Selected,
  /// Through the current user's local daemon (`AUV_ENDPOINT`, the registered
  /// daemon, or one started for this call) to its local Runner.
  LocalRunner,
}

/// Resolution order of the design: a root selection keeps its route; then
/// `--no-daemon`, a command without a Runner route, and a dry run that cannot
/// resolve a selection yet stay direct; everything else uses the local Runner.
/// `AUV_ENDPOINT` only changes which daemon the local Runner route uses, see
/// [`crate::local_daemon::connect_or_start`].
pub(crate) fn invoke_route(
  selection: &auv::selection::RootSelection,
  no_daemon: bool,
  command: &auv_cli_invoke::InvokeCommand,
  dry_run: bool,
) -> InvokeRoute {
  // TODO(selected-invoke-dry-run): other legacy dry-runs remain local until
  // the control plane can resolve selection without creating a Run. Keyboard
  // validation depends on the selected driver, even for explicit global input.
  // Use the command's shared keyboard contract so new commands cannot fall
  // through a second, frontend-only command-ID list.
  let dry_run_stays_direct = dry_run && command.target != TargetPolicy::OptionalKeyboard;
  if !selection.is_empty() {
    return if dry_run_stays_direct {
      InvokeRoute::Direct
    } else {
      InvokeRoute::Selected
    };
  }
  if no_daemon || dry_run_stays_direct || command.runner == auv_cli_invoke::RunnerSupport::NoRoute {
    InvokeRoute::Direct
  } else {
    InvokeRoute::LocalRunner
  }
}

/// Resolves the daemon-side context for a routed call. The local Runner route
/// is an empty selection on the local daemon: `Client::from_context` resolves
/// its endpoint (`AUV_ENDPOINT`, then discovery) and the implicit Run takes
/// the daemon's local Device. Returns the trace store root of a daemon this
/// route started or discovered.
pub(crate) async fn resolve_route(
  route: InvokeRoute,
  selection: &auv::selection::RootSelection,
) -> Result<Option<(crate::commands::plugin::ResolvedExecutionContext, Option<PathBuf>)>, auv_cli_invoke::InvokeFailure> {
  match route {
    InvokeRoute::Direct => Ok(None),
    InvokeRoute::Selected => Ok(Some((crate::commands::plugin::resolve_invoke_context(selection).await?, None))),
    InvokeRoute::LocalRunner => {
      let daemon = crate::local_daemon::connect_or_start().await?;
      let local = auv::selection::RootSelection {
        credential: selection.credential.clone(),
        ..Default::default()
      };
      let context = crate::commands::plugin::resolve_invoke_context(&local).await?;
      Ok(Some((context, Some(daemon.store_root))))
    }
  }
}

async fn execute(
  request: InvokeRequest,
  typed_args: auv_cli_invoke::TypedInvokeArgs,
  store_root: Option<PathBuf>,
  output: auv_cli_invoke::InvokeOutputOptions,
  selection: &auv::selection::RootSelection,
  no_daemon: bool,
  project_root: &Path,
) -> Result<i32, String> {
  let registry = auv_cli_invoke::default_registry();
  let command = registry.resolve(&request.command_id).cloned().ok_or_else(|| format!("unknown invoke command: {}", request.command_id))?;
  let route = invoke_route(selection, no_daemon, &command, request.dry_run);
  // `--store-root` wins; the direct and selected routes keep the
  // working-directory store and open it before resolving, as before; the
  // local Runner route records beside its daemon once that is known.
  let early_root = match (&store_root, route) {
    (Some(root), _) => Some(root.clone()),
    (None, InvokeRoute::LocalRunner) => None,
    (None, _) => Some(project_root.join(".auv").join("store")),
  };
  let early = early_root.map(|root| build_tracing(&root).map(|authority| (root, authority))).transpose()?;
  let (selected_context, daemon_store_root) = match resolve_route(route, selection).await {
    Ok(Some((context, daemon_store_root))) => (Some(context), daemon_store_root),
    Ok(None) => (None, None),
    Err(failure) => {
      // Preserve the legacy human stderr boundary. JSON requests use the
      // normal invoke renderer so failure_details.code survives selection.
      if !output.json {
        return Err(failure.message);
      }
      let result = auv_cli_invoke::InvokeResult::from_command_result(auv_tracing::RunId::new(), &command, Err(failure));
      return Ok(auv_cli_invoke::render_invoke_result(&result, output)?.exit_code);
    }
  };
  let (trace_root, authority) = match early {
    Some(early) => early,
    None => {
      let root = daemon_store_root.expect("the local Runner route resolves its daemon's store");
      let authority = build_tracing(&root)?;
      (root, authority)
    }
  };
  let remote_daemon = selected_context.as_ref().map(|resolved| auv_cli_invoke::runner::SelectedDaemon {
    context: resolved.context.clone(),
    credential: resolved.credential.clone(),
  });
  let input = auv_cli_invoke::InvokeCommandInput {
    command_id: request.command_id.clone(),
    target: request.target,
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
    match remote_daemon {
      // The typed remote adapter contains the complete invoke command match.
      // Keep that large async state machine off the Windows main-thread stack.
      Some(daemon) => Box::pin(auv_cli_invoke::runner::invoke(input, daemon)).await,
      None => invoked_command.invoke(input).await,
    }
  });
  let mut direct_result = root.instrument(future).await;
  if let Some(context) = selected_context
    && let Err(error) = context.finish(direct_result.is_ok()).await
  {
    if direct_result.is_ok() {
      direct_result = Err(error.into());
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
  let result = auv_cli_invoke::InvokeResult::from_command_result(run_id, &command, direct_result)
    .with_artifact_paths(artifact_paths)
    .with_store_root(trace_root);
  Ok(auv_cli_invoke::render_invoke_result(&result, output)?.exit_code)
}

#[derive(Clone)]
struct CliTracing {
  dispatch: auv_tracing::Dispatch,
  store: Arc<auv_tracing::FileTracingStore>,
}

fn build_tracing(root: &Path) -> Result<CliTracing, String> {
  let store = Arc::new(
    auv_tracing::FileTracingStore::open(root).map_err(|error| format!("failed to open tracing store {}: {error}", root.display()))?,
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

pub(crate) fn tracing_run_id(selected: Option<&crate::commands::plugin::ResolvedExecutionContext>) -> Result<auv_tracing::RunId, String> {
  let Some(control_run_id) = selected.and_then(|selected| selected.context.run_id.as_deref()) else {
    return Ok(auv_tracing::RunId::new());
  };
  auv_tracing::RunId::from_daemon_run_id(control_run_id)
    .map_err(|error| format!("selected Run ID {control_run_id:?} cannot be projected into the tracing Run identity: {error}"))
}

#[cfg(test)]
mod tests {
  use super::*;

  fn route(selection: &auv::selection::RootSelection, no_daemon: bool, command_id: &str, dry_run: bool) -> InvokeRoute {
    let registry = auv_cli_invoke::default_registry();
    invoke_route(selection, no_daemon, registry.resolve(command_id).expect("core command"), dry_run)
  }

  #[test]
  fn invoke_without_a_selection_uses_the_local_runner_unless_it_cannot() {
    let none = auv::selection::RootSelection::default();

    assert_eq!(route(&none, false, "display.list", false), InvokeRoute::LocalRunner);
    assert_eq!(route(&none, true, "display.list", false), InvokeRoute::Direct, "--no-daemon");
    assert_eq!(route(&none, false, "scan.coverage", false), InvokeRoute::Direct, "no Runner route");
    // TODO(selected-invoke-dry-run) keeps device-free dry runs in this
    // process; keyboard dry runs validate against the Runner's driver.
    assert_eq!(route(&none, false, "display.list", true), InvokeRoute::Direct);
    assert_eq!(route(&none, false, "input.pressKeys", true), InvokeRoute::LocalRunner);
  }

  #[test]
  fn a_root_selection_keeps_the_selected_route() {
    let device = auv::selection::RootSelection {
      device_name: Some("desk".into()),
      ..Default::default()
    };

    assert_eq!(route(&device, false, "display.list", false), InvokeRoute::Selected);
    assert_eq!(route(&device, false, "input.pressKeys", true), InvokeRoute::Selected);
    assert_eq!(route(&device, false, "display.list", true), InvokeRoute::Direct);
  }
}
