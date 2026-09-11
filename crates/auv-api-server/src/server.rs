//! AUV daemon API server lifecycle and listener orchestration.

pub(crate) mod runner_grpc_proxy;

use std::fmt;
use std::net::{IpAddr, SocketAddr};
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use crate::authentication::Authenticator;
use crate::control::{Control, Pairing};
use crate::protocol::grpc::daemon::v1::{
  DeviceServiceGrpc, DiscoveryServiceGrpc, HealthServiceGrpc, PairingServiceGrpc, RunServiceGrpc, RunnerClassServiceGrpc, RunnerServiceGrpc,
};
use auv_api_proto::auv::api::daemon::v1::device_service_server::DeviceServiceServer;
use auv_api_proto::auv::api::daemon::v1::discovery_service_server::DiscoveryServiceServer;
use auv_api_proto::auv::api::daemon::v1::health_service_server::HealthServiceServer;
use auv_api_proto::auv::api::daemon::v1::pairing_service_server::PairingServiceServer;
use auv_api_proto::auv::api::daemon::v1::run_service_server::RunServiceServer;
use auv_api_proto::auv::api::daemon::v1::runner_class_service_server::RunnerClassServiceServer;
use auv_api_proto::auv::api::daemon::v1::runner_service_server::RunnerServiceServer;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
#[cfg(windows)]
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
#[cfg(unix)]
use tokio_stream::wrappers::UnixListenerStream;
use tokio_util::sync::CancellationToken;

/// Default loopback host for a local TCP listener.
pub const DEFAULT_API_HOST: &str = "127.0.0.1";
/// Default port for a local TCP listener.
pub const DEFAULT_API_PORT: u16 = 9847;

/// Server-side endpoint on which the API accepts gRPC connections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListenEndpoint {
  /// Loopback-only TCP. Paired remote TCP uses the separate bearer-authorized
  /// variant.
  // TODO(local-tcp-authentication): loopback is not user identity on a multi-user
  // host. Add a descriptor-delivered local credential before treating TCP as
  // equivalent to owner-checked Unix transport outside development use.
  Tcp {
    /// Host or address to bind.
    host: String,
    /// TCP port to bind.
    port: u16,
  },
  /// Paired gRPC authenticated by an opaque Device bearer.
  ///
  Remote {
    /// Host or address to bind.
    host: String,
    /// TCP port to bind.
    port: u16,
  },
  /// Local gRPC over a Unix domain socket.
  #[cfg(unix)]
  Unix {
    /// Unix-domain socket path.
    path: PathBuf,
  },
  /// Local gRPC over a Windows named pipe protected for its owner.
  #[cfg(windows)]
  NamedPipe {
    /// Pipe name below the local `\\.\pipe\` namespace.
    name: String,
  },
}

impl Default for ListenEndpoint {
  fn default() -> Self {
    Self::Tcp {
      host: DEFAULT_API_HOST.to_string(),
      port: DEFAULT_API_PORT,
    }
  }
}

/// Protocol-listener configuration consumed by a server-side SDK backend.
#[derive(Clone, Default)]
pub struct BindConfig {
  /// Primary listener exposed by the server.
  pub listen: ListenEndpoint,
  /// Extra listeners that share the same daemon control plane.
  pub additional_listeners: Vec<ListenEndpoint>,
  /// Optional pairing backend used for enrollment and bearer authentication.
  pub pairing: Option<Arc<dyn Pairing>>,
  /// Optional inactivity deadline applied to daemon-owned runner supervision.
  pub daemon_idle_timeout: Option<std::time::Duration>,
  /// Private Unix listener used by executable Runners when no caller-local
  /// Unix listener was configured. Windows Runners use per-process named
  /// pipes instead. The daemon SDK owns the path decision.
  pub internal_runner_parent: Option<PathBuf>,
}

/// Resolved endpoint of a bound server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoundEndpoint {
  /// Bound caller-local TCP address.
  Tcp(SocketAddr),
  /// Bound paired-bearer TCP address.
  Remote(SocketAddr),
  #[cfg(unix)]
  /// Bound caller-local Unix-domain socket path.
  Unix(PathBuf),
  #[cfg(windows)]
  /// Bound caller-local Windows named-pipe name.
  NamedPipe(String),
}

impl fmt::Display for BoundEndpoint {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Tcp(address) => write!(f, "http://{address}"),
      Self::Remote(address) => write!(f, "http://{address}"),
      #[cfg(unix)]
      Self::Unix(path) => write!(f, "unix://{}", path.display()),
      #[cfg(windows)]
      Self::NamedPipe(name) => write!(f, "npipe://./pipe/{name}"),
    }
  }
}

enum BoundListener {
  Tcp(TcpListener),
  #[cfg(unix)]
  Unix {
    listener: UnixListener,
    cleanup: UnixSocketCleanup,
  },
  #[cfg(windows)]
  NamedPipe {
    server: NamedPipeServer,
    name: String,
  },
}

/// Bound server whose endpoint is observable before serving begins.
// TODO(server-handle): a cloneable running-server control handle is deferred
// because current callers only need pre-serve endpoint inspection and injected
// cancellation; add it when an owner-approved runtime status, reload, or
// pause/resume operation needs that interface.
pub struct Server {
  endpoints: Vec<BoundEndpoint>,
  listeners: Vec<BoundListenerState>,
  daemon: Arc<dyn Control>,
  daemon_idle_timeout: Option<std::time::Duration>,
}

struct BoundListenerState {
  listener: BoundListener,
  authenticator: Authenticator,
}

impl Server {
  /// Binds protocol listeners, then asks the daemon SDK to construct the
  /// control backend with the concrete caller-local parent endpoint.
  pub async fn bind_with<F>(config: BindConfig, factory: F) -> Result<Self, String>
  where
    F: FnOnce(Option<String>) -> Result<Arc<dyn Control>, String>,
  {
    let pairing = config.pairing;
    let mut configured = Vec::with_capacity(1 + config.additional_listeners.len());
    configured.push(config.listen);
    configured.extend(config.additional_listeners);
    let mut endpoints = Vec::with_capacity(configured.len());
    let mut listeners = Vec::with_capacity(configured.len());
    for endpoint in configured {
      let (listener, endpoint, authenticator) = bind_listener(endpoint, pairing.clone()).await?;
      endpoints.push(endpoint);
      listeners.push(BoundListenerState {
        listener,
        authenticator,
      });
    }

    let mut parent_endpoint = {
      #[cfg(unix)]
      {
        endpoints
          .iter()
          .find(|endpoint| matches!(endpoint, BoundEndpoint::Unix(_)))
          .or_else(|| endpoints.iter().find(|endpoint| matches!(endpoint, BoundEndpoint::Tcp(_))))
      }
      #[cfg(not(unix))]
      #[cfg(not(windows))]
      {
        endpoints.iter().find(|endpoint| matches!(endpoint, BoundEndpoint::Tcp(_)))
      }
      #[cfg(windows)]
      {
        endpoints
          .iter()
          .find(|endpoint| matches!(endpoint, BoundEndpoint::NamedPipe(_)))
          .or_else(|| endpoints.iter().find(|endpoint| matches!(endpoint, BoundEndpoint::Tcp(_))))
      }
    }
    .map(ToString::to_string);

    if parent_endpoint.is_none()
      && let Some(path) = config.internal_runner_parent
    {
      #[cfg(unix)]
      let endpoint = ListenEndpoint::Unix { path };
      #[cfg(not(unix))]
      let endpoint = ListenEndpoint::Tcp {
        host: DEFAULT_API_HOST.to_string(),
        port: 0,
      };
      #[cfg(not(unix))]
      let _ = path;

      let (listener, endpoint, authenticator) = bind_listener(endpoint, pairing).await?;
      parent_endpoint = Some(endpoint.to_string());
      endpoints.push(endpoint);
      listeners.push(BoundListenerState {
        listener,
        authenticator,
      });
    }
    let daemon = factory(parent_endpoint)?;
    Ok(Self {
      endpoints,
      listeners,
      daemon,
      daemon_idle_timeout: config.daemon_idle_timeout,
    })
  }

  /// Primary endpoint, retained for callers that configure one listener.
  pub fn endpoint(&self) -> &BoundEndpoint {
    self.endpoints.first().expect("bind always produces a primary endpoint")
  }

  /// Every endpoint that was bound atomically before readiness.
  pub fn endpoints(&self) -> &[BoundEndpoint] {
    &self.endpoints
  }

  /// Endpoint safe for caller-local discovery, preferring owner-protected IPC
  /// over loopback TCP and never returning a paired remote endpoint.
  pub fn discovery_endpoint(&self) -> Option<&BoundEndpoint> {
    #[cfg(unix)]
    if let Some(endpoint) = self.endpoints.iter().find(|endpoint| matches!(endpoint, BoundEndpoint::Unix(_))) {
      return Some(endpoint);
    }
    #[cfg(windows)]
    if let Some(endpoint) = self.endpoints.iter().find(|endpoint| matches!(endpoint, BoundEndpoint::NamedPipe(_))) {
      return Some(endpoint);
    }
    self.endpoints.iter().find(|endpoint| matches!(endpoint, BoundEndpoint::Tcp(_)))
  }

  /// Serves every listener until cancellation or one listener fails. One
  /// unexpected listener failure cancels the complete daemon instance.
  pub async fn serve(self, shutdown: CancellationToken) -> Result<(), String> {
    let daemon = self.daemon;
    let idle_shutdown =
      self.daemon_idle_timeout.map(|timeout| tokio::spawn(shutdown_when_daemon_idle(Arc::clone(&daemon), shutdown.clone(), timeout)));
    let mut servers = tokio::task::JoinSet::new();
    for listener in self.listeners {
      let daemon = Arc::clone(&daemon);
      let listener_shutdown = shutdown.clone();
      servers.spawn(async move { serve_listener(listener, daemon, listener_shutdown).await });
    }

    let mut errors = Vec::new();
    while let Some(result) = servers.join_next().await {
      match result {
        Ok(Ok(())) if !shutdown.is_cancelled() => {
          errors.push("API listener stopped before daemon shutdown".to_string());
          shutdown.cancel();
        }
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
          errors.push(error);
          shutdown.cancel();
        }
        Err(error) => {
          errors.push(format!("API listener task failed: {error}"));
          shutdown.cancel();
        }
      }
    }
    shutdown.cancel();
    daemon.shutdown().await;
    if let Some(idle_shutdown) = idle_shutdown
      && let Err(error) = idle_shutdown.await
    {
      errors.push(format!("daemon idle task failed: {error}"));
    }
    if errors.is_empty() {
      Ok(())
    } else {
      Err(errors.join("; "))
    }
  }
}

async fn serve_listener(listener: BoundListenerState, daemon: Arc<dyn Control>, shutdown: CancellationToken) -> Result<(), String> {
  let authenticator = listener.authenticator;
  let pairing_service = PairingServiceGrpc::new(authenticator.pairing());
  let discovery_service = DiscoveryServiceGrpc::new(Arc::clone(&daemon));
  let health_service = HealthServiceGrpc;
  let device_service = DeviceServiceGrpc::new(Arc::clone(&daemon));
  let runner_service = RunnerServiceGrpc::new(Arc::clone(&daemon));
  let runner_class_service = RunnerClassServiceGrpc::new(Arc::clone(&daemon));
  let run_service = RunServiceGrpc::new(Arc::clone(&daemon));
  let grpc_routes = tonic::service::Routes::new(PairingServiceServer::new(pairing_service))
    .add_service(DiscoveryServiceServer::new(discovery_service))
    .add_service(HealthServiceServer::new(health_service))
    .add_service(DeviceServiceServer::new(device_service))
    .add_service(RunnerServiceServer::new(runner_service))
    .add_service(RunnerClassServiceServer::new(runner_class_service))
    .add_service(RunServiceServer::new(run_service))
    .into_axum_router()
    .fallback({
      let proxy = runner_grpc_proxy::RunnerGrpcProxy::new(Arc::clone(&daemon));
      move |request| {
        let proxy = proxy.clone();
        async move { proxy.forward(request).await }
      }
    });

  let mut authentication = crate::middleware::authentication::Builder::new();
  crate::rest::register_authentication(&mut authentication);
  authentication.public_grpc::<PairingServiceServer<PairingServiceGrpc>>("PairDevice");
  authentication.public_grpc::<HealthServiceServer<HealthServiceGrpc>>("Check");
  let authentication = authentication.build(authenticator.clone());

  let routes = crate::rest::router(Arc::clone(&daemon), authenticator)
    .fallback_service(grpc_routes)
    .layer(axum::middleware::from_fn_with_state(authentication, crate::middleware::authentication::authenticate))
    .layer(tower_http::cors::CorsLayer::permissive());

  match listener.listener {
    BoundListener::Tcp(listener) => axum::serve(listener, routes.into_make_service())
      .with_graceful_shutdown(shutdown.cancelled_owned())
      .await
      .map_err(|error| format!("API server failed: {error}")),
    #[cfg(unix)]
    BoundListener::Unix {
      listener,
      cleanup: _cleanup,
    } => tonic::transport::Server::builder()
      .accept_http1(true)
      .add_routes(routes.into())
      .serve_with_incoming_shutdown(UnixListenerStream::new(listener), shutdown.cancelled_owned())
      .await
      .map_err(|error| format!("API server failed: {error}")),
    #[cfg(windows)]
    BoundListener::NamedPipe { server, name } => {
      let incoming = futures_util::stream::try_unfold((server, name), |(server, name)| async move {
        server.connect().await?;
        // Create the next listening instance before yielding the connected one,
        // so a concurrent caller does not observe a gap in pipe availability.
        let next = create_named_pipe(&name, false)?;
        Ok::<_, std::io::Error>(Some((NamedPipeIo(server), (next, name))))
      });
      tonic::transport::Server::builder()
        .accept_http1(true)
        .add_routes(routes.into())
        .serve_with_incoming_shutdown(incoming, shutdown.cancelled_owned())
        .await
        .map_err(|error| format!("API server failed: {error}"))
    }
  }
}

async fn shutdown_when_daemon_idle(daemon: Arc<dyn Control>, shutdown: CancellationToken, timeout: std::time::Duration) {
  let poll_interval = timeout.min(std::time::Duration::from_secs(1));
  let mut interval = tokio::time::interval(poll_interval);
  interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
  let mut idle_since = tokio::time::Instant::now();
  loop {
    tokio::select! {
      _ = shutdown.cancelled() => return,
      _ = interval.tick() => {
        if daemon.has_live_runners() {
          idle_since = tokio::time::Instant::now();
        } else if idle_since.elapsed() >= timeout {
          shutdown.cancel();
          return;
        }
      }
    }
  }
}

async fn bind_listener(
  endpoint: ListenEndpoint,
  pairing: Option<Arc<dyn Pairing>>,
) -> Result<(BoundListener, BoundEndpoint, Authenticator), String> {
  Ok(match endpoint {
    ListenEndpoint::Tcp { host, port } => {
      let bind_addr = resolve_loopback_bind_addr(&host, port).await?;
      let listener = TcpListener::bind(bind_addr).await.map_err(|error| format!("failed to bind API server {bind_addr}: {error}"))?;
      let local_address = listener.local_addr().map_err(|error| format!("failed to read API server address: {error}"))?;
      assert_socket_addr_is_loopback(local_address)?;
      (
        BoundListener::Tcp(listener),
        BoundEndpoint::Tcp(local_address),
        Authenticator::local(
          #[cfg(unix)]
          None,
          pairing,
        ),
      )
    }
    ListenEndpoint::Remote { host, port } => {
      let bind_addr = resolve_remote_bind_addr(&host, port)?;
      let listener = TcpListener::bind(bind_addr).await.map_err(|error| format!("failed to bind remote API server {bind_addr}: {error}"))?;
      let local_address = listener.local_addr().map_err(|error| format!("failed to read remote API server address: {error}"))?;
      let pairing = pairing.ok_or_else(|| "remote API listener requires pairing".to_string())?;
      (BoundListener::Tcp(listener), BoundEndpoint::Remote(local_address), Authenticator::paired_bearer(pairing))
    }
    #[cfg(unix)]
    ListenEndpoint::Unix { path } => {
      let (listener, cleanup) = bind_unix(&path)?;
      let owner_uid = cleanup.owner_uid;
      (BoundListener::Unix { listener, cleanup }, BoundEndpoint::Unix(path), Authenticator::local(Some(owner_uid), pairing))
    }
    #[cfg(windows)]
    ListenEndpoint::NamedPipe { name } => {
      let server = create_named_pipe(&name, true).map_err(|error| format!("failed to bind API named pipe {name:?}: {error}"))?;
      (
        BoundListener::NamedPipe {
          server,
          name: name.clone(),
        },
        BoundEndpoint::NamedPipe(name),
        Authenticator::local(pairing),
      )
    }
  })
}

#[cfg(windows)]
fn create_named_pipe(name: &str, first: bool) -> std::io::Result<NamedPipeServer> {
  use windows::Win32::Foundation::{BOOL, HLOCAL, LocalFree};
  use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
  use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
  use windows::core::w;

  if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')) {
    return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "named-pipe name contains unsupported characters"));
  }

  let mut descriptor = PSECURITY_DESCRIPTOR::default();
  // `OW` is the object-owner SID. The protected DACL grants full access only
  // to the creating user and LocalSystem; no Everyone or Administrators ACE is
  // inherited. PIPE_REJECT_REMOTE_CLIENTS independently rejects network opens.
  // SAFETY: `descriptor` is valid writable output storage, the SDDL literal is
  // NUL-terminated, and the returned allocation is released with LocalFree
  // after CreateNamedPipeW has synchronously copied the security descriptor.
  unsafe {
    ConvertStringSecurityDescriptorToSecurityDescriptorW(w!("D:P(A;;GA;;;SY)(A;;GA;;;OW)"), SDDL_REVISION_1, &mut descriptor, None)
      .map_err(std::io::Error::other)?;
  }
  let mut attributes = SECURITY_ATTRIBUTES {
    nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
    lpSecurityDescriptor: descriptor.0,
    bInheritHandle: BOOL(0),
  };
  let mut options = ServerOptions::new();
  options.first_pipe_instance(first).reject_remote_clients(true);
  let path = format!(r"\\.\pipe\{name}");
  // SAFETY: `attributes` and its descriptor remain live for this synchronous
  // call. Tokio only forwards the pointer to CreateNamedPipeW and does not
  // retain it in the returned server handle.
  let result = unsafe { options.create_with_security_attributes_raw(&path, (&raw mut attributes).cast()) };
  // SAFETY: ConvertStringSecurityDescriptorToSecurityDescriptorW returned this
  // allocation and transfers ownership to the caller, which must use LocalFree.
  unsafe {
    let _ = LocalFree(HLOCAL(descriptor.0));
  }
  result
}

#[cfg(windows)]
struct NamedPipeIo(NamedPipeServer);

#[cfg(windows)]
impl tokio::io::AsyncRead for NamedPipeIo {
  fn poll_read(
    mut self: std::pin::Pin<&mut Self>,
    context: &mut std::task::Context<'_>,
    buffer: &mut tokio::io::ReadBuf<'_>,
  ) -> std::task::Poll<std::io::Result<()>> {
    std::pin::Pin::new(&mut self.0).poll_read(context, buffer)
  }
}

#[cfg(windows)]
impl tokio::io::AsyncWrite for NamedPipeIo {
  fn poll_write(
    mut self: std::pin::Pin<&mut Self>,
    context: &mut std::task::Context<'_>,
    buffer: &[u8],
  ) -> std::task::Poll<std::io::Result<usize>> {
    std::pin::Pin::new(&mut self.0).poll_write(context, buffer)
  }

  fn poll_flush(mut self: std::pin::Pin<&mut Self>, context: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
    std::pin::Pin::new(&mut self.0).poll_flush(context)
  }

  fn poll_shutdown(mut self: std::pin::Pin<&mut Self>, context: &mut std::task::Context<'_>) -> std::task::Poll<std::io::Result<()>> {
    std::pin::Pin::new(&mut self.0).poll_shutdown(context)
  }
}

#[cfg(windows)]
impl tonic::transport::server::Connected for NamedPipeIo {
  type ConnectInfo = ();

  fn connect_info(&self) -> Self::ConnectInfo {}
}

fn resolve_remote_bind_addr(host: &str, port: u16) -> Result<SocketAddr, String> {
  let ip = host.parse::<IpAddr>().map_err(|error| format!("remote listen host must be an explicit IP address, got {host:?}: {error}"))?;
  Ok(SocketAddr::new(ip, port))
}

/// Rejects host strings that are not allowed loopback listen targets.
pub fn assert_loopback_host(host: &str) -> Result<(), String> {
  if host.eq_ignore_ascii_case("localhost") {
    return Ok(());
  }
  match host.parse::<IpAddr>() {
    Ok(ip) if ip.is_loopback() => Ok(()),
    Ok(_) => Err(format!("API server refuses non-loopback host: {host}")),
    Err(_) => Err(format!("API server refuses unrecognized host: {host}")),
  }
}

/// Verifies a bound socket address is loopback-only.
pub fn assert_socket_addr_is_loopback(addr: SocketAddr) -> Result<(), String> {
  if addr.ip().is_loopback() {
    return Ok(());
  }
  Err(format!("API server refused non-loopback bind address: {addr}"))
}

async fn resolve_loopback_bind_addr(host: &str, port: u16) -> Result<SocketAddr, String> {
  assert_loopback_host(host)?;
  if host.eq_ignore_ascii_case("localhost") {
    let mut addresses =
      tokio::net::lookup_host((host, port)).await.map_err(|error| format!("failed to resolve localhost for API server: {error}"))?;
    return addresses
      .find(|address| address.ip().is_loopback())
      .ok_or_else(|| "localhost did not resolve to a loopback address".to_string());
  }
  let ip = host.parse::<IpAddr>().map_err(|error| format!("failed to parse API host {host}: {error}"))?;
  Ok(SocketAddr::new(ip, port))
}

#[cfg(unix)]
fn bind_unix(path: &Path) -> Result<(UnixListener, UnixSocketCleanup), String> {
  if path.exists() {
    return Err(format!("API Unix socket path already exists: {}", path.display()));
  }
  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent).map_err(|error| format!("failed to create API socket directory {}: {error}", parent.display()))?;
  }
  let listener = UnixListener::bind(path).map_err(|error| format!("failed to bind API Unix socket {}: {error}", path.display()))?;
  // Local transport skips pairing, so the socket itself must not grant access
  // to group/other users while peer-caller projection remains deferred.
  use std::os::unix::fs::PermissionsExt;
  std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
    .map_err(|error| format!("failed to protect API Unix socket {}: {error}", path.display()))?;
  let cleanup = UnixSocketCleanup::new(path)?;
  Ok((listener, cleanup))
}

#[cfg(unix)]
struct UnixSocketCleanup {
  path: PathBuf,
  device: u64,
  inode: u64,
  owner_uid: u32,
}

#[cfg(unix)]
impl UnixSocketCleanup {
  fn new(path: &Path) -> Result<Self, String> {
    use std::os::unix::fs::MetadataExt;
    let metadata =
      std::fs::symlink_metadata(path).map_err(|error| format!("failed to inspect API Unix socket {}: {error}", path.display()))?;
    Ok(Self {
      path: path.to_path_buf(),
      device: metadata.dev(),
      inode: metadata.ino(),
      owner_uid: metadata.uid(),
    })
  }
}

#[cfg(unix)]
impl Drop for UnixSocketCleanup {
  fn drop(&mut self) {
    use std::os::unix::fs::MetadataExt;
    let Ok(metadata) = std::fs::symlink_metadata(&self.path) else {
      return;
    };
    // NOTICE(unix-socket-cleanup): only unlink the exact filesystem object we
    // bound; another process may have replaced the path while shutdown raced.
    if metadata.dev() == self.device && metadata.ino() == self.inode {
      let _ = std::fs::remove_file(&self.path);
    }
  }
}
