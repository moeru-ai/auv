//! Server-side daemon SDK: listener configuration, discovery publication,
//! serving, and shutdown lifecycle.

mod daemon;
mod devices;

mod discovery;
#[cfg(windows)]
mod durable_windows;
mod pairing;
mod resource_id;

use std::path::{Path, PathBuf};

pub use auv_api_server::server::{BoundEndpoint, ListenEndpoint};
pub use daemon::runner_provider;

/// Configuration for binding a daemon server and its owned state.
///
/// Start from [`Config::new`] and adjust it with the chainable setters, so new
/// options never break existing callers.
#[non_exhaustive]
pub struct Config {
  /// Fresh identity for this daemon instance; generated when omitted.
  pub id: Option<uuid::Uuid>,
  /// Protocol listeners served by this daemon.
  pub listeners: Vec<ListenEndpoint>,
  /// Root for daemon state and durable Run records.
  pub store_root: PathBuf,
  /// Persistent pairing database. Pairing is always available; paired TCP
  /// listeners and owner-issued tokens share this store.
  pub pairing_store: PathBuf,
  /// Optional discovery descriptor path.
  pub discovery_file: Option<PathBuf>,
  /// Whether this daemon registers as the caller's default local daemon. A
  /// registered daemon binds its owner socket next to the discovery descriptor
  /// and publishes that descriptor; an unregistered one uses a private owner
  /// socket and publishes nothing, so it never replaces the default daemon.
  pub register: bool,
  /// Optional shutdown deadline after all Runners become idle.
  pub daemon_idle_timeout: Option<std::time::Duration>,
  /// Which local processes may call the owner socket or named pipe.
  pub local_clients: auv_api_server::LocalClientPolicy,
  /// Operator-trusted custom Runner providers.
  pub runner_providers: Vec<runner_provider::RunnerProviderConfig>,
  /// First-party Runner runtime definitions.
  pub first_party_runners: runner_provider::FirstPartyRunnerRuntimes,
}

impl Config {
  /// Defaults: no extra listeners (a private owner socket or pipe is added),
  /// pairing store `<store_root>/pairings.json`, not registered, no idle
  /// timeout, no Runner providers, and [`LocalClientPolicy::LauncherOnly`]
  /// with a fresh token, so only this process can call the owner listener.
  /// Use [`Server::owner_connection`] to connect.
  ///
  /// [`LocalClientPolicy::LauncherOnly`]: auv_api_server::LocalClientPolicy::LauncherOnly
  pub fn new(store_root: impl Into<PathBuf>) -> Result<Self, String> {
    let store_root = store_root.into();
    Ok(Self {
      id: None,
      listeners: Vec::new(),
      pairing_store: store_root.join("pairings.json"),
      store_root,
      discovery_file: None,
      register: false,
      daemon_idle_timeout: None,
      local_clients: auv_api_server::LocalClientPolicy::LauncherOnly(auv_api_server::LauncherToken::generate()?),
      runner_providers: Vec::new(),
      first_party_runners: runner_provider::FirstPartyRunnerRuntimes::default(),
    })
  }

  /// Sets the instance identity reported through health.
  pub fn id(mut self, id: uuid::Uuid) -> Self {
    self.id = Some(id);
    self
  }

  /// Adds one listener; `http://` listeners always require a paired bearer.
  pub fn listen(mut self, listener: ListenEndpoint) -> Self {
    self.listeners.push(listener);
    self
  }

  /// Replaces the pairing token and Device-bearer store.
  pub fn pairing_store(mut self, path: impl Into<PathBuf>) -> Self {
    self.pairing_store = path.into();
    self
  }

  /// Registers as the default local daemon, publishing discovery metadata at
  /// `discovery_file` or the default path.
  pub fn register(mut self, discovery_file: Option<PathBuf>) -> Self {
    self.register = true;
    self.discovery_file = discovery_file;
    self
  }

  /// Stops the daemon after this long without live Runners.
  pub fn daemon_idle_timeout(mut self, timeout: std::time::Duration) -> Self {
    self.daemon_idle_timeout = Some(timeout);
    self
  }

  /// Chooses which local processes may call the owner listener.
  pub fn local_clients(mut self, policy: auv_api_server::LocalClientPolicy) -> Self {
    self.local_clients = policy;
    self
  }

  /// Adds one operator-trusted custom Runner provider.
  pub fn runner_provider(mut self, provider: runner_provider::RunnerProviderConfig) -> Self {
    self.runner_providers.push(provider);
    self
  }

  /// Sets the built-in Runners, for example
  /// [`runner_provider::FirstPartyRunnerRuntimes::local_driver`].
  pub fn first_party_runners(mut self, runners: runner_provider::FirstPartyRunnerRuntimes) -> Self {
    self.first_party_runners = runners;
    self
  }
}

/// Endpoint and credential that reach a daemon's owner listener: the Rust
/// counterpart of the JS SDK `daemon.connectionOptions`. Pass both to
/// [`auv::Client::connect`].
#[derive(Clone)]
pub struct OwnerConnection {
  /// `unix://` or `npipe://` URI of the owner listener.
  pub endpoint: String,
  /// Launcher token, or `None` under `AnyProcessOfOsUser`.
  pub credential: Option<zeroize::Zeroizing<String>>,
}

impl std::fmt::Debug for OwnerConnection {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    formatter
      .debug_struct("OwnerConnection")
      .field("endpoint", &self.endpoint)
      .field("credential", &self.credential.as_ref().map(|_| "[REDACTED]"))
      .finish()
  }
}

/// Parses one listener URI. Every `http://` listener requires a paired Device
/// bearer, including loopback; owner authority comes only from local IPC.
pub fn parse_listener(listener: &str) -> Result<ListenEndpoint, String> {
  if let Some(authority) = listener.strip_prefix("http://") {
    let address = authority
      .parse::<std::net::SocketAddr>()
      .map_err(|error| format!("invalid listener URI {listener:?}; expected http://IP:PORT: {error}"))?;
    return Ok(ListenEndpoint::Remote {
      host: address.ip().to_string(),
      port: address.port(),
    });
  }
  match listener.parse::<auv_api_client::ConnectEndpoint>().map_err(|error| format!("invalid listener URI: {error}"))? {
    auv_api_client::ConnectEndpoint::Tcp(_) => Err(format!("invalid listener URI {listener:?}; expected http://IP:PORT")),
    #[cfg(unix)]
    auv_api_client::ConnectEndpoint::Unix(path) => Ok(ListenEndpoint::Unix { path }),
    #[cfg(windows)]
    auv_api_client::ConnectEndpoint::NamedPipe(name) => Ok(ListenEndpoint::NamedPipe { name }),
  }
}

/// Returns the owner-checked IPC listener the daemon adds to the configured
/// ones, if any. It is the daemon's owner channel: first pairing tokens,
/// discovery, and executable Runner callbacks all depend on it.
fn owner_listener(listeners: &[ListenEndpoint], discovery_file: Option<&Path>, register: bool) -> Result<Option<ListenEndpoint>, String> {
  #[cfg(unix)]
  {
    if listeners.iter().any(|listener| matches!(listener, ListenEndpoint::Unix { .. })) {
      return Ok(None);
    }
    let path = if register {
      let descriptor = discovery_file.map(Path::to_path_buf).map_or_else(discovery::default_path, Ok).map_err(|error| error.to_string())?;
      let parent = descriptor.parent().ok_or_else(|| format!("daemon descriptor path has no parent: {}", descriptor.display()))?;
      parent.join("auv.sock")
    } else {
      // NOTICE: the private socket lives in the temp directory because Unix
      // socket paths are limited to about 104 bytes and store roots can be deep.
      std::env::temp_dir().join(format!("auv-{}-{:x}.sock", std::process::id(), uuid::Uuid::now_v7().as_u128() as u64))
    };
    Ok(Some(ListenEndpoint::Unix { path }))
  }
  #[cfg(windows)]
  {
    let _ = (discovery_file, register);
    // The daemon always runs as its user; the LocalSystem Helper Host is a
    // separate service. An owner pipe is therefore always reachable by that
    // user, including beside `--listen http://...` network listeners.
    if listeners.iter().any(|listener| matches!(listener, ListenEndpoint::NamedPipe { .. })) {
      return Ok(None);
    }
    Ok(Some(ListenEndpoint::NamedPipe {
      name: format!("auv-{}", uuid::Uuid::now_v7()),
    }))
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (listeners, discovery_file, register);
    Ok(None)
  }
}

/// Bound daemon server with discovery publication and graceful shutdown.
pub struct Server {
  inner: auv_api_server::server::Server,
  local_clients: auv_api_server::LocalClientPolicy,
  #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
  device_local: std::sync::Arc<devices::LocalState>,
  discovery_file: Option<PathBuf>,
  register: bool,
  store_root: PathBuf,
}

impl Server {
  /// Binds all configured listeners and opens daemon-owned state.
  pub async fn bind(config: Config) -> Result<Self, String> {
    let pairing = pairing::PairingStore::open(config.pairing_store);
    let pairing = Some(std::sync::Arc::new(pairing.map_err(|error| format!("failed to open pairing store: {error}"))?)
      as std::sync::Arc<dyn auv_api_server::control::Pairing>);
    let owner = owner_listener(&config.listeners, config.discovery_file.as_deref(), config.register)?;
    let mut listeners = config.listeners.into_iter().chain(owner);
    let listen = listeners.next().ok_or_else(|| "daemon requires at least one listener".to_string())?;
    let store_root = config.store_root;
    let published_store_root = store_root.clone();
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    let device_local = std::sync::Arc::new(devices::LocalState::open(&store_root, pairing.clone())?);
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    let local_state_for_daemon = std::sync::Arc::clone(&device_local);
    let runner_providers = config.runner_providers;
    let first_party_runners = config.first_party_runners;
    let bound = auv_api_server::server::Server::bind_with(
      auv_api_server::server::BindConfig {
        id: config.id.unwrap_or_else(uuid::Uuid::now_v7).to_string(),
        listen,
        additional_listeners: listeners.collect(),
        pairing,
        daemon_idle_timeout: config.daemon_idle_timeout,
        local_clients: config.local_clients.clone(),
      },
      move |parent_endpoint| {
        Ok(std::sync::Arc::new(daemon::Daemon::open_with_runner_providers_and_parent_endpoint(
          &store_root,
          parent_endpoint,
          first_party_runners,
          runner_providers,
          #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
          local_state_for_daemon,
        )?))
      },
    )
    .await?;
    Ok(Self {
      inner: bound,
      local_clients: config.local_clients,
      #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
      device_local,
      discovery_file: config.discovery_file,
      register: config.register,
      store_root: published_store_root,
    })
  }

  /// Returns the primary bound endpoint.
  pub fn endpoint(&self) -> &BoundEndpoint {
    self.inner.endpoint()
  }
  /// Returns every bound endpoint.
  pub fn endpoints(&self) -> &[BoundEndpoint] {
    self.inner.endpoints()
  }
  /// Endpoint and credential for this process to call its own daemon.
  pub fn owner_connection(&self) -> Option<OwnerConnection> {
    let endpoint = self.inner.discovery_endpoint()?.to_string();
    let credential = match &self.local_clients {
      auv_api_server::LocalClientPolicy::LauncherOnly(token) | auv_api_server::LocalClientPolicy::LauncherAndPaired(token) => {
        Some(zeroize::Zeroizing::new(token.expose().to_owned()))
      }
      _ => None,
    };
    Some(OwnerConnection {
      endpoint,
      credential,
    })
  }

  /// Returns the endpoint published for implicit discovery, when available.
  pub fn discovery_endpoint(&self) -> Option<&BoundEndpoint> {
    self.inner.discovery_endpoint()
  }

  /// Serves until the supplied cancellation token fires or a listener fails.
  pub async fn serve(self, shutdown: tokio_util::sync::CancellationToken) -> Result<(), String> {
    let _descriptor = if self.register {
      let path = self.discovery_file.map_or_else(discovery::default_path, Ok).map_err(|error| error.to_string())?;
      self
        .inner
        .discovery_endpoint()
        .map(|endpoint| {
          tracing::info!(endpoint = %endpoint, discovery = %path.display(), "publishing daemon discovery");
          discovery::PublishedDescriptor::publish(path, endpoint.to_string(), &self.store_root)
        })
        .transpose()?
    } else {
      None
    };

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    {
      // These independent listeners share state but never share route tables.
      // If either fails, stop the other and wait for its socket cleanup.
      let mut paired = Box::pin(self.inner.serve(shutdown.clone()));
      let mut local = Box::pin(self.device_local.serve(shutdown.clone()));
      tokio::select! {
        result = &mut paired => {
          shutdown.cancel();
          let local_result = local.await;
          result?;
          local_result
        }
        result = &mut local => {
          shutdown.cancel();
          let paired_result = paired.await;
          result?;
          paired_result
        }
      }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    self.inner.serve(shutdown).await
  }
}

#[cfg(test)]
#[path = "server_test.rs"]
mod tests;
