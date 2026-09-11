//! Authentication middleware and route authentication requirements.

use std::collections::HashMap;
use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use tonic::Status;
use tonic::server::NamedService;

use crate::authentication::Authenticator;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Requirement {
  Request,
  WebSocketOpen,
  Public,
}

/// Builds the authentication requirements next to route registration.
#[derive(Default)]
pub(crate) struct Builder {
  requirements: HashMap<(Method, String), Requirement>,
}

impl Builder {
  pub(crate) fn new() -> Self {
    Self::default()
  }

  /// Allows a route to run without an authenticated caller.
  pub(crate) fn public(&mut self, method: Method, path: impl Into<String>) {
    self.register(method, path, Requirement::Public);
  }

  /// Allows a remote WebSocket to authenticate from its Open message.
  pub(crate) fn websocket_open(&mut self, method: Method, path: impl Into<String>) {
    self.register(method, path, Requirement::WebSocketOpen);
  }

  /// Allows one generated gRPC method to run without an authenticated caller.
  pub(crate) fn public_grpc<S: NamedService>(&mut self, method: &str) {
    self.public(Method::POST, format!("/{}/{}", S::NAME, method));
  }

  fn register(&mut self, method: Method, path: impl Into<String>, requirement: Requirement) {
    let route = (method, path.into());
    assert!(
      self.requirements.insert(route.clone(), requirement).is_none(),
      "authentication requirement registered twice for {} {}",
      route.0,
      route.1
    );
  }

  pub(crate) fn build(self, authenticator: Authenticator) -> Middleware {
    Middleware {
      authenticator,
      requirements: Arc::new(self.requirements),
    }
  }
}

/// State used by the authentication middleware.
#[derive(Clone)]
pub(crate) struct Middleware {
  authenticator: Authenticator,
  requirements: Arc<HashMap<(Method, String), Requirement>>,
}

impl Middleware {
  fn requirement(&self, request: &Request) -> Requirement {
    self.requirements.get(&(request.method().clone(), request.uri().path().to_owned())).copied().unwrap_or(Requirement::Request)
  }
}

/// Authenticates protected HTTP and gRPC requests before protocol adapters run.
pub(crate) async fn authenticate(State(state): State<Middleware>, mut request: Request, next: Next) -> Response {
  let requirement = state.requirement(&request);
  if requirement == Requirement::Public
    || (requirement == Requirement::WebSocketOpen && state.authenticator.authenticates_websocket_open() && is_websocket_upgrade(&request))
  {
    return next.run(request).await;
  }

  match state.authenticator.authenticate_http(&request) {
    Ok(caller) => {
      request.extensions_mut().insert(caller);
      next.run(request).await
    }
    Err(status) => authentication_error(&request, status),
  }
}

fn is_websocket_upgrade(request: &Request) -> bool {
  request.headers().get(header::UPGRADE).and_then(|value| value.to_str().ok()).is_some_and(|value| value.eq_ignore_ascii_case("websocket"))
}

fn authentication_error(request: &Request, status: Status) -> Response {
  if request
    .headers()
    .get(header::CONTENT_TYPE)
    .and_then(|value| value.to_str().ok())
    .is_some_and(|value| value.starts_with("application/grpc"))
  {
    return status.into_http::<Body>();
  }
  let (http_status, code) = match status.code() {
    tonic::Code::Unauthenticated => (StatusCode::UNAUTHORIZED, "unauthenticated"),
    tonic::Code::PermissionDenied => (StatusCode::FORBIDDEN, "permission_denied"),
    _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal"),
  };
  let body = serde_json::json!({
    "type": format!("urn:auv:error:{code}"),
    "title": code,
    "status": http_status.as_u16(),
    "detail": status.message(),
  });
  (http_status, [(header::CONTENT_TYPE, "application/problem+json")], body.to_string()).into_response()
}

#[cfg(test)]
mod tests {
  use super::*;

  struct ExampleService;

  impl NamedService for ExampleService {
    const NAME: &'static str = "example.v1.ExampleService";
  }

  fn requirement(builder: Builder, method: Method, path: &str) -> Requirement {
    let middleware = Middleware {
      authenticator: Authenticator::local(
        #[cfg(unix)]
        None,
        None,
      ),
      requirements: Arc::new(builder.requirements),
    };
    middleware.requirement(&Request::builder().method(method).uri(path).body(Body::empty()).unwrap())
  }

  #[test]
  fn unregistered_routes_require_request_authentication() {
    assert_eq!(requirement(Builder::new(), Method::GET, "/unregistered"), Requirement::Request);
  }

  #[test]
  fn registration_matches_both_method_and_exact_path() {
    let mut builder = Builder::new();
    builder.public(Method::POST, "/pair");

    assert_eq!(requirement(builder, Method::GET, "/pair"), Requirement::Request);

    let mut builder = Builder::new();
    builder.public(Method::POST, "/pair");
    assert_eq!(requirement(builder, Method::POST, "/pair/other"), Requirement::Request);
  }

  #[test]
  fn grpc_registration_uses_the_generated_service_name() {
    let mut builder = Builder::new();
    builder.public_grpc::<ExampleService>("Pair");

    assert_eq!(requirement(builder, Method::POST, "/example.v1.ExampleService/Pair"), Requirement::Public);
  }
}
