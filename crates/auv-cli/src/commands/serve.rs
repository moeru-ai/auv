use std::io::Write as _;
use std::path::PathBuf;

use clap::Args;

/// Run the AUV daemon API in the foreground.
#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Every daemon has a local listener: the --local-socket paths, or otherwise an added Unix socket (Linux, macOS) or named pipe (Windows). It is published for discovery unless --no-register is set, and only callers with owner authority (launcher-token or os-user) can create pairing tokens.\n\nRemote (--remote-bind) listeners always require a paired Device bearer, including loopback addresses.\n\nExamples:\n  # Serve on the default local socket\n  auv serve\n\n  # Also accept paired Devices over the network\n  auv serve --remote-bind 0.0.0.0:9847\n\n  # Run a temporary daemon that does not replace the default one\n  auv serve --no-register --store-root /tmp/auv-scratch"
)]
pub struct ServeArgs {
  /// Fresh daemon instance UUID for launcher health verification.
  #[arg(long)]
  pub id: Option<uuid::Uuid>,
  /// Unix socket path (Linux, macOS) or named pipe (Windows, `\\.\pipe\NAME`
  /// or `NAME`) for the local listener. May be repeated. Without one, the
  /// daemon adds its own.
  #[arg(long = "local-socket", value_name = "PATH")]
  pub local_sockets: Vec<String>,

  /// TCP address for a remote listener, such as `127.0.0.1:9847`. May be
  /// repeated. Without one, the daemon opens no network listener.
  #[arg(long = "remote-bind", value_name = "IP:PORT")]
  pub remote_binds: Vec<std::net::SocketAddr>,

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

  /// How local callers may prove themselves, comma separated.
  /// `launcher-token` reads a 64-character hexadecimal token from the first
  /// line of stdin and requires --exit-on-stdin-close; the token never
  /// appears in argv or the environment, which other processes of the same
  /// user can read. `os-user` admits every process of this OS user.
  #[arg(long, value_enum, value_delimiter = ',', default_values_t = [LocalAuthMethod::OsUser])]
  pub local_auth_allowed_methods: Vec<LocalAuthMethod>,

  /// How remote callers may prove themselves, comma separated.
  #[arg(long, value_enum, value_delimiter = ',', default_values_t = [RemoteAuthMethod::PairedDevice])]
  pub remote_auth_allowed_methods: Vec<RemoteAuthMethod>,

  /// Load an operator-trusted custom Runner provider manifest. May be repeated.
  #[arg(long = "runner-provider", value_name = "PATH")]
  pub runner_providers: Vec<PathBuf>,
}

/// Command-line spelling of `auv_api_server::LocalAuthMethod`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum LocalAuthMethod {
  /// The token of the process that launched the daemon, read from stdin.
  LauncherToken,
  /// An active paired Device bearer.
  PairedDevice,
  /// Any process running as this OS user.
  OsUser,
}

/// Command-line spelling of `auv_api_server::RemoteAuthMethod`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum RemoteAuthMethod {
  /// An active paired Device bearer.
  PairedDevice,
}

pub async fn run(args: ServeArgs, project_root: &std::path::Path) -> Result<i32, String> {
  tracing::info!(version = env!("CARGO_PKG_VERSION"), "auv serve starting");
  if let Some(directory) = crate::logging::directory() {
    crate::logging::spawn_expiry(directory.to_path_buf());
  }
  let shutdown = tokio_util::sync::CancellationToken::new();
  let signal = shutdown.clone();
  tokio::spawn(async move {
    auv_api_server::termination::requested().await;
    signal.cancel();
  });
  // Read the token before the lifeline thread takes over stdin.
  let launcher_token = if args.local_auth_allowed_methods.contains(&LocalAuthMethod::LauncherToken) {
    if !args.exit_on_stdin_close {
      return Err("--local-auth-allowed-methods launcher-token requires --exit-on-stdin-close: the launcher holds the stdin pipe".into());
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
    Some(token)
  } else {
    None
  };
  let local_methods = args.local_auth_allowed_methods.iter().map(|method| match method {
    LocalAuthMethod::LauncherToken => auv_api_server::LocalAuthMethod::LauncherToken(launcher_token.clone().expect("read above")),
    LocalAuthMethod::PairedDevice => auv_api_server::LocalAuthMethod::PairedDevice,
    LocalAuthMethod::OsUser => auv_api_server::LocalAuthMethod::OsUser,
  });
  let mut local = auv_daemon::LocalListener::new()?.allowed_auth_methods(local_methods);
  for socket in &args.local_sockets {
    local = bind_local_socket(local, socket);
  }
  let remote_methods = args.remote_auth_allowed_methods.iter().map(|method| match method {
    RemoteAuthMethod::PairedDevice => auv_api_server::RemoteAuthMethod::PairedDevice,
  });
  let mut remote = auv_daemon::RemoteListener::new().allowed_auth_methods(remote_methods);
  for address in &args.remote_binds {
    remote = remote.bind(*address);
  }
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
    .local(local)
    .remote(remote)
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

#[cfg(unix)]
fn bind_local_socket(local: auv_daemon::LocalListener, socket: &str) -> auv_daemon::LocalListener {
  local.bind_unix(socket)
}

#[cfg(windows)]
fn bind_local_socket(local: auv_daemon::LocalListener, socket: &str) -> auv_daemon::LocalListener {
  local.bind_named_pipe(socket.strip_prefix(r"\\.\pipe\").unwrap_or(socket))
}

fn resolve_path(root: &std::path::Path, path: &std::path::Path) -> PathBuf {
  if path.is_absolute() {
    path.to_path_buf()
  } else {
    root.join(path)
  }
}
