use std::io::Write as _;
use std::path::PathBuf;

use clap::Args;

/// Run the AUV daemon API in the foreground.
#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Every daemon has an owner-only local channel: a configured unix:// or npipe:// listener, or otherwise an added Unix socket (Linux, macOS) or named pipe (Windows). It is published for discovery unless --no-register is set, and only it can create pairing tokens.\n\nhttp:// listeners always require a paired Device bearer, including loopback addresses.\n\nExamples:\n  # Serve on the default local socket\n  auv serve\n\n  # Also accept paired Devices over the network\n  auv serve --listen http://0.0.0.0:9847\n\n  # Run a temporary daemon that does not replace the default one\n  auv serve --no-register --store-root /tmp/auv-scratch"
)]
pub struct ServeArgs {
  /// Fresh daemon instance UUID for launcher health verification.
  #[arg(long)]
  pub id: Option<uuid::Uuid>,
  /// Additional listener URI. May be repeated with unix://, npipe://, or
  /// http://IP:PORT; http:// listeners require a paired Device bearer.
  #[arg(long = "listen", value_name = "URI")]
  pub listeners: Vec<String>,

  /// Pairing token and Device-bearer store [default: <STORE_ROOT>/pairings.json].
  #[arg(long, value_name = "PATH")]
  pub pairing_store: Option<PathBuf>,

  /// Root directory used for daemon control state and recorded runs.
  #[arg(long, value_name = "PATH")]
  pub store_root: Option<PathBuf>,

  /// Publish daemon discovery metadata at this path.
  #[arg(long, value_name = "PATH", conflicts_with = "no_register")]
  pub discovery_file: Option<PathBuf>,

  /// Do not register as the default local daemon: bind the owner socket at a
  /// private path and publish no discovery descriptor.
  #[arg(long)]
  pub no_register: bool,

  /// Stop the daemon after this many seconds without live Runners.
  #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
  pub daemon_idle_timeout: Option<u64>,

  /// Stop the daemon when stdin reaches end of file. An owning process keeps
  /// the write end open, so the daemon follows it out even when it is killed.
  #[arg(long)]
  pub exit_on_stdin_close: bool,

  /// Which local processes may call the owner socket or named pipe.
  /// `launcher-only` reads a 64-character hexadecimal launcher token from the
  /// first line of stdin; callers must present it as `authorization: Bearer`.
  /// The token never appears in argv or the environment, which other
  /// processes of the same user can read.
  #[arg(long, value_enum, default_value_t = LocalClients::AnyProcessOfOsUser)]
  pub local_clients: LocalClients,

  /// Load an operator-trusted custom Runner provider manifest. May be repeated.
  #[arg(long = "runner-provider", value_name = "PATH")]
  pub runner_providers: Vec<PathBuf>,
}

/// Command-line spelling of `auv_api_server::LocalClientPolicy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum LocalClients {
  /// Only the process that launched the daemon. Requires --exit-on-stdin-close.
  LauncherOnly,
  /// Any process running as this OS user.
  AnyProcessOfOsUser,
}

pub async fn run(args: ServeArgs, project_root: &std::path::Path) -> Result<i32, String> {
  tracing::info!(version = env!("CARGO_PKG_VERSION"), "auv serve starting");
  if let Some(directory) = crate::logging::directory() {
    crate::logging::spawn_expiry(directory.to_path_buf());
  }
  let listeners = args.listeners.iter().map(|listener| auv_daemon::parse_listener(listener)).collect::<Result<Vec<_>, _>>()?;
  let shutdown = tokio_util::sync::CancellationToken::new();
  let signal = shutdown.clone();
  tokio::spawn(async move {
    auv_api_server::termination::requested().await;
    signal.cancel();
  });
  // Read the token before the lifeline thread takes over stdin.
  let local_clients = match args.local_clients {
    LocalClients::AnyProcessOfOsUser => auv_api_server::LocalClientPolicy::AnyProcessOfOsUser,
    LocalClients::LauncherOnly => {
      if !args.exit_on_stdin_close {
        return Err("--local-clients launcher-only requires --exit-on-stdin-close: the launcher holds the stdin pipe".into());
      }
      use std::io::BufRead as _;
      let mut line = zeroize::Zeroizing::new(String::new());
      std::io::stdin().lock().read_line(&mut line).map_err(|error| format!("failed to read the launcher token from stdin: {error}"))?;
      let token = auv_api_server::LauncherToken::parse(line.trim_end_matches(['\r', '\n']))?;
      #[cfg(target_os = "linux")]
      // A non-dumpable process hides its memory and /proc environment from other
      // processes of the same user, which could otherwise read the token back.
      rustix::process::set_dumpable_behavior(rustix::process::DumpableBehavior::NotDumpable)
        .map_err(|error| format!("failed to protect the launcher token in memory: {error}"))?;
      auv_api_server::LocalClientPolicy::LauncherOnly(token)
    }
  };
  if args.exit_on_stdin_close {
    let signal = shutdown.clone();
    // A blocking stdin read cannot be cancelled, so it runs on a detached
    // thread that process exit discards instead of a runtime blocking task.
    std::thread::spawn(move || {
      let _ = std::io::copy(&mut std::io::stdin().lock(), &mut std::io::sink());
      signal.cancel();
    });
  }

  let store_root = args.store_root.map_or_else(|| project_root.join(".auv").join("store"), |path| resolve_path(project_root, &path));
  let providers = args
    .runner_providers
    .iter()
    .map(|path| {
      let path = resolve_path(project_root, path);
      auv_daemon::runner_provider::RunnerProviderConfig::load_json(&path)
        .map_err(|error| format!("failed to load --runner-provider {}: {error}", path.display()))
    })
    .collect::<Result<Vec<_>, _>>()?;
  let executable = std::env::current_exe().map_err(|error| format!("failed to resolve the auv executable for Runner hosting: {error}"))?;
  let mut config = auv_daemon::Config::new(&store_root)?
    .local_clients(local_clients)
    .first_party_runners(auv_daemon::runner_provider::FirstPartyRunnerRuntimes::local_driver(executable, &store_root)?);
  if let Some(path) = args.pairing_store {
    config = config.pairing_store(resolve_path(project_root, &path));
  }
  if !args.no_register {
    config = config.register(args.discovery_file);
  }
  if let Some(id) = args.id {
    config = config.id(id);
  }
  if let Some(seconds) = args.daemon_idle_timeout {
    config = config.daemon_idle_timeout(std::time::Duration::from_secs(seconds));
  }
  for listener in listeners {
    config = config.listen(listener);
  }
  for provider in providers {
    config = config.runner_provider(provider);
  }
  let server = auv_daemon::Server::bind(config).await?;

  for endpoint in server.endpoints() {
    println!("auv serve: {endpoint}");
  }

  std::io::stdout().flush().map_err(|error| format!("failed to flush daemon listener log: {error}"))?;
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
