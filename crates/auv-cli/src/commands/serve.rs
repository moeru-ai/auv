use std::io::Write as _;
use std::path::PathBuf;

use clap::Args;

/// Run the AUV daemon API in the foreground.
#[derive(Clone, Debug, Args)]
pub struct ServeArgs {
  /// Listener URI. May be repeated with unix://, npipe://, or http://IP:PORT.
  #[arg(long = "listen", value_name = "URI")]
  pub listeners: Vec<String>,

  /// Durable short-token and Device-bearer authentication store.
  #[arg(long, value_name = "PATH")]
  pub pairing_store: Option<PathBuf>,

  /// Root directory used for daemon control state and recorded runs.
  #[arg(long, value_name = "PATH")]
  pub store_root: Option<PathBuf>,

  /// Publish daemon discovery metadata at this path.
  #[arg(long, value_name = "PATH", conflicts_with = "no_discovery")]
  pub discovery_file: Option<PathBuf>,

  /// Do not publish this foreground daemon for implicit client discovery.
  #[arg(long)]
  pub no_discovery: bool,

  /// Stop the daemon after this many seconds without live Runners.
  #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
  pub daemon_idle_timeout: Option<u64>,

  /// Load an operator-trusted custom Runner provider manifest. May be repeated.
  #[arg(long = "runner-provider", value_name = "PATH")]
  pub runner_providers: Vec<PathBuf>,
}

pub async fn run(args: ServeArgs, project_root: &std::path::Path) -> Result<i32, String> {
  let listeners = if args.listeners.is_empty() {
    vec![auv_daemon::default_local_listener(
      args.discovery_file.as_deref(),
    )?]
  } else {
    args.listeners
  };
  let listeners =
    listeners.iter().map(|listener| auv_daemon::parse_listener(listener, args.pairing_store.is_some())).collect::<Result<Vec<_>, _>>()?;
  run_listeners(
    HostOptions {
      listeners,
      pairing_store: args.pairing_store,
      store_root: args.store_root,
      discovery_file: args.discovery_file,
      publish_discovery: !args.no_discovery,
      daemon_idle_timeout: args.daemon_idle_timeout,
      runner_providers: args.runner_providers,
    },
    project_root,
  )
  .await
}

pub(super) struct HostOptions {
  pub listeners: Vec<auv_daemon::ListenEndpoint>,
  pub pairing_store: Option<PathBuf>,
  pub store_root: Option<PathBuf>,
  pub discovery_file: Option<PathBuf>,
  pub publish_discovery: bool,
  pub daemon_idle_timeout: Option<u64>,
  pub runner_providers: Vec<PathBuf>,
}

pub(super) async fn run_listeners(options: HostOptions, project_root: &std::path::Path) -> Result<i32, String> {
  let store_root = options.store_root.map_or_else(|| project_root.join(".auv").join("store"), |path| resolve_path(project_root, &path));
  let providers = options
    .runner_providers
    .iter()
    .map(|path| {
      let path = resolve_path(project_root, path);
      auv_daemon::runner_provider::RunnerProviderConfig::load_json(&path)
        .map_err(|error| format!("failed to load --runner-provider {}: {error}", path.display()))
    })
    .collect::<Result<Vec<_>, _>>()?;
  let server = auv_daemon::Server::bind(auv_daemon::Config {
    listeners: options.listeners,
    first_party_runners: first_party_runner_runtimes(&store_root)?,
    store_root,
    pairing_store: options.pairing_store.map(|path| resolve_path(project_root, &path)),
    discovery_file: options.discovery_file,
    publish_discovery: options.publish_discovery,
    daemon_idle_timeout: options.daemon_idle_timeout.map(std::time::Duration::from_secs),
    runner_providers: providers,
  })
  .await?;
  for endpoint in server.endpoints() {
    println!("auv serve: {endpoint}");
  }
  std::io::stdout().flush().map_err(|error| format!("failed to flush daemon readiness line: {error}"))?;
  let shutdown = tokio_util::sync::CancellationToken::new();
  let signal = shutdown.clone();
  tokio::spawn(async move {
    if tokio::signal::ctrl_c().await.is_ok() {
      signal.cancel();
    }
  });
  server.serve(shutdown).await?;
  Ok(0)
}

fn resolve_path(root: &std::path::Path, path: &std::path::Path) -> PathBuf {
  if path.is_absolute() {
    path.to_path_buf()
  } else {
    root.join(path)
  }
}

fn first_party_runner_runtimes(store_root: &std::path::Path) -> Result<auv_daemon::runner_provider::FirstPartyRunnerRuntimes, String> {
  use auv_daemon::runner_provider::{ExecutableRunnerRuntime, RunnerRuntime};
  use std::collections::BTreeMap;
  let executable = std::env::current_exe().map_err(|error| format!("failed to resolve the auv executable for Runner hosting: {error}"))?;
  let runner_state_root = store_root.join("runner-state").join("auv.core.local");
  let runner_state_root =
    runner_state_root.to_str().ok_or_else(|| format!("local Runner state path is not valid UTF-8: {}", runner_state_root.display()))?;
  let environment = BTreeMap::from([(crate::runner::STATE_ROOT_ENV.to_string(), runner_state_root.to_string())]);
  Ok(auv_daemon::runner_provider::FirstPartyRunnerRuntimes {
    local_driver: Some(RunnerRuntime::Executable(ExecutableRunnerRuntime {
      executable,
      arguments: vec![
        crate::runner::INTERNAL_SENTINEL.to_string(),
        crate::runner::LOCAL_DRIVER_ROLE.to_string(),
      ],
      working_directory: None,
      environment,
    })),
  })
}
