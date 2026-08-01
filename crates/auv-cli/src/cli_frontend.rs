// Shared frontend for the root `auv` binary.

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::Arc;

use auv_cli_common::{
  TableRow,
  outputs::formats::table::{self, TableOptions},
};

use crate::cli::{CliCommand, DeviceTrustAction, TracingOptions, parse_cli_os, version_text};
use crate::commands::devices::{DeviceProfilesCommand, ProfileWriteArgs};
use crate::commands::pairing::PairingCommand;

pub async fn run_root() -> Result<i32, String> {
  let command = parse_cli_os(env::args_os().skip(1))?;
  dispatch(command).await
}

pub fn exit_status(result: Result<i32, String>) -> i32 {
  match result {
    Ok(exit_code) => exit_code,
    Err(error) => {
      eprintln!("error: {error}");
      1
    }
  }
}

pub(crate) async fn dispatch(command: CliCommand) -> Result<i32, String> {
  if matches!(&command, CliCommand::Version) {
    print!("{}", version_text());
    return Ok(0);
  }

  let project_root = env::current_dir().map_err(|error| format!("failed to resolve current directory: {error}"))?;
  if let CliCommand::XtaskGenerateSwiftBridge = &command {
    let outputs = crate::xtask::generate_swift_bridge_for_ide(&project_root)?;
    println!("generated Swift bridge files for IDE indexing");
    for output in outputs {
      println!("output: {output}");
    }
    return Ok(0);
  }

  if let CliCommand::McpServe = &command {
    crate::mcp::serve_stdio(project_root.clone()).await?;
    return Ok(0);
  }

  if let CliCommand::PermissionCheck { json } = &command {
    run_permission_check(*json)?;
    return Ok(0);
  }

  if let CliCommand::Pairing { store, command } = &command {
    run_pairing(&project_root, store.as_deref(), command)?;
    return Ok(0);
  }

  if let CliCommand::DeviceTrust {
    store,
    device,
    action,
  } = &command
  {
    run_device_trust(&project_root, store.as_deref(), device, *action)?;
    return Ok(0);
  }

  if let CliCommand::DeviceProfiles { command } = &command {
    run_device_profiles(command)?;
    return Ok(0);
  }

  if let CliCommand::Serve {
    listeners,
    tls_certificate,
    tls_private_key,
    client_ca_certificate,
    pairing_store,
    store_root,
    discovery_file,
    no_discovery,
    daemon_idle_timeout,
    runner_providers,
  } = &command
  {
    let listeners = if listeners.is_empty() {
      vec![default_local_listener(discovery_file.as_deref())?]
    } else {
      listeners.clone()
    };
    let remote_tls = match (tls_certificate, tls_private_key, client_ca_certificate, pairing_store) {
      (Some(certificate), Some(private_key), Some(client_ca), Some(pairing_store)) => Some(RemoteTlsPaths {
        server_certificate: resolve_path(&project_root, certificate),
        server_private_key: resolve_path(&project_root, private_key),
        client_ca_certificate: resolve_path(&project_root, client_ca),
        pairing_store: resolve_path(&project_root, pairing_store),
      }),
      (None, None, None, None) => None,
      _ => unreachable!("partial remote TLS options are rejected by the CLI parser"),
    };
    let listens = listeners.iter().map(|listener| parse_listener(listener, remote_tls.as_ref())).collect::<Result<Vec<_>, _>>()?;
    return serve_foreground(
      listens,
      resolve_store_root(&project_root, store_root.as_ref()),
      discovery_file.clone(),
      *no_discovery,
      *daemon_idle_timeout,
      runner_providers,
    )
    .await;
  }

  if let CliCommand::ApiServerServe {
    host,
    port,
    remote_listen,
    tls_certificate,
    tls_private_key,
    client_ca_certificate,
    pairing_store,
    #[cfg(unix)]
    unix_socket,
    store_root,
    discovery_file,
    no_discovery,
    daemon_idle_timeout,
    runner_providers,
  } = &command
  {
    let store_root = resolve_store_root(&project_root, store_root.as_ref());
    let listen = if let Some(remote_listen) = remote_listen {
      auv_api_server::transport::ListenEndpoint::RemoteTls {
        host: remote_listen.clone(),
        port: *port,
        server_certificate: resolve_path(&project_root, tls_certificate.as_ref().expect("remote TLS certificate was validated")),
        server_private_key: resolve_path(&project_root, tls_private_key.as_ref().expect("remote TLS key was validated")),
        client_ca_certificate: resolve_path(&project_root, client_ca_certificate.as_ref().expect("remote client CA was validated")),
        pairing_store: resolve_path(&project_root, pairing_store.as_ref().expect("remote pairing store was validated")),
      }
    } else {
      #[cfg(unix)]
      if let Some(path) = unix_socket {
        auv_api_server::transport::ListenEndpoint::Unix {
          path: resolve_path(&project_root, path),
        }
      } else {
        auv_api_server::transport::ListenEndpoint::Tcp {
          host: host.clone(),
          port: *port,
        }
      }
      #[cfg(not(unix))]
      auv_api_server::transport::ListenEndpoint::Tcp {
        host: host.clone(),
        port: *port,
      }
    };
    return serve_foreground(vec![listen], store_root, discovery_file.clone(), *no_discovery, *daemon_idle_timeout, runner_providers).await;
  }

  if let CliCommand::DeviceList {
    endpoint,
    json,
    parent_context,
  } = &command
  {
    return run_device_list(endpoint.as_deref(), parent_context.device_id.as_deref(), parent_context.device_name.as_deref(), *json).await;
  }
  if let CliCommand::DeviceGet {
    endpoint,
    device_id,
    json,
    parent_context,
  } = &command
  {
    let context = crate::plugin::resolve_builtin_context(parent_context, endpoint.as_deref()).await?;
    if context.device_id.as_deref().is_some_and(|selected| selected != device_id) {
      return Err(format!("Device argument {device_id:?} conflicts with root Device selection"));
    }
    return run_device_get(endpoint.as_deref(), device_id, *json).await;
  }
  if let CliCommand::RunnerCreate {
    endpoint,
    runner_class,
    lifecycle,
    json,
    parent_context,
  } = &command
  {
    let context = crate::plugin::resolve_builtin_context(parent_context, endpoint.as_deref()).await?;
    return run_runner_create(endpoint.as_deref(), context.device_id.as_deref(), runner_class, *lifecycle, *json).await;
  }
  if let CliCommand::RunnerList {
    endpoint,
    json,
    parent_context,
  } = &command
  {
    let context = crate::plugin::resolve_builtin_context(parent_context, endpoint.as_deref()).await?;
    return run_runner_list(endpoint.as_deref(), context.device_id.as_deref(), *json).await;
  }
  if let CliCommand::RunnerClassList {
    endpoint,
    json,
    parent_context,
  } = &command
  {
    let context = crate::plugin::resolve_builtin_context(parent_context, endpoint.as_deref()).await?;
    return run_runner_class_list(endpoint.as_deref(), context.device_id.as_deref(), *json).await;
  }
  if let CliCommand::RunnerGet {
    endpoint,
    runner_id,
    json,
    parent_context,
  } = &command
  {
    let context = crate::plugin::resolve_builtin_context(parent_context, endpoint.as_deref()).await?;
    return run_runner_get(endpoint.as_deref(), context.device_id.as_deref(), runner_id, *json).await;
  }
  if let CliCommand::RunnerStop {
    endpoint,
    runner_id,
    json,
    parent_context,
  } = &command
  {
    let context = crate::plugin::resolve_builtin_context(parent_context, endpoint.as_deref()).await?;
    return run_runner_stop(endpoint.as_deref(), context.device_id.as_deref(), runner_id, *json).await;
  }
  if let CliCommand::RunCreate {
    endpoint,
    device_ids,
    json,
    parent_context,
  } = &command
  {
    if parent_context.run_id.is_some() {
      return Err("root --run cannot be combined with `auv run create`".to_string());
    }
    let context = crate::plugin::resolve_builtin_context(parent_context, endpoint.as_deref()).await?;
    let inherited_device_ids = if device_ids.is_empty() {
      context.device_id.as_ref().map_or_else(Vec::new, |device_id| vec![device_id.clone()])
    } else {
      device_ids.clone()
    };
    return run_create(endpoint.as_deref(), &inherited_device_ids, *json).await;
  }
  if let CliCommand::RunList {
    endpoint,
    json,
    parent_context,
  } = &command
  {
    let context = crate::plugin::resolve_builtin_context(parent_context, endpoint.as_deref()).await?;
    return run_list(endpoint.as_deref(), context.device_id.as_deref(), context.run_id.as_deref(), *json).await;
  }
  if let CliCommand::RunGet {
    endpoint,
    run_id,
    json,
    parent_context,
  } = &command
  {
    let context = crate::plugin::resolve_builtin_context(parent_context, endpoint.as_deref()).await?;
    validate_run_argument(run_id, &context)?;
    return run_get(endpoint.as_deref(), context.device_id.as_deref(), run_id, *json).await;
  }
  if let CliCommand::RunStop {
    endpoint,
    run_id,
    outcome,
    json,
    parent_context,
  } = &command
  {
    let context = crate::plugin::resolve_builtin_context(parent_context, endpoint.as_deref()).await?;
    validate_run_argument(run_id, &context)?;
    return run_stop(endpoint.as_deref(), context.device_id.as_deref(), run_id, *outcome, *json).await;
  }

  let mut exit_code = 0;
  match command {
    CliCommand::Help(help) => {
      print!("{help}");
    }
    CliCommand::Version => unreachable!("version is handled before runtime setup"),
    CliCommand::PermissionCheck { .. } => {
      unreachable!("permission check is handled before runtime setup")
    }
    CliCommand::XtaskGenerateSwiftBridge => unreachable!("xtask is handled before runtime setup"),
    CliCommand::InvokeHelp { command_id } => {
      let registry = auv_cli_invoke::default_registry();
      if let Some(command_id) = command_id {
        let command = registry
          .resolve(&command_id)
          .ok_or_else(|| format!("unknown command {command_id}; use `auv invoke --help` to inspect available entries"))?;
        print!("{}", auv_cli_invoke::render_command_help(command));
      } else {
        print!("{}", auv_cli_invoke::render_help_index(&registry));
      }
    }
    CliCommand::Invoke {
      request,
      typed_args,
      tracing,
      output,
      parent_context,
    } => {
      let authority = build_cli_tracing(&project_root, &tracing)?;
      let registry = auv_cli_invoke::default_registry();
      let command =
        registry.resolve(&request.command_id).cloned().ok_or_else(|| format!("unknown invoke command: {}", request.command_id))?;
      // TODO(selected-invoke-dry-run): validate Device/Run selection without
      // creating a Run once the control plane has a side-effect-free resolve
      // operation. The current dry-run remains local to preserve its no-I/O
      // contract.
      let selected_context = if parent_context != crate::cli::ParentContextOptions::default() && !request.dry_run {
        Some(crate::plugin::resolve_invoke_context(&parent_context).await?)
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
      let run_id = tracing_run_id_for_selected_context(selected_context.as_ref())?;
      let root = auv_tracing::dispatcher::with_default(&authority.dispatch, || auv_tracing::Context::root(run_id));
      let future = root.in_scope(|| async move {
        auv_tracing::emit_event!(InvokeFrontendLifecycle { frontend: "cli" });
        match remote_context {
          Some(context) => invoke_on_selected_runner(input, context).await,
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
      if let Some(failure) = flush_cli_recording(&authority.dispatch).await {
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
      let outcome = auv_cli_invoke::render_invoke_result(&result, output)?;
      exit_code = outcome.exit_code;
    }
    CliCommand::McpServe => {
      unreachable!("mcp serve is handled before runtime setup")
    }
    CliCommand::ApiServerServe { .. } => {
      unreachable!("api-server serve is handled before runtime setup")
    }
    CliCommand::Serve { .. } => unreachable!("serve is handled before runtime setup"),
    CliCommand::DeviceList { .. } | CliCommand::DeviceGet { .. } | CliCommand::DeviceProfiles { .. } | CliCommand::DeviceTrust { .. } => {
      unreachable!("Device commands are handled before runtime setup")
    }
    CliCommand::RunnerCreate { .. }
    | CliCommand::RunnerList { .. }
    | CliCommand::RunnerClassList { .. }
    | CliCommand::RunnerGet { .. }
    | CliCommand::RunnerStop { .. } => unreachable!("Runner commands are handled before runtime setup"),
    CliCommand::RunCreate { .. } | CliCommand::RunList { .. } | CliCommand::RunGet { .. } | CliCommand::RunStop { .. } => {
      unreachable!("Run commands are handled before runtime setup")
    }
    CliCommand::Pairing { .. } => {
      unreachable!("pairing is handled before runtime setup")
    }
    CliCommand::PluginList => {
      exit_code = crate::plugin::list()?;
    }
    CliCommand::External {
      command_name,
      arguments,
      parent_context,
    } => {
      exit_code = crate::plugin::execute(&command_name, &arguments, &parent_context).await?;
    }
  }

  Ok(exit_code)
}

fn default_local_listener(discovery_file: Option<&Path>) -> Result<String, String> {
  #[cfg(unix)]
  {
    let descriptor = match discovery_file {
      Some(path) => path.to_path_buf(),
      None => crate::daemon_discovery::default_path().map_err(|error| error.to_string())?,
    };
    let parent = descriptor.parent().ok_or_else(|| format!("daemon descriptor path has no parent: {}", descriptor.display()))?;
    let socket = parent.join("auv.sock");
    Ok(format!("unix://{}", socket.display()))
  }
  #[cfg(not(unix))]
  Ok(format!("http://{}:{}", auv_api_server::transport::DEFAULT_API_HOST, auv_api_server::transport::DEFAULT_API_PORT))
}

struct RemoteTlsPaths {
  server_certificate: PathBuf,
  server_private_key: PathBuf,
  client_ca_certificate: PathBuf,
  pairing_store: PathBuf,
}

fn parse_listener(listener: &str, remote_tls: Option<&RemoteTlsPaths>) -> Result<auv_api_server::transport::ListenEndpoint, String> {
  if let Some(authority) = listener.strip_prefix("https://") {
    let address = authority
      .parse::<std::net::SocketAddr>()
      .map_err(|error| format!("invalid paired --listen URI {listener:?}; expected https://IP:PORT: {error}"))?;
    let remote_tls = remote_tls.ok_or_else(|| format!("paired --listen URI {listener:?} omitted TLS/pairing options"))?;
    return Ok(auv_api_server::transport::ListenEndpoint::RemoteTls {
      host: address.ip().to_string(),
      port: address.port(),
      server_certificate: remote_tls.server_certificate.clone(),
      server_private_key: remote_tls.server_private_key.clone(),
      client_ca_certificate: remote_tls.client_ca_certificate.clone(),
      pairing_store: remote_tls.pairing_store.clone(),
    });
  }
  let endpoint = listener.parse::<auv_api_client::ConnectEndpoint>().map_err(|error| format!("invalid --listen URI: {error}"))?;
  match endpoint {
    auv_api_client::ConnectEndpoint::Tcp(uri) => {
      let host = uri.host().ok_or_else(|| "--listen TCP URI omitted host".to_string())?.to_string();
      let port = uri.port_u16().unwrap_or(80);
      Ok(auv_api_server::transport::ListenEndpoint::Tcp { host, port })
    }
    #[cfg(unix)]
    auv_api_client::ConnectEndpoint::Unix(path) => Ok(auv_api_server::transport::ListenEndpoint::Unix { path }),
  }
}

fn capability(service: &str, methods: &[&str]) -> auv_api_proto::auv::api::core::v1::RunnerCapability {
  auv_api_proto::auv::api::core::v1::RunnerCapability {
    service: service.to_string(),
    methods: methods.iter().map(|method| (*method).to_string()).collect(),
  }
}

fn selected_required_capabilities(command_id: &str) -> Option<Vec<auv_api_proto::auv::api::core::v1::RunnerCapability>> {
  Some(match command_id {
    "display.list" => vec![capability(
      "auv.api.driver.v1.DisplayService",
      &["ListDisplays"],
    )],
    "display.capture" => vec![capability(
      "auv.api.driver.v1.CaptureService",
      &["CaptureDisplay"],
    )],
    "screen.captureRegion" => vec![capability(
      "auv.api.driver.v1.CaptureService",
      &["CaptureRegion"],
    )],
    "screen.findText" | "screen.waitForText" => {
      vec![capability(
        "auv.api.driver.v1.TextRecognitionService",
        &["FindDisplayText"],
      )]
    }
    "screen.clickText" => vec![
      capability("auv.api.driver.v1.TextRecognitionService", &["FindDisplayText"]),
      capability("auv.api.driver.v1.InputService", &["ClickScreenPoint"]),
    ],
    "window.list" => vec![capability(
      "auv.api.driver.v1.WindowService",
      &["ListWindows"],
    )],
    "window.capture" => vec![
      capability("auv.api.driver.v1.WindowService", &["ResolveWindow"]),
      capability("auv.api.driver.v1.CaptureService", &["CaptureWindow"]),
    ],
    "window.findText" | "window.waitForText" => vec![
      capability("auv.api.driver.v1.WindowService", &["ResolveWindow"]),
      capability("auv.api.driver.v1.TextRecognitionService", &["FindWindowText"]),
    ],
    "window.clickText" => vec![
      capability("auv.api.driver.v1.WindowService", &["ResolveWindow"]),
      capability("auv.api.driver.v1.TextRecognitionService", &["FindWindowText"]),
      capability("auv.api.driver.v1.InputService", &["ClickWindowPoint"]),
    ],
    "input.typeText" => vec![capability("auv.api.driver.v1.InputService", &["TypeText"])],
    "input.pasteText" => vec![capability("auv.api.driver.v1.InputService", &["PasteText"])],
    "input.key" => vec![capability("auv.api.driver.v1.InputService", &["PressKey"])],
    "input.clickWindowPoint" => vec![
      capability("auv.api.driver.v1.WindowService", &["ResolveWindow"]),
      capability("auv.api.driver.v1.InputService", &["ClickWindowPoint"]),
    ],
    "input.focusText" | "input.axFocusText" => vec![capability(
      "auv.api.driver.macos.v1.AccessibilityService",
      &["FocusText"],
    )],
    "app.probePermissions" => vec![capability(
      "auv.api.driver.macos.v1.PermissionService",
      &["ProbePermissions"],
    )],
    "app.activate" => vec![capability(
      "auv.api.driver.macos.v1.ApplicationService",
      &["ActivateBundleId"],
    )],
    "mediaControl.nowPlaying" => vec![capability(
      "auv.api.driver.macos.v1.MediaControlService",
      &["GetNowPlaying"],
    )],
    "mediaControl.play" => vec![capability(
      "auv.api.driver.macos.v1.MediaControlService",
      &["Play"],
    )],
    "mediaControl.pause" => vec![capability(
      "auv.api.driver.macos.v1.MediaControlService",
      &["Pause"],
    )],
    "mediaControl.togglePlayPause" => vec![capability(
      "auv.api.driver.macos.v1.MediaControlService",
      &["TogglePlayPause"],
    )],
    "mediaControl.next" => vec![capability(
      "auv.api.driver.macos.v1.MediaControlService",
      &["NextTrack"],
    )],
    "mediaControl.previous" => vec![capability(
      "auv.api.driver.macos.v1.MediaControlService",
      &["PreviousTrack"],
    )],
    "overlay.outline" | "overlay.cursor" | "overlay.status" | "overlay.captureFrame" | "overlay.clickTarget" => {
      vec![capability(
        "auv.api.driver.v1.OverlayService",
        &["ShowOverlay"],
      )]
    }
    _ => return None,
  })
}

async fn wait_for_selected_text<R, Call, Future, HasMatches>(
  command_id: &str,
  query: &str,
  options: auv_driver::WaitOptions,
  cancellation: &auv_cli_invoke::InvokeCancellation,
  mut call: Call,
  has_matches: HasMatches,
) -> Result<R, String>
where
  Call: FnMut() -> Future,
  Future: std::future::Future<Output = Result<R, String>>,
  HasMatches: Fn(&R) -> bool,
{
  let started = std::time::Instant::now();
  loop {
    cancellation.check().map_err(|error| error.to_string())?;
    let response = call().await?;
    if has_matches(&response) {
      return Ok(response);
    }
    if started.elapsed() >= options.timeout {
      return Err(format!("{command_id} did not find text {query:?} before timeout"));
    }
    tokio::select! {
      _ = cancellation.cancelled() => return Err("invoke cancelled".to_string()),
      _ = tokio::time::sleep(options.poll_interval) => {}
    }
  }
}

async fn invoke_on_selected_runner(
  input: auv_cli_invoke::InvokeCommandInput,
  context: auv_api_client::AuvContext,
) -> auv_cli_invoke::InvokeCommandResult {
  let command_id = input.command_id.as_str();
  if command_id == "app.probePermissions" && input.target_application_id.is_some() {
    return Err("app.probePermissions cannot use --target".to_string());
  }
  if command_id == "app.activate" && input.target_application_id.as_ref().is_none_or(|target| target.trim().is_empty()) {
    return Err("app.activate requires --target".to_string());
  }
  if matches!(command_id, "input.focusText" | "input.axFocusText")
    && input.target_application_id.as_ref().is_none_or(|target| target.trim().is_empty())
  {
    return Err(format!("{command_id} requires --target"));
  }
  if command_id.starts_with("mediaControl.") && input.target_application_id.is_some() {
    return Err(if command_id == "mediaControl.nowPlaying" {
      "mediaControl.nowPlaying cannot use --target; the macOS now-playing state is system-wide".to_string()
    } else {
      format!("{command_id} cannot use --target; macOS media controls are system-wide")
    });
  }
  if command_id.starts_with("overlay.") && input.target_application_id.is_some() {
    return Err(format!("{command_id} cannot use --target; overlays use global screen coordinates"));
  }
  if command_id.starts_with("overlay.") {
    let plan = auv_cli_invoke::commands::overlay::plan_overlay(&input)?;
    if input.dry_run || !input.overlay_enabled()? {
      return auv_cli_invoke::commands::overlay::selected_overlay_output(&plan, false);
    }
  }
  if matches!(command_id, "input.typeText" | "input.pasteText" | "input.key") && input.target_application_id.is_some() {
    return Err(format!("{command_id} cannot use --target until typed input target activation is available"));
  }
  if matches!(command_id, "screen.findText" | "screen.waitForText" | "screen.clickText" | "screen.captureRegion")
    && input.target_application_id.is_some()
  {
    return Err(format!("{command_id} cannot use --target until typed target activation is available"));
  }
  let required_capabilities = match selected_required_capabilities(command_id) {
    Some(required) => required,
    // TODO(typed-invoke-runner-coverage): map each remaining registered core
    // command only after its owning typed service is available; see
    // 2026-07-31-device-run-runner-aggregated-api-design.md. A selected Device
    // must fail explicitly rather than silently executing the command locally.
    None => {
      return Err(format!(
        "invoke command {command_id:?} does not yet have a typed Runner adapter; omit root --device/--device-id/--run only when local execution is intended"
      ));
    }
  };
  let auv = auv_api_client::placement::AuvClient::from_context(context).await.map_err(|error| error.to_string())?;
  let run = auv.run(Default::default()).await.map_err(|error| format!("resolve selected Run failed: {error}"))?;
  let runner = run
    .runner(auv_api_client::placement::RunnerOptions {
      required_capabilities,
      lifecycle: auv_api_proto::auv::api::core::v1::RunnerLifecycle::UnlessIdle,
      idle_timeout: Some(prost_types::Duration {
        seconds: 30,
        nanos: 0,
      }),
      operation_capacity: 1,
      ..Default::default()
    })
    .await
    .map_err(|error| format!("claim core Runner for {command_id} failed: {error}"))?;

  let invoked = match command_id {
    "app.activate" => {
      let target = input.target_application_id.as_deref().expect("validated target").trim();
      runner
        .macos()
        .applications()
        .activate_bundle_id(
          target,
          Some(prost_types::Duration {
            seconds: 0,
            nanos: 150_000_000,
          }),
        )
        .await
        .map_err(|status| format!("ApplicationService/ActivateBundleId failed: {status}"))
        .and_then(|result| selected_activation_output(target, &result))
    }
    "app.probePermissions" => runner
      .macos()
      .permissions()
      .probe()
      .await
      .map_err(|status| format!("PermissionService/ProbePermissions failed: {status}"))
      .and_then(|probe| auv_cli_invoke::commands::app::permission_probe_output(&probe)),
    "input.focusText" | "input.axFocusText" => {
      let candidate = input.inputs.get("candidate").cloned().unwrap_or_default();
      let selector = if candidate.trim().is_empty() {
        auv_driver::AxTextSelector::Query(input.inputs.get("query").cloned().unwrap_or_default())
      } else {
        auv_driver::AxTextSelector::Path(candidate.clone())
      };
      runner
        .macos()
        .accessibility()
        .focus_text(auv_driver::FocusTextOptions {
          app: input.target_application_id.clone().expect("validated target"),
          selector,
          expected_role: None,
        })
        .await
        .map_err(|status| format!("AccessibilityService/FocusText failed: {status}"))
        .and_then(|result| auv_cli_invoke::commands::input::focus_text_output(&result, &candidate))
    }
    "mediaControl.nowPlaying" => runner
      .macos()
      .media()
      .now_playing()
      .await
      .map_err(|status| format!("MediaControlService/GetNowPlaying failed: {status}"))
      .and_then(|state| auv_cli_invoke::commands::media_control::now_playing_state_output(&state)),
    "mediaControl.play" => runner
      .macos()
      .media()
      .play()
      .await
      .map_err(|status| format!("MediaControlService/Play failed: {status}"))
      .and_then(|outcome| auv_cli_invoke::commands::media_control::media_control_output(&outcome)),
    "mediaControl.pause" => runner
      .macos()
      .media()
      .pause()
      .await
      .map_err(|status| format!("MediaControlService/Pause failed: {status}"))
      .and_then(|outcome| auv_cli_invoke::commands::media_control::media_control_output(&outcome)),
    "mediaControl.togglePlayPause" => runner
      .macos()
      .media()
      .toggle_play_pause()
      .await
      .map_err(|status| format!("MediaControlService/TogglePlayPause failed: {status}"))
      .and_then(|outcome| auv_cli_invoke::commands::media_control::media_control_output(&outcome)),
    "mediaControl.next" => runner
      .macos()
      .media()
      .next_track()
      .await
      .map_err(|status| format!("MediaControlService/NextTrack failed: {status}"))
      .and_then(|outcome| auv_cli_invoke::commands::media_control::media_control_output(&outcome)),
    "mediaControl.previous" => runner
      .macos()
      .media()
      .previous_track()
      .await
      .map_err(|status| format!("MediaControlService/PreviousTrack failed: {status}"))
      .and_then(|outcome| auv_cli_invoke::commands::media_control::media_control_output(&outcome)),
    "overlay.outline" | "overlay.cursor" | "overlay.status" | "overlay.captureFrame" | "overlay.clickTarget" => {
      let plan = auv_cli_invoke::commands::overlay::plan_overlay(&input)?;
      runner
        .overlay()
        .show(&plan.overlay, plan.options)
        .await
        .map_err(|status| format!("OverlayService/ShowOverlay failed: {status}"))
        .and_then(|()| auv_cli_invoke::commands::overlay::selected_overlay_output(&plan, true))
    }
    "display.list" => {
      runner.displays().list().await.map_err(|status| format!("DisplayService/ListDisplays failed: {status}")).and_then(|displays| {
        let displays = displays.into_iter().map(display_from_proto).collect::<Result<Vec<_>, String>>()?;
        auv_cli_invoke::commands::display::list_displays_output(&auv_driver::ObservedDisplays { displays })
      })
    }
    "display.capture" => match runner.displays().capture(None).await {
      Err(status) => Err(format!("CaptureService/CaptureDisplay failed: {status}")),
      Ok(response) => {
        let capture = (|| {
          Ok(auv_driver::DisplayCapture {
            display: display_from_proto(response.display.ok_or_else(|| "CaptureDisplay response omitted Display".to_string())?)?,
            capture: capture_from_proto(response.capture.ok_or_else(|| "CaptureDisplay response omitted CapturedFrame".to_string())?)?,
          })
        })();
        match capture {
          Ok(capture) => auv_cli_invoke::commands::display::recorded_display_capture_output(&capture).await,
          Err(error) => Err(error),
        }
      }
    },
    "screen.captureRegion" => match selected_screen_region(&input) {
      Err(error) => Err(error),
      Ok(region) => match runner.displays().capture_region(region, None).await {
        Err(status) => Err(format!("CaptureService/CaptureRegion failed: {status}")),
        Ok(response) => {
          let capture = (|| {
            Ok(auv_driver::RegionCapture {
              display: display_from_proto(response.display.ok_or_else(|| "CaptureRegion response omitted Display".to_string())?)?,
              capture: capture_from_proto(response.capture.ok_or_else(|| "CaptureRegion response omitted CapturedFrame".to_string())?)?,
            })
          })();
          match capture {
            Ok(capture) => auv_cli_invoke::commands::screen::recorded_region_capture_output(&capture).await,
            Err(error) => Err(error),
          }
        }
      },
    },
    "window.list" => {
      runner.windows().list().await.map_err(|status| format!("WindowService/ListWindows failed: {status}")).and_then(|windows| {
        let windows = windows.into_iter().map(window_from_proto).collect::<Result<Vec<_>, String>>()?;
        auv_cli_invoke::commands::window::list_windows_output(&windows)
      })
    }
    "window.capture" => {
      let selector = selected_window_selector(&input);
      let response = match runner.windows().resolve(selector).await {
        Err(status) => Err(status),
        Ok(window) => window.capture().await,
      };
      match response {
        Err(status) => Err(format!("WindowService/ResolveWindow or CaptureService/CaptureWindow failed: {status}")),
        Ok(response) => {
          let capture = (|| {
            Ok(auv_cli_invoke::commands::window::WindowCapture {
              window: window_from_proto(response.window.ok_or_else(|| "CaptureWindow response omitted Window".to_string())?)?,
              capture: capture_from_proto(response.capture.ok_or_else(|| "CaptureWindow response omitted CapturedFrame".to_string())?)?,
            })
          })();
          match capture {
            Ok(capture) => auv_cli_invoke::commands::window::recorded_window_capture_output(&capture).await,
            Err(error) => Err(error),
          }
        }
      }
    }
    "window.findText" => match input.inputs.get("query").cloned() {
      None => Err("window.findText omitted its typed query argument".to_string()),
      Some(query) => {
        let response = match runner.windows().resolve(selected_window_selector(&input)).await {
          Err(status) => Err(status),
          Ok(window) => window.find_text(query).await,
        };
        match response {
          Err(status) => Err(format!("WindowService/ResolveWindow or TextRecognitionService/FindWindowText failed: {status}")),
          Ok(response) => {
            let projected = (|| {
              let result = auv_cli_invoke::commands::window::WindowTextRecognition {
                window: window_from_proto(response.window.ok_or_else(|| "FindWindowText response omitted Window".to_string())?)?,
                matches: ocr_matches_from_proto(response.matches)?,
              };
              let capture =
                capture_from_proto(response.capture.ok_or_else(|| "FindWindowText response omitted source capture".to_string())?)?;
              Ok((result, capture))
            })();
            match projected {
              Ok((result, capture)) => auv_cli_invoke::commands::window::recorded_window_text_matches_output(&result, &capture),
              Err(error) => Err(error),
            }
          }
        }
      }
    },
    "window.waitForText" => match input.inputs.get("query").cloned() {
      None => Err("window.waitForText omitted its typed query argument".to_string()),
      Some(query) => match runner.windows().resolve(selected_window_selector(&input)).await {
        Err(status) => Err(format!("WindowService/ResolveWindow failed: {status}")),
        Ok(window) => {
          let response = wait_for_selected_text(
            command_id,
            &query,
            auv_driver::WaitOptions::default(),
            &input.cancellation,
            || {
              let window = window.clone();
              let query = query.clone();
              async move { window.find_text(query).await.map_err(|status| format!("TextRecognitionService/FindWindowText failed: {status}")) }
            },
            |response| !response.matches.is_empty(),
          )
          .await;
          match response {
            Err(error) => Err(error),
            Ok(response) => {
              let projected = (|| {
                let result = auv_cli_invoke::commands::window::WindowTextRecognition {
                  window: window_from_proto(response.window.ok_or_else(|| "FindWindowText response omitted Window".to_string())?)?,
                  matches: ocr_matches_from_proto(response.matches)?,
                };
                let capture =
                  capture_from_proto(response.capture.ok_or_else(|| "FindWindowText response omitted source capture".to_string())?)?;
                Ok((result, capture))
              })();
              match projected {
                Ok((result, capture)) => auv_cli_invoke::commands::window::recorded_window_text_matches_output(&result, &capture),
                Err(error) => Err(error),
              }
            }
          }
        }
      },
    },
    "screen.findText" => match input.inputs.get("query").cloned() {
      None => Err("screen.findText omitted its typed query argument".to_string()),
      Some(query) => {
        let response = runner.displays().find_text(None, query).await;
        match response {
          Err(status) => Err(format!("TextRecognitionService/FindDisplayText failed: {status}")),
          Ok(response) => {
            let projected = (|| {
              let _display = display_from_proto(response.display.ok_or_else(|| "FindDisplayText response omitted Display".to_string())?)?;
              let matches = ocr_matches_from_proto(response.matches)?;
              let capture =
                capture_from_proto(response.capture.ok_or_else(|| "FindDisplayText response omitted source capture".to_string())?)?;
              Ok((matches, capture))
            })();
            match projected {
              Ok((matches, capture)) => auv_cli_invoke::commands::screen::recorded_screen_text_matches_output(&matches, &capture),
              Err(error) => Err(error),
            }
          }
        }
      }
    },
    "screen.clickText" => {
      async {
        let query = input.inputs.get("query").cloned().ok_or_else(|| "screen.clickText omitted its typed query argument".to_string())?;
        let recognized = runner
          .displays()
          .find_text(None, query.clone())
          .await
          .map_err(|status| format!("TextRecognitionService/FindDisplayText failed: {status}"))?;
        let _display = display_from_proto(recognized.display.ok_or_else(|| "FindDisplayText response omitted Display".to_string())?)?;
        let matches = ocr_matches_from_proto(recognized.matches)?;
        let capture = capture_from_proto(recognized.capture.ok_or_else(|| "FindDisplayText response omitted source capture".to_string())?)?;
        let point = matches.best_match().ok_or_else(|| format!("screen.clickText did not find text {query:?}"))?.action_point();
        let response = runner
          .input()
          .click_screen_point(
            auv_api_proto::auv::api::driver::v1::ScreenPoint {
              x: point.x,
              y: point.y,
            },
            Some(selected_screen_click_options(&input)?),
          )
          .await
          .map_err(|status| format!("InputService/ClickScreenPoint failed: {status}"))?;
        let point = response.point.ok_or_else(|| "ClickScreenPoint response omitted ScreenPoint".to_string())?;
        let action =
          input_action_from_proto(response.action.ok_or_else(|| "ClickScreenPoint response omitted InputActionResult".to_string())?)?;
        let result = auv_cli_invoke::commands::screen::ScreenTextClick {
          matches,
          point: auv_driver::Point::new(point.x, point.y),
          action,
        };
        auv_cli_invoke::commands::screen::recorded_screen_text_click_output(&result, &capture)
      }
      .await
    }
    "screen.waitForText" => match input.inputs.get("query").cloned() {
      None => Err("screen.waitForText omitted its typed query argument".to_string()),
      Some(query) => {
        let displays = runner.displays();
        let response = wait_for_selected_text(
          command_id,
          &query,
          auv_driver::WaitOptions::default(),
          &input.cancellation,
          || {
            let displays = displays.clone();
            let query = query.clone();
            async move {
              displays.find_text(None, query).await.map_err(|status| format!("TextRecognitionService/FindDisplayText failed: {status}"))
            }
          },
          |response| !response.matches.is_empty(),
        )
        .await;
        match response {
          Err(error) => Err(error),
          Ok(response) => {
            let projected = (|| {
              let _display = display_from_proto(response.display.ok_or_else(|| "FindDisplayText response omitted Display".to_string())?)?;
              let matches = ocr_matches_from_proto(response.matches)?;
              let capture =
                capture_from_proto(response.capture.ok_or_else(|| "FindDisplayText response omitted source capture".to_string())?)?;
              Ok((matches, capture))
            })();
            match projected {
              Ok((matches, capture)) => auv_cli_invoke::commands::screen::recorded_screen_text_matches_output(&matches, &capture),
              Err(error) => Err(error),
            }
          }
        }
      }
    },
    "window.clickText" => {
      async {
        let query = input.inputs.get("query").cloned().ok_or_else(|| "window.clickText omitted its typed query argument".to_string())?;
        let resolved = runner
          .windows()
          .resolve(selected_window_selector(&input))
          .await
          .map_err(|status| format!("WindowService/ResolveWindow failed: {status}"))?;
        let resolved_window = window_from_proto(resolved.resource().clone())?;
        let recognized =
          resolved.find_text(query.clone()).await.map_err(|status| format!("TextRecognitionService/FindWindowText failed: {status}"))?;
        let matches = ocr_matches_from_proto(recognized.matches)?;
        let capture = capture_from_proto(recognized.capture.ok_or_else(|| "FindWindowText response omitted source capture".to_string())?)?;
        let matched = matches.best_match().ok_or_else(|| format!("window.clickText did not find text {query:?}"))?;
        let point = matched_window_point(&resolved_window, matched)?;
        let wire_options = selected_click_options(&input)?;
        let options = driver_click_options_from_proto(&wire_options)?;
        let response = resolved
          .click(
            auv_api_proto::auv::api::driver::v1::WindowPoint {
              x: point.point().x,
              y: point.point().y,
            },
            Some(wire_options),
          )
          .await
          .map_err(|status| format!("InputService/ClickWindowPoint failed: {status}"))?;
        let clicked_window = window_from_proto(response.window.ok_or_else(|| "ClickWindowPoint response omitted Window".to_string())?)?;
        if clicked_window.reference != resolved_window.reference {
          return Err("ClickWindowPoint response changed the resolved WindowRef".to_string());
        }
        let returned_point = response.point.ok_or_else(|| "ClickWindowPoint response omitted WindowPoint".to_string())?;
        let action =
          input_action_from_proto(response.action.ok_or_else(|| "ClickWindowPoint response omitted InputActionResult".to_string())?)?;
        let result = auv_cli_invoke::commands::window::WindowTextClick {
          window: clicked_window,
          matches,
          point: auv_driver::WindowPoint::new(returned_point.x, returned_point.y),
          options,
          action,
        };
        auv_cli_invoke::commands::window::recorded_window_text_click_output(&result, &capture)
      }
      .await
    }
    "input.typeText" => match input.inputs.get("text").cloned() {
      None => Err("input.typeText omitted its typed text argument".to_string()),
      Some(text) => runner
        .input()
        .type_text(text, Some(Default::default()))
        .await
        .map_err(|status| format!("InputService/TypeText failed: {status}"))
        .and_then(|response| {
          let action = input_action_from_proto(response.action.ok_or_else(|| "TypeText response omitted InputActionResult".to_string())?)?;
          auv_cli_invoke::emit_input_action_result(&action);
          auv_cli_invoke::commands::input::input_action_output(&action)
        }),
    },
    "input.pasteText" => match input.inputs.get("text").cloned() {
      None => Err("input.pasteText omitted its typed text argument".to_string()),
      Some(text) => runner
        .input()
        .paste_text(text, Some(Default::default()))
        .await
        .map_err(|status| format!("InputService/PasteText failed: {status}"))
        .and_then(|response| {
          let action = input_action_from_proto(response.action.ok_or_else(|| "PasteText response omitted InputActionResult".to_string())?)?;
          auv_cli_invoke::emit_input_action_result(&action);
          auv_cli_invoke::commands::input::input_action_output(&action)
        }),
    },
    "input.key" => match input.inputs.get("key").cloned() {
      None => Err("input.key omitted its typed key argument".to_string()),
      Some(key) => {
        runner.input().press_key(key.clone(), None).await.map_err(|status| format!("InputService/PressKey failed: {status}")).and_then(
          |response| {
            let action = input_action_from_proto(response.action.ok_or_else(|| "PressKey response omitted InputActionResult".to_string())?)?;
            auv_cli_invoke::emit_input_action_result(&action);
            auv_cli_invoke::commands::input::press_key_output(&action, &key)
          },
        )
      }
    },
    "input.clickWindowPoint" => match runner.windows().resolve(selected_window_selector(&input)).await {
      Err(status) => Err(format!("WindowService/ResolveWindow failed: {status}")),
      Ok(resolved) => match window_from_proto(resolved.resource().clone()) {
        Err(error) => Err(error),
        Ok(window) => match (selected_window_point(&input, &window), selected_click_options(&input)) {
          (Err(error), _) | (_, Err(error)) => Err(error),
          (Ok(point), Ok(options)) => {
            match resolved
              .click(
                auv_api_proto::auv::api::driver::v1::WindowPoint {
                  x: point.point().x,
                  y: point.point().y,
                },
                Some(options),
              )
              .await
            {
              Err(status) => Err(format!("InputService/ClickWindowPoint failed: {status}")),
              Ok(response) => {
                let projected = (|| {
                  let point = response.point.ok_or_else(|| "ClickWindowPoint response omitted WindowPoint".to_string())?;
                  let action = input_action_from_proto(
                    response.action.ok_or_else(|| "ClickWindowPoint response omitted InputActionResult".to_string())?,
                  )?;
                  Ok(auv_cli_invoke::commands::input::WindowPointClickResult {
                    window: window_from_proto(response.window.ok_or_else(|| "ClickWindowPoint response omitted Window".to_string())?)?,
                    point: auv_driver::WindowPoint::new(point.x, point.y),
                    action: Some(action),
                  })
                })();
                match projected {
                  Ok(result) => {
                    if let Some(action) = result.action.as_ref() {
                      auv_cli_invoke::emit_input_action_result(action);
                    }
                    auv_cli_invoke::commands::input::window_point_click_output_without_overlay(result)
                  }
                  Err(error) => Err(error),
                }
              }
            }
          }
        },
      },
    },
    _ => unreachable!("typed Runner adapter was selected above"),
  };
  let released = runner.release().await.map_err(|status| format!("release core Runner lease failed: {status}"));
  let output = invoked?;
  released?;
  Ok(output)
}

fn selected_activation_output(target: &str, result: &auv_driver::ApplicationActivationResult) -> auv_cli_invoke::InvokeCommandResult {
  if result.requested_bundle_id != target {
    return Err("ActivateBundleId response changed the requested bundle id".to_string());
  }
  auv_cli_invoke::commands::app::activation_output(result)
}

fn display_from_proto(display: auv_api_proto::auv::api::driver::v1::Display) -> Result<auv_driver::Display, String> {
  let frame = display.frame.ok_or_else(|| format!("Display {:?} omitted its screen frame", display.display_id))?;
  Ok(auv_driver::Display {
    id: display.display_id,
    name: display.name,
    frame: auv_driver::Rect::new(frame.x, frame.y, frame.width, frame.height),
    coordinate_space: auv_driver::CoordinateSpace::Screen,
    scale_factor: display.scale_factor,
    is_primary: display.primary,
    is_builtin: display.builtin,
  })
}

fn window_from_proto(window: auv_api_proto::auv::api::driver::v1::Window) -> Result<auv_driver::Window, String> {
  let reference = window.r#ref.ok_or_else(|| "Window omitted its reference".to_string())?;
  let frame = window.frame.ok_or_else(|| format!("Window {:?} omitted its screen frame", reference.window_id))?;
  Ok(auv_driver::Window {
    reference: auv_driver::WindowRef {
      id: reference.window_id,
    },
    title: window.title,
    app_name: window.application_name,
    app_bundle_id: window.application_bundle_id,
    process_id: window.process_id,
    frame: auv_driver::Rect::new(frame.x, frame.y, frame.width, frame.height),
    coordinate_space: auv_driver::CoordinateSpace::Screen,
    is_main: window.is_main,
    is_visible: window.is_visible,
  })
}

fn capture_from_proto(capture: auv_api_proto::auv::api::driver::v1::CapturedFrame) -> Result<auv_driver::Capture, String> {
  let image = capture.image.ok_or_else(|| "CapturedFrame omitted its RGBA image".to_string())?;
  let bounds = capture.bounds.ok_or_else(|| "CapturedFrame omitted its screen bounds".to_string())?;
  let image = image::RgbaImage::from_raw(image.width, image.height, image.data)
    .ok_or_else(|| "CapturedFrame contains malformed RGBA8 data".to_string())?;
  Ok(auv_driver::Capture {
    image,
    bounds: auv_driver::Rect::new(bounds.x, bounds.y, bounds.width, bounds.height),
    scale_factor: capture.scale_factor,
    backend: capture.backend,
    fallback_reason: capture.fallback_reason,
  })
}

fn ocr_matches_from_proto(matches: Vec<auv_api_proto::auv::api::driver::v1::TextMatch>) -> Result<auv_driver::OcrMatches, String> {
  Ok(auv_driver::OcrMatches {
    matches: matches
      .into_iter()
      .map(|matched| {
        let bounds = matched.bounds.ok_or_else(|| format!("text match {:?} omitted its screen bounds", matched.text))?;
        Ok(auv_driver::OcrMatch {
          text: matched.text,
          confidence: matched.confidence,
          bounds: auv_driver::Rect::new(bounds.x, bounds.y, bounds.width, bounds.height),
        })
      })
      .collect::<Result<Vec<_>, String>>()?,
  })
}

fn selected_screen_region(input: &auv_cli_invoke::InvokeCommandInput) -> Result<auv_api_proto::auv::api::driver::v1::ScreenRect, String> {
  let number = |name: &str| {
    input
      .inputs
      .get(name)
      .ok_or_else(|| format!("screen.captureRegion omitted --{name}"))?
      .parse::<f64>()
      .map_err(|error| format!("screen.captureRegion has invalid --{name}: {error}"))
  };
  let region = auv_api_proto::auv::api::driver::v1::ScreenRect {
    x: number("x")?,
    y: number("y")?,
    width: number("width")?,
    height: number("height")?,
  };
  if !region.x.is_finite() || !region.y.is_finite() {
    return Err("screen.captureRegion requires finite --x and --y".to_string());
  }
  if !region.width.is_finite() || !region.height.is_finite() || region.width <= 0.0 || region.height <= 0.0 {
    return Err("screen.captureRegion requires --width and --height greater than zero".to_string());
  }
  Ok(region)
}

fn selected_window_point(
  input: &auv_cli_invoke::InvokeCommandInput,
  window: &auv_driver::Window,
) -> Result<auv_driver::WindowPoint, String> {
  let number = |name: &str| {
    input
      .inputs
      .get(name)
      .map(|value| value.parse::<f64>().map_err(|error| format!("input.clickWindowPoint has invalid --{name}: {error}")))
      .transpose()
  };
  let offset_x = number("offset-x")?;
  let offset_y = number("offset-y")?;
  let relative_x = number("relative-x")?;
  let relative_y = number("relative-y")?;
  let point = match (offset_x, offset_y, relative_x, relative_y) {
    (Some(x), Some(y), None, None) if x.is_finite() && y.is_finite() && x >= 0.0 && y >= 0.0 => auv_driver::WindowPoint::new(x, y),
    (None, None, Some(x), Some(y)) if x.is_finite() && y.is_finite() && (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y) => {
      auv_driver::WindowPoint::new(window.frame.size.width * x, window.frame.size.height * y)
    }
    (Some(_), Some(_), None, None) => return Err("input.clickWindowPoint requires finite non-negative window offsets".to_string()),
    (None, None, Some(_), Some(_)) => return Err("input.clickWindowPoint requires relative coordinates within 0..=1".to_string()),
    _ => return Err("input.clickWindowPoint requires --offset-x/--offset-y or --relative-x/--relative-y".to_string()),
  };
  let point_value = point.point();
  if !(0.0..=window.frame.size.width).contains(&point_value.x) || !(0.0..=window.frame.size.height).contains(&point_value.y) {
    return Err(format!(
      "input.clickWindowPoint point {},{} is outside target window bounds 0..={},0..={}",
      point_value.x, point_value.y, window.frame.size.width, window.frame.size.height
    ));
  }
  Ok(point)
}

fn selected_click_options(input: &auv_cli_invoke::InvokeCommandInput) -> Result<auv_api_proto::auv::api::driver::v1::ClickOptions, String> {
  use auv_api_proto::auv::api::driver::v1::{InputPolicy, WindowClickStrategy};

  let command_id = input.command_id.as_str();

  let policy = match input.inputs.get("input-policy").map(String::as_str) {
    None if command_id == "screen.clickText" => InputPolicy::ForegroundPreferred,
    None | Some("background-preferred") => InputPolicy::BackgroundPreferred,
    Some("background-only") => InputPolicy::BackgroundOnly,
    Some("foreground-preferred") => InputPolicy::ForegroundPreferred,
    Some(value) => return Err(format!("{command_id} has unknown --input-policy {value:?}")),
  };
  let count = input
    .inputs
    .get("click-count")
    .map(|value| value.parse::<u32>().map_err(|error| format!("{command_id} has invalid --click-count: {error}")))
    .transpose()?
    .unwrap_or(1);
  if !(1..=u32::from(u8::MAX)).contains(&count) {
    return Err(format!("{command_id} requires --click-count within 1..=255"));
  }
  let interval_ms = input
    .inputs
    .get("click-interval-ms")
    .map(|value| value.parse::<u64>().map_err(|error| format!("{command_id} has invalid --click-interval-ms: {error}")))
    .transpose()?
    .unwrap_or(75);
  let interval = (count > 1).then(|| prost_types::Duration {
    seconds: i64::try_from(interval_ms / 1000).unwrap_or(i64::MAX),
    nanos: i32::try_from((interval_ms % 1000) * 1_000_000).expect("subsecond milliseconds fit i32"),
  });
  if count > 1 && interval_ms == 0 {
    return Err(format!("{command_id} requires a positive --click-interval-ms for repeated clicks"));
  }
  Ok(auv_api_proto::auv::api::driver::v1::ClickOptions {
    policy: policy as i32,
    click: Some(auv_api_proto::auv::api::driver::v1::Click { count, interval }),
    window_strategy: WindowClickStrategy::ChromiumCompatible as i32,
  })
}

fn selected_screen_click_options(
  input: &auv_cli_invoke::InvokeCommandInput,
) -> Result<auv_api_proto::auv::api::driver::v1::ScreenClickOptions, String> {
  Ok(auv_api_proto::auv::api::driver::v1::ScreenClickOptions {
    click: selected_click_options(input)?.click,
  })
}

fn driver_click_options_from_proto(options: &auv_api_proto::auv::api::driver::v1::ClickOptions) -> Result<auv_driver::ClickOptions, String> {
  use auv_api_proto::auv::api::driver::v1::{InputPolicy as ProtoPolicy, WindowClickStrategy as ProtoStrategy};

  let policy = match ProtoPolicy::try_from(options.policy).map_err(|_| format!("unknown InputPolicy value {}", options.policy))? {
    ProtoPolicy::Unspecified | ProtoPolicy::BackgroundPreferred => auv_driver::InputPolicy::BackgroundPreferred,
    ProtoPolicy::BackgroundOnly => auv_driver::InputPolicy::BackgroundOnly,
    ProtoPolicy::ForegroundPreferred => auv_driver::InputPolicy::ForegroundPreferred,
  };
  let click = match options.click.as_ref() {
    None => auv_driver::Click::Single,
    Some(click) if click.count == 1 => auv_driver::Click::Single,
    Some(click) => {
      let interval = click.interval.as_ref().ok_or_else(|| "repeated click omitted its interval".to_string())?;
      if interval.seconds < 0 || interval.nanos < 0 {
        return Err("click interval must not be negative".to_string());
      }
      let interval = std::time::Duration::new(
        u64::try_from(interval.seconds).map_err(|_| "click interval seconds do not fit u64".to_string())?,
        u32::try_from(interval.nanos).map_err(|_| "click interval nanos do not fit u32".to_string())?,
      );
      match click.count {
        2 => auv_driver::Click::Double { interval },
        count if (3..=u32::from(u8::MAX)).contains(&count) => auv_driver::Click::Repeated {
          count: u8::try_from(count).expect("validated click count fits u8"),
          interval,
        },
        count => return Err(format!("click count {count} is outside 1..=255")),
      }
    }
  };
  let window_strategy = match ProtoStrategy::try_from(options.window_strategy) {
    Ok(ProtoStrategy::Unspecified | ProtoStrategy::ChromiumCompatible) => auv_driver::WindowClickStrategy::ChromiumCompatible,
    Ok(ProtoStrategy::PidTargeted) => auv_driver::WindowClickStrategy::PidTargeted,
    Err(_) => return Err(format!("unknown WindowClickStrategy value {}", options.window_strategy)),
  };
  Ok(auv_driver::ClickOptions {
    policy,
    click,
    window_strategy,
  })
}

fn matched_window_point(window: &auv_driver::Window, matched: &auv_driver::OcrMatch) -> Result<auv_driver::WindowPoint, String> {
  let screen_point = matched.action_point();
  let point = auv_driver::WindowPoint::new(screen_point.x - window.frame.origin.x, screen_point.y - window.frame.origin.y);
  let point_value = point.point();
  if !(0.0..=window.frame.size.width).contains(&point_value.x) || !(0.0..=window.frame.size.height).contains(&point_value.y) {
    return Err(format!("recognized text point {},{} is outside resolved window bounds", screen_point.x, screen_point.y));
  }
  Ok(point)
}

fn selected_window_selector(input: &auv_cli_invoke::InvokeCommandInput) -> auv_api_proto::auv::api::driver::v1::WindowSelector {
  use auv_api_proto::auv::api::driver::v1::window_selector::{Application, Window};

  let application = input
    .target_application_id
    .as_ref()
    .map(|bundle_id| Application::ApplicationBundleId(bundle_id.clone()))
    .unwrap_or(Application::FrontmostApplication(true));
  let window = input
    .inputs
    .get("title")
    .filter(|title| !title.trim().is_empty())
    .map(|title| Window::TitleContains(title.clone()))
    .unwrap_or(Window::MainVisible(true));
  auv_api_proto::auv::api::driver::v1::WindowSelector {
    application: Some(application),
    window: Some(window),
  }
}

fn input_action_from_proto(action: auv_api_proto::auv::api::driver::v1::InputActionResult) -> Result<auv_driver::InputActionResult, String> {
  use auv_api_proto::auv::api::driver::v1::{DisturbanceLevel as ProtoDisturbance, InputDeliveryPath as ProtoPath};

  fn path(value: i32) -> Result<auv_driver::InputDeliveryPath, String> {
    Ok(match ProtoPath::try_from(value).map_err(|_| format!("unknown InputDeliveryPath value {value}"))? {
      ProtoPath::Unspecified => return Err("InputDeliveryPath must not be unspecified".to_string()),
      ProtoPath::Noop => auv_driver::InputDeliveryPath::Noop,
      ProtoPath::AxPress => auv_driver::InputDeliveryPath::AxPress,
      ProtoPath::AxFocus => auv_driver::InputDeliveryPath::AxFocus,
      ProtoPath::AxSetValue => auv_driver::InputDeliveryPath::AxSetValue,
      ProtoPath::AxScroll => auv_driver::InputDeliveryPath::AxScroll,
      ProtoPath::AxSelectedText => auv_driver::InputDeliveryPath::AxSelectedText,
      ProtoPath::WindowTargetedMouse => auv_driver::InputDeliveryPath::WindowTargetedMouse,
      ProtoPath::WindowTargetedWheel => auv_driver::InputDeliveryPath::WindowTargetedWheel,
      ProtoPath::WindowTargetedKeyboard => auv_driver::InputDeliveryPath::WindowTargetedKeyboard,
      ProtoPath::WindowTargetedKeyboardScroll => auv_driver::InputDeliveryPath::WindowTargetedKeyboardScroll,
      ProtoPath::ClipboardPaste => auv_driver::InputDeliveryPath::ClipboardPaste,
      ProtoPath::ForegroundSystemEvents => auv_driver::InputDeliveryPath::ForegroundSystemEvents,
      ProtoPath::Unsupported => auv_driver::InputDeliveryPath::Unsupported,
    })
  }

  fn disturbance(value: i32) -> Result<auv_driver::DisturbanceLevel, String> {
    Ok(match ProtoDisturbance::try_from(value).map_err(|_| format!("unknown DisturbanceLevel value {value}"))? {
      ProtoDisturbance::Unspecified => return Err("DisturbanceLevel must not be unspecified".to_string()),
      ProtoDisturbance::None => auv_driver::DisturbanceLevel::None,
      ProtoDisturbance::Temporary => auv_driver::DisturbanceLevel::Temporary,
      ProtoDisturbance::Foreground => auv_driver::DisturbanceLevel::Foreground,
      ProtoDisturbance::Unknown => auv_driver::DisturbanceLevel::Unknown,
    })
  }

  let result = auv_driver::InputActionResult {
    selected_path: path(action.selected_path)?,
    attempts: action
      .attempts
      .into_iter()
      .map(|attempt| {
        Ok(auv_driver::InputAttempt {
          path: path(attempt.path)?,
          succeeded: attempt.succeeded,
          message: attempt.message,
        })
      })
      .collect::<Result<Vec<_>, String>>()?,
    mouse_disturbance: disturbance(action.mouse_disturbance)?,
    focus_disturbance: disturbance(action.focus_disturbance)?,
    clipboard_disturbance: disturbance(action.clipboard_disturbance)?,
  };
  result.validate()?;
  Ok(result)
}

async fn serve_foreground(
  listeners: Vec<auv_api_server::transport::ListenEndpoint>,
  store_root: PathBuf,
  discovery_file: Option<PathBuf>,
  no_discovery: bool,
  daemon_idle_timeout: Option<std::time::Duration>,
  runner_provider_paths: &[PathBuf],
) -> Result<i32, String> {
  let runner_providers = runner_provider_paths
    .iter()
    .map(|path| {
      let path = if path.is_absolute() {
        path.clone()
      } else {
        std::env::current_dir().map_err(|error| format!("failed to resolve --runner-provider {}: {error}", path.display()))?.join(path)
      };
      auv_api_server::runner_provider::RunnerProviderConfig::load_json(&path)
        .map_err(|error| format!("failed to load --runner-provider {}: {error}", path.display()))
    })
    .collect::<Result<Vec<_>, _>>()?;
  let mut listeners = listeners.into_iter();
  let listen = listeners.next().ok_or_else(|| "auv serve requires at least one listener".to_string())?;
  let bound = auv_api_server::transport::bind(auv_api_server::transport::ApiServeConfig {
    listen,
    additional_listeners: listeners.collect(),
    store_root,
    daemon_idle_timeout,
    runner_providers,
    first_party_runners: first_party_runner_runtimes()?,
  })
  .await?;
  let _published_descriptor = if no_discovery {
    None
  } else {
    let path = match discovery_file {
      Some(path) => path,
      None => crate::daemon_discovery::default_path().map_err(|error| error.to_string())?,
    };
    let local_endpoint =
      bound.discovery_endpoint().ok_or_else(|| "daemon discovery requires a local Unix or loopback listener".to_string())?;
    Some(crate::daemon_discovery::PublishedDescriptor::publish(path, local_endpoint.to_string())?)
  };
  for endpoint in bound.endpoints() {
    println!("auv serve: {endpoint}");
  }
  std::io::stdout().flush().map_err(|error| format!("failed to flush daemon readiness line: {error}"))?;
  let shutdown = tokio_util::sync::CancellationToken::new();
  let signal_shutdown = shutdown.clone();
  tokio::spawn(async move {
    if tokio::signal::ctrl_c().await.is_ok() {
      signal_shutdown.cancel();
    }
  });
  bound.serve(shutdown).await?;
  Ok(0)
}

fn first_party_runner_runtimes() -> Result<auv_api_server::runner_provider::FirstPartyRunnerRuntimes, String> {
  #[cfg(unix)]
  {
    use auv_api_server::runner_provider::{ExecutableRunnerRuntime, RunnerRuntime};

    let executable = std::env::current_exe().map_err(|error| format!("failed to resolve the auv executable for Runner hosting: {error}"))?;
    let runtime = |role: &str| {
      RunnerRuntime::Executable(ExecutableRunnerRuntime {
        executable: executable.clone(),
        arguments: vec![
          crate::INTERNAL_RUNNER_SENTINEL.to_string(),
          role.to_string(),
        ],
      })
    };
    Ok(auv_api_server::runner_provider::FirstPartyRunnerRuntimes {
      local_driver: Some(runtime(crate::LOCAL_RUNNER_ROLE)),
      inference_ultralytics: Some(runtime(crate::INFERENCE_RUNNER_ROLE)),
    })
  }
  #[cfg(not(unix))]
  {
    // TODO(first-party-runner-windows-host): publish built-in runtimes after
    // inherited named-pipe transport replaces the Unix descriptor contract.
    Ok(Default::default())
  }
}

enum ResolvedApiEndpoint {
  Selected(auv_api_client::ConnectEndpoint),
  NotDiscovered,
}

fn resolve_api_endpoint(explicit: Option<&str>) -> Result<ResolvedApiEndpoint, String> {
  let Some(selected) = auv_api_client::discovery::resolve(explicit).map_err(|error| error.to_string())? else {
    return Ok(ResolvedApiEndpoint::NotDiscovered);
  };
  Ok(ResolvedApiEndpoint::Selected(selected))
}

async fn connected_api_client(explicit: Option<&str>) -> Result<Option<auv_api_client::Client>, String> {
  let ResolvedApiEndpoint::Selected(endpoint) = resolve_api_endpoint(explicit)? else {
    return Ok(None);
  };
  let display = endpoint.to_string();
  auv_api_client::Client::connect(endpoint)
    .await
    .map(Some)
    .map_err(|error| format!("failed to connect to AUV API server at {display}: {error}"))
}

#[derive(TableRow)]
struct DeviceTableRow {
  #[table(header = "DEVICE ID")]
  device_id: String,
  name: Option<String>,
  platform: Option<String>,
  local: bool,
  status: String,
  profile: Option<String>,
}

#[derive(TableRow)]
struct DeviceProfileTableRow {
  #[table(header = "PROFILE")]
  config_profile: String,
  #[table(header = "DEVICE ID")]
  device_id: String,
  name: String,
  endpoint: String,
  #[table(header = "CREDENTIAL")]
  credential_profile: String,
}

#[derive(TableRow)]
struct RunnerClassTableRow {
  #[table(header = "CLASS")]
  runner_class: String,
  name: String,
  available: bool,
  #[table(header = "DEVICE ID")]
  device_id: Option<String>,
  lifecycles: String,
  capabilities: usize,
}

#[derive(TableRow)]
struct RunTableRow {
  #[table(header = "RUN ID")]
  run_id: String,
  phase: String,
  #[table(header = "DEVICE IDS")]
  device_ids: String,
}

#[derive(TableRow)]
struct RunnerTableRow {
  #[table(header = "RUNNER ID")]
  runner_id: String,
  class: String,
  phase: String,
  pid: Option<u32>,
  #[table(header = "DEVICE ID")]
  device_id: Option<String>,
  lifecycle: String,
  #[table(header = "LEASES")]
  active_run_leases: u32,
  #[table(header = "OPERATIONS")]
  operation_usage: String,
}

fn print_table<R: table::TableRow>(rows: &[R], empty_message: &'static str) {
  println!("{}", table::render(rows, TableOptions::default().empty_message(empty_message)));
}

fn short_enum_name(value: &str, prefix: &str) -> String {
  value.strip_prefix(prefix).unwrap_or(value).to_ascii_lowercase().replace('_', "-")
}

fn device_table_row(device: &auv_api_proto::auv::api::core::v1::Device, status: &str) -> DeviceTableRow {
  let platform = auv_api_proto::auv::api::core::v1::DevicePlatform::try_from(device.platform)
    .unwrap_or(auv_api_proto::auv::api::core::v1::DevicePlatform::Unspecified)
    .as_str_name();
  DeviceTableRow {
    device_id: device.r#ref.as_ref().map(|reference| reference.device_id.clone()).unwrap_or_else(|| "<missing>".to_string()),
    name: (!device.name.is_empty()).then(|| device.name.clone()),
    platform: Some(short_enum_name(platform, "DEVICE_PLATFORM_")),
    local: device.local,
    status: status.to_string(),
    profile: None,
  }
}

fn configured_device_table_row(device: &auv_api_client::profile::ConfiguredDevice) -> DeviceTableRow {
  DeviceTableRow {
    device_id: device.device_id().to_string(),
    name: (!device.device_name().is_empty()).then(|| device.device_name().to_string()),
    platform: None,
    local: false,
    status: "offline".to_string(),
    profile: Some(device.config_profile().to_string()),
  }
}

fn device_profile_table_row(device: &auv_api_client::profile::ConfiguredDevice) -> DeviceProfileTableRow {
  DeviceProfileTableRow {
    config_profile: device.config_profile().to_string(),
    device_id: device.device_id().to_string(),
    name: device.device_name().to_string(),
    endpoint: device.endpoint().to_string(),
    credential_profile: device.credential_profile().to_string(),
  }
}

async fn run_device_list(endpoint: Option<&str>, device_id: Option<&str>, device_name: Option<&str>, json: bool) -> Result<i32, String> {
  let mut devices = match connected_api_client(endpoint).await {
    Ok(Some(mut client)) => client.list_devices().await.map_err(|status| format!("ListDevices failed: {status}"))?,
    Ok(None) => Vec::new(),
    Err(error) if endpoint.is_none() => {
      eprintln!("warning: local AUV daemon is unavailable: {error}");
      Vec::new()
    }
    Err(error) => return Err(error),
  };
  if let Some(device_id) = device_id {
    devices.retain(|device| device.r#ref.as_ref().is_some_and(|reference| reference.device_id == device_id));
  }
  if let Some(device_name) = device_name {
    devices.retain(|device| device.name == device_name);
  }
  let profiles = match auv_api_client::profile::ProfileStore::from_env().map_err(|error| error.to_string())?.list_devices() {
    Ok(profiles) => profiles,
    Err(auv_api_client::profile::ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
    Err(error) => return Err(error.to_string()),
  };
  if json {
    let mut values = devices
      .iter()
      .map(|device| {
        let mut value = device_json(device);
        value["source"] = serde_json::json!("daemon");
        value["status"] = serde_json::json!("online");
        value
      })
      .collect::<Vec<_>>();
    for profile in profiles
      .iter()
      .filter(|profile| device_id.is_none_or(|id| profile.device_id() == id) && device_name.is_none_or(|name| profile.device_name() == name))
    {
      if devices.iter().any(|device| device.r#ref.as_ref().is_some_and(|reference| reference.device_id == profile.device_id())) {
        continue;
      }
      values.push(configured_device_json(profile));
    }
    println!("{}", serde_json::to_string_pretty(&values).map_err(|error| format!("failed to encode Device list: {error}"))?);
  } else {
    let mut rows = devices.iter().map(|device| device_table_row(device, "online")).collect::<Vec<_>>();
    for profile in profiles
      .iter()
      .filter(|profile| device_id.is_none_or(|id| profile.device_id() == id) && device_name.is_none_or(|name| profile.device_name() == name))
    {
      if devices.iter().any(|device| device.r#ref.as_ref().is_some_and(|reference| reference.device_id == profile.device_id())) {
        continue;
      }
      rows.push(configured_device_table_row(profile));
    }
    print_table(&rows, "(no devices)");
  }
  Ok(0)
}

fn configured_device_json(device: &auv_api_client::profile::ConfiguredDevice) -> serde_json::Value {
  serde_json::json!({
    "device_id": device.device_id(),
    "name": device.device_name(),
    "local": false,
    "source": "configured_profile",
    "status": "offline",
    "config_profile": device.config_profile(),
    "credential_profile": device.credential_profile(),
    "endpoint": device.endpoint().to_string(),
    "server_name": device.server_name(),
  })
}

fn run_device_profiles(command: &DeviceProfilesCommand) -> Result<(), String> {
  let store = auv_api_client::profile::ProfileStore::from_env().map_err(|error| error.to_string())?;
  match command {
    DeviceProfilesCommand::List(args) => {
      let profiles = match store.list_devices() {
        Ok(profiles) => profiles,
        Err(auv_api_client::profile::ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.to_string()),
      };
      if args.json {
        let values = profiles.iter().map(configured_device_json).collect::<Vec<_>>();
        println!("{}", serde_json::to_string_pretty(&values).map_err(|error| error.to_string())?);
      } else {
        let rows = profiles.iter().map(device_profile_table_row).collect::<Vec<_>>();
        print_table(&rows, "(no device profiles)");
      }
    }
    DeviceProfilesCommand::Get(args) => {
      let profile = store.get_device(&args.name).map_err(|error| error.to_string())?;
      if args.json {
        println!("{}", serde_json::to_string_pretty(&configured_device_json(&profile)).map_err(|error| error.to_string())?);
      } else {
        print_table(&[device_profile_table_row(&profile)], "(no device profile)");
      }
    }
    DeviceProfilesCommand::Create(args) => {
      let (profile, credentials) = profile_inputs(args)?;
      store.create(&args.name, profile, credentials).map_err(|error| error.to_string())?;
    }
    DeviceProfilesCommand::Update(args) => {
      let (profile, credentials) = profile_inputs(args)?;
      store.update(&args.name, profile, credentials).map_err(|error| error.to_string())?;
    }
    DeviceProfilesCommand::Delete(args) => store.delete(&args.name).map_err(|error| error.to_string())?,
  }
  Ok(())
}

fn profile_inputs(
  args: &ProfileWriteArgs,
) -> Result<(auv_api_client::profile::DeviceProfileInput, Option<auv_api_client::profile::CredentialProfileInput>), String> {
  let credentials = match (&args.server_ca_certificate, &args.client_certificate, &args.client_private_key) {
    (Some(ca), Some(certificate), Some(key)) => Some(auv_api_client::profile::CredentialProfileInput {
      server_ca_certificate: ca.clone(),
      client_certificate: certificate.clone(),
      client_private_key: key.clone(),
    }),
    (None, None, None) => None,
    _ => return Err("credential paths must be supplied together".to_string()),
  };
  Ok((
    auv_api_client::profile::DeviceProfileInput {
      device_id: args.device_id.clone(),
      device_name: args.device_name.clone(),
      endpoint: args.endpoint.clone(),
      server_name: args.server_name.clone(),
      credential_profile: args.credential_profile.clone(),
    },
    credentials,
  ))
}

async fn run_device_get(endpoint: Option<&str>, device_id: &str, json: bool) -> Result<i32, String> {
  let Some(mut client) = connected_api_client(endpoint).await? else {
    return Err("no AUV daemon was discovered".to_string());
  };
  let device = client.get_device(device_id).await.map_err(|status| format!("GetDevice failed: {status}"))?;
  if json {
    println!("{}", serde_json::to_string_pretty(&device_json(&device)).map_err(|error| format!("failed to encode Device: {error}"))?);
  } else {
    print_table(&[device_table_row(&device, "online")], "(no device)");
  }
  Ok(0)
}

fn device_json(device: &auv_api_proto::auv::api::core::v1::Device) -> serde_json::Value {
  let platform = auv_api_proto::auv::api::core::v1::DevicePlatform::try_from(device.platform)
    .unwrap_or(auv_api_proto::auv::api::core::v1::DevicePlatform::Unspecified)
    .as_str_name();
  serde_json::json!({
    "device_id": device.r#ref.as_ref().map(|reference| reference.device_id.as_str()),
    "name": device.name,
    "platform": platform,
    "local": device.local,
    "labels": device.labels,
  })
}

async fn run_runner_create(
  endpoint: Option<&str>,
  device_id: Option<&str>,
  runner_class: &str,
  lifecycle: i32,
  json: bool,
) -> Result<i32, String> {
  let mut client = required_client(endpoint).await?;
  let runner = client
    .create_runner(auv_api_proto::auv::api::core::v1::CreateRunnerRequest {
      device: device_id.map(|device_id| auv_api_proto::auv::api::core::v1::DeviceRef {
        device_id: device_id.to_string(),
      }),
      runner_class: Some(auv_api_proto::auv::api::core::v1::RunnerClassRef {
        runner_class: runner_class.to_string(),
      }),
      labels: Default::default(),
      lifecycle,
      idle_timeout: None,
    })
    .await
    .map_err(|status| format!("CreateRunner failed: {status}"))?;
  print_runner(&runner, json)?;
  Ok(0)
}

async fn run_runner_class_list(endpoint: Option<&str>, device_id: Option<&str>, json: bool) -> Result<i32, String> {
  let Some(mut client) = connected_api_client(endpoint).await? else {
    if json {
      println!("[]");
    } else {
      print_table(&Vec::<RunnerClassTableRow>::new(), "(no runner classes)");
    }
    return Ok(0);
  };
  let classes = client
    .list_runner_classes(device_id.map(|device_id| auv_api_proto::auv::api::core::v1::DeviceRef {
      device_id: device_id.to_string(),
    }))
    .await
    .map_err(|status| format!("ListRunnerClasses failed: {status}"))?;
  let values = classes.iter().map(runner_class_json).collect::<Vec<_>>();
  if json {
    println!("{}", serde_json::to_string_pretty(&values).map_err(|error| error.to_string())?);
  } else {
    let rows = classes.iter().map(runner_class_table_row).collect::<Vec<_>>();
    print_table(&rows, "(no runner classes)");
  }
  Ok(0)
}

fn runner_class_table_row(class: &auv_api_proto::auv::api::core::v1::RunnerClass) -> RunnerClassTableRow {
  let lifecycles = class
    .supported_lifecycles
    .iter()
    .map(|lifecycle| {
      let lifecycle = auv_api_proto::auv::api::core::v1::RunnerLifecycle::try_from(*lifecycle)
        .unwrap_or(auv_api_proto::auv::api::core::v1::RunnerLifecycle::Unspecified)
        .as_str_name();
      short_enum_name(lifecycle, "RUNNER_LIFECYCLE_")
    })
    .collect::<Vec<_>>()
    .join(",");
  RunnerClassTableRow {
    runner_class: class.r#ref.as_ref().map(|reference| reference.runner_class.clone()).unwrap_or_else(|| "<missing>".to_string()),
    name: class.display_name.clone(),
    available: class.available,
    device_id: class.device.as_ref().map(|reference| reference.device_id.clone()),
    lifecycles,
    capabilities: class.capabilities.iter().map(|capability| capability.methods.len()).sum(),
  }
}

fn runner_class_json(class: &auv_api_proto::auv::api::core::v1::RunnerClass) -> serde_json::Value {
  serde_json::json!({
    "runner_class": class.r#ref.as_ref().map(|reference| reference.runner_class.as_str()),
    "device_id": class.device.as_ref().map(|reference| reference.device_id.as_str()),
    "display_name": class.display_name,
    "available": class.available,
    "supported_lifecycles": class.supported_lifecycles.iter().map(|lifecycle| {
      auv_api_proto::auv::api::core::v1::RunnerLifecycle::try_from(*lifecycle)
        .unwrap_or(auv_api_proto::auv::api::core::v1::RunnerLifecycle::Unspecified)
        .as_str_name()
    }).collect::<Vec<_>>(),
    "capabilities": class.capabilities.iter().map(|capability| serde_json::json!({
      "service": capability.service,
      "methods": capability.methods,
    })).collect::<Vec<_>>(),
  })
}

async fn run_create(endpoint: Option<&str>, device_ids: &[String], json: bool) -> Result<i32, String> {
  let mut client = required_client(endpoint).await?;
  let run = client
    .create_run(auv_api_proto::auv::api::core::v1::CreateRunRequest {
      devices: device_ids
        .iter()
        .map(|device_id| auv_api_proto::auv::api::core::v1::DeviceRef {
          device_id: device_id.clone(),
        })
        .collect(),
      labels: Default::default(),
    })
    .await
    .map_err(|status| format!("CreateRun failed: {status}"))?;
  print_run(&run, json)?;
  Ok(0)
}

async fn run_list(endpoint: Option<&str>, device_id: Option<&str>, run_id: Option<&str>, json: bool) -> Result<i32, String> {
  let Some(mut client) = connected_api_client(endpoint).await? else {
    if json {
      println!("[]");
    } else {
      print_table(&Vec::<RunTableRow>::new(), "(no runs)");
    }
    return Ok(0);
  };
  let mut runs = client.list_runs().await.map_err(|status| format!("ListRuns failed: {status}"))?;
  if let Some(device_id) = device_id {
    runs.retain(|run| run.devices.iter().any(|device| device.device_id == device_id));
  }
  if let Some(run_id) = run_id {
    runs.retain(|run| run.r#ref.as_ref().is_some_and(|reference| reference.run_id == run_id));
  }
  if json {
    println!("{}", serde_json::to_string_pretty(&runs.iter().map(run_json).collect::<Vec<_>>()).map_err(|error| error.to_string())?);
  } else {
    let rows = runs.iter().map(run_table_row).collect::<Vec<_>>();
    print_table(&rows, "(no runs)");
  }
  Ok(0)
}

async fn run_get(endpoint: Option<&str>, device_id: Option<&str>, run_id: &str, json: bool) -> Result<i32, String> {
  let mut client = required_client(endpoint).await?;
  let run = client.get_run(run_id).await.map_err(|status| format!("GetRun failed: {status}"))?;
  validate_run_device(&run, device_id)?;
  print_run(&run, json)?;
  Ok(0)
}

async fn run_stop(endpoint: Option<&str>, device_id: Option<&str>, run_id: &str, outcome: i32, json: bool) -> Result<i32, String> {
  let mut client = required_client(endpoint).await?;
  if device_id.is_some() {
    let run = client.get_run(run_id).await.map_err(|status| format!("GetRun failed: {status}"))?;
    validate_run_device(&run, device_id)?;
  }
  let outcome = auv_api_proto::auv::api::core::v1::RunOutcome::try_from(outcome).map_err(|_| "Run outcome is invalid".to_string())?;
  let run = client.stop_run(run_id, outcome).await.map_err(|status| format!("StopRun failed: {status}"))?;
  print_run(&run, json)?;
  Ok(0)
}

fn validate_run_argument(run_id: &str, context: &auv_api_client::AuvContext) -> Result<(), String> {
  if context.run_id.as_deref().is_some_and(|selected| selected != run_id) {
    return Err(format!("Run argument {run_id:?} conflicts with root --run"));
  }
  Ok(())
}

fn validate_run_device(run: &auv_api_proto::auv::api::core::v1::Run, expected_device_id: Option<&str>) -> Result<(), String> {
  if let Some(expected_device_id) = expected_device_id
    && !run.devices.iter().any(|device| device.device_id == expected_device_id)
  {
    return Err(format!("Run is not attached to selected Device {expected_device_id:?}"));
  }
  Ok(())
}

fn print_run(run: &auv_api_proto::auv::api::core::v1::Run, json: bool) -> Result<(), String> {
  if json {
    println!("{}", serde_json::to_string_pretty(&run_json(run)).map_err(|error| error.to_string())?);
  } else {
    print_table(&[run_table_row(run)], "(no run)");
  }
  Ok(())
}

fn run_table_row(run: &auv_api_proto::auv::api::core::v1::Run) -> RunTableRow {
  let phase = auv_api_proto::auv::api::core::v1::RunPhase::try_from(run.phase)
    .unwrap_or(auv_api_proto::auv::api::core::v1::RunPhase::Unspecified)
    .as_str_name();
  RunTableRow {
    run_id: run.r#ref.as_ref().map(|reference| reference.run_id.clone()).unwrap_or_else(|| "<missing>".to_string()),
    phase: short_enum_name(phase, "RUN_PHASE_"),
    device_ids: run.devices.iter().map(|device| device.device_id.as_str()).collect::<Vec<_>>().join(","),
  }
}

fn run_json(run: &auv_api_proto::auv::api::core::v1::Run) -> serde_json::Value {
  let phase = auv_api_proto::auv::api::core::v1::RunPhase::try_from(run.phase)
    .unwrap_or(auv_api_proto::auv::api::core::v1::RunPhase::Unspecified)
    .as_str_name();
  serde_json::json!({
    "run_id": run.r#ref.as_ref().map(|reference| reference.run_id.as_str()),
    "phase": phase,
    "device_ids": run.devices.iter().map(|device| device.device_id.as_str()).collect::<Vec<_>>(),
    "labels": run.labels,
    "created_at": timestamp_json(run.created_at.as_ref()),
  })
}

async fn run_runner_list(endpoint: Option<&str>, device_id: Option<&str>, json: bool) -> Result<i32, String> {
  let Some(mut client) = connected_api_client(endpoint).await? else {
    if json {
      println!("[]");
    } else {
      print_table(&Vec::<RunnerTableRow>::new(), "(no runners)");
    }
    return Ok(0);
  };
  let mut runners = client.list_runners().await.map_err(|status| format!("ListRunners failed: {status}"))?;
  if let Some(device_id) = device_id {
    runners.retain(|runner| runner.device.as_ref().is_some_and(|device| device.device_id == device_id));
  }
  if json {
    println!("{}", serde_json::to_string_pretty(&runners.iter().map(runner_json).collect::<Vec<_>>()).map_err(|error| error.to_string())?);
  } else {
    let rows = runners.iter().map(runner_table_row).collect::<Vec<_>>();
    print_table(&rows, "(no runners)");
  }
  Ok(0)
}

async fn run_runner_get(endpoint: Option<&str>, device_id: Option<&str>, runner_id: &str, json: bool) -> Result<i32, String> {
  let mut client = required_client(endpoint).await?;
  let runner = client.get_runner(runner_id).await.map_err(|status| format!("GetRunner failed: {status}"))?;
  validate_runner_device(&runner, device_id)?;
  print_runner(&runner, json)?;
  Ok(0)
}

async fn run_runner_stop(endpoint: Option<&str>, device_id: Option<&str>, runner_id: &str, json: bool) -> Result<i32, String> {
  let mut client = required_client(endpoint).await?;
  if device_id.is_some() {
    let runner = client.get_runner(runner_id).await.map_err(|status| format!("GetRunner failed: {status}"))?;
    validate_runner_device(&runner, device_id)?;
  }
  let runner = client.delete_runner(runner_id).await.map_err(|status| format!("DeleteRunner failed: {status}"))?;
  print_runner(&runner, json)?;
  Ok(0)
}

fn validate_runner_device(runner: &auv_api_proto::auv::api::core::v1::Runner, expected_device_id: Option<&str>) -> Result<(), String> {
  if let Some(expected_device_id) = expected_device_id
    && runner.device.as_ref().is_none_or(|device| device.device_id != expected_device_id)
  {
    return Err(format!("Runner is not owned by selected Device {expected_device_id:?}"));
  }
  Ok(())
}

async fn required_client(endpoint: Option<&str>) -> Result<auv_api_client::Client, String> {
  connected_api_client(endpoint).await?.ok_or_else(|| "no AUV daemon was discovered".to_string())
}

fn print_runner(runner: &auv_api_proto::auv::api::core::v1::Runner, json: bool) -> Result<(), String> {
  if json {
    println!("{}", serde_json::to_string_pretty(&runner_json(runner)).map_err(|error| error.to_string())?);
  } else {
    print_table(&[runner_table_row(runner)], "(no runner)");
  }
  Ok(())
}

fn runner_table_row(runner: &auv_api_proto::auv::api::core::v1::Runner) -> RunnerTableRow {
  let phase = auv_api_proto::auv::api::core::v1::RunnerPhase::try_from(runner.phase)
    .unwrap_or(auv_api_proto::auv::api::core::v1::RunnerPhase::Unspecified)
    .as_str_name();
  let lifecycle = auv_api_proto::auv::api::core::v1::RunnerLifecycle::try_from(runner.lifecycle)
    .unwrap_or(auv_api_proto::auv::api::core::v1::RunnerLifecycle::Unspecified)
    .as_str_name();
  RunnerTableRow {
    runner_id: runner.r#ref.as_ref().map(|reference| reference.runner_id.clone()).unwrap_or_else(|| "<missing>".to_string()),
    class: runner.runner_class.as_ref().map(|reference| reference.runner_class.clone()).unwrap_or_else(|| "<missing>".to_string()),
    phase: short_enum_name(phase, "RUNNER_PHASE_"),
    pid: (runner.process_id != 0).then_some(runner.process_id),
    device_id: runner.device.as_ref().map(|reference| reference.device_id.clone()),
    lifecycle: short_enum_name(lifecycle, "RUNNER_LIFECYCLE_"),
    active_run_leases: runner.active_run_leases,
    operation_usage: format!("{}/{}", runner.active_operations, runner.operation_capacity),
  }
}

fn runner_json(runner: &auv_api_proto::auv::api::core::v1::Runner) -> serde_json::Value {
  let phase = auv_api_proto::auv::api::core::v1::RunnerPhase::try_from(runner.phase)
    .unwrap_or(auv_api_proto::auv::api::core::v1::RunnerPhase::Unspecified)
    .as_str_name();
  serde_json::json!({
    "runner_id": runner.r#ref.as_ref().map(|reference| reference.runner_id.as_str()),
    "device_id": runner.device.as_ref().map(|reference| reference.device_id.as_str()),
    "runner_class": runner.runner_class.as_ref().map(|reference| reference.runner_class.as_str()),
    "phase": phase,
    "process_id": runner.process_id,
    "labels": runner.labels,
    "capabilities": runner.capabilities.iter().map(|capability| serde_json::json!({
      "service": capability.service,
      "methods": capability.methods,
    })).collect::<Vec<_>>(),
    "descriptor_set_sha256": hex::encode(&runner.descriptor_set_sha256),
  })
}

fn timestamp_json(timestamp: Option<&prost_types::Timestamp>) -> serde_json::Value {
  timestamp.map_or(serde_json::Value::Null, |timestamp| serde_json::json!({ "seconds": timestamp.seconds, "nanos": timestamp.nanos }))
}

#[derive(serde::Serialize)]
struct PermissionCheckReport {
  platform: &'static str,
  process_id: u32,
  executable: Option<String>,
  accessibility: &'static str,
  screen_recording_preflight: &'static str,
  screen_capture_kit: &'static str,
  all_ok: bool,
  warnings: Vec<String>,
  recommendation: String,
}

fn run_permission_check(json: bool) -> Result<(), String> {
  let report = collect_permission_check()?;

  if json {
    println!("{}", serde_json::to_string_pretty(&report).map_err(|error| format!("failed to encode permission report: {error}"))?);
  } else {
    print_permission_check_report(&report);
  }

  Ok(())
}

#[cfg(target_os = "macos")]
fn collect_permission_check() -> Result<PermissionCheckReport, String> {
  let native = auv_driver_macos::native::permission::probe_native_permissions()?;
  let all_ok = native.accessibility == "granted" && native.screen_capture_kit == "granted";
  let mut warnings = Vec::new();

  if native.screen_recording == "missing" && native.screen_capture_kit == "granted" {
    warnings.push(
      "CGPreflightScreenCaptureAccess reports missing, but the ScreenCaptureKit probe works; this can happen when the launch host owns TCC attribution."
        .to_string(),
    );
  }

  Ok(PermissionCheckReport {
    platform: "macos",
    process_id: process::id(),
    executable: env::current_exe().ok().map(|path| path.display().to_string()),
    accessibility: native.accessibility,
    screen_recording_preflight: native.screen_recording,
    screen_capture_kit: native.screen_capture_kit,
    all_ok,
    warnings,
    recommendation: permission_recommendation(native.accessibility, native.screen_capture_kit),
  })
}

#[cfg(not(target_os = "macos"))]
fn collect_permission_check() -> Result<PermissionCheckReport, String> {
  Err("permission check is currently implemented only for macOS".to_string())
}

fn permission_recommendation(accessibility: &str, screen_capture_kit: &str) -> String {
  match (accessibility, screen_capture_kit) {
    ("granted", "granted") => "AUV has the macOS permissions needed for capture and AX-backed automation.".to_string(),
    ("missing", "missing") => {
      "Grant Accessibility and Screen Recording to the terminal or app that launches auv, then rerun this check.".to_string()
    }
    ("missing", _) => "Grant Accessibility to the terminal or app that launches auv, then rerun this check.".to_string(),
    (_, "missing") => "Grant Screen Recording to the terminal or app that launches auv, then rerun this check.".to_string(),
    _ => "Review the permission statuses above before running desktop automation.".to_string(),
  }
}

fn print_permission_check_report(report: &PermissionCheckReport) {
  println!("AUV permission check");
  println!("platform: {}", report.platform);
  println!("process: {}", report.process_id);
  if let Some(executable) = &report.executable {
    println!("executable: {executable}");
  }
  println!("accessibility: {}", permission_status_line(report.accessibility));
  println!("screen recording preflight: {}", permission_status_line(report.screen_recording_preflight));
  println!("screen capture kit probe: {}", permission_status_line(report.screen_capture_kit));
  for warning in &report.warnings {
    println!("warning: {warning}");
  }
  println!("all ok: {}", report.all_ok);
  println!("recommendation: {}", report.recommendation);
}

fn permission_status_line(status: &str) -> String {
  match status {
    "granted" => "[ok] granted".to_string(),
    "missing" => "[missing] missing".to_string(),
    other => format!("[unknown] {other}"),
  }
}

pub(crate) fn resolve_store_root(project_root: &Path, explicit: Option<&PathBuf>) -> PathBuf {
  explicit.cloned().unwrap_or_else(|| project_root.join(".auv").join("store"))
}

fn resolve_path(project_root: &Path, path: &Path) -> PathBuf {
  if path.is_absolute() {
    path.to_path_buf()
  } else {
    project_root.join(path)
  }
}

fn run_device_trust(project_root: &Path, store: Option<&Path>, selector: &str, action: DeviceTrustAction) -> Result<(), String> {
  use auv_api_server::authority::PairingStore;

  let selector = selector.trim();
  if selector.is_empty() {
    return Err("Device selector must not be empty".to_string());
  }
  let store_path = store.map_or_else(|| project_root.join(".auv").join("pairings.json"), |path| resolve_path(project_root, path));
  // TODO(live-pairing-admin): Device trust mutation currently owns the store
  // lock and therefore runs only while the daemon is stopped. Re-open this
  // boundary when an audited, owner-authorized local administration RPC lands.
  let store =
    PairingStore::open(store_path.clone()).map_err(|error| format!("failed to open pairing store {}: {error}", store_path.display()))?;
  let records = store.list();
  let pair_id = if records.iter().any(|record| record.pair_id == selector) {
    selector.to_string()
  } else {
    let mut matches = records.iter().filter(|record| record.label == selector).map(|record| record.pair_id.clone()).collect::<Vec<_>>();
    matches.sort();
    match matches.as_slice() {
      [] => return Err(format!("unknown paired Device {selector:?}")),
      [pair_id] => pair_id.clone(),
      _ => {
        return Err(format!("paired Device name {selector:?} is ambiguous; use one of these stable IDs: {}", matches.join(", ")));
      }
    }
  };

  match action {
    DeviceTrustAction::Unpair => store.remove_pair(&pair_id).map_err(|error| format!("failed to unpair Device {pair_id}: {error}"))?,
    DeviceTrustAction::Enable => store.set_enabled(&pair_id, true).map_err(|error| format!("failed to enable Device {pair_id}: {error}"))?,
    DeviceTrustAction::Disable => {
      store.set_enabled(&pair_id, false).map_err(|error| format!("failed to disable Device {pair_id}: {error}"))?
    }
  }
  Ok(())
}

fn run_pairing(project_root: &Path, store: Option<&Path>, command: &PairingCommand) -> Result<(), String> {
  use auv_api_server::authority::{CredentialState, PairingCredential, PairingRecord, PairingStore};

  let store_path = store.map_or_else(|| project_root.join(".auv").join("pairings.json"), |path| resolve_path(project_root, path));
  // TODO(live-pairing-admin): this owner tool intentionally takes the same
  // exclusive store lock as the daemon and therefore provisions only while it
  // is stopped. Add live mutations only through an owner-authorized local
  // admin RPC with audit evidence.
  let store =
    PairingStore::open(store_path.clone()).map_err(|error| format!("failed to open pairing store {}: {error}", store_path.display()))?;
  match command {
    PairingCommand::List { json } => {
      let devices = store.list();
      if *json {
        println!(
          "{}",
          serde_json::to_string_pretty(&serde_json::json!({ "revision": store.revision(), "devices": devices }))
            .map_err(|error| format!("failed to encode pairing store: {error}"))?
        );
      } else {
        for device in devices {
          println!(
            "{}\t{}\t{}",
            device.pair_id,
            if device.enabled {
              "enabled"
            } else {
              "disabled"
            },
            device.label
          );
          for scope in device.scopes {
            println!("  scope\t{scope:?}");
          }
          for credential in device.credentials {
            println!("  certificate\t{}\t{:?}", credential.certificate_fingerprint.as_str(), credential.state);
          }
        }
      }
    }
    PairingCommand::Add {
      pair_id,
      label,
      certificate,
      scope,
    } => {
      let fingerprint = certificate_fingerprint(project_root, certificate)?;
      let pair_id = pair_id.clone().unwrap_or_else(|| uuid::Uuid::now_v7().to_string());
      store
        .insert(PairingRecord {
          pair_id: pair_id.clone(),
          label: label.clone(),
          enabled: true,
          scopes: scope.iter().copied().map(Into::into).collect(),
          credentials: vec![PairingCredential {
            certificate_fingerprint: fingerprint.clone(),
            state: CredentialState::Active,
          }],
        })
        .map_err(|error| format!("failed to add paired device: {error}"))?;
      println!("pair id: {pair_id}");
      println!("certificate sha256: {}", fingerprint.as_str());
    }
    PairingCommand::Rotate {
      pair_id,
      certificate,
    } => {
      let fingerprint = certificate_fingerprint(project_root, certificate)?;
      store.add_credential(pair_id, fingerprint.clone()).map_err(|error| format!("failed to rotate paired credential: {error}"))?;
      println!("certificate sha256: {}", fingerprint.as_str());
    }
    PairingCommand::SetScopes { pair_id, scope } => {
      store
        .set_scopes(pair_id, scope.iter().copied().map(Into::into).collect())
        .map_err(|error| format!("failed to update paired scopes: {error}"))?;
    }
    PairingCommand::Enable { pair_id } => {
      store.set_enabled(pair_id, true).map_err(|error| format!("failed to enable paired device: {error}"))?;
    }
    PairingCommand::Disable { pair_id } => {
      store.set_enabled(pair_id, false).map_err(|error| format!("failed to disable paired device: {error}"))?;
    }
    PairingCommand::Revoke { certificate } => {
      let fingerprint = certificate_fingerprint(project_root, certificate)?;
      store.revoke_credential(&fingerprint).map_err(|error| format!("failed to revoke paired credential: {error}"))?;
    }
  }
  Ok(())
}

fn certificate_fingerprint(project_root: &Path, certificate: &Path) -> Result<auv_api_server::authority::CertificateFingerprint, String> {
  let path = resolve_path(project_root, certificate);
  let pem = std::fs::read(&path).map_err(|error| format!("failed to read client certificate {}: {error}", path.display()))?;
  auv_api_server::authority::CertificateFingerprint::from_pem(&pem)
    .map_err(|error| format!("failed to parse client certificate {}: {error}", path.display()))
}

#[derive(Clone)]
struct CliTracing {
  dispatch: auv_tracing::Dispatch,
  store: Arc<auv_tracing::FileTracingStore>,
}

fn build_cli_tracing(project_root: &Path, options: &TracingOptions) -> Result<CliTracing, String> {
  let store_root = resolve_store_root(project_root, options.store_root.as_ref());
  let store = Arc::new(
    auv_tracing::FileTracingStore::open(&store_root)
      .map_err(|error| format!("failed to open tracing store {}: {error}", store_root.display()))?,
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

async fn flush_cli_recording(dispatch: &auv_tracing::Dispatch) -> Option<String> {
  dispatch.flush().await.err().map(|error| error.to_string())
}

fn tracing_run_id_for_selected_context(selected: Option<&crate::plugin::ResolvedExecutionContext>) -> Result<auv_tracing::RunId, String> {
  let Some(control_run_id) = selected.and_then(|selected| selected.context.run_id.as_deref()) else {
    return Ok(auv_tracing::RunId::new());
  };
  let value = control_run_id
    .strip_prefix("run_")
    .ok_or_else(|| format!("selected Run ID {control_run_id:?} cannot be projected into the tracing Run identity"))?;
  value.parse().map_err(|error| format!("selected Run ID {control_run_id:?} cannot be projected into the tracing Run identity: {error}"))
}

#[cfg(test)]
mod selected_invoke_tests {
  use super::*;
  use auv_api_proto::auv::api::driver::v1 as proto;

  #[test]
  fn selected_control_run_and_tracing_run_share_one_uuid_identity() {
    let selected = crate::plugin::ResolvedExecutionContext {
      context: auv_api_client::AuvContext {
        run_id: Some("run_019fb919-3a0c-73d2-a06e-9f94a150ccfb".to_string()),
        ..Default::default()
      },
      implicit_run_id: None,
    };

    assert_eq!(tracing_run_id_for_selected_context(Some(&selected)).unwrap().to_string(), "019fb919-3a0c-73d2-a06e-9f94a150ccfb");
  }

  #[test]
  fn malformed_selected_control_run_is_not_silently_recorded_as_another_run() {
    let selected = crate::plugin::ResolvedExecutionContext {
      context: auv_api_client::AuvContext {
        run_id: Some("external-run".to_string()),
        ..Default::default()
      },
      implicit_run_id: None,
    };

    let error = tracing_run_id_for_selected_context(Some(&selected)).expect_err("malformed selected Run must fail closed");
    assert!(error.contains("cannot be projected"));
  }

  #[test]
  fn selected_text_commands_claim_the_exact_typed_capabilities() {
    let focus = vec![capability(
      "auv.api.driver.macos.v1.AccessibilityService",
      &["FocusText"],
    )];
    assert_eq!(selected_required_capabilities("input.focusText"), Some(focus.clone()));
    assert_eq!(selected_required_capabilities("input.axFocusText"), Some(focus));

    let capabilities = selected_required_capabilities("screen.waitForText").expect("screen wait adapter");
    assert_eq!(
      capabilities,
      vec![capability(
        "auv.api.driver.v1.TextRecognitionService",
        &["FindDisplayText"],
      )]
    );

    let capabilities = selected_required_capabilities("window.waitForText").expect("window wait adapter");
    assert_eq!(
      capabilities,
      vec![
        capability("auv.api.driver.v1.WindowService", &["ResolveWindow"]),
        capability("auv.api.driver.v1.TextRecognitionService", &["FindWindowText"],),
      ]
    );

    let capabilities = selected_required_capabilities("window.clickText").expect("window click adapter");
    assert_eq!(
      capabilities,
      vec![
        capability("auv.api.driver.v1.WindowService", &["ResolveWindow"]),
        capability("auv.api.driver.v1.TextRecognitionService", &["FindWindowText"],),
        capability("auv.api.driver.v1.InputService", &["ClickWindowPoint"]),
      ]
    );
    let capabilities = selected_required_capabilities("screen.clickText").expect("screen click adapter");
    assert_eq!(
      capabilities,
      vec![
        capability("auv.api.driver.v1.TextRecognitionService", &["FindDisplayText"]),
        capability("auv.api.driver.v1.InputService", &["ClickScreenPoint"]),
      ]
    );
    assert_eq!(
      selected_required_capabilities("input.pasteText").expect("paste adapter"),
      vec![capability("auv.api.driver.v1.InputService", &["PasteText"])]
    );
    assert_eq!(
      selected_required_capabilities("app.probePermissions").expect("permission adapter"),
      vec![capability(
        "auv.api.driver.macos.v1.PermissionService",
        &["ProbePermissions"]
      )]
    );
    assert_eq!(
      selected_required_capabilities("app.activate").expect("application adapter"),
      vec![capability(
        "auv.api.driver.macos.v1.ApplicationService",
        &["ActivateBundleId"]
      )]
    );
    assert_eq!(
      selected_required_capabilities("mediaControl.nowPlaying").expect("now-playing adapter"),
      vec![capability(
        "auv.api.driver.macos.v1.MediaControlService",
        &["GetNowPlaying"]
      )]
    );
    for (command_id, method) in [
      ("mediaControl.play", "Play"),
      ("mediaControl.pause", "Pause"),
      ("mediaControl.togglePlayPause", "TogglePlayPause"),
      ("mediaControl.next", "NextTrack"),
      ("mediaControl.previous", "PreviousTrack"),
    ] {
      assert_eq!(
        selected_required_capabilities(command_id).expect("media command adapter"),
        [capability(
          "auv.api.driver.macos.v1.MediaControlService",
          &[method]
        )]
      );
    }
    for command_id in [
      "overlay.outline",
      "overlay.cursor",
      "overlay.status",
      "overlay.captureFrame",
      "overlay.clickTarget",
    ] {
      assert_eq!(
        selected_required_capabilities(command_id).expect("overlay adapter"),
        [capability(
          "auv.api.driver.v1.OverlayService",
          &["ShowOverlay"]
        )]
      );
    }
  }

  #[tokio::test]
  async fn selected_permission_probe_rejects_irrelevant_application_target() {
    let error = invoke_on_selected_runner(
      auv_cli_invoke::InvokeCommandInput {
        command_id: "app.probePermissions".to_string(),
        target_application_id: Some("com.apple.TextEdit".to_string()),
        inputs: Default::default(),
        typed_args: None,
        dry_run: false,
        cancellation: Default::default(),
      },
      Default::default(),
    )
    .await
    .expect_err("permission probe target must fail before daemon resolution");
    assert_eq!(error, "app.probePermissions cannot use --target");
  }

  #[tokio::test]
  async fn selected_application_activation_requires_target_before_daemon_resolution() {
    let error = invoke_on_selected_runner(
      auv_cli_invoke::InvokeCommandInput {
        command_id: "app.activate".to_string(),
        target_application_id: None,
        inputs: Default::default(),
        typed_args: None,
        dry_run: false,
        cancellation: Default::default(),
      },
      Default::default(),
    )
    .await
    .expect_err("missing target must fail before daemon resolution");
    assert_eq!(error, "app.activate requires --target");
  }

  #[test]
  fn selected_application_activation_rejects_changed_requested_bundle() {
    let error = selected_activation_output(
      "com.example.Requested",
      &auv_driver::ApplicationActivationResult {
        requested_bundle_id: "com.example.Changed".to_string(),
        verification: auv_driver::ApplicationActivationVerification::Unavailable {
          reason: "fixture".to_string(),
        },
      },
    )
    .expect_err("daemon response must retain the exact requested target");
    assert_eq!(error, "ActivateBundleId response changed the requested bundle id");
  }

  #[tokio::test]
  async fn selected_now_playing_rejects_application_target_before_daemon_resolution() {
    let error = invoke_on_selected_runner(
      auv_cli_invoke::InvokeCommandInput {
        command_id: "mediaControl.nowPlaying".to_string(),
        target_application_id: Some("com.example.Player".to_string()),
        inputs: Default::default(),
        typed_args: None,
        dry_run: false,
        cancellation: Default::default(),
      },
      Default::default(),
    )
    .await
    .expect_err("now-playing target must fail before daemon resolution");
    assert_eq!(error, "mediaControl.nowPlaying cannot use --target; the macOS now-playing state is system-wide");
  }

  #[tokio::test]
  async fn selected_media_commands_reject_application_target_before_daemon_resolution() {
    for command_id in [
      "mediaControl.play",
      "mediaControl.pause",
      "mediaControl.togglePlayPause",
      "mediaControl.next",
      "mediaControl.previous",
    ] {
      let error = invoke_on_selected_runner(
        auv_cli_invoke::InvokeCommandInput {
          command_id: command_id.to_string(),
          target_application_id: Some("com.example.Player".to_string()),
          inputs: Default::default(),
          typed_args: None,
          dry_run: false,
          cancellation: auv_cli_invoke::InvokeCancellation::new(),
        },
        Default::default(),
      )
      .await
      .expect_err("target must fail before daemon resolution");
      assert_eq!(error, format!("{command_id} cannot use --target; macOS media controls are system-wide"));
    }
  }

  #[tokio::test]
  async fn selected_disabled_overlay_validates_without_resolving_a_daemon() {
    let arguments = [
      "overlay.outline",
      "--x",
      "10",
      "--y",
      "20",
      "--width",
      "30",
      "--height",
      "40",
      "--no-overlay",
    ]
    .into_iter()
    .map(str::to_string)
    .collect::<Vec<_>>();
    let auv_cli_invoke::InvokeCliParse::Invoke {
      command_id,
      target_application_id,
      inputs,
      typed_args,
      dry_run,
      ..
    } = auv_cli_invoke::parse_invoke_args(&arguments).expect("parse overlay")
    else {
      panic!("expected overlay invocation");
    };
    let output = invoke_on_selected_runner(
      auv_cli_invoke::InvokeCommandInput {
        command_id,
        target_application_id,
        inputs,
        typed_args: Some(typed_args),
        dry_run,
        cancellation: Default::default(),
      },
      Default::default(),
    )
    .await
    .expect("disabled overlay must not require daemon discovery");
    assert_eq!(output.report.expect("overlay report").fields.last().expect("overlay status").value, "disabled");
  }

  #[test]
  fn selected_screen_click_does_not_send_window_only_policy() {
    let input = auv_cli_invoke::InvokeCommandInput {
      command_id: "screen.clickText".to_string(),
      target_application_id: None,
      inputs: Default::default(),
      typed_args: None,
      dry_run: false,
      cancellation: Default::default(),
    };
    let options = selected_screen_click_options(&input).expect("screen click options");
    assert_eq!(options.click.as_ref().map(|click| click.count), Some(1));
  }

  #[tokio::test]
  async fn selected_text_wait_retries_until_the_first_matching_response() {
    let mut responses = std::collections::VecDeque::from([Vec::<u8>::new(), vec![1]]);
    let calls = std::cell::Cell::new(0);
    let response = wait_for_selected_text(
      "screen.waitForText",
      "Ready",
      auv_driver::WaitOptions {
        timeout: std::time::Duration::from_secs(1),
        poll_interval: std::time::Duration::ZERO,
      },
      &Default::default(),
      || {
        calls.set(calls.get() + 1);
        let response = responses.pop_front().expect("fixture response");
        async move { Ok(response) }
      },
      |response| !response.is_empty(),
    )
    .await
    .expect("second response matches");

    assert_eq!(response, vec![1]);
    assert_eq!(calls.get(), 2);
  }

  #[tokio::test]
  async fn selected_text_wait_preserves_timeout_semantics_after_one_exact_call() {
    let calls = std::cell::Cell::new(0);
    let error = wait_for_selected_text(
      "window.waitForText",
      "Ready",
      auv_driver::WaitOptions {
        timeout: std::time::Duration::ZERO,
        poll_interval: std::time::Duration::ZERO,
      },
      &Default::default(),
      || {
        calls.set(calls.get() + 1);
        async { Ok(Vec::<u8>::new()) }
      },
      |response| !response.is_empty(),
    )
    .await
    .expect_err("empty response at the deadline times out");

    assert_eq!(error, "window.waitForText did not find text \"Ready\" before timeout");
    assert_eq!(calls.get(), 1);
  }

  #[test]
  fn input_action_projection_preserves_typed_delivery_evidence() {
    let action = input_action_from_proto(proto::InputActionResult {
      selected_path: proto::InputDeliveryPath::ForegroundSystemEvents as i32,
      attempts: vec![proto::InputAttempt {
        path: proto::InputDeliveryPath::ForegroundSystemEvents as i32,
        succeeded: true,
        message: None,
      }],
      mouse_disturbance: proto::DisturbanceLevel::None as i32,
      focus_disturbance: proto::DisturbanceLevel::Foreground as i32,
      clipboard_disturbance: proto::DisturbanceLevel::None as i32,
    })
    .expect("valid protobuf delivery evidence");

    assert_eq!(action.selected_path, auv_driver::InputDeliveryPath::ForegroundSystemEvents);
    assert_eq!(
      action.attempts,
      vec![auv_driver::InputAttempt::success(
        auv_driver::InputDeliveryPath::ForegroundSystemEvents
      )]
    );
    assert_eq!(action.focus_disturbance, auv_driver::DisturbanceLevel::Foreground);
  }

  #[test]
  fn input_action_projection_rejects_unspecified_wire_enums() {
    let error = input_action_from_proto(proto::InputActionResult {
      selected_path: proto::InputDeliveryPath::Unspecified as i32,
      ..Default::default()
    })
    .expect_err("unspecified path must not become canonical driver evidence");
    assert!(error.contains("must not be unspecified"));
  }

  #[test]
  fn capture_projection_preserves_rgba_and_screen_contract() {
    let capture = capture_from_proto(proto::CapturedFrame {
      image: Some(auv_api_proto::auv::api::image::v1::RgbaFrame {
        width: 2,
        height: 1,
        data: vec![1, 2, 3, 4, 5, 6, 7, 8],
      }),
      bounds: Some(proto::ScreenRect {
        x: -10.0,
        y: 4.0,
        width: 2.0,
        height: 1.0,
      }),
      scale_factor: 1.0,
      backend: "fixture".to_string(),
      fallback_reason: None,
    })
    .expect("valid RGBA capture");

    assert_eq!(capture.image.as_raw(), &[1, 2, 3, 4, 5, 6, 7, 8]);
    assert_eq!(capture.bounds, auv_driver::Rect::new(-10.0, 4.0, 2.0, 1.0));
    assert_eq!(capture.backend, "fixture");
  }

  #[test]
  fn selected_window_selector_keeps_hierarchical_parent_context() {
    let mut inputs = std::collections::BTreeMap::new();
    inputs.insert("title".to_string(), "Preferences".to_string());
    let selector = selected_window_selector(&auv_cli_invoke::InvokeCommandInput {
      command_id: "window.capture".to_string(),
      target_application_id: Some("com.example.app".to_string()),
      inputs,
      typed_args: None,
      dry_run: false,
      cancellation: Default::default(),
    });

    assert_eq!(selector.application, Some(proto::window_selector::Application::ApplicationBundleId("com.example.app".to_string())));
    assert_eq!(selector.window, Some(proto::window_selector::Window::TitleContains("Preferences".to_string())));
  }

  #[test]
  fn selected_window_point_projects_relative_coordinates_and_click_policy() {
    let mut inputs = std::collections::BTreeMap::new();
    inputs.insert("relative-x".to_string(), "0.25".to_string());
    inputs.insert("relative-y".to_string(), "0.5".to_string());
    inputs.insert("input-policy".to_string(), "background-only".to_string());
    inputs.insert("click-count".to_string(), "2".to_string());
    inputs.insert("click-interval-ms".to_string(), "80".to_string());
    let input = auv_cli_invoke::InvokeCommandInput {
      command_id: "input.clickWindowPoint".to_string(),
      target_application_id: None,
      inputs,
      typed_args: None,
      dry_run: false,
      cancellation: Default::default(),
    };
    let window = auv_driver::Window {
      reference: auv_driver::WindowRef {
        id: "window_fixture".to_string(),
      },
      title: None,
      app_name: None,
      app_bundle_id: None,
      process_id: None,
      frame: auv_driver::Rect::new(10.0, 20.0, 400.0, 200.0),
      coordinate_space: auv_driver::CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    };

    assert_eq!(selected_window_point(&input, &window).unwrap(), auv_driver::WindowPoint::new(100.0, 100.0));
    let options = selected_click_options(&input).unwrap();
    assert_eq!(options.policy, proto::InputPolicy::BackgroundOnly as i32);
    assert_eq!(options.click.as_ref().map(|click| click.count), Some(2));
    assert_eq!(options.click.and_then(|click| click.interval).map(|duration| duration.nanos), Some(80_000_000));
  }

  #[test]
  fn selected_screen_text_click_defaults_to_foreground_input() {
    let input = auv_cli_invoke::InvokeCommandInput {
      command_id: "screen.clickText".to_string(),
      target_application_id: None,
      inputs: std::collections::BTreeMap::new(),
      typed_args: None,
      dry_run: false,
      cancellation: Default::default(),
    };

    let options = selected_click_options(&input).expect("screen click options");
    assert_eq!(options.policy, proto::InputPolicy::ForegroundPreferred as i32);
    assert_eq!(options.click.as_ref().map(|click| click.count), Some(1));
  }

  #[test]
  fn selected_window_text_click_projects_screen_match_and_reuses_click_options() {
    let window = auv_driver::Window {
      reference: auv_driver::WindowRef {
        id: "window_fixture".to_string(),
      },
      title: None,
      app_name: None,
      app_bundle_id: None,
      process_id: None,
      frame: auv_driver::Rect::new(100.0, 200.0, 400.0, 300.0),
      coordinate_space: auv_driver::CoordinateSpace::Screen,
      is_main: true,
      is_visible: true,
    };
    let matched = auv_driver::OcrMatch {
      text: "Play".to_string(),
      confidence: 0.9,
      bounds: auv_driver::Rect::new(140.0, 250.0, 80.0, 20.0),
    };
    assert_eq!(matched_window_point(&window, &matched).unwrap(), auv_driver::WindowPoint::new(80.0, 60.0));

    let mut inputs = std::collections::BTreeMap::new();
    inputs.insert("input-policy".to_string(), "foreground-preferred".to_string());
    inputs.insert("click-count".to_string(), "3".to_string());
    inputs.insert("click-interval-ms".to_string(), "60".to_string());
    let input = auv_cli_invoke::InvokeCommandInput {
      command_id: "window.clickText".to_string(),
      target_application_id: None,
      inputs,
      typed_args: None,
      dry_run: false,
      cancellation: Default::default(),
    };
    let wire = selected_click_options(&input).unwrap();
    assert_eq!(
      driver_click_options_from_proto(&wire).unwrap(),
      auv_driver::ClickOptions {
        policy: auv_driver::InputPolicy::ForegroundPreferred,
        click: auv_driver::Click::Repeated {
          count: 3,
          interval: std::time::Duration::from_millis(60),
        },
        window_strategy: auv_driver::WindowClickStrategy::ChromiumCompatible,
      }
    );
  }
}
