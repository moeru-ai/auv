//! Tests for raw Runner gRPC proxy routing inputs.

use super::*;

#[test]
fn parses_exact_grpc_method_path() {
  assert_eq!(grpc_method("/auv.example.v1.ExampleService/Get").unwrap(), ("auv.example.v1.ExampleService", "Get"));
  assert!(grpc_method("/auv.example.v1.ExampleService").is_err());
  assert!(grpc_method("/auv.example.v1.ExampleService/Get/extra").is_err());
}

#[test]
fn opaque_forwarding_cannot_shadow_daemon_services() {
  assert_eq!(reject_daemon_namespace("auv.api.daemon.v1.FutureService").unwrap_err().code(), Code::Unimplemented);
  assert_eq!(reject_daemon_namespace("auv.api.daemon.v1.FutureService").unwrap_err().code(), Code::Unimplemented);
  assert!(reject_daemon_namespace("auv.netease_music.v1.SongService").is_ok());
  assert!(reject_daemon_namespace("grpc.health.v1.Health").is_ok());
  assert!(reject_daemon_namespace("grpc.reflection.v1.ServerReflection").is_ok());
}

#[test]
fn routing_metadata_is_parsed_without_a_protobuf_envelope() {
  let mut headers = http::HeaderMap::new();
  headers.insert(ROUTE_DEVICE_METADATA, "device_local".parse().unwrap());
  headers.insert(ROUTE_RUN_METADATA, "run_test".parse().unwrap());
  headers.insert(ROUTE_RUNNER_CLASS_METADATA, "netease-music.personal".parse().unwrap());
  assert_eq!(
    runner_route(&headers).unwrap(),
    RunnerRoute {
      device_id: Some("device_local".to_string()),
      run_id: Some("run_test".to_string()),
      runner_class: "netease-music.personal".to_string(),
    }
  );
}

#[test]
fn route_metadata_requires_exactly_one_runner_class() {
  assert_eq!(runner_route(&http::HeaderMap::new()).unwrap_err().code(), Code::InvalidArgument);
  let mut headers = http::HeaderMap::new();
  headers.append(ROUTE_RUNNER_CLASS_METADATA, "one".parse().unwrap());
  headers.append(ROUTE_RUNNER_CLASS_METADATA, "two".parse().unwrap());
  assert_eq!(runner_route(&headers).unwrap_err().code(), Code::InvalidArgument);
}
