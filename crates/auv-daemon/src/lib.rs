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
  /// Unix socket or named-pipe listener and how its callers authenticate.
  pub local: LocalListener,
  /// TCP listeners and how their callers authenticate. No endpoints means no
  /// network listener.
  pub remote: RemoteListener,
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
  /// Operator-trusted custom Runner providers.
  pub runner_providers: Vec<runner_provider::RunnerProviderConfig>,
  /// First-party Runner runtime definitions.
  pub first_party_runners: runner_provider::FirstPartyRunnerRuntimes,
}

impl Config {
  /// Defaults: [`LocalListener::new`] (a private socket or pipe that only
  /// this process's launcher token opens), no network listener, pairing store
  /// `<store_root>/pairings.json`, not registered, no idle timeout and no
  /// Runner providers. Use [`Server::owner_connection`] to connect.
  pub fn new(store_root: impl Into<PathBuf>) -> Result<Self, String> {
    let store_root = store_root.into();
    Ok(Self {
      id: None,
      local: LocalListener::new()?,
      remote: RemoteListener::new(),
      pairing_store: store_root.join("pairings.json"),
      store_root,
      discovery_file: None,
      register: false,
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      first_party_runners: runner_provider::FirstPartyRunnerRuntimes::default(),
    })
  }

  /// Sets the instance identity reported through health.
  pub fn id(mut self, id: uuid::Uuid) -> Self {
    self.id = Some(id);
    self
  }

  /// Replaces the Unix socket or named-pipe listener settings.
  pub fn local(mut self, local: LocalListener) -> Self {
    self.local = local;
    self
  }

  /// Replaces the TCP listener settings.
  pub fn remote(mut self, remote: RemoteListener) -> Self {
    self.remote = remote;
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

/// Listener settings shared by the local and remote sides: where to bind and
/// how callers may prove themselves. [`LocalListener`] and [`RemoteListener`]
/// fix the endpoint and method types.
#[derive(Clone)]
#[non_exhaustive]
pub struct ListenerConfig<E, M> {
  /// Endpoints to bind.
  pub endpoints: Vec<E>,
  /// How callers may prove themselves; validated when the server binds.
  pub allowed_auth_methods: Vec<M>,
}

impl<E, M> ListenerConfig<E, M> {
  /// Adds one endpoint to bind.
  pub fn bind(mut self, endpoint: E) -> Self {
    self.endpoints.push(endpoint);
    self
  }

  /// Replaces how callers may prove themselves.
  pub fn allowed_auth_methods(mut self, methods: impl IntoIterator<Item = M>) -> Self {
    self.allowed_auth_methods = methods.into_iter().collect();
    self
  }
}

/// A Unix socket path or Windows named-pipe name for the local listener.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum LocalEndpoint {
  #[cfg(unix)]
  /// Unix domain socket path.
  Unix(PathBuf),
  #[cfg(windows)]
  /// Named-pipe name without the `\\.\pipe\` prefix.
  NamedPipe(String),
}

/// Local (Unix socket or named pipe) listener settings.
pub type LocalListener = ListenerConfig<LocalEndpoint, auv_api_server::LocalAuthMethod>;
/// Remote (TCP) listener settings. Every remote caller needs a paired bearer.
pub type RemoteListener = ListenerConfig<std::net::SocketAddr, auv_api_server::RemoteAuthMethod>;

impl LocalListener {
  /// No endpoints, so binding adds a private socket or pipe, and only the
  /// launcher token, freshly generated, admits callers.
  pub fn new() -> Result<Self, String> {
    Ok(Self {
      endpoints: Vec::new(),
      allowed_auth_methods: vec![auv_api_server::LocalAuthMethod::LauncherToken(
        auv_api_server::LauncherToken::generate()?,
      )],
    })
  }

  /// Adds a Unix socket to bind.
  #[cfg(unix)]
  pub fn bind_unix(self, path: impl Into<PathBuf>) -> Self {
    self.bind(LocalEndpoint::Unix(path.into()))
  }

  /// Adds a named pipe to bind.
  #[cfg(windows)]
  pub fn bind_named_pipe(self, name: impl Into<String>) -> Self {
    self.bind(LocalEndpoint::NamedPipe(name.into()))
  }

  fn launcher_token(&self) -> Option<&auv_api_server::LauncherToken> {
    self.allowed_auth_methods.iter().find_map(|method| match method {
      auv_api_server::LocalAuthMethod::LauncherToken(token) => Some(token),
      _ => None,
    })
  }
}

impl LocalEndpoint {
  fn to_listen_endpoint(&self) -> ListenEndpoint {
    match self {
      #[cfg(unix)]
      Self::Unix(path) => ListenEndpoint::Unix { path: path.clone() },
      #[cfg(windows)]
      Self::NamedPipe(name) => ListenEndpoint::NamedPipe { name: name.clone() },
    }
  }
}

impl RemoteListener {
  /// No endpoints, so no network listener, and paired Device bearers only.
  pub fn new() -> Self {
    Self {
      endpoints: Vec::new(),
      allowed_auth_methods: vec![auv_api_server::RemoteAuthMethod::PairedDevice],
    }
  }
}

impl Default for RemoteListener {
  fn default() -> Self {
    Self::new()
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

/// Returns the local listener the daemon adds when none is configured. It is
/// the daemon's owner channel: first pairing tokens, discovery, and
/// executable Runner callbacks all depend on it.
fn owner_listener(discovery_file: Option<&Path>, register: bool) -> Result<Option<ListenEndpoint>, String> {
  #[cfg(unix)]
  {
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
    // user, including beside `--remote-bind` network listeners.
    Ok(Some(ListenEndpoint::NamedPipe {
      name: format!("auv-{}", uuid::Uuid::now_v7()),
    }))
  }
  #[cfg(not(any(unix, windows)))]
  {
    let _ = (discovery_file, register);
    Ok(None)
  }
}

/// Bound daemon server with discovery publication and graceful shutdown.
pub struct Server {
  inner: auv_api_server::server::Server,
  launcher_token: Option<auv_api_server::LauncherToken>,
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
    let mut listeners = config.local.endpoints.iter().map(LocalEndpoint::to_listen_endpoint).collect::<Vec<_>>();
    if listeners.is_empty() {
      listeners.extend(owner_listener(config.discovery_file.as_deref(), config.register)?);
    }
    listeners.extend(config.remote.endpoints.iter().map(|address| ListenEndpoint::Remote {
      host: address.ip().to_string(),
      port: address.port(),
    }));
    let launcher_token = config.local.launcher_token().cloned();
    let mut listeners = listeners.into_iter();
    let listen = listeners.next().ok_or_else(|| "daemon requires at least one listener".to_string())?;
    let store_root = config.store_root;
    let published_store_root = store_root.clone();
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    let device_local = std::sync::Arc::new(devices::LocalState::open(&store_root, pairing.clone())?);
    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    let local_state_for_daemon = std::sync::Arc::clone(&device_local);
    let runner_providers = config.runner_providers;
    // Runners call back with the launcher's authority.
    let parent_credential = config.local.launcher_token().map(|token| zeroize::Zeroizing::new(token.expose().to_owned()));
    let first_party_runners = config.first_party_runners;
    let bound = auv_api_server::server::Server::bind_with(
      auv_api_server::server::BindConfig {
        id: config.id.unwrap_or_else(uuid::Uuid::now_v7).to_string(),
        listen,
        additional_listeners: listeners.collect(),
        pairing,
        daemon_idle_timeout: config.daemon_idle_timeout,
        local_auth_methods: config.local.allowed_auth_methods,
        remote_auth_methods: config.remote.allowed_auth_methods,
      },
      move |parent_endpoint| {
        Ok(std::sync::Arc::new(daemon::Daemon::open_with_runner_providers_and_parent_endpoint(
          &store_root,
          parent_endpoint,
          parent_credential,
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
      launcher_token,
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
    Some(OwnerConnection {
      endpoint: self.inner.discovery_endpoint()?.to_string(),
      credential: self.launcher_token.as_ref().map(|token| zeroize::Zeroizing::new(token.expose().to_owned())),
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
