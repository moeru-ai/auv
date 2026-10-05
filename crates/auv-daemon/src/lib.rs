//! Server-side daemon SDK: listener configuration, discovery publication,
//! serving, and shutdown lifecycle.

mod daemon;
mod devices;

/// Fixed ProgramData root for the LocalSystem-owned Windows Device entry store.
#[cfg(target_os = "windows")]
pub fn windows_device_entry_store_root() -> Result<std::path::PathBuf, String> {
  devices::windows_store_root()
}

/// Issue one short-lived pairing token before starting the Windows service.
/// The caller must run as LocalSystem in Session 0. The protected store's
/// lifetime lock prevents concurrent mutation by a running daemon, and only
/// the token digest is persisted. The plaintext must stay on the target host
/// until a client consumes it once.
#[cfg(target_os = "windows")]
pub fn issue_windows_bootstrap_token() -> Result<String, String> {
  let store = pairing::PairingStore::open_system(windows_device_entry_store_root()?.join("pairings.json"))
    .map_err(|error| format!("failed to open protected pairing store: {error}"))?;

  let token = store
    .issue_token(Some(std::time::Duration::from_secs(20 * 60)))
    .map_err(|error| format!("failed to issue bootstrap pairing token: {error}"))?;
  Ok(token.expose_once())
}

mod discovery;
mod pairing;
mod resource_id;

use std::path::{Path, PathBuf};

pub use auv_api_server::server::{BoundEndpoint, ListenEndpoint};
pub use daemon::runner_provider;

/// Configuration for binding a daemon server and its owned state.
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
  /// Operator-trusted custom Runner providers.
  pub runner_providers: Vec<runner_provider::RunnerProviderConfig>,
  /// First-party Runner runtime definitions.
  pub first_party_runners: runner_provider::FirstPartyRunnerRuntimes,
  /// Admit the privileged Windows Device entry host only from SCM mode.
  #[cfg(windows)]
  pub enable_device_entry: bool,
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
    // TODO(windows-owner-listener): Windows adds its owner pipe only when no
    // listener was configured. Always binding one is deferred because the
    // LocalSystem service's pipe owner is SYSTEM, which ordinary users cannot
    // reach; see session-api/2026-10-05-windows-helper-and-daemon-service-architecture-research.md.
    Ok(listeners.is_empty().then(|| ListenEndpoint::NamedPipe {
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
  #[cfg(any(target_os = "linux", target_os = "macos"))]
  device_local: std::sync::Arc<devices::LocalState>,
  #[cfg(windows)]
  device_local: Option<std::sync::Arc<devices::LocalState>>,
  discovery_file: Option<PathBuf>,
  register: bool,
}

impl Server {
  /// Binds all configured listeners and opens daemon-owned state.
  pub async fn bind(config: Config) -> Result<Self, String> {
    #[cfg(windows)]
    let pairing = if config.enable_device_entry {
      pairing::PairingStore::open_system(config.pairing_store)
    } else {
      pairing::PairingStore::open(config.pairing_store)
    };

    #[cfg(not(windows))]
    let pairing = pairing::PairingStore::open(config.pairing_store);
    let pairing = Some(std::sync::Arc::new(pairing.map_err(|error| format!("failed to open pairing store: {error}"))?)
      as std::sync::Arc<dyn auv_api_server::control::Pairing>);
    let owner = owner_listener(&config.listeners, config.discovery_file.as_deref(), config.register)?;
    let mut listeners = config.listeners.into_iter().chain(owner);
    let listen = listeners.next().ok_or_else(|| "daemon requires at least one listener".to_string())?;
    let store_root = config.store_root;
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let device_local = std::sync::Arc::new(devices::LocalState::open(&store_root, pairing.clone())?);
    #[cfg(windows)]
    let device_local = if config.enable_device_entry {
      Some(std::sync::Arc::new(devices::LocalState::open(&store_root, pairing.clone())?))
    } else {
      None
    };

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    let local_state_for_daemon = std::sync::Arc::clone(&device_local);
    #[cfg(windows)]
    let local_state_for_daemon = device_local.clone();
    let runner_providers = config.runner_providers;
    let first_party_runners = config.first_party_runners;
    let bound = auv_api_server::server::Server::bind_with(
      auv_api_server::server::BindConfig {
        id: config.id.unwrap_or_else(uuid::Uuid::now_v7).to_string(),
        listen,
        additional_listeners: listeners.collect(),
        pairing,
        daemon_idle_timeout: config.daemon_idle_timeout,
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
      #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
      device_local,
      discovery_file: config.discovery_file,
      register: config.register,
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
  /// Returns the endpoint published for implicit discovery, when available.
  pub fn discovery_endpoint(&self) -> Option<&BoundEndpoint> {
    self.inner.discovery_endpoint()
  }

  /// Serves until the supplied cancellation token fires or a listener fails.
  pub async fn serve(self, shutdown: tokio_util::sync::CancellationToken) -> Result<(), String> {
    let _descriptor = if self.register {
      let path = self.discovery_file.map_or_else(discovery::default_path, Ok).map_err(|error| error.to_string())?;
      self.inner.discovery_endpoint().map(|endpoint| discovery::PublishedDescriptor::publish(path, endpoint.to_string())).transpose()?
    } else {
      None
    };

    #[cfg(any(target_os = "linux", target_os = "macos"))]
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

    #[cfg(windows)]
    {
      if let Some(device_local) = self.device_local {
        let mut paired = Box::pin(self.inner.serve(shutdown.clone()));
        let mut local = Box::pin(device_local.serve(shutdown.clone()));
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
      } else {
        self.inner.serve(shutdown).await
      }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    self.inner.serve(shutdown).await
  }
}

#[cfg(test)]
#[path = "server_test.rs"]
mod tests;
