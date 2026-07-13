//! Loopback-only server lifecycle for the session API.
//!
//! This module owns configuration, listen policy, binding, readiness output,
//! and tonic server lifecycle. RPC adaptation lives in [`super::grpc`].
//!
//! NOTICE: The server intentionally has no TLS or non-loopback mode. Remote
//! access requires a separately approved authentication and transport design.

use std::io::Write;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use auv_api_proto::v1::session::session_service_server::SessionServiceServer;
use tokio::net::TcpListener;
use tokio_stream::wrappers::TcpListenerStream;

use super::grpc::SessionServiceGrpc;
use super::handler::SessionApiHandler;

pub const DEFAULT_SESSION_API_HOST: &str = "127.0.0.1";
pub const DEFAULT_SESSION_API_PORT: u16 = 9847;

/// Configuration for the loopback session API gRPC server.
#[derive(Clone, Debug)]
pub struct SessionApiServeConfig {
  pub host: String,
  pub port: u16,
  pub store_root: std::path::PathBuf,
}

impl Default for SessionApiServeConfig {
  fn default() -> Self {
    Self {
      host: DEFAULT_SESSION_API_HOST.to_string(),
      port: DEFAULT_SESSION_API_PORT,
      store_root: std::path::PathBuf::new(),
    }
  }
}

fn assert_loopback_host(host: &str) -> Result<(), String> {
  if host.eq_ignore_ascii_case("localhost") {
    return Ok(());
  }
  match host.parse::<IpAddr>() {
    Ok(ip) if ip.is_loopback() => Ok(()),
    Ok(_) => Err(format!("session API server refuses non-loopback host: {host}")),
    Err(_) => Err(format!("session API server refuses unrecognized host: {host}")),
  }
}

fn assert_socket_addr_is_loopback(addr: SocketAddr) -> Result<(), String> {
  if addr.ip().is_loopback() {
    return Ok(());
  }
  Err(format!("session API server refused non-loopback bind address: {addr}"))
}

async fn resolve_loopback_bind_addr(host: &str, port: u16) -> Result<SocketAddr, String> {
  assert_loopback_host(host)?;
  if host.eq_ignore_ascii_case("localhost") {
    let mut addresses =
      tokio::net::lookup_host((host, port)).await.map_err(|error| format!("failed to resolve localhost for session API server: {error}"))?;
    return addresses
      .find(|address| address.ip().is_loopback())
      .ok_or_else(|| "localhost did not resolve to a loopback address".to_string());
  }
  let ip = host.parse::<IpAddr>().map_err(|error| format!("failed to parse session API host {host}: {error}"))?;
  Ok(SocketAddr::new(ip, port))
}

async fn bind_session_api(config: &SessionApiServeConfig) -> Result<(TcpListener, SocketAddr), String> {
  let bind_addr = resolve_loopback_bind_addr(&config.host, config.port).await?;
  let display_address = format!("{bind_addr}");
  let listener =
    TcpListener::bind(bind_addr).await.map_err(|error| format!("failed to bind session API server {display_address}: {error}"))?;
  let local_address = listener.local_addr().map_err(|error| format!("failed to read session API server address: {error}"))?;
  assert_socket_addr_is_loopback(local_address)?;
  Ok((listener, local_address))
}

pub(crate) async fn serve_on_listener(
  listener: TcpListener,
  local_address: SocketAddr,
  store_root: std::path::PathBuf,
) -> Result<(), String> {
  println!("session API: grpc://{local_address}");
  // Flush so subprocess clients reading piped stdout see readiness without a
  // block-buffer delay.
  std::io::stdout().flush().map_err(|error| format!("failed to flush session API readiness line: {error}"))?;
  let handler = Arc::new(SessionApiHandler::new(store_root));
  let service = SessionServiceGrpc::new(handler);
  tonic::transport::Server::builder()
    .add_service(SessionServiceServer::new(service))
    .serve_with_incoming(TcpListenerStream::new(listener))
    .await
    .map_err(|error| format!("session API server failed: {error}"))?;
  Ok(())
}

/// Starts the loopback-only session API gRPC server and blocks until shutdown.
pub async fn serve(config: SessionApiServeConfig) -> Result<SocketAddr, String> {
  let (listener, local_address) = bind_session_api(&config).await?;
  serve_on_listener(listener, local_address, config.store_root).await?;
  Ok(local_address)
}

#[cfg(test)]
mod tests {
  use auv_api_proto::v1::session as proto;
  use auv_api_proto::v1::session::session_service_client::SessionServiceClient;

  use super::*;
  use crate::api::session_service::durability::INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT;
  use crate::api::session_service::test_fixtures::session_api_temp_store_root;

  #[test]
  fn assert_loopback_host_accepts_loopback_literals() {
    for host in ["127.0.0.1", "localhost", "LOCALHOST", "::1"] {
      assert_loopback_host(host).unwrap_or_else(|error| panic!("{host}: {error}"));
    }
  }

  #[test]
  fn assert_loopback_host_rejects_non_loopback() {
    for host in ["0.0.0.0", "192.168.1.1", "example.com"] {
      let error = assert_loopback_host(host).expect_err(host);
      assert!(error.contains(host), "error should mention host: {error}");
    }
  }

  #[test]
  fn assert_socket_addr_is_loopback_rejects_non_loopback() {
    let error = assert_socket_addr_is_loopback("192.168.1.1:9847".parse().expect("socket addr")).expect_err("non-loopback address");
    assert!(error.contains("192.168.1.1"));
  }

  #[tokio::test]
  async fn resolve_loopback_bind_addr_resolves_localhost() {
    let address = resolve_loopback_bind_addr("localhost", 0).await.expect("resolve localhost");
    assert!(address.ip().is_loopback());
  }

  #[tokio::test]
  async fn grpc_create_session_round_trip() {
    let store_root = session_api_temp_store_root("server");
    let config = SessionApiServeConfig {
      host: DEFAULT_SESSION_API_HOST.to_string(),
      port: 0,
      store_root: store_root.clone(),
    };
    let (listener, local_address) = bind_session_api(&config).await.expect("bind");
    assert!(local_address.ip().is_loopback());
    let server = tokio::spawn(serve_on_listener(listener, local_address, store_root));

    let endpoint = format!("http://{local_address}");
    let mut client = SessionServiceClient::connect(endpoint).await.expect("connect client");

    let response = client
      .create_session(proto::CreateSessionRequest {
        client_label: "server-test".to_string(),
      })
      .await
      .expect("create_session")
      .into_inner();
    let session = response.session.expect("session ref");
    assert!(!session.session_id.is_empty());

    server.abort();
    let _ = server.await;
  }

  #[tokio::test]
  async fn grpc_invoke_and_get_operation_round_trips() {
    let store_root = session_api_temp_store_root("server");
    let config = SessionApiServeConfig {
      host: DEFAULT_SESSION_API_HOST.to_string(),
      port: 0,
      store_root: store_root.clone(),
    };
    let (listener, local_address) = bind_session_api(&config).await.expect("bind");
    let server = tokio::spawn(serve_on_listener(listener, local_address, store_root));

    let endpoint = format!("http://{local_address}");
    let mut client = SessionServiceClient::connect(endpoint).await.expect("connect client");

    let session = client
      .create_session(proto::CreateSessionRequest {
        client_label: String::new(),
      })
      .await
      .expect("create_session")
      .into_inner()
      .session
      .expect("session ref");

    let invoke_response = client
      .invoke(proto::InvokeRequest {
        session: Some(session),
        command_id: "fixture.observe".to_string(),
        json_payload: Vec::new(),
      })
      .await
      .expect("invoke")
      .into_inner();
    assert_eq!(invoke_response.status, "completed");
    let operation = invoke_response.operation.expect("operation ref");

    let get_response = client
      .get_operation(proto::GetOperationRequest {
        operation: Some(operation),
      })
      .await
      .expect("get_operation should succeed after invoke")
      .into_inner();

    assert_eq!(get_response.status, "completed");
    assert_eq!(get_response.output_summary, "fixture observed");
    assert!(get_response.known_limits.iter().any(|limit| limit == INVOKE_SYNTHETIC_OPERATION_RESULT_KNOWN_LIMIT));

    server.abort();
    let _ = server.await;
  }
}
