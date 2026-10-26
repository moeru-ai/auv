use super::*;
use auv_api_proto::auv::api::driver::v1 as driver_proto;
use auv_api_proto::auv::api::driver::v1::display_service_client::DisplayServiceClient;

const DISPLAY_SERVICE: &str = "auv.api.driver.v1.DisplayService";

impl RunnerSupervisor {
  fn new(local_device: daemon_proto::DeviceRef) -> Self {
    Self::with_providers(local_device, None, None, FirstPartyRunnerRuntimes::default(), Vec::new())
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

  let ready = spawn_ready(&provider, None, "runner_test").await.expect("connect remote Runner");
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

/// A cross-platform RemoteGrpc fixture: no endpoint is contacted because the
/// lazy channel only connects on first RPC.
fn remote_grpc_runner(lifecycle: daemon_proto::RunnerLifecycle, active_operations: u64) -> ManagedRunner {
  ManagedRunner {
    record: daemon_proto::Runner {
      r#ref: Some(daemon_proto::RunnerRef {
        runner_id: "runner_test".to_string(),
      }),
      lifecycle: lifecycle as i32,
      phase: daemon_proto::RunnerPhase::Ready as i32,
      active_operations,
      ..daemon_proto::Runner::default()
    },
    runtime: ManagedRunnerRuntime::RemoteGrpc,
    channel: Channel::from_static("http://[::]:1").connect_lazy(),
    display_name: "test Runner".to_string(),
    run_affinities: 0,
  }
}

fn registered_phase(supervisor: &RunnerSupervisor, runner_id: &str) -> Option<i32> {
  supervisor.runners.lock().expect("registry").get(runner_id).map(|managed| managed.record.phase)
}

/// Polls a registry-derived condition with 2ms sleeps until it holds or the
/// bounded deadline passes, so a lost wakeup or a stranded entry fails the
/// test with a message instead of hanging it.
async fn poll_until(mut condition: impl FnMut() -> bool, message: &str) {
  let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
  while !condition() {
    assert!(tokio::time::Instant::now() < deadline, "condition never held: {message}");
    tokio::time::sleep(Duration::from_millis(2)).await;
  }
}

fn spawn_delete(
  supervisor: &Arc<RunnerSupervisor>,
  grace_period: Option<prost_types::Duration>,
  force: bool,
) -> tokio::task::JoinHandle<Result<daemon_proto::DeleteRunnerResponse, RunnerError>> {
  let supervisor = supervisor.clone();
  tokio::spawn(async move { supervisor.delete("runner_test", grace_period, force).await })
}

#[tokio::test]
async fn concurrent_delete_single_owner_second_delete_conflicts() {
  let supervisor = Arc::new(RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  }));
  supervisor
    .runners
    .lock()
    .expect("registry")
    .insert("runner_test".to_string(), remote_grpc_runner(daemon_proto::RunnerLifecycle::UnlessShutdown, 0));
  let (_channel, permit) =
    supervisor.begin_external_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").expect("admit operation while the Runner is Ready");
  let delete_owner = spawn_delete(&supervisor, None, false);
  poll_until(
    || registered_phase(&supervisor, "runner_test") == Some(daemon_proto::RunnerPhase::Draining as i32),
    "the first delete must claim the Runner as Draining",
  )
  .await;

  // The conflict check is synchronous, so the second delete must return
  // promptly instead of racing the first delete into the registry. The
  // message is the pinned contract; the typed variant maps to the
  // failed_precondition gRPC status.
  let conflict = tokio::time::timeout(Duration::from_millis(500), supervisor.delete("runner_test", None, false))
    .await
    .expect("the second delete must return promptly instead of racing the first")
    .expect_err("a Runner claimed by an in-flight delete must reject a second delete");
  assert!(conflict.to_string().contains("already being deleted"), "unexpected second-delete error: {conflict}");
  drop(permit);
  let deleted = tokio::time::timeout(Duration::from_secs(5), delete_owner)
    .await
    .expect("the first delete must finish once the operation is released")
    .expect("the first delete task joins")
    .expect("the first delete owns the deletion");
  assert_eq!(deleted.runner.expect("record").phase, daemon_proto::RunnerPhase::Stopped as i32);
  assert!(supervisor.get("runner_test").is_err(), "the deleted Runner leaves the registry");
}

#[tokio::test]
async fn delete_waits_for_active_operations_and_completes_after_release() {
  let supervisor = Arc::new(RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  }));
  supervisor
    .runners
    .lock()
    .expect("registry")
    .insert("runner_test".to_string(), remote_grpc_runner(daemon_proto::RunnerLifecycle::UnlessShutdown, 0));
  let (_channel, permit) = supervisor.begin_external_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").expect("admit operation");
  let delete_owner = spawn_delete(&supervisor, None, false);
  poll_until(
    || registered_phase(&supervisor, "runner_test") == Some(daemon_proto::RunnerPhase::Draining as i32),
    "the delete must claim the Runner as Draining",
  )
  .await;
  // The active operation must hold the delete open: sample the task state
  // for a bounded window instead of sleeping and hoping.
  for _ in 0..10 {
    assert!(!delete_owner.is_finished(), "the delete finished while an operation was still active");
    tokio::time::sleep(Duration::from_millis(10)).await;
  }
  drop(permit);
  let deleted = tokio::time::timeout(Duration::from_secs(5), delete_owner)
    .await
    .expect("the delete must finish once the operation is released")
    .expect("the delete task joins")
    .expect("the delete owns the deletion");
  assert_eq!(deleted.runner.expect("record").phase, daemon_proto::RunnerPhase::Stopped as i32);
  assert!(supervisor.get("runner_test").is_err(), "the deleted Runner leaves the registry");
}

#[tokio::test]
async fn ephemeral_release_leaves_deletion_to_the_delete_owner() {
  let supervisor = Arc::new(RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  }));
  supervisor
    .runners
    .lock()
    .expect("registry")
    .insert("runner_test".to_string(), remote_grpc_runner(daemon_proto::RunnerLifecycle::Ephemeral, 0));
  let (_channel, permit) = supervisor.begin_external_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").expect("admit operation");
  let delete_owner = spawn_delete(&supervisor, None, false);
  poll_until(
    || registered_phase(&supervisor, "runner_test") == Some(daemon_proto::RunnerPhase::Draining as i32),
    "the delete must claim the Runner as Draining",
  )
  .await;
  drop(permit);
  let deleted = tokio::time::timeout(Duration::from_secs(5), delete_owner)
    .await
    .expect("an Ephemeral permit release must hand the entry to the delete owner instead of removing it")
    .expect("the delete task joins")
    .expect("the delete owns the deletion");
  assert_eq!(deleted.runner.expect("record").phase, daemon_proto::RunnerPhase::Stopped as i32);
  assert!(supervisor.get("runner_test").is_err(), "the deleted Runner leaves the registry");
}

#[tokio::test]
async fn shutdown_takes_over_from_draining_delete() {
  let supervisor = Arc::new(RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  }));
  supervisor
    .runners
    .lock()
    .expect("registry")
    .insert("runner_test".to_string(), remote_grpc_runner(daemon_proto::RunnerLifecycle::UnlessShutdown, 0));
  let (_channel, permit) = supervisor.begin_external_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").expect("admit operation");
  let delete_owner = spawn_delete(&supervisor, None, false);
  poll_until(
    || registered_phase(&supervisor, "runner_test") == Some(daemon_proto::RunnerPhase::Draining as i32),
    "the delete must claim the Runner as Draining",
  )
  .await;
  supervisor.shutdown().await;
  drop(permit);
  let result = tokio::time::timeout(Duration::from_secs(5), delete_owner)
    .await
    .expect("a draining delete must resolve once shutdown takes the registry")
    .expect("the delete task joins");
  assert!(matches!(result, Err(RunnerError::Unknown(_))), "shutdown must hand the Runner back as Unknown, got {result:?}");
}

#[tokio::test]
async fn idle_stop_and_ephemeral_release_never_stop_a_draining_runner() {
  // (a) An Ephemeral permit release during Draining must leave the entry to
  // its delete owner instead of removing and stopping it.
  let runners = Arc::new(Mutex::new(HashMap::new()));
  let mut managed = remote_grpc_runner(daemon_proto::RunnerLifecycle::Ephemeral, 1);
  managed.record.phase = daemon_proto::RunnerPhase::Draining as i32;
  runners.lock().expect("registry").insert("runner_test".to_string(), managed);
  let (stop_now, schedule) = decrement_activity_locked(&runners, "runner_test", false).expect("release the last operation");
  assert!(stop_now.is_none(), "a Draining Runner is owned by its delete, not by lifecycle release");
  assert!(schedule.is_none());
  assert!(runners.lock().expect("registry").contains_key("runner_test"), "the delete owner still finds its entry");

  // (b) An expired idle deadline must not remove a Draining Runner behind
  // the delete owner's back.
  let runners = Arc::new(Mutex::new(HashMap::new()));
  let mut managed = remote_grpc_runner(daemon_proto::RunnerLifecycle::UnlessIdle, 0);
  managed.record.idle_timeout = Some(prost_types::Duration {
    seconds: 0,
    nanos: 50_000_000,
  });
  let deadline = std::time::SystemTime::now();
  managed.record.idle_deadline = Some(timestamp_from_system_time(deadline));
  managed.record.phase = daemon_proto::RunnerPhase::Draining as i32;
  runners.lock().expect("registry").insert("runner_test".to_string(), managed);
  schedule_idle_stop(runners.clone(), "runner_test".to_string(), deadline, Duration::from_millis(50));
  tokio::time::sleep(Duration::from_millis(150)).await;
  assert!(runners.lock().expect("registry").contains_key("runner_test"), "the idle stop must not remove a Draining Runner");

  // (c) A Ready UnlessIdle Runner still schedules its idle stop on release.
  let runners = Arc::new(Mutex::new(HashMap::new()));
  let mut managed = remote_grpc_runner(daemon_proto::RunnerLifecycle::UnlessIdle, 1);
  managed.record.idle_timeout = Some(prost_types::Duration {
    seconds: 0,
    nanos: 50_000_000,
  });
  runners.lock().expect("registry").insert("runner_test".to_string(), managed);
  let (stop_now, schedule) = decrement_activity_locked(&runners, "runner_test", false).expect("release the last operation");
  assert!(stop_now.is_none());
  assert!(schedule.is_some(), "a Ready UnlessIdle Runner still schedules its idle stop");
  assert!(runners.lock().expect("registry").get("runner_test").expect("the idle Runner remains registered").record.idle_deadline.is_some());
}

#[tokio::test]
async fn cancelled_delete_future_still_completes_deletion() {
  let supervisor = Arc::new(RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  }));
  supervisor
    .runners
    .lock()
    .expect("registry")
    .insert("runner_test".to_string(), remote_grpc_runner(daemon_proto::RunnerLifecycle::UnlessShutdown, 0));
  let (_channel, permit) = supervisor.begin_external_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").expect("admit operation");
  let delete_owner = spawn_delete(&supervisor, None, false);
  poll_until(
    || registered_phase(&supervisor, "runner_test") == Some(daemon_proto::RunnerPhase::Draining as i32),
    "the delete must claim the Runner as Draining",
  )
  .await;
  // Cancelling the caller must not strand the claimed Runner: the spawned
  // deletion task owns it to completion.
  delete_owner.abort();
  drop(permit);
  let removed = tokio::time::timeout(Duration::from_secs(5), async {
    loop {
      if !supervisor.runners.lock().expect("registry").contains_key("runner_test") {
        break;
      }
      tokio::time::sleep(Duration::from_millis(5)).await;
    }
  })
  .await;
  assert!(removed.is_ok(), "a cancelled delete caller must not strand its Draining Runner");
  assert!(supervisor.get("runner_test").is_err(), "the deleted Runner leaves the registry");
}

#[tokio::test]
async fn remote_grpc_delete_detaches_without_stopping_the_endpoint() {
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
  let ready = spawn_ready(&provider, None, "runner_test").await.expect("connect remote Runner");

  let supervisor = RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  });
  supervisor.runners.lock().expect("registry").insert(
    "runner_test".to_string(),
    ManagedRunner {
      record: daemon_proto::Runner {
        r#ref: Some(daemon_proto::RunnerRef {
          runner_id: "runner_test".to_string(),
        }),
        lifecycle: daemon_proto::RunnerLifecycle::UnlessShutdown as i32,
        phase: daemon_proto::RunnerPhase::Ready as i32,
        ..daemon_proto::Runner::default()
      },
      runtime: ready.runtime,
      channel: ready.channel,
      display_name: ready.display_name,
      run_affinities: 0,
    },
  );

  let deleted = supervisor.delete("runner_test", None, false).await.expect("full delete path for a RemoteGrpc Runner");
  assert_eq!(deleted.runner.expect("record").phase, daemon_proto::RunnerPhase::Stopped as i32);
  assert!(supervisor.get("runner_test").is_err(), "the deleted Runner leaves the registry");

  let channel = tonic::transport::Endpoint::from_shared(format!("http://{address}"))
    .expect("remote endpoint")
    .connect()
    .await
    .expect("remote endpoint remains reachable after delete");
  let mut client = tonic_health::pb::health_client::HealthClient::new(channel);
  assert!(client.check(tonic_health::pb::HealthCheckRequest::default()).await.is_ok());
  server.abort();
}

#[tokio::test]
async fn draining_runner_rejects_new_operations() {
  let supervisor = Arc::new(RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  }));
  supervisor
    .runners
    .lock()
    .expect("registry")
    .insert("runner_test".to_string(), remote_grpc_runner(daemon_proto::RunnerLifecycle::UnlessShutdown, 0));
  let (_channel, permit) =
    supervisor.begin_external_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").expect("admit operation while the Runner is Ready");
  let delete_owner = spawn_delete(&supervisor, None, false);
  poll_until(
    || registered_phase(&supervisor, "runner_test") == Some(daemon_proto::RunnerPhase::Draining as i32),
    "the delete must claim the Runner as Draining",
  )
  .await;
  assert!(
    supervisor.begin_external_operation("runner_test", DISPLAY_SERVICE, "ListDisplays").is_err(),
    "a Draining Runner must not admit new operations"
  );
  drop(permit);
  let deleted = tokio::time::timeout(Duration::from_secs(5), delete_owner)
    .await
    .expect("the delete must finish once the operation is released")
    .expect("the delete task joins")
    .expect("the delete owns the deletion");
  assert_eq!(deleted.runner.expect("record").phase, daemon_proto::RunnerPhase::Stopped as i32);
}

// Test children are inert sleepers owned by the test and always terminated:
// the delete under test kills them (force, or after the grace deadline), and
// the Windows command additionally sets kill_on_drop(true) so a failing test
// never leaks the process.
#[cfg(unix)]
fn executable_runner(lifecycle: daemon_proto::RunnerLifecycle) -> ManagedRunner {
  let child = tokio::process::Command::new("/bin/sleep").arg("10").spawn().expect("spawn inert test child");
  ManagedRunner {
    record: daemon_proto::Runner {
      r#ref: Some(daemon_proto::RunnerRef {
        runner_id: "runner_test".to_string(),
      }),
      lifecycle: lifecycle as i32,
      phase: daemon_proto::RunnerPhase::Ready as i32,
      ..daemon_proto::Runner::default()
    },
    runtime: ManagedRunnerRuntime::Executable { child },
    channel: Channel::from_static("http://[::]:1").connect_lazy(),
    display_name: "test Runner".to_string(),
    run_affinities: 0,
  }
}

#[cfg(windows)]
fn executable_runner(lifecycle: daemon_proto::RunnerLifecycle) -> ManagedRunner {
  let child = tokio::process::Command::new("powershell")
    .args(["-NoProfile", "-Command", "Start-Sleep -Seconds 10"])
    .kill_on_drop(true)
    .spawn()
    .expect("spawn inert test child");
  ManagedRunner {
    record: daemon_proto::Runner {
      r#ref: Some(daemon_proto::RunnerRef {
        runner_id: "runner_test".to_string(),
      }),
      lifecycle: lifecycle as i32,
      phase: daemon_proto::RunnerPhase::Ready as i32,
      ..daemon_proto::Runner::default()
    },
    runtime: ManagedRunnerRuntime::Executable { child },
    channel: Channel::from_static("http://[::]:1").connect_lazy(),
    display_name: "test Runner".to_string(),
    run_affinities: 0,
  }
}

#[tokio::test]
async fn force_and_grace_semantics_preserved() {
  let supervisor = RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  });
  supervisor
    .runners
    .lock()
    .expect("registry")
    .insert("runner_test".to_string(), executable_runner(daemon_proto::RunnerLifecycle::UnlessShutdown));
  let deleted = supervisor.delete("runner_test", None, true).await.expect("force delete terminates the child");
  assert_eq!(deleted.runner.expect("record").phase, daemon_proto::RunnerPhase::Stopped as i32);
  assert!(supervisor.get("runner_test").is_err(), "the deleted Runner leaves the registry");

  let supervisor = RunnerSupervisor::new(daemon_proto::DeviceRef {
    device_id: "device_test".to_string(),
  });
  supervisor
    .runners
    .lock()
    .expect("registry")
    .insert("runner_test".to_string(), executable_runner(daemon_proto::RunnerLifecycle::UnlessShutdown));
  let grace = prost_types::Duration {
    seconds: 0,
    nanos: 50_000_000,
  };
  let deleted = tokio::time::timeout(Duration::from_secs(5), supervisor.delete("runner_test", Some(grace), false))
    .await
    .expect("the grace delete must finish once its deadline expires")
    .expect("the grace delete stops the child after its deadline");
  assert_eq!(deleted.runner.expect("record").phase, daemon_proto::RunnerPhase::Stopped as i32);
  assert!(supervisor.get("runner_test").is_err(), "the deleted Runner leaves the registry");
}

// ROOT CAUSE:
//
// The daemon adopted whichever process opened the Runner pipe first. Before
// the fix, a local process watching `\\.\pipe\` could connect before the
// spawned Runner, receive the daemon's requests, and block Runner startup.
// The daemon now disconnects any client whose PID is not the spawned Runner.
#[cfg(windows)]
#[tokio::test]
async fn runner_pipe_disconnects_a_client_that_is_not_the_spawned_runner() {
  use tokio::io::AsyncReadExt as _;

  let name = format!("auv-runner-test-{}", uuid::Uuid::now_v7());
  let path = format!(r"\\.\pipe\{name}");
  let pipe = auv_api_server::windows_pipe::create_owner_only(&path, true).unwrap();
  let mut impostor = tokio::net::windows::named_pipe::ClientOptions::new().open(&path).unwrap();
  // NamedPipeClientStream.Connect retries while the impostor holds the pipe.
  let mut runner = tokio::process::Command::new("powershell")
    .args([
      "-NoProfile",
      "-Command",
      &format!("$p = New-Object IO.Pipes.NamedPipeClientStream('.', '{name}', 'InOut'); $p.Connect(20000); Start-Sleep 20"),
    ])
    .kill_on_drop(true)
    .spawn()
    .unwrap();

  let accepted = tokio::time::timeout(std::time::Duration::from_secs(30), super::accept_runner_client(&path, pipe, runner.id().unwrap()))
    .await
    .expect("the spawned Runner was never accepted")
    .unwrap();
  assert_eq!(auv_api_server::windows_pipe::client_process_id(&accepted).unwrap(), runner.id().unwrap());

  let mut buffer = [0u8; 1];
  let impostor_read = tokio::time::timeout(std::time::Duration::from_secs(5), impostor.read(&mut buffer)).await;
  assert!(matches!(impostor_read, Ok(Ok(0) | Err(_))), "the impostor stayed connected");
  let _ = runner.kill().await;
}
