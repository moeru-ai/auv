use super::{
  AuthorizationInterceptor, ConnectEndpoint, ROUTE_DEVICE_METADATA, ROUTE_RUN_METADATA, ROUTE_RUNNER_CLASS_METADATA, RunnerRoute,
  RunnerRouteInterceptor,
};
use tonic::service::Interceptor as _;

#[test]
fn endpoint_parser_round_trips_loopback_tcp() {
  for value in [
    "http://127.0.0.1:9847",
    "http://[::1]:9847",
    "http://localhost:9847",
  ] {
    let endpoint = value.parse::<ConnectEndpoint>().expect(value);
    assert_eq!(endpoint.to_string(), value);
  }
}

#[cfg(unix)]
#[test]
fn endpoint_parser_round_trips_absolute_unix_path() {
  let endpoint = "unix:///tmp/auv.sock".parse::<ConnectEndpoint>().expect("Unix endpoint");
  assert_eq!(endpoint.to_string(), "unix:///tmp/auv.sock");
}

#[cfg(windows)]
#[test]
fn endpoint_parser_round_trips_windows_named_pipe() {
  let endpoint = "npipe://./pipe/auv-0198-test".parse::<ConnectEndpoint>().expect("named-pipe endpoint");
  assert_eq!(endpoint.to_string(), "npipe://./pipe/auv-0198-test");
  assert!("npipe://./pipe/".parse::<ConnectEndpoint>().is_err());
  assert!("npipe://./pipe/auv/nested".parse::<ConnectEndpoint>().is_err());
}

#[test]
fn endpoint_parser_accepts_remote_http_and_rejects_unsupported_tls() {
  assert!("http://example.com:9847".parse::<ConnectEndpoint>().is_ok());
  assert!("https://127.0.0.1:9847".parse::<ConnectEndpoint>().is_err());
}

#[test]
fn sensitive_authorization_debug_is_redacted() {
  let mut authorization = "Bearer bearer-secret".parse::<tonic::metadata::MetadataValue<tonic::metadata::Ascii>>().unwrap();
  authorization.set_sensitive(true);
  let interceptor = AuthorizationInterceptor {
    authorization: Some(authorization),
  };
  assert!(!format!("{interceptor:?}").contains("bearer-secret"));
}

#[test]
fn route_interceptor_adds_only_out_of_band_routing_metadata() {
  let mut interceptor = RunnerRouteInterceptor::new(RunnerRoute {
    device_id: Some("device_local".to_string()),
    run_id: Some("run_test".to_string()),
    runner_class: "netease-music.personal".to_string(),
  })
  .unwrap();
  let request = interceptor.call(tonic::Request::new(())).unwrap();
  assert_eq!(request.metadata().get(ROUTE_DEVICE_METADATA).unwrap(), "device_local");
  assert_eq!(request.metadata().get(ROUTE_RUN_METADATA).unwrap(), "run_test");
  assert_eq!(request.metadata().get(ROUTE_RUNNER_CLASS_METADATA).unwrap(), "netease-music.personal");
}

#[test]
fn route_interceptor_carries_the_callers_trace_context() {
  let mut interceptor = RunnerRouteInterceptor::new(RunnerRoute {
    device_id: None,
    run_id: None,
    runner_class: "auv.core.local".to_string(),
  })
  .unwrap();
  let untraced = interceptor.call(tonic::Request::new(())).unwrap();
  assert!(auv_tracing::propagation::FIELD_NAMES.iter().all(|name| !untraced.metadata().contains_key(*name)));

  let run_id = auv_tracing::RunId::new();
  let dispatch = auv_tracing::configure().build().unwrap();
  let request = auv_tracing::dispatcher::with_default(&dispatch, || {
    auv_tracing::Context::root(run_id).in_scope(|| interceptor.call(tonic::Request::new(())).unwrap())
  });
  assert_eq!(request.metadata().get("auv-trace-run-id").unwrap(), run_id.to_string().as_str());
  assert_eq!(request.metadata().get("auv-trace-version").unwrap(), "1");
}

#[tokio::test]
async fn connect_paired_rejects_malformed_credentials_without_panic() {
  for malformed in [
    "",
    "   ",
    "bearer token with spaces",
    "token\nnewline",
    "token\rreturn",
    "token\0null",
    "token\tspace",
    "token-\u{1f600}-emoji",
    "token-\u{4e2d}\u{6587}-chinese",
  ] {
    let result = super::Client::connect_paired(super::PairedConnectConfig {
      endpoint: "http://127.0.0.1:9847".parse().unwrap(),
      device_credential: malformed.to_string(),
    })
    .await;
    match result {
      Err(super::PairedConnectError::InvalidCredential) => {}
      other => panic!("expected InvalidCredential for {malformed:?}, got {other:?}"),
    }
  }
}

#[tokio::test]
async fn connect_with_credential_rejects_malformed_credentials() {
  for malformed in [
    "",
    "   ",
    "bearer token with spaces",
    "token\nnewline",
    "token\rreturn",
    "token\0null",
    "token\tspace",
    "token-\u{1f600}-emoji",
  ] {
    let result = super::Client::connect_with_credential("http://127.0.0.1:9847".parse().unwrap(), malformed).await;
    match result {
      Err(super::CredentialConnectError::InvalidCredential) => {}
      other => panic!("expected InvalidCredential for {malformed:?}, got {other:?}"),
    }
  }
}

#[test]
fn paired_connect_error_display_and_debug_do_not_leak_credential() {
  let err = super::PairedConnectError::InvalidCredential;
  let display = format!("{err}");
  let debug = format!("{err:?}");
  assert_eq!(display, "credential must be visible ASCII");
  assert!(!debug.contains("secret"));
}
