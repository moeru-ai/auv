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
