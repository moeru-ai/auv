//! Server-side daemon SDK: listener configuration, discovery publication,
//! serving, and shutdown lifecycle.

mod daemon;
// TODO(device-session-windows-stack): Windows compilation remains deferred to
// the Windows adapter PR because the shared stores require its protected
// LocalSystem persistence and session-host capability.
#[cfg(unix)]
mod devices;
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
  /// Optional persistent pairing database.
  pub pairing_store: Option<PathBuf>,
  /// Optional discovery descriptor path.
  pub discovery_file: Option<PathBuf>,
  /// Whether serving publishes a discovery descriptor.
  pub publish_discovery: bool,
  /// Optional shutdown deadline after all Runners become idle.
  pub daemon_idle_timeout: Option<std::time::Duration>,
  /// Operator-trusted custom Runner providers.
  pub runner_providers: Vec<runner_provider::RunnerProviderConfig>,
  /// First-party Runner runtime definitions.
  pub first_party_runners: runner_provider::FirstPartyRunnerRuntimes,
}

/// Returns the platform-default local listener URI.
pub fn default_local_listener(discovery_file: Option<&Path>) -> Result<String, String> {
  #[cfg(unix)]
  {
    let descriptor = discovery_file.map(Path::to_path_buf).map_or_else(discovery::default_path, Ok).map_err(|error| error.to_string())?;
    let parent = descriptor.parent().ok_or_else(|| format!("daemon descriptor path has no parent: {}", descriptor.display()))?;
    Ok(format!("unix://{}", parent.join("auv.sock").display()))
  }
  #[cfg(windows)]
  {
    let _ = discovery_file;
    Ok(format!("npipe://./pipe/auv-{}", uuid::Uuid::now_v7()))
  }
  #[cfg(not(any(unix, windows)))]
  Ok(format!("http://{}:{}", auv_api_server::server::DEFAULT_API_HOST, auv_api_server::server::DEFAULT_API_PORT))
}

/// Parses one listener URI and applies the remote-listener trust policy.
pub fn parse_listener(listener: &str, paired_tcp: bool) -> Result<ListenEndpoint, String> {
  if let Some(authority) = listener.strip_prefix("http://") {
    let address = authority
      .parse::<std::net::SocketAddr>()
      .map_err(|error| format!("invalid listener URI {listener:?}; expected http://IP:PORT: {error}"))?;
    if paired_tcp || !address.ip().is_loopback() {
      return Ok(ListenEndpoint::Remote {
        host: address.ip().to_string(),
        port: address.port(),
      });
    }
  }
  match listener.parse::<auv_api_client::ConnectEndpoint>().map_err(|error| format!("invalid listener URI: {error}"))? {
    auv_api_client::ConnectEndpoint::Tcp(uri) => Ok(ListenEndpoint::Tcp {
      host: uri.host().ok_or_else(|| "listener TCP URI omitted host".to_string())?.to_string(),
      port: uri.port_u16().unwrap_or(80),
    }),
    #[cfg(unix)]
    auv_api_client::ConnectEndpoint::Unix(path) => Ok(ListenEndpoint::Unix { path }),
    #[cfg(windows)]
    auv_api_client::ConnectEndpoint::NamedPipe(name) => Ok(ListenEndpoint::NamedPipe { name }),
  }
}

/// Bound daemon server with discovery publication and graceful shutdown.
pub struct Server {
  inner: auv_api_server::server::Server,
  #[cfg(target_os = "linux")]
  device_local: std::sync::Arc<devices::LocalState>,
  discovery_file: Option<PathBuf>,
  publish_discovery: bool,
}

impl Server {
  /// Binds all configured listeners and opens daemon-owned state.
  pub async fn bind(config: Config) -> Result<Self, String> {
    let pairing = config
      .pairing_store
      .map(pairing::PairingStore::open)
      .transpose()
      .map_err(|error| format!("failed to open pairing store: {error}"))?
      .map(|store| std::sync::Arc::new(store) as std::sync::Arc<dyn auv_api_server::control::Pairing>);
    let mut listeners = config.listeners.into_iter();
    let listen = listeners.next().ok_or_else(|| "daemon requires at least one listener".to_string())?;
    let internal_runner_parent = {
      #[cfg(unix)]
      {
        let executable_runner_requires_parent =
          config.runner_providers.iter().any(|provider| matches!(provider.runtime, runner_provider::RunnerRuntime::Executable(_)))
            || config
              .first_party_runners
              .local_driver
              .as_ref()
              .is_some_and(|runtime| matches!(runtime, runner_provider::RunnerRuntime::Executable(_)));
        executable_runner_requires_parent.then(|| internal_runner_parent_socket(&config.store_root))
      }
      #[cfg(not(unix))]
      {
        None
      }
    };
    let store_root = config.store_root;
    #[cfg(target_os = "linux")]
    let device_local = std::sync::Arc::new(devices::LocalState::open(&store_root, pairing.clone())?);
    #[cfg(target_os = "linux")]
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
        internal_runner_parent,
      },
      move |parent_endpoint| {
        Ok(std::sync::Arc::new(daemon::Daemon::open_with_runner_providers_and_parent_endpoint(
          &store_root,
          parent_endpoint,
          first_party_runners,
          runner_providers,
          #[cfg(target_os = "linux")]
          local_state_for_daemon,
        )?))
      },
    )
    .await?;
    Ok(Self {
      inner: bound,
      #[cfg(target_os = "linux")]
      device_local,
      discovery_file: config.discovery_file,
      publish_discovery: config.publish_discovery,
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
    let _descriptor = if self.publish_discovery {
      let path = self.discovery_file.map_or_else(discovery::default_path, Ok).map_err(|error| error.to_string())?;
      self.inner.discovery_endpoint().map(|endpoint| discovery::PublishedDescriptor::publish(path, endpoint.to_string())).transpose()?
    } else {
      None
    };

    #[cfg(target_os = "linux")]
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

    #[cfg(not(target_os = "linux"))]
    self.inner.serve(shutdown).await
  }
}

#[cfg(unix)]
fn internal_runner_parent_socket(store_root: &Path) -> PathBuf {
  use std::hash::{Hash as _, Hasher as _};
  let mut hash = std::collections::hash_map::DefaultHasher::new();
  store_root.hash(&mut hash);
  std::env::temp_dir().join(format!("auv-parent-{}-{:x}.sock", std::process::id(), hash.finish()))
}

#[cfg(test)]
#[path = "server_test.rs"]
mod tests;
