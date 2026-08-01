//! Tonic transport adapters for the AUV daemon API.

use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use auv_api_proto::auv::api::core::v1::device_service_server::DeviceServiceServer;
use auv_api_proto::auv::api::core::v1::discovery_service_server::DiscoveryServiceServer;
use auv_api_proto::auv::api::core::v1::run_service_server::RunServiceServer;
use auv_api_proto::auv::api::core::v1::runner_class_service_server::RunnerClassServiceServer;
use auv_api_proto::auv::api::core::v1::runner_service_server::RunnerServiceServer;
use auv_api_proto::auv::api::driver::macos::v1::accessibility_service_server::AccessibilityServiceServer;
use auv_api_proto::auv::api::driver::macos::v1::application_service_server::ApplicationServiceServer;
use auv_api_proto::auv::api::driver::macos::v1::media_control_service_server::MediaControlServiceServer;
use auv_api_proto::auv::api::driver::macos::v1::permission_service_server::PermissionServiceServer;
use auv_api_proto::auv::api::driver::v1::capture_service_server::CaptureServiceServer;
use auv_api_proto::auv::api::driver::v1::display_service_server::DisplayServiceServer;
use auv_api_proto::auv::api::driver::v1::input_service_server::InputServiceServer;
use auv_api_proto::auv::api::driver::v1::overlay_service_server::OverlayServiceServer;
use auv_api_proto::auv::api::driver::v1::text_recognition_service_server::TextRecognitionServiceServer;
use auv_api_proto::auv::api::driver::v1::window_service_server::WindowServiceServer;
use auv_api_proto::v1::inference::object_detection_service_server::ObjectDetectionServiceServer;
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::UnixListener;
use tokio_stream::wrappers::TcpListenerStream;
#[cfg(unix)]
use tokio_stream::wrappers::UnixListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Status};

use crate::authority::{ApiScope, PairingError, PairingStore, PrincipalId};
use crate::control_grpc::{
  AccessibilityServiceGrpc, ApplicationServiceGrpc, CaptureServiceGrpc, DeviceServiceGrpc, DiscoveryServiceGrpc, DisplayServiceGrpc,
  InputServiceGrpc, MediaControlServiceGrpc, ObjectDetectionServiceGrpc, OverlayServiceGrpc, PermissionServiceGrpc, RunServiceGrpc,
  RunnerClassServiceGrpc, RunnerServiceGrpc, TextRecognitionServiceGrpc, WindowServiceGrpc,
};
use crate::handler::ApiHandler;

pub const DEFAULT_API_HOST: &str = "127.0.0.1";
pub const DEFAULT_API_PORT: u16 = 9847;

/// Server-side endpoint on which the API accepts gRPC connections.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ListenEndpoint {
  /// Loopback-only TCP. Remote TCP requires an explicit authentication and TLS
  /// policy and is therefore not represented by this variant.
  // TODO(local-tcp-authority): loopback is not user identity on a multi-user
  // host. Add a descriptor-delivered local credential before treating TCP as
  // equivalent to owner-checked Unix transport outside development use.
  Tcp { host: String, port: u16 },
  /// Remote gRPC with mandatory mutual TLS and paired-certificate authority.
  RemoteTls {
    host: String,
    port: u16,
    server_certificate: PathBuf,
    server_private_key: PathBuf,
    client_ca_certificate: PathBuf,
    pairing_store: PathBuf,
  },
  /// Local gRPC over a Unix domain socket.
  #[cfg(unix)]
  Unix { path: PathBuf },
}

impl Default for ListenEndpoint {
  fn default() -> Self {
    Self::Tcp {
      host: DEFAULT_API_HOST.to_string(),
      port: DEFAULT_API_PORT,
    }
  }
}

/// Configuration for one AUV API server instance.
#[derive(Clone, Debug, Default)]
pub struct ApiServeConfig {
  /// Primary listener retained as the single-listener convenience.
  pub listen: ListenEndpoint,
  /// Additional listeners sharing the same daemon control plane and Runner
  /// supervisor. Each listener retains its own authority and TLS policy.
  pub additional_listeners: Vec<ListenEndpoint>,
  pub store_root: PathBuf,
  /// Operator-trusted custom Runner providers registered before serving.
  ///
  /// This experimental configuration is validated before any child is
  /// started. Child reflection cannot add or widen a provider policy.
  pub runner_providers: Vec<crate::runner_provider::RunnerProviderConfig>,
  /// Built-in Runner implementations explicitly hosted by this process.
  pub first_party_runners: crate::runner_provider::FirstPartyRunnerRuntimes,
  /// Optional process-level idle timeout. Live Runners keep the daemon alive.
  pub daemon_idle_timeout: Option<std::time::Duration>,
}

/// Resolved endpoint of a bound server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BoundEndpoint {
  Tcp(SocketAddr),
  RemoteTls(SocketAddr),
  #[cfg(unix)]
  Unix(PathBuf),
}

impl fmt::Display for BoundEndpoint {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Tcp(address) => write!(f, "http://{address}"),
      Self::RemoteTls(address) => write!(f, "https://{address}"),
      #[cfg(unix)]
      Self::Unix(path) => write!(f, "unix://{}", path.display()),
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
}

/// Bound server whose endpoint is observable before serving begins.
pub struct BoundApi {
  endpoints: Vec<BoundEndpoint>,
  listeners: Vec<BoundListenerState>,
  handler: Arc<ApiHandler>,
  daemon_idle_timeout: Option<std::time::Duration>,
}

struct BoundListenerState {
  listener: BoundListener,
  authority: RequestAuthority,
  tls: Option<tonic::transport::ServerTlsConfig>,
}

impl BoundApi {
  /// Primary endpoint, retained for callers that configure one listener.
  pub fn endpoint(&self) -> &BoundEndpoint {
    self.endpoints.first().expect("bind always produces a primary endpoint")
  }

  /// Every endpoint that was bound atomically before readiness.
  pub fn endpoints(&self) -> &[BoundEndpoint] {
    &self.endpoints
  }

  /// Endpoint safe for caller-local discovery, preferring Unix over loopback
  /// TCP and never returning a credential-dependent remote endpoint.
  pub fn discovery_endpoint(&self) -> Option<&BoundEndpoint> {
    #[cfg(unix)]
    if let Some(endpoint) = self.endpoints.iter().find(|endpoint| matches!(endpoint, BoundEndpoint::Unix(_))) {
      return Some(endpoint);
    }
    self.endpoints.iter().find(|endpoint| matches!(endpoint, BoundEndpoint::Tcp(_)))
  }

  /// Serves every listener until cancellation or one listener fails. One
  /// unexpected listener failure cancels the complete daemon instance.
  pub async fn serve(self, shutdown: CancellationToken) -> Result<(), String> {
    let handler = self.handler;
    let idle_shutdown =
      self.daemon_idle_timeout.map(|timeout| tokio::spawn(shutdown_when_daemon_idle(Arc::clone(&handler), shutdown.clone(), timeout)));
    let mut servers = tokio::task::JoinSet::new();
    for listener in self.listeners {
      let handler = Arc::clone(&handler);
      let listener_shutdown = shutdown.clone();
      servers.spawn(async move { serve_listener(listener, handler, listener_shutdown).await });
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
    handler.shutdown().await;
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

async fn serve_listener(listener: BoundListenerState, handler: Arc<ApiHandler>, shutdown: CancellationToken) -> Result<(), String> {
  let inference_service = ObjectDetectionServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let discovery_service = DiscoveryServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let device_service = DeviceServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let runner_service = RunnerServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let runner_class_service = RunnerClassServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let run_service = RunServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let display_service = DisplayServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let window_service = WindowServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let capture_service = CaptureServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let input_service = InputServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let permission_service = PermissionServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let media_control_service = MediaControlServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let overlay_service = OverlayServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let application_service = ApplicationServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let accessibility_service = AccessibilityServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let text_recognition_service = TextRecognitionServiceGrpc::new(Arc::clone(&handler), listener.authority.clone());
  let mut server = tonic::transport::Server::builder();
  if let Some(tls) = listener.tls {
    server = server.tls_config(tls).map_err(|error| format!("failed to configure API TLS: {error}"))?;
  }
  let grpc_routes = tonic::service::Routes::new(DiscoveryServiceServer::new(discovery_service))
    .add_service(DeviceServiceServer::new(device_service))
    .add_service(RunnerServiceServer::new(runner_service))
    .add_service(RunnerClassServiceServer::new(runner_class_service))
    .add_service(RunServiceServer::new(run_service))
    .add_service(DisplayServiceServer::new(display_service))
    .add_service(WindowServiceServer::new(window_service))
    .add_service(CaptureServiceServer::new(capture_service).max_encoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES))
    .add_service(InputServiceServer::new(input_service))
    .add_service(PermissionServiceServer::new(permission_service))
    .add_service(MediaControlServiceServer::new(media_control_service))
    .add_service(OverlayServiceServer::new(overlay_service))
    .add_service(ApplicationServiceServer::new(application_service))
    .add_service(AccessibilityServiceServer::new(accessibility_service))
    .add_service(
      TextRecognitionServiceServer::new(text_recognition_service)
        .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
        .max_encoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES),
    )
    .add_service(
      ObjectDetectionServiceServer::new(inference_service)
        .max_decoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES)
        .max_encoding_message_size(auv_api_proto::MAX_CAPTURE_GRPC_MESSAGE_BYTES),
    )
    .into_axum_router()
    .fallback({
      let aggregated = crate::aggregated_grpc::AggregatedGrpc::new(Arc::clone(&handler), listener.authority.clone());
      move |request| {
        let aggregated = aggregated.clone();
        async move { aggregated.forward(request).await }
      }
    });
  let routes = crate::rest::router(Arc::clone(&handler), listener.authority).fallback_service(grpc_routes);
  match listener.listener {
    BoundListener::Tcp(listener) => server
      .add_routes(routes.into())
      .serve_with_incoming_shutdown(TcpListenerStream::new(listener), shutdown.cancelled_owned())
      .await
      .map_err(|error| format!("API server failed: {error}")),
    #[cfg(unix)]
    BoundListener::Unix {
      listener,
      cleanup: _cleanup,
    } => server
      .add_routes(routes.into())
      .serve_with_incoming_shutdown(UnixListenerStream::new(listener), shutdown.cancelled_owned())
      .await
      .map_err(|error| format!("API server failed: {error}")),
  }
}

pub(crate) fn require_host_model_access(principal: &PrincipalId) -> Result<(), Status> {
  if principal.is_local_owner() {
    return Ok(());
  }
  // TODO(remote-model-sources): paired callers cannot supply daemon-host
  // model paths until a server-owned model registry and per-model authority
  // replace this local filesystem capability.
  Err(Status::permission_denied("paired devices cannot load detector models from daemon-host paths"))
}

async fn shutdown_when_daemon_idle(handler: Arc<ApiHandler>, shutdown: CancellationToken, timeout: std::time::Duration) {
  let poll_interval = timeout.min(std::time::Duration::from_secs(1));
  let mut interval = tokio::time::interval(poll_interval);
  interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
  let mut idle_since = tokio::time::Instant::now();
  loop {
    tokio::select! {
      _ = shutdown.cancelled() => return,
      _ = interval.tick() => {
        if handler.has_live_resources() {
          idle_since = tokio::time::Instant::now();
        } else if idle_since.elapsed() >= timeout {
          shutdown.cancel();
          return;
        }
      }
    }
  }
}

/// Binds without starting request processing so a process supervisor can
/// publish readiness only after the endpoint actually exists.
pub async fn bind(config: ApiServeConfig) -> Result<BoundApi, String> {
  // Provider files and descriptor policy are admitted before the bound server
  // can be returned as ready to a daemon frontend.
  let handler = Arc::new(ApiHandler::new_with_runner_providers(config.store_root, config.first_party_runners, config.runner_providers)?);
  let mut configured = Vec::with_capacity(1 + config.additional_listeners.len());
  configured.push(config.listen);
  configured.extend(config.additional_listeners);
  let mut endpoints = Vec::with_capacity(configured.len());
  let mut listeners = Vec::with_capacity(configured.len());
  for endpoint in configured {
    let (listener, endpoint, authority, tls) = bind_listener(endpoint).await?;
    endpoints.push(endpoint);
    listeners.push(BoundListenerState {
      listener,
      authority,
      tls,
    });
  }
  Ok(BoundApi {
    endpoints,
    listeners,
    handler,
    daemon_idle_timeout: config.daemon_idle_timeout,
  })
}

async fn bind_listener(
  endpoint: ListenEndpoint,
) -> Result<(BoundListener, BoundEndpoint, RequestAuthority, Option<tonic::transport::ServerTlsConfig>), String> {
  Ok(match endpoint {
    ListenEndpoint::Tcp { host, port } => {
      let bind_addr = resolve_loopback_bind_addr(&host, port).await?;
      let listener = TcpListener::bind(bind_addr).await.map_err(|error| format!("failed to bind API server {bind_addr}: {error}"))?;
      let local_address = listener.local_addr().map_err(|error| format!("failed to read API server address: {error}"))?;
      assert_socket_addr_is_loopback(local_address)?;
      (
        BoundListener::Tcp(listener),
        BoundEndpoint::Tcp(local_address),
        RequestAuthority::local(
          #[cfg(unix)]
          None,
        ),
        None,
      )
    }
    ListenEndpoint::RemoteTls {
      host,
      port,
      server_certificate,
      server_private_key,
      client_ca_certificate,
      pairing_store,
    } => {
      install_tls_crypto_provider();
      let bind_addr = resolve_remote_bind_addr(&host, port)?;
      let listener = TcpListener::bind(bind_addr).await.map_err(|error| format!("failed to bind remote API server {bind_addr}: {error}"))?;
      let local_address = listener.local_addr().map_err(|error| format!("failed to read remote API server address: {error}"))?;
      let certificate = std::fs::read(&server_certificate)
        .map_err(|error| format!("failed to read server certificate {}: {error}", server_certificate.display()))?;
      let private_key = std::fs::read(&server_private_key)
        .map_err(|error| format!("failed to read server private key {}: {error}", server_private_key.display()))?;
      let client_ca = std::fs::read(&client_ca_certificate)
        .map_err(|error| format!("failed to read client CA certificate {}: {error}", client_ca_certificate.display()))?;
      let store = PairingStore::open(pairing_store).map_err(|error| format!("failed to open pairing store: {error}"))?;
      let tls = tonic::transport::ServerTlsConfig::new()
        .identity(tonic::transport::Identity::from_pem(certificate, private_key))
        .client_ca_root(tonic::transport::Certificate::from_pem(client_ca));
      (BoundListener::Tcp(listener), BoundEndpoint::RemoteTls(local_address), RequestAuthority::paired_mtls(store), Some(tls))
    }
    #[cfg(unix)]
    ListenEndpoint::Unix { path } => {
      let (listener, cleanup) = bind_unix(&path)?;
      let owner_uid = cleanup.owner_uid;
      (BoundListener::Unix { listener, cleanup }, BoundEndpoint::Unix(path), RequestAuthority::local(Some(owner_uid)), None)
    }
  })
}

fn install_tls_crypto_provider() {
  // NOTICE: Cargo feature unification can enable rustls ring and aws-lc-rs in
  // one AUV process, which prevents rustls from choosing automatically. This
  // transport deliberately selects tonic's `tls-ring` provider. Remove the
  // explicit install if the workspace standardizes one provider or rustls
  // supports deterministic multi-provider selection.
  // See `https://docs.rs/rustls/0.23/rustls/crypto/struct.CryptoProvider.html`.
  let _ = rustls::crypto::ring::default_provider().install_default();
}

fn resolve_remote_bind_addr(host: &str, port: u16) -> Result<SocketAddr, String> {
  let ip =
    host.parse::<IpAddr>().map_err(|error| format!("remote TLS listen host must be an explicit IP address, got {host:?}: {error}"))?;
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
  // to group/other users while peer-principal projection remains deferred.
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

/// Explicit authority mode attached to a listener. TLS metadata can never fall
/// through to local-owner authorization.
#[derive(Clone)]
pub enum RequestAuthority {
  Local {
    #[cfg(unix)]
    allowed_unix_uid: Option<u32>,
  },
  PairedMtls {
    store: PairingStore,
  },
}

impl RequestAuthority {
  pub fn local(#[cfg(unix)] allowed_unix_uid: Option<u32>) -> Self {
    Self::Local {
      #[cfg(unix)]
      allowed_unix_uid,
    }
  }

  pub fn paired_mtls(store: PairingStore) -> Self {
    Self::PairedMtls { store }
  }

  pub(crate) fn principal<T>(&self, request: &Request<T>, scope: ApiScope) -> Result<PrincipalId, Status> {
    self.principal_from_extensions(request.extensions(), scope)
  }

  pub(crate) fn principal_from_extensions(&self, extensions: &axum::http::Extensions, scope: ApiScope) -> Result<PrincipalId, Status> {
    match self {
      Self::Local {
        #[cfg(unix)]
        allowed_unix_uid,
      } => {
        #[cfg(unix)]
        if let Some(allowed_uid) = allowed_unix_uid {
          let peer_uid = extensions
            .get::<tonic::transport::server::UdsConnectInfo>()
            .and_then(|info| info.peer_cred.as_ref())
            .map(tokio::net::unix::UCred::uid);
          if peer_uid != Some(*allowed_uid) {
            return Err(Status::permission_denied("Unix peer credentials do not match the API server owner"));
          }
        }
        Ok(PrincipalId::local_owner())
      }
      Self::PairedMtls { store } => {
        let certificates = extensions
          .get::<tonic::transport::server::TlsConnectInfo<tonic::transport::server::TcpConnectInfo>>()
          .and_then(tonic::transport::server::TlsConnectInfo::peer_certs)
          .ok_or_else(|| Status::unauthenticated("paired client certificate required"))?;
        let leaf = certificates.first().ok_or_else(|| Status::unauthenticated("paired client certificate required"))?;
        store.authorize_der(leaf.as_ref(), scope).map_err(map_pairing_authority_error)
      }
    }
  }
}

fn map_pairing_authority_error(error: PairingError) -> Status {
  match error {
    PairingError::Unauthenticated => Status::unauthenticated("client certificate is not an active paired credential"),
    PairingError::MissingScope { .. } => Status::permission_denied("paired device lacks the required API scope"),
    _ => Status::internal("paired-device authority store failed"),
  }
}

#[cfg(test)]
mod tests {
  use auv_api_client::placement::{RunOptions, RunSelection};
  use auv_api_client::{Client, ConnectEndpoint, PairedConnectConfig};
  use auv_api_proto::auv::api::core::v1 as core_proto;
  use auv_api_proto::auv::api::driver::macos::v1 as macos_proto;
  use auv_api_proto::auv::api::driver::macos::v1::accessibility_service_client::AccessibilityServiceClient;
  use auv_api_proto::auv::api::driver::macos::v1::application_service_client::ApplicationServiceClient;
  use auv_api_proto::auv::api::driver::macos::v1::media_control_service_client::MediaControlServiceClient;
  use auv_api_proto::auv::api::driver::macos::v1::permission_service_client::PermissionServiceClient;
  use auv_api_proto::auv::api::driver::v1 as driver_proto;
  use auv_api_proto::auv::api::driver::v1::input_service_client::InputServiceClient;
  use auv_api_proto::auv::api::driver::v1::overlay_service_client::OverlayServiceClient;
  use prost::Message as _;
  use rcgen::{BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose};
  use tonic::Code;

  use crate::authority::{CertificateFingerprint, CredentialState, PairingCredential, PairingRecord};
  use crate::runner_provider::{
    ExecutableRunnerRuntime, RunnerProviderConfig, RunnerProviderLifecycle, RunnerProviderServiceConfig, RunnerRuntime,
  };
  use crate::test_fixtures::api_temp_store_root;

  use super::*;

  fn catalog_provider(directory: &std::path::Path) -> RunnerProviderConfig {
    let descriptor_set = directory.join("catalog.binpb");
    let descriptor = auv_api_proto::descriptor_set_for_service("auv.api.driver.v1.DisplayService").expect("Display descriptor closure");
    std::fs::write(&descriptor_set, descriptor).expect("write catalog descriptor");
    let executable = directory.join("catalog-runner");
    std::fs::write(&executable, b"#!/bin/sh\nexit 0\n").expect("write catalog executable");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).expect("secure catalog executable");
    }
    let services = vec![RunnerProviderServiceConfig {
      name: "auv.api.driver.v1.DisplayService".to_string(),
      externally_exposed: true,
    }];
    let descriptor_set_sha256 =
      RunnerProviderConfig::canonical_descriptor_sha256(&descriptor_set, &services).expect("pin catalog descriptor");
    RunnerProviderConfig {
      runner_class: "auv.test.catalog".to_string(),
      display_name: "Catalog test".to_string(),
      runtime: RunnerRuntime::Executable(ExecutableRunnerRuntime {
        executable,
        arguments: Vec::new(),
      }),
      descriptor_set,
      descriptor_set_sha256,
      services,
      supported_lifecycles: vec![RunnerProviderLifecycle::Ephemeral],
      operation_capacity: 1,
    }
  }

  #[test]
  fn loopback_policy_rejects_remote_tcp() {
    assert!(assert_loopback_host("127.0.0.1").is_ok());
    assert!(assert_loopback_host("::1").is_ok());
    assert!(assert_loopback_host("0.0.0.0").is_err());
    assert!(assert_loopback_host("192.168.1.1").is_err());
  }

  #[test]
  fn daemon_host_model_paths_are_local_owner_only() {
    assert!(require_host_model_access(&PrincipalId::local_owner()).is_ok());
    let denied = require_host_model_access(&PrincipalId::paired_device("remote-test")).expect_err("paired model path must be denied");
    assert_eq!(denied.code(), Code::PermissionDenied);
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn multi_listener_bind_is_atomic_and_cleans_up_bound_unix_sockets() {
    let directory = tempfile::tempdir().expect("temporary multi-listener directory");
    let socket_path = directory.path().join("auv.sock");
    let result = bind(ApiServeConfig {
      listen: ListenEndpoint::Unix {
        path: socket_path.clone(),
      },
      additional_listeners: vec![ListenEndpoint::Unix {
        path: socket_path.clone(),
      }],
      store_root: directory.path().join("runs"),
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      first_party_runners: Default::default(),
    })
    .await;
    let error = match result {
      Ok(_) => panic!("all listeners must bind before readiness"),
      Err(error) => error,
    };

    assert!(error.contains("already exists"));
    assert!(!socket_path.exists(), "failed multi-listener bind removes sockets bound earlier in the same attempt");
  }

  #[tokio::test]
  async fn tcp_client_reaches_typed_control_and_capability_services() {
    let config = ApiServeConfig {
      listen: ListenEndpoint::Tcp {
        host: DEFAULT_API_HOST.to_string(),
        port: 0,
      },
      additional_listeners: Vec::new(),
      store_root: api_temp_store_root("tcp"),
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      first_party_runners: Default::default(),
    };
    let bound = bind(config).await.expect("bind TCP server");
    let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
      panic!("TCP endpoint");
    };
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(bound.serve(shutdown.clone()));
    let endpoint = format!("http://{address}").parse().expect("valid loopback endpoint");
    let mut client = Client::connect(endpoint).await.expect("connect TCP client");
    let devices = client.list_devices().await.expect("list Devices through gRPC");
    assert_eq!(devices.len(), 1);
    assert!(devices[0].local);
    let typed_ocr_error = client
      .recognize_runner_text(core_proto::RunnerLeaseRef::default(), driver_proto::RecognizeTextRequest::default())
      .await
      .expect_err("typed OCR requires a capture before Runner admission");
    assert_eq!(typed_ocr_error.code(), Code::InvalidArgument);
    let typed_find_error = client
      .find_runner_window_text(core_proto::RunnerLeaseRef::default(), driver_proto::FindWindowTextRequest::default())
      .await
      .expect_err("typed window text lookup requires selector and query before Runner admission");
    assert_eq!(typed_find_error.code(), Code::InvalidArgument);
    shutdown.cancel();
    server.await.expect("join server").expect("serve TCP");
  }

  #[tokio::test]
  async fn rest_discovery_lists_the_auv_api_namespace() {
    let config = ApiServeConfig {
      listen: ListenEndpoint::Tcp {
        host: DEFAULT_API_HOST.to_string(),
        port: 0,
      },
      additional_listeners: Vec::new(),
      store_root: api_temp_store_root("rest-discovery"),
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      first_party_runners: Default::default(),
    };
    let bound = bind(config).await.expect("bind TCP server");
    let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
      panic!("TCP endpoint");
    };
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(bound.serve(shutdown.clone()));
    install_tls_crypto_provider();
    let client = reqwest::Client::builder().http2_prior_knowledge().build().expect("build REST test client");

    let response = client.get(format!("http://{address}/apis")).send().await.expect("request API discovery");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    assert_eq!(response.headers().get("content-type").and_then(|value| value.to_str().ok()), Some("application/protobuf"));
    let discovery =
      core_proto::ListApiNamespacesResponse::decode(response.bytes().await.expect("read API discovery")).expect("decode API discovery");
    assert_eq!(
      discovery.namespaces,
      vec![core_proto::ApiNamespace {
        name: "auv".to_string(),
      }]
    );

    shutdown.cancel();
    server.await.expect("join server").expect("serve TCP");
  }

  #[tokio::test]
  async fn typed_service_discovery_exposes_trusted_discoverable_methods() {
    let directory = tempfile::tempdir().expect("catalog provider directory");
    let config = ApiServeConfig {
      listen: ListenEndpoint::Tcp {
        host: DEFAULT_API_HOST.to_string(),
        port: 0,
      },
      additional_listeners: Vec::new(),
      store_root: api_temp_store_root("rest-service-catalog"),
      daemon_idle_timeout: None,
      runner_providers: vec![catalog_provider(directory.path())],
      first_party_runners: Default::default(),
    };
    let bound = bind(config).await.expect("bind TCP server");
    let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
      panic!("TCP endpoint");
    };
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(bound.serve(shutdown.clone()));
    install_tls_crypto_provider();
    let client = reqwest::Client::builder().http2_prior_knowledge().build().expect("build REST test client");

    let response = client.get(format!("http://{address}/apis/auv/core/v1/services")).send().await.expect("request service catalog");
    assert_eq!(response.status(), reqwest::StatusCode::OK);
    let catalog =
      core_proto::ListServicesResponse::decode(response.bytes().await.expect("read service catalog")).expect("decode service catalog");
    let service = catalog
      .services
      .iter()
      .find(|service| service.runner_class == "auv.test.catalog")
      .expect("trusted custom RunnerClass is discoverable");
    assert_eq!(service.full_name, "auv.api.driver.v1.DisplayService");
    assert_eq!(service.methods.len(), 1);
    assert_eq!(service.methods[0].full_name, "auv.api.driver.v1.DisplayService.ListDisplays");
    assert!(service.methods[0].discoverable);
    assert_eq!(service.methods[0].effect, auv_api_proto::auv::api::annotations::v1::MethodEffect::ReadOnly as i32);

    let mut grpc = Client::connect(format!("http://{address}").parse::<ConnectEndpoint>().expect("loopback endpoint"))
      .await
      .expect("connect typed AUV client");
    assert_eq!(grpc.list_services().await.expect("list gRPC service catalog"), catalog.services);

    let unversioned = client.get(format!("http://{address}/apis/auv/services")).send().await.expect("probe unversioned catalog path");
    assert_ne!(
      unversioned.headers().get(reqwest::header::CONTENT_TYPE).and_then(|value| value.to_str().ok()),
      Some("application/x-protobuf")
    );

    shutdown.cancel();
    server.await.expect("join server").expect("serve TCP");
  }

  #[tokio::test]
  async fn rest_discovery_lists_auv_groups_versions_and_resources() {
    let config = ApiServeConfig {
      listen: ListenEndpoint::Tcp {
        host: DEFAULT_API_HOST.to_string(),
        port: 0,
      },
      additional_listeners: Vec::new(),
      store_root: api_temp_store_root("rest-resource-discovery"),
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      first_party_runners: Default::default(),
    };
    let bound = bind(config).await.expect("bind TCP server");
    let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
      panic!("TCP endpoint");
    };
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(bound.serve(shutdown.clone()));
    install_tls_crypto_provider();
    let client = reqwest::Client::builder().http2_prior_knowledge().build().expect("build REST test client");

    let namespace = client.get(format!("http://{address}/apis/auv")).send().await.expect("request AUV namespace discovery");
    assert_eq!(namespace.status(), reqwest::StatusCode::OK);
    let namespace = core_proto::GetApiNamespaceResponse::decode(namespace.bytes().await.expect("read namespace discovery"))
      .expect("decode namespace discovery");
    assert_eq!(namespace.namespace, "auv");
    assert_eq!(
      namespace.groups,
      vec![
        core_proto::ApiGroup {
          name: "core".to_string(),
          versions: vec!["v1".to_string()],
        },
        core_proto::ApiGroup {
          name: "runtime".to_string(),
          versions: vec!["v1".to_string()],
        },
      ]
    );

    let core = client.get(format!("http://{address}/apis/auv/core/v1")).send().await.expect("request core discovery");
    assert_eq!(core.status(), reqwest::StatusCode::OK);
    let core =
      core_proto::GetApiGroupVersionResponse::decode(core.bytes().await.expect("read core discovery")).expect("decode core discovery");
    assert_eq!(core.group, "core");
    assert_eq!(core.version, "v1");
    assert_eq!(core.resources.len(), 2);
    assert_eq!(core.resources[0].name, "devices");
    assert_eq!(
      core.resources[0].operations,
      vec![
        core_proto::ApiResourceOperation::List as i32,
        core_proto::ApiResourceOperation::Get as i32
      ]
    );
    assert_eq!(core.resources[1].name, "services");
    assert_eq!(core.resources[1].operations, vec![core_proto::ApiResourceOperation::List as i32]);

    let runtime = client.get(format!("http://{address}/apis/auv/runtime/v1")).send().await.expect("request runtime discovery");
    assert_eq!(runtime.status(), reqwest::StatusCode::OK);
    let runtime = core_proto::GetApiGroupVersionResponse::decode(runtime.bytes().await.expect("read runtime discovery"))
      .expect("decode runtime discovery");
    assert_eq!(
      runtime.resources.iter().map(|resource| resource.name.as_str()).collect::<Vec<_>>(),
      vec!["runners", "runnerclasses", "runnerleases", "runs"]
    );
    assert_eq!(
      runtime.resources[0].operations,
      vec![
        core_proto::ApiResourceOperation::List as i32,
        core_proto::ApiResourceOperation::Get as i32,
      ],
      "Runner mutations are not discoverable until a provider exists"
    );
    assert_eq!(
      runtime.resources[1].operations,
      vec![
        core_proto::ApiResourceOperation::List as i32,
        core_proto::ApiResourceOperation::Get as i32,
      ]
    );
    assert_eq!(
      runtime.resources[2].operations,
      vec![
        core_proto::ApiResourceOperation::Create as i32,
        core_proto::ApiResourceOperation::Delete as i32,
      ],
      "Runner lease mutations are visible to control-manage authority"
    );
    assert_eq!(
      runtime.resources[3].operations,
      vec![
        core_proto::ApiResourceOperation::List as i32,
        core_proto::ApiResourceOperation::Get as i32,
        core_proto::ApiResourceOperation::Create as i32,
      ]
    );

    shutdown.cancel();
    server.await.expect("join server").expect("serve TCP");
  }

  #[tokio::test]
  async fn rest_devices_list_and_get_the_persistent_local_device() {
    let store_root = api_temp_store_root("rest-devices");
    let (first_id, first_server) = {
      let bound = bind(ApiServeConfig {
        listen: ListenEndpoint::Tcp {
          host: DEFAULT_API_HOST.to_string(),
          port: 0,
        },
        additional_listeners: Vec::new(),
        store_root: store_root.clone(),
        daemon_idle_timeout: None,
        runner_providers: Vec::new(),
        first_party_runners: Default::default(),
      })
      .await
      .expect("bind first server");
      let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
        panic!("TCP endpoint");
      };
      let shutdown = CancellationToken::new();
      let server = tokio::spawn(bound.serve(shutdown.clone()));
      install_tls_crypto_provider();
      let client = reqwest::Client::builder().http2_prior_knowledge().build().expect("build REST test client");
      let response = client.get(format!("http://{address}/apis/auv/core/v1/devices")).send().await.expect("list devices");
      assert_eq!(response.status(), reqwest::StatusCode::OK);
      let response = core_proto::ListDevicesResponse::decode(response.bytes().await.expect("read devices")).expect("decode devices");
      assert_eq!(response.devices.len(), 1);
      let device = response.devices.into_iter().next().expect("local device");
      assert!(device.local);
      let device_id = device.r#ref.as_ref().expect("device ref").device_id.clone();
      assert!(device_id.starts_with("device_"));

      let get = client.get(format!("http://{address}/apis/auv/core/v1/devices/{device_id}")).send().await.expect("get device");
      assert_eq!(get.status(), reqwest::StatusCode::OK);
      let get = core_proto::GetDeviceResponse::decode(get.bytes().await.expect("read device")).expect("decode device");
      assert_eq!(get.device, Some(device));
      shutdown.cancel();
      (device_id, server)
    };
    first_server.await.expect("join first server").expect("serve first server");

    let bound = bind(ApiServeConfig {
      listen: ListenEndpoint::Tcp {
        host: DEFAULT_API_HOST.to_string(),
        port: 0,
      },
      additional_listeners: Vec::new(),
      store_root,
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      first_party_runners: Default::default(),
    })
    .await
    .expect("bind restarted server");
    let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
      panic!("TCP endpoint");
    };
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(bound.serve(shutdown.clone()));
    let client = reqwest::Client::builder().http2_prior_knowledge().build().expect("build restarted REST test client");
    let response =
      client.get(format!("http://{address}/apis/auv/core/v1/devices/{first_id}")).send().await.expect("get persistent local device");
    assert_eq!(response.status(), reqwest::StatusCode::OK);

    shutdown.cancel();
    server.await.expect("join restarted server").expect("serve restarted server");
  }

  #[tokio::test]
  async fn rest_runs_create_list_and_get_under_the_local_device() {
    let bound = bind(ApiServeConfig {
      listen: ListenEndpoint::Tcp {
        host: DEFAULT_API_HOST.to_string(),
        port: 0,
      },
      additional_listeners: Vec::new(),
      store_root: api_temp_store_root("rest-runs"),
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      first_party_runners: Default::default(),
    })
    .await
    .expect("bind server");
    let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
      panic!("TCP endpoint");
    };
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(bound.serve(shutdown.clone()));
    install_tls_crypto_provider();
    let client = reqwest::Client::builder().http2_prior_knowledge().build().expect("build REST test client");

    let create = client
      .post(format!("http://{address}/apis/auv/runtime/v1/runs"))
      .header("content-type", "application/protobuf")
      .body(
        core_proto::CreateRunRequest {
          labels: std::collections::HashMap::from([("purpose".to_string(), "test".to_string())]),
          devices: Vec::new(),
        }
        .encode_to_vec(),
      )
      .send()
      .await
      .expect("create Run");
    assert_eq!(create.status(), reqwest::StatusCode::OK);
    let created = core_proto::CreateRunResponse::decode(create.bytes().await.expect("read created Run")).expect("decode created Run");
    let run = created.run.expect("created Run");
    let run_id = run.r#ref.as_ref().expect("Run ref").run_id.clone();
    assert!(run_id.starts_with("run_"));
    assert_eq!(run.phase, core_proto::RunPhase::Running as i32);
    assert_eq!(run.devices.len(), 1);
    assert_eq!(run.labels.get("purpose").map(String::as_str), Some("test"));

    let list = client.get(format!("http://{address}/apis/auv/runtime/v1/runs")).send().await.expect("list Runs");
    assert_eq!(list.status(), reqwest::StatusCode::OK);
    let list = core_proto::ListRunsResponse::decode(list.bytes().await.expect("read Runs")).expect("decode Runs");
    assert_eq!(list.runs, vec![run.clone()]);

    let get = client.get(format!("http://{address}/apis/auv/runtime/v1/runs/{run_id}")).send().await.expect("get Run");
    assert_eq!(get.status(), reqwest::StatusCode::OK);
    let get = core_proto::GetRunResponse::decode(get.bytes().await.expect("read Run")).expect("decode Run");
    assert_eq!(get.run, Some(run));

    shutdown.cancel();
    server.await.expect("join server").expect("serve server");
  }

  #[tokio::test]
  async fn rest_runners_are_empty_and_creation_fails_without_a_provider() {
    let bound = bind(ApiServeConfig {
      listen: ListenEndpoint::Tcp {
        host: DEFAULT_API_HOST.to_string(),
        port: 0,
      },
      additional_listeners: Vec::new(),
      store_root: api_temp_store_root("rest-runners"),
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      first_party_runners: Default::default(),
    })
    .await
    .expect("bind server");
    let BoundEndpoint::Tcp(address) = bound.endpoint().clone() else {
      panic!("TCP endpoint");
    };
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(bound.serve(shutdown.clone()));
    install_tls_crypto_provider();
    let client = reqwest::Client::builder().http2_prior_knowledge().build().expect("build REST test client");

    let list = client.get(format!("http://{address}/apis/auv/runtime/v1/runners")).send().await.expect("list Runners");
    assert_eq!(list.status(), reqwest::StatusCode::OK);
    let list = core_proto::ListRunnersResponse::decode(list.bytes().await.expect("read Runners")).expect("decode Runners");
    assert!(list.runners.is_empty());

    let create = client
      .post(format!("http://{address}/apis/auv/runtime/v1/runners"))
      .header("content-type", "application/protobuf")
      .body(
        core_proto::CreateRunnerRequest {
          device: None,
          runner_class: Some(core_proto::RunnerClassRef {
            runner_class: "auv.core.local".to_string(),
          }),
          labels: std::collections::HashMap::new(),
          lifecycle: core_proto::RunnerLifecycle::UnlessShutdown as i32,
          idle_timeout: None,
        }
        .encode_to_vec(),
      )
      .send()
      .await
      .expect("create Runner without provider");
    assert_eq!(create.status(), reqwest::StatusCode::NOT_IMPLEMENTED);
    assert_eq!(create.headers().get("content-type").and_then(|value| value.to_str().ok()), Some("application/problem+json"));

    shutdown.cancel();
    server.await.expect("join server").expect("serve server");
  }

  #[tokio::test]
  async fn paired_mtls_derives_identity_and_enforces_current_scopes() {
    let directory = tempfile::tempdir().expect("temporary TLS directory");
    let socket_path = directory.path().join("auv.sock");
    let (ca, issuer) = test_certificate_authority();
    let (server_certificate, server_key) = test_leaf_certificate(&issuer, "localhost", ExtendedKeyUsagePurpose::ServerAuth);
    let (paired_certificate, paired_key) = test_leaf_certificate(&issuer, "paired-client", ExtendedKeyUsagePurpose::ClientAuth);
    let (limited_certificate, limited_key) = test_leaf_certificate(&issuer, "limited-client", ExtendedKeyUsagePurpose::ClientAuth);
    let (unpaired_certificate, unpaired_key) = test_leaf_certificate(&issuer, "unpaired-client", ExtendedKeyUsagePurpose::ClientAuth);

    let server_certificate_path = directory.path().join("server.pem");
    let server_private_key_path = directory.path().join("server-key.pem");
    let client_ca_path = directory.path().join("client-ca.pem");
    std::fs::write(&server_certificate_path, server_certificate.pem()).expect("write server certificate");
    std::fs::write(&server_private_key_path, server_key.serialize_pem()).expect("write server key");
    std::fs::write(&client_ca_path, ca.pem()).expect("write client CA");

    let pairing_store_path = directory.path().join("authority").join("pairings.json");
    let pairing_store = PairingStore::open(pairing_store_path.clone()).expect("open pairing store for provisioning");
    pairing_store
      .upsert(PairingRecord {
        pair_id: "workstation-owner".to_string(),
        label: "owner workstation".to_string(),
        enabled: true,
        scopes: vec![
          ApiScope::OperationsExecute,
          ApiScope::ControlInspect,
          ApiScope::ControlManage,
        ],
        credentials: vec![PairingCredential {
          certificate_fingerprint: CertificateFingerprint::from_der(paired_certificate.der().as_ref()),
          state: CredentialState::Active,
        }],
      })
      .expect("provision paired client");
    pairing_store
      .upsert(PairingRecord {
        pair_id: "auditor".to_string(),
        label: "read-only auditor".to_string(),
        enabled: true,
        scopes: vec![ApiScope::ControlInspect],
        credentials: vec![PairingCredential {
          certificate_fingerprint: CertificateFingerprint::from_der(limited_certificate.der().as_ref()),
          state: CredentialState::Active,
        }],
      })
      .expect("provision limited client");
    drop(pairing_store);

    let bound = bind(ApiServeConfig {
      listen: ListenEndpoint::RemoteTls {
        host: "127.0.0.1".to_string(),
        port: 0,
        server_certificate: server_certificate_path,
        server_private_key: server_private_key_path,
        client_ca_certificate: client_ca_path,
        pairing_store: pairing_store_path,
      },
      additional_listeners: vec![ListenEndpoint::Unix {
        path: socket_path.clone(),
      }],
      store_root: directory.path().join("runs"),
      daemon_idle_timeout: None,
      runner_providers: vec![catalog_provider(directory.path())],
      first_party_runners: Default::default(),
    })
    .await
    .expect("bind local Unix and paired mTLS listeners");
    assert_eq!(bound.endpoints().len(), 2);
    assert_eq!(bound.discovery_endpoint(), Some(&BoundEndpoint::Unix(socket_path.clone())));
    let address = bound
      .endpoints()
      .iter()
      .find_map(|endpoint| match endpoint {
        BoundEndpoint::RemoteTls(address) => Some(*address),
        _ => None,
      })
      .expect("remote TLS endpoint");
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(bound.serve(shutdown.clone()));
    let endpoint = format!("https://{address}");
    let connect_config = |certificate: &Certificate, key: &KeyPair| PairedConnectConfig {
      endpoint: endpoint.parse().expect("valid TLS endpoint"),
      server_name: "localhost".to_string(),
      server_ca_certificate_pem: ca.pem().into_bytes(),
      client_certificate_pem: certificate.pem().into_bytes(),
      client_private_key_pem: key.serialize_pem().into_bytes(),
    };

    let mut local = Client::connect(ConnectEndpoint::Unix(socket_path.clone())).await.expect("connect local Unix client");
    let local_devices = local.list_devices().await.expect("local client lists canonical Device");
    local.create_run(core_proto::CreateRunRequest::default()).await.expect("local client creates shared Run");
    let mut paired = Client::connect_paired(connect_config(&paired_certificate, &paired_key)).await.expect("connect paired client");
    paired.clone().placement().local().expect_err("paired remote transport cannot satisfy caller-local placement");
    assert_eq!(paired.list_devices().await.expect("paired client lists canonical Device"), local_devices);
    assert!(paired.list_runs().await.expect("paired Runs remain principal-scoped").is_empty());
    paired.create_run(core_proto::CreateRunRequest::default()).await.expect("paired client creates Run");
    assert_eq!(local.list_runs().await.expect("local Runs remain principal-scoped").len(), 1);
    assert_eq!(paired.list_runs().await.expect("paired client lists its Run").len(), 1);
    let mut paired_input =
      connect_paired_input(connect_config(&paired_certificate, &paired_key)).await.expect("connect paired Input client");
    let admitted = paired_input
      .press_key(driver_proto::PressKeyRequest::default())
      .await
      .expect_err("authorized Input call reaches typed request validation");
    assert_eq!(admitted.code(), Code::InvalidArgument);
    let devices = paired_rest_client(&ca, &paired_certificate, &paired_key)
      .get(format!("https://localhost:{}/apis/auv/core/v1/devices", address.port()))
      .send()
      .await
      .expect("list Devices through paired REST");
    assert_eq!(devices.status(), reqwest::StatusCode::OK);
    let services = paired_rest_client(&ca, &paired_certificate, &paired_key)
      .get(format!("https://localhost:{}/apis/auv/core/v1/services", address.port()))
      .send()
      .await
      .expect("discover executable services through paired REST");
    let services = core_proto::ListServicesResponse::decode(services.bytes().await.expect("read paired service catalog"))
      .expect("decode paired service catalog");
    assert!(services.services.iter().any(|service| service.runner_class == "auv.test.catalog"));

    let mut limited = Client::connect_paired(connect_config(&limited_certificate, &limited_key)).await.expect("connect limited client");
    assert_eq!(limited.list_devices().await.expect("limited client may inspect Devices").len(), 1);
    let mut limited_input =
      connect_paired_input(connect_config(&limited_certificate, &limited_key)).await.expect("connect limited Input client");
    let input_denied =
      limited_input.press_key(driver_proto::PressKeyRequest::default()).await.expect_err("ControlInspect does not authorize input delivery");
    assert_eq!(input_denied.code(), Code::PermissionDenied);
    let screen_click_denied = limited_input
      .click_screen_point(driver_proto::ClickScreenPointRequest::default())
      .await
      .expect_err("ControlInspect does not authorize screen-point input delivery");
    assert_eq!(screen_click_denied.code(), Code::PermissionDenied);
    let paste_denied = limited_input
      .paste_text(driver_proto::PasteTextRequest::default())
      .await
      .expect_err("ControlInspect does not authorize clipboard-backed input delivery");
    assert_eq!(paste_denied.code(), Code::PermissionDenied);
    let mut limited_permission =
      connect_paired_permission(connect_config(&limited_certificate, &limited_key)).await.expect("connect limited Permission client");
    let permission_denied = limited_permission
      .probe_permissions(macos_proto::ProbePermissionsRequest::default())
      .await
      .expect_err("ControlInspect does not authorize permission probing");
    assert_eq!(permission_denied.code(), Code::PermissionDenied);
    let mut limited_application =
      connect_paired_application(connect_config(&limited_certificate, &limited_key)).await.expect("connect limited Application client");
    let application_denied = limited_application
      .activate_bundle_id(macos_proto::ActivateBundleIdRequest::default())
      .await
      .expect_err("ControlInspect does not authorize application activation");
    assert_eq!(application_denied.code(), Code::PermissionDenied);
    let mut limited_accessibility =
      connect_paired_accessibility(connect_config(&limited_certificate, &limited_key)).await.expect("connect limited Accessibility client");
    let accessibility_denied = limited_accessibility
      .focus_text(macos_proto::FocusTextRequest::default())
      .await
      .expect_err("ControlInspect does not authorize accessibility input delivery");
    assert_eq!(accessibility_denied.code(), Code::PermissionDenied);
    let mut limited_media =
      connect_paired_media_control(connect_config(&limited_certificate, &limited_key)).await.expect("connect limited MediaControl client");
    let media_denied = limited_media
      .get_now_playing(macos_proto::GetNowPlayingRequest::default())
      .await
      .expect_err("ControlInspect does not authorize now-playing reads");
    assert_eq!(media_denied.code(), Code::PermissionDenied);
    let media_input_denied =
      limited_media.play(macos_proto::PlayRequest::default()).await.expect_err("ControlInspect does not authorize media input delivery");
    assert_eq!(media_input_denied.code(), Code::PermissionDenied);
    let mut limited_overlay =
      connect_paired_overlay(connect_config(&limited_certificate, &limited_key)).await.expect("connect limited Overlay client");
    let overlay_denied = limited_overlay
      .show_overlay(driver_proto::ShowOverlayRequest::default())
      .await
      .expect_err("ControlInspect does not authorize overlay mutation");
    assert_eq!(overlay_denied.code(), Code::PermissionDenied);
    let denied = limited.create_run(core_proto::CreateRunRequest::default()).await.expect_err("limited client cannot manage Runs");
    assert_eq!(denied.code(), Code::PermissionDenied);
    let rest_denied = paired_rest_client(&ca, &limited_certificate, &limited_key)
      .post(format!("https://localhost:{}/apis/auv/runtime/v1/runs", address.port()))
      .header("content-type", "application/protobuf")
      .body(core_proto::CreateRunRequest::default().encode_to_vec())
      .send()
      .await
      .expect("limited REST request completes");
    assert_eq!(rest_denied.status(), reqwest::StatusCode::FORBIDDEN);
    let control_allowed = paired_rest_client(&ca, &limited_certificate, &limited_key)
      .get(format!("https://localhost:{}/apis/auv/core/v1/devices", address.port()))
      .send()
      .await
      .expect("limited control request completes");
    assert_eq!(control_allowed.status(), reqwest::StatusCode::OK);
    let discovery = paired_rest_client(&ca, &limited_certificate, &limited_key)
      .get(format!("https://localhost:{}/apis/auv/runtime/v1", address.port()))
      .send()
      .await
      .expect("limited discovery request completes");
    assert_eq!(discovery.status(), reqwest::StatusCode::OK);
    let discovery = core_proto::GetApiGroupVersionResponse::decode(discovery.bytes().await.expect("read limited discovery"))
      .expect("decode limited discovery");
    assert!(discovery.resources.iter().all(|resource| {
      resource.operations.iter().all(|operation| {
        matches!(
          core_proto::ApiResourceOperation::try_from(*operation),
          Ok(core_proto::ApiResourceOperation::List | core_proto::ApiResourceOperation::Get)
        )
      })
    }));
    let limited_services = paired_rest_client(&ca, &limited_certificate, &limited_key)
      .get(format!("https://localhost:{}/apis/auv/core/v1/services", address.port()))
      .send()
      .await
      .expect("limited service discovery request completes");
    assert_eq!(limited_services.status(), reqwest::StatusCode::OK);
    let limited_services = core_proto::ListServicesResponse::decode(limited_services.bytes().await.expect("read limited service catalog"))
      .expect("decode limited service catalog");
    assert!(limited_services.services.is_empty(), "principal without operations_execute cannot see callable methods");

    let mut unpaired = Client::connect_paired(connect_config(&unpaired_certificate, &unpaired_key)).await.expect("connect unpaired client");
    let unauthenticated = unpaired.list_devices().await.expect_err("unpaired certificate is rejected");
    assert_eq!(unauthenticated.code(), Code::Unauthenticated);
    let rest_unauthenticated = paired_rest_client(&ca, &unpaired_certificate, &unpaired_key)
      .get(format!("https://localhost:{}/apis/auv/core/v1/devices", address.port()))
      .send()
      .await
      .expect("unpaired REST request completes");
    assert_eq!(rest_unauthenticated.status(), reqwest::StatusCode::UNAUTHORIZED);

    shutdown.cancel();
    server.await.expect("join server").expect("serve paired mTLS");
    assert!(!socket_path.exists());
  }

  #[cfg(unix)]
  #[tokio::test]
  async fn unix_client_uses_typed_services_and_cleans_up_socket() {
    let directory = tempfile::tempdir().expect("temporary Unix socket directory");
    let socket_path = directory.path().join("auv.sock");
    let config = ApiServeConfig {
      listen: ListenEndpoint::Unix {
        path: socket_path.clone(),
      },
      additional_listeners: Vec::new(),
      store_root: api_temp_store_root("unix"),
      daemon_idle_timeout: None,
      runner_providers: Vec::new(),
      first_party_runners: Default::default(),
    };
    let bound = bind(config).await.expect("bind Unix server");
    assert!(socket_path.exists());
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(std::fs::metadata(&socket_path).unwrap().permissions().mode() & 0o777, 0o600);
    let shutdown = CancellationToken::new();
    let server = tokio::spawn(bound.serve(shutdown.clone()));
    let mut client = Client::connect(ConnectEndpoint::Unix(socket_path.clone())).await.expect("connect Unix client");
    let implicit = client.placement().local().expect("local transport").run(RunOptions::default()).await.expect("implicit local Run");
    assert!(implicit.is_owned());
    assert!(implicit.device().is_some_and(|device| device.local));
    let implicit_id = implicit.resource().r#ref.as_ref().expect("Run ref").run_id.clone();
    let completed = implicit.finish_if_owned(core_proto::RunOutcome::Succeeded).await.expect("finish owned Run");
    assert_eq!(completed.phase, core_proto::RunPhase::Succeeded as i32);

    let mut unavailable = auv_api_client::placement::RunnerOptions::default();
    unavailable.runner_class = "auv.test.missing".to_string();
    client.placement().runner(unavailable).await.expect_err("failed implicit claim must surface");
    let canceled = client
      .list_runs()
      .await
      .expect("list compensated Runs")
      .into_iter()
      .filter(|run| run.phase == core_proto::RunPhase::Canceled as i32)
      .count();
    assert_eq!(canceled, 1, "a failed claim compensates its implicitly created Run");

    let attached_source = client.create_run(core_proto::CreateRunRequest::default()).await.expect("create attached Run");
    let attached_id = attached_source.r#ref.as_ref().expect("Run ref").run_id.clone();
    let attached = client
      .placement()
      .run(RunOptions {
        selection: RunSelection::Existing(attached_id.clone()),
        ..Default::default()
      })
      .await
      .expect("attach existing Run");
    assert!(!attached.is_owned());
    let still_running = attached.finish_if_owned(core_proto::RunOutcome::Failed).await.expect("borrowed finish is a no-op");
    assert_eq!(still_running.phase, core_proto::RunPhase::Running as i32);
    assert_eq!(client.get_run(attached_id.clone()).await.expect("borrowed Run remains").phase, core_proto::RunPhase::Running as i32);
    client.stop_run(attached_id, core_proto::RunOutcome::Canceled).await.expect("cleanup borrowed Run");
    assert_eq!(client.get_run(implicit_id).await.expect("completed Run remains queryable").phase, core_proto::RunPhase::Succeeded as i32);

    shutdown.cancel();
    server.await.expect("join server").expect("serve Unix");
    assert!(!socket_path.exists());
  }

  fn test_certificate_authority() -> (Certificate, Issuer<'static, KeyPair>) {
    let mut params = CertificateParams::new(Vec::<String>::new()).expect("empty CA subject names");
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![
      KeyUsagePurpose::DigitalSignature,
      KeyUsagePurpose::KeyCertSign,
      KeyUsagePurpose::CrlSign,
    ];
    let key = KeyPair::generate().expect("generate CA key");
    let certificate = params.self_signed(&key).expect("self-sign CA");
    (certificate, Issuer::new(params, key))
  }

  fn test_leaf_certificate(issuer: &Issuer<'static, KeyPair>, name: &str, purpose: ExtendedKeyUsagePurpose) -> (Certificate, KeyPair) {
    let mut params = CertificateParams::new(vec![name.to_string()]).expect("valid test subject name");
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![purpose];
    let key = KeyPair::generate().expect("generate leaf key");
    let certificate = params.signed_by(&key, issuer).expect("sign leaf certificate");
    (certificate, key)
  }

  fn paired_rest_client(ca: &Certificate, certificate: &Certificate, key: &KeyPair) -> reqwest::Client {
    let identity = format!("{}{}", certificate.pem(), key.serialize_pem());
    reqwest::Client::builder()
      .http2_prior_knowledge()
      .add_root_certificate(reqwest::Certificate::from_pem(ca.pem().as_bytes()).expect("parse test CA"))
      .identity(reqwest::Identity::from_pem(identity.as_bytes()).expect("parse test client identity"))
      .build()
      .expect("build paired REST client")
  }

  async fn connect_paired_input(
    config: PairedConnectConfig,
  ) -> Result<InputServiceClient<tonic::transport::Channel>, tonic::transport::Error> {
    install_tls_crypto_provider();
    let tls = tonic::transport::ClientTlsConfig::new()
      .domain_name(config.server_name)
      .ca_certificate(tonic::transport::Certificate::from_pem(config.server_ca_certificate_pem))
      .identity(tonic::transport::Identity::from_pem(config.client_certificate_pem, config.client_private_key_pem));
    let channel = tonic::transport::Endpoint::from_shared(config.endpoint.to_string())?.tls_config(tls)?.connect().await?;
    Ok(InputServiceClient::new(channel))
  }

  async fn connect_paired_overlay(
    config: PairedConnectConfig,
  ) -> Result<OverlayServiceClient<tonic::transport::Channel>, tonic::transport::Error> {
    install_tls_crypto_provider();
    let tls = tonic::transport::ClientTlsConfig::new()
      .domain_name(config.server_name)
      .ca_certificate(tonic::transport::Certificate::from_pem(config.server_ca_certificate_pem))
      .identity(tonic::transport::Identity::from_pem(config.client_certificate_pem, config.client_private_key_pem));
    let channel = tonic::transport::Endpoint::from_shared(config.endpoint.to_string())?.tls_config(tls)?.connect().await?;
    Ok(OverlayServiceClient::new(channel))
  }

  async fn connect_paired_permission(
    config: PairedConnectConfig,
  ) -> Result<PermissionServiceClient<tonic::transport::Channel>, tonic::transport::Error> {
    install_tls_crypto_provider();
    let tls = tonic::transport::ClientTlsConfig::new()
      .domain_name(config.server_name)
      .ca_certificate(tonic::transport::Certificate::from_pem(config.server_ca_certificate_pem))
      .identity(tonic::transport::Identity::from_pem(config.client_certificate_pem, config.client_private_key_pem));
    let channel = tonic::transport::Endpoint::from_shared(config.endpoint.to_string())?.tls_config(tls)?.connect().await?;
    Ok(PermissionServiceClient::new(channel))
  }

  async fn connect_paired_media_control(
    config: PairedConnectConfig,
  ) -> Result<MediaControlServiceClient<tonic::transport::Channel>, tonic::transport::Error> {
    install_tls_crypto_provider();
    let tls = tonic::transport::ClientTlsConfig::new()
      .domain_name(config.server_name)
      .ca_certificate(tonic::transport::Certificate::from_pem(config.server_ca_certificate_pem))
      .identity(tonic::transport::Identity::from_pem(config.client_certificate_pem, config.client_private_key_pem));
    let channel = tonic::transport::Endpoint::from_shared(config.endpoint.to_string())?.tls_config(tls)?.connect().await?;
    Ok(MediaControlServiceClient::new(channel))
  }

  async fn connect_paired_application(
    config: PairedConnectConfig,
  ) -> Result<ApplicationServiceClient<tonic::transport::Channel>, tonic::transport::Error> {
    install_tls_crypto_provider();
    let tls = tonic::transport::ClientTlsConfig::new()
      .domain_name(config.server_name)
      .ca_certificate(tonic::transport::Certificate::from_pem(config.server_ca_certificate_pem))
      .identity(tonic::transport::Identity::from_pem(config.client_certificate_pem, config.client_private_key_pem));
    let channel = tonic::transport::Endpoint::from_shared(config.endpoint.to_string())?.tls_config(tls)?.connect().await?;
    Ok(ApplicationServiceClient::new(channel))
  }

  async fn connect_paired_accessibility(
    config: PairedConnectConfig,
  ) -> Result<AccessibilityServiceClient<tonic::transport::Channel>, tonic::transport::Error> {
    install_tls_crypto_provider();
    let tls = tonic::transport::ClientTlsConfig::new()
      .domain_name(config.server_name)
      .ca_certificate(tonic::transport::Certificate::from_pem(config.server_ca_certificate_pem))
      .identity(tonic::transport::Identity::from_pem(config.client_certificate_pem, config.client_private_key_pem));
    let channel = tonic::transport::Endpoint::from_shared(config.endpoint.to_string())?.tls_config(tls)?.connect().await?;
    Ok(AccessibilityServiceClient::new(channel))
  }
}
