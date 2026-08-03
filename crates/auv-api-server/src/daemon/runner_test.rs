use super::*;
use auv_api_proto::auv::api::driver::v1 as driver_proto;
use auv_api_proto::auv::api::driver::v1::display_service_client::DisplayServiceClient;

const DISPLAY_SERVICE: &str = "auv.api.driver.v1.DisplayService";

impl RunnerSupervisor {
  fn new(local_device: daemon_proto::DeviceRef) -> Self {
    Self::with_providers(local_device, None, FirstPartyRunnerRuntimes::default(), Vec::new())
      .expect("empty RunnerProvider configuration is valid")
  }
}

#[derive(Default)]
struct RemoteDisplayFixture;

#[tonic::async_trait]
impl driver_proto::display_service_server::DisplayService for RemoteDisplayFixture {
  async fn list_displays(
    &self,
    _request: tonic::Request<driver_proto::ListDisplaysRequest>,
  ) -> Result<tonic::Response<driver_proto::ListDisplaysResponse>, tonic::Status> {
    tokio::time::sleep(Duration::from_millis(100)).await;
    Ok(tonic::Response::new(driver_proto::ListDisplaysResponse::default()))
  }
}

#[tokio::test]
async fn remote_grpc_runtime_connects_without_owning_the_endpoint_process() {
  use driver_proto::display_service_server::DisplayServiceServer;
  use tokio_stream::wrappers::TcpListenerStream;

  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind remote Runner fixture");
  let address = listener.local_addr().expect("remote Runner fixture address");
  let display = DisplayServiceServer::new(RemoteDisplayFixture);
  let (health_reporter, health) = tonic_health::server::health_reporter();
  health_reporter.set_serving::<DisplayServiceServer<RemoteDisplayFixture>>().await;
  let descriptor = auv_api_proto::descriptor_set_for_service(DISPLAY_SERVICE).expect("remote Runner descriptor");
  let reflection = tonic_reflection::server::Builder::configure()
    .register_encoded_file_descriptor_set(&descriptor)
    .build_v1()
    .expect("remote Runner reflection");
  let server = tokio::spawn(async move {
    tonic::transport::Server::builder()
      .add_service(health)
      .add_service(reflection)
      .add_service(display)
      .serve_with_incoming(TcpListenerStream::new(listener))
      .await
  });

  let registry = RunnerProviderRegistry::build_with_first_party(
    None,
    vec![RunnerProviderConfig {
      runner_class: "example.runner.remote".to_string(),
      runtime: RunnerRuntime::RemoteGrpc(crate::runner_provider::RemoteGrpcRunnerRuntime {
        endpoint: format!("http://{address}"),
      }),
    }],
  )
  .expect("remote provider");
  let provider = registry.get("example.runner.remote").expect("remote provider").clone();

  let ready = spawn_ready(&provider, None).await.expect("connect remote Runner");
  assert_eq!(ready.process_id, 0);
  let operation_channel = ready.channel.clone();
  let operation =
    tokio::spawn(async move { DisplayServiceClient::new(operation_channel).list_displays(driver_proto::ListDisplaysRequest {}).await });
  operation.await.expect("join business RPC").expect("business RPC");
  let mut managed = ManagedRunner {
    record: daemon_proto::Runner::default(),
    runtime: ready.runtime,
    channel: ready.channel,
    display_name: ready.display_name,
    run_affinities: 0,
  };
  stop_managed_in_place(&mut managed, None, false).await.expect("detach remote Runner");

  let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
    .expect("remote endpoint")
    .connect()
    .await
    .expect("remote endpoint remains reachable after detach");
  let mut client = tonic_health::pb::health_client::HealthClient::new(channel);
  assert!(client.check(tonic_health::pb::HealthCheckRequest::default()).await.is_ok());
  server.abort();
}

#[cfg(unix)]
async fn managed_runner(lifecycle: daemon_proto::RunnerLifecycle, run_affinities: u64) -> ManagedRunner {
  let child = tokio::process::Command::new("/bin/sleep").arg("10").spawn().expect("spawn inert test child");
  ManagedRunner {
    record: daemon_proto::Runner {
      r#ref: Some(daemon_proto::RunnerRef {
        runner_id: "runner_test".to_string(),
      }),
      lifecycle: lifecycle as i32,
      idle_timeout: Some(prost_types::Duration {
        seconds: 0,
        nanos: 50_000_000,
      }),
      phase: daemon_proto::RunnerPhase::Ready as i32,
      ..daemon_proto::Runner::default()
    },
    runtime: ManagedRunnerRuntime::Executable { child },
    channel: Channel::from_static("http://[::]:1").connect_lazy(),
    display_name: "test Runner".to_string(),
    run_affinities,
  }
}

#[cfg(unix)]
#[tokio::test]
async fn dropped_operation_permit_balances_cancellation_safe_accounting() {
  let supervisor = RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  });
  let managed = managed_runner(daemon_proto::RunnerLifecycle::UnlessShutdown, 0).await;
  supervisor.runners.lock().expect("registry").insert("runner_test".to_string(), managed);

  let (_channel, permit) = supervisor.begin_external_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").expect("admit operation");
  assert_eq!(supervisor.get("runner_test").expect("Runner").runner.expect("record").active_operations, 1);
  drop(permit);
  assert_eq!(supervisor.get("runner_test").expect("Runner").runner.expect("record").active_operations, 0);

  let managed = supervisor.runners.lock().expect("registry").remove("runner_test").expect("managed Runner");
  let mut managed = managed;
  stop_managed_in_place(&mut managed, None, true).await.expect("stop test Runner");
}

#[cfg(unix)]
#[tokio::test]
async fn aggregated_admission_routes_the_registered_endpoint_without_a_method_allowlist() {
  let supervisor = RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  });
  let managed = managed_runner(daemon_proto::RunnerLifecycle::UnlessShutdown, 0).await;
  supervisor.runners.lock().expect("registry").insert("runner_test".to_string(), managed);

  let (_channel, permit) =
    supervisor.begin_external_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").expect("registered endpoint is externally routable");
  drop(permit);
  let managed = supervisor.runners.lock().expect("registry").remove("runner_test").expect("managed Runner");
  let mut managed = managed;
  stop_managed_in_place(&mut managed, None, true).await.expect("stop test Runner");
}

#[cfg(unix)]
#[tokio::test]
async fn final_activity_selects_ephemeral_stop_or_unless_idle_deadline() {
  let runners = Arc::new(Mutex::new(HashMap::new()));
  let managed = managed_runner(daemon_proto::RunnerLifecycle::Ephemeral, 1).await;
  runners.lock().expect("registry").insert("runner_test".to_string(), managed);
  let (ephemeral, deadline) = decrement_activity_locked(&runners, "runner_test", true).expect("release ephemeral affinity");
  assert!(deadline.is_none());
  let mut ephemeral = ephemeral.expect("ephemeral Runner stops immediately");
  stop_managed_in_place(&mut ephemeral, None, true).await.expect("stop ephemeral test Runner");

  let managed = managed_runner(daemon_proto::RunnerLifecycle::UnlessIdle, 1).await;
  runners.lock().expect("registry").insert("runner_test".to_string(), managed);
  let (stopped, deadline) = decrement_activity_locked(&runners, "runner_test", true).expect("release idle affinity");
  assert!(stopped.is_none());
  assert!(deadline.is_some());
  assert!(runners.lock().expect("registry").get("runner_test").expect("idle Runner remains registered").record.idle_deadline.is_some());
  let managed = runners.lock().expect("registry").remove("runner_test").expect("managed Runner");
  let mut managed = managed;
  stop_managed_in_place(&mut managed, None, true).await.expect("stop idle test Runner");
}
