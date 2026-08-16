//! Connection, authentication, and routed transport for one gRPC daemon.

use super::clients;

use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use auv_api_proto::auv::api::daemon::v1::device_service_client::DeviceServiceClient;
use auv_api_proto::auv::api::daemon::v1::discovery_service_client::DiscoveryServiceClient;
use auv_api_proto::auv::api::daemon::v1::pairing_service_client::PairingServiceClient;
use auv_api_proto::auv::api::daemon::v1::run_service_client::RunServiceClient;
use auv_api_proto::auv::api::daemon::v1::runner_class_service_client::RunnerClassServiceClient;
use auv_api_proto::auv::api::daemon::v1::runner_service_client::RunnerServiceClient;
use tonic::metadata::{Ascii, MetadataValue};
use tonic::service::Interceptor;
use tonic::transport::{Channel, Endpoint};

pub const ROUTE_DEVICE_METADATA: &str = "auv-device-id";
pub const ROUTE_RUN_METADATA: &str = "auv-run-id";
pub const ROUTE_RUNNER_CLASS_METADATA: &str = "auv-runner-class";

// Tonic requires an HTTP origin even when a custom connector supplies local
// IPC. The connector discards this URI and opens the socket or pipe directly.
const LOCAL_IPC_ORIGIN: &str = "http://localhost";

/// Daemon routing input carried as gRPC metadata, independently of the
/// application-owned protobuf request and response messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerRoute {
  pub device_id: Option<String>,
  pub run_id: Option<String>,
  pub runner_class: String,
}

#[derive(Clone, Debug)]
pub struct RunnerRouteInterceptor {
  device_id: Option<MetadataValue<Ascii>>,
  run_id: Option<MetadataValue<Ascii>>,
  runner_class: MetadataValue<Ascii>,
  authorization: Option<MetadataValue<Ascii>>,
}

impl RunnerRouteInterceptor {
  pub fn new(route: RunnerRoute) -> Result<Self, tonic::Status> {
    if route.runner_class.trim().is_empty() {
      return Err(tonic::Status::invalid_argument("Runner route must include runner_class"));
    }
    let metadata = |name: &'static str, value: String| {
      value.parse::<MetadataValue<Ascii>>().map_err(|_| tonic::Status::invalid_argument(format!("{name} is not valid gRPC metadata")))
    };
    Ok(Self {
      device_id: route.device_id.map(|value| metadata(ROUTE_DEVICE_METADATA, value)).transpose()?,
      run_id: route.run_id.map(|value| metadata(ROUTE_RUN_METADATA, value)).transpose()?,
      runner_class: metadata(ROUTE_RUNNER_CLASS_METADATA, route.runner_class)?,
      authorization: None,
    })
  }

  fn with_authorization(mut self, authorization: Option<MetadataValue<Ascii>>) -> Self {
    self.authorization = authorization;
    self
  }
}

impl Interceptor for RunnerRouteInterceptor {
  fn call(&mut self, mut request: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
    for name in [
      ROUTE_DEVICE_METADATA,
      ROUTE_RUN_METADATA,
      ROUTE_RUNNER_CLASS_METADATA,
    ] {
      if request.metadata().contains_key(name) {
        return Err(tonic::Status::invalid_argument(format!("{name} metadata is already present")));
      }
    }
    if let Some(value) = &self.device_id {
      request.metadata_mut().insert(ROUTE_DEVICE_METADATA, value.clone());
    }
    if let Some(value) = &self.run_id {
      request.metadata_mut().insert(ROUTE_RUN_METADATA, value.clone());
    }
    request.metadata_mut().insert(ROUTE_RUNNER_CLASS_METADATA, self.runner_class.clone());
    if let Some(authorization) = &self.authorization {
      request.metadata_mut().insert("authorization", authorization.clone());
    }
    Ok(request)
  }
}

pub type RoutedTransport = tonic::service::interceptor::InterceptedService<Channel, RunnerRouteInterceptor>;

/// Endpoint and opaque bearer credential for one paired remote daemon.
#[derive(Clone)]
pub struct PairedConnectConfig {
  pub endpoint: http::Uri,
  pub device_credential: String,
}

impl fmt::Debug for PairedConnectConfig {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.debug_struct("PairedConnectConfig").field("endpoint", &self.endpoint).field("device_credential", &"[REDACTED]").finish()
  }
}

#[derive(Debug, thiserror::Error)]
pub enum PairedConnectError {
  #[error(transparent)]
  Transport(#[from] tonic::transport::Error),
}

#[derive(Clone, Debug)]
pub(crate) struct AuthorizationInterceptor {
  authorization: Option<MetadataValue<Ascii>>,
}

impl Interceptor for AuthorizationInterceptor {
  fn call(&mut self, mut request: tonic::Request<()>) -> Result<tonic::Request<()>, tonic::Status> {
    if let Some(authorization) = &self.authorization {
      request.metadata_mut().insert("authorization", authorization.clone());
    }
    Ok(request)
  }
}

pub(crate) type ApiTransport = tonic::service::interceptor::InterceptedService<Channel, AuthorizationInterceptor>;

/// Address of an AUV API server from the client's point of view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectEndpoint {
  /// gRPC over HTTP/2 to a TCP endpoint, for example `http://127.0.0.1:9847`.
  Tcp(http::Uri),
  /// gRPC over HTTP/2 carried by a local Unix domain socket.
  #[cfg(unix)]
  Unix(PathBuf),
  /// gRPC over HTTP/2 carried by a local Windows named pipe.
  #[cfg(windows)]
  NamedPipe(String),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum EndpointParseError {
  #[error("invalid AUV API endpoint URI: {0}")]
  InvalidUri(String),
  #[error("Unix endpoint path must be absolute: {0}")]
  RelativeUnixPath(String),
  #[error("invalid Windows named-pipe endpoint: {0}")]
  InvalidNamedPipe(String),
  #[error("unsupported AUV API endpoint scheme: {0}")]
  UnsupportedScheme(String),
}

impl FromStr for ConnectEndpoint {
  type Err = EndpointParseError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    if let Some(path) = value.strip_prefix("unix://") {
      let path = PathBuf::from(path);
      if !path.is_absolute() {
        return Err(EndpointParseError::RelativeUnixPath(value.to_string()));
      }
      #[cfg(unix)]
      return Ok(Self::Unix(path));
      #[cfg(not(unix))]
      return Err(EndpointParseError::UnsupportedScheme("unix".to_string()));
    }
    if let Some(name) = value.strip_prefix("npipe://./pipe/") {
      if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')) {
        return Err(EndpointParseError::InvalidNamedPipe(value.to_string()));
      }
      #[cfg(windows)]
      return Ok(Self::NamedPipe(name.to_string()));
      #[cfg(not(windows))]
      return Err(EndpointParseError::UnsupportedScheme("npipe".to_string()));
    }

    let uri = value.parse::<http::Uri>().map_err(|error| EndpointParseError::InvalidUri(error.to_string()))?;
    let scheme = uri.scheme_str().unwrap_or_default();
    if scheme != "http" {
      return Err(EndpointParseError::UnsupportedScheme(scheme.to_string()));
    }
    uri.host().ok_or_else(|| EndpointParseError::InvalidUri("TCP endpoint omitted host".to_string()))?;
    if !matches!(uri.path(), "" | "/") || uri.query().is_some() {
      return Err(EndpointParseError::InvalidUri("TCP endpoint must not include a path or query".to_string()));
    }
    Ok(Self::Tcp(uri))
  }
}

impl std::fmt::Display for ConnectEndpoint {
  fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Tcp(uri) => write!(formatter, "http://{}", uri.authority().expect("validated TCP endpoint has authority")),
      #[cfg(unix)]
      Self::Unix(path) => write!(formatter, "unix://{}", path.display()),
      #[cfg(windows)]
      Self::NamedPipe(name) => write!(formatter, "npipe://./pipe/{name}"),
    }
  }
}

// Endpoint precedence and descriptor discovery remain process-lifecycle policy
// in auv-cli; this transport client connects only to its caller's selection.

/// Transport client connected to one AUV daemon.
#[derive(Clone, Debug)]
pub struct Client {
  channel: Channel,
  authorization: Option<MetadataValue<Ascii>>,
  discovery: DiscoveryServiceClient<ApiTransport>,
  pairing: PairingServiceClient<ApiTransport>,
  device: DeviceServiceClient<ApiTransport>,
  runner: RunnerServiceClient<ApiTransport>,
  runner_class: RunnerClassServiceClient<ApiTransport>,
  run: RunServiceClient<ApiTransport>,
}

impl Client {
  /// Wraps an already established transport channel for embedding and tests.
  pub fn from_channel(channel: Channel) -> Self {
    Self::from_channel_with_authorization(channel, None)
  }

  fn from_channel_with_authorization(channel: Channel, authorization: Option<MetadataValue<Ascii>>) -> Self {
    let transport = || {
      tonic::service::interceptor::InterceptedService::new(
        channel.clone(),
        AuthorizationInterceptor {
          authorization: authorization.clone(),
        },
      )
    };
    Self {
      channel: channel.clone(),
      authorization: authorization.clone(),
      discovery: DiscoveryServiceClient::new(transport()),
      pairing: PairingServiceClient::new(transport()),
      device: DeviceServiceClient::new(transport()),
      runner: RunnerServiceClient::new(transport()),
      runner_class: RunnerClassServiceClient::new(transport()),
      run: RunServiceClient::new(transport()),
    }
  }

  pub fn discovery(&self) -> clients::daemon::v1::discovery::Client {
    clients::daemon::v1::discovery::Client::new(self.discovery.clone())
  }

  pub fn pairing(&self) -> clients::daemon::v1::pairing::Client {
    clients::daemon::v1::pairing::Client::new(self.pairing.clone())
  }

  pub fn devices(&self) -> clients::daemon::v1::device::Client {
    clients::daemon::v1::device::Client::new(self.device.clone())
  }

  pub fn runner_classes(&self) -> clients::daemon::v1::runner_class::Client {
    clients::daemon::v1::runner_class::Client::new(self.runner_class.clone())
  }

  pub fn runs(&self) -> clients::daemon::v1::run::Client {
    clients::daemon::v1::run::Client::new(self.run.clone())
  }

  pub fn runners(&self) -> clients::daemon::v1::runner::Client {
    clients::daemon::v1::runner::Client::new(self.runner.clone())
  }

  /// Builds an opaque transport routed by daemon metadata. Generated clients
  /// use this transport without importing AUV lifecycle messages.
  pub fn routed_transport(&self, route: RunnerRoute) -> Result<RoutedTransport, tonic::Status> {
    Ok(tonic::service::interceptor::InterceptedService::new(
      self.channel.clone(),
      RunnerRouteInterceptor::new(route)?.with_authorization(self.authorization.clone()),
    ))
  }

  /// Connects to one API server without selecting a Device, Run, or Runner.
  pub async fn connect(endpoint: ConnectEndpoint) -> Result<Self, tonic::transport::Error> {
    let channel = match endpoint {
      ConnectEndpoint::Tcp(uri) => Endpoint::from_shared(uri.to_string())?.connect().await?,
      #[cfg(unix)]
      ConnectEndpoint::Unix(path) => {
        let endpoint = Endpoint::from_static(LOCAL_IPC_ORIGIN);
        endpoint
          .connect_with_connector(tower::service_fn(move |_: http::Uri| {
            let path = path.clone();
            async move { tokio::net::UnixStream::connect(path).await.map(hyper_util::rt::TokioIo::new) }
          }))
          .await?
      }
      #[cfg(windows)]
      ConnectEndpoint::NamedPipe(name) => {
        let endpoint = Endpoint::from_static(LOCAL_IPC_ORIGIN);
        endpoint
          .connect_with_connector(tower::service_fn(move |_: http::Uri| {
            let path = format!(r"\\.\pipe\{name}");
            async move { open_named_pipe(path).await }
          }))
          .await?
      }
    };
    Ok(Self::from_channel(channel))
  }

  /// Connects to a paired daemon and adds its opaque bearer to every gRPC request.
  pub async fn connect_paired(config: PairedConnectConfig) -> Result<Self, PairedConnectError> {
    let mut authorization = format!("Bearer {}", config.device_credential)
      .parse::<MetadataValue<Ascii>>()
      .expect("opaque pairing credentials are generated from ASCII-safe encoding");
    authorization.set_sensitive(true);
    let channel = Endpoint::from_shared(config.endpoint.to_string())?.connect().await?;
    Ok(Self::from_channel_with_authorization(channel, Some(authorization)))
  }
}

#[cfg(windows)]
async fn open_named_pipe(path: String) -> std::io::Result<hyper_util::rt::TokioIo<tokio::net::windows::named_pipe::NamedPipeClient>> {
  const ERROR_PIPE_BUSY: i32 = 231;
  // NOTICE(named-pipe-busy-retry): Windows exposes no async accept backlog for
  // named pipes. Retry the transient busy state while the server creates its
  // next instance. Remove this policy if Tokio adds an async WaitNamedPipe API.
  let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
  loop {
    match tokio::net::windows::named_pipe::ClientOptions::new().open(&path) {
      Ok(client) => return Ok(hyper_util::rt::TokioIo::new(client)),
      Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY) && tokio::time::Instant::now() < deadline => {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
      }
      Err(error) => return Err(error),
    }
  }
}

#[cfg(test)]
#[path = "client_test.rs"]
mod tests;
