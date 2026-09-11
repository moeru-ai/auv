#![cfg(unix)]

use std::fs;
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use auv::profile::ProfileStore;
use auv::{AuvContext, Client};
use auv_api_client::protocol::grpc::Client as GrpcClient;
use auv_api_proto::auv::api::daemon::v1 as daemon_proto;

struct ChildGuard(Child);

impl Drop for ChildGuard {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

#[tokio::test]
async fn paired_profile_uses_short_token_bearer_without_pki() {
  let directory = tempfile::tempdir().expect("paired profile directory");
  let store_root = directory.path().join("store");
  let control_root = store_root.join("control");
  fs::create_dir_all(&control_root).unwrap();
  let target_device_id = "018f1f00000070008000000000000001018f1f00000070008000000000000001";
  fs::write(control_root.join("device-id"), format!("{target_device_id}\n")).unwrap();

  let pairing_path = directory.path().join("pairings.json");
  let socket = directory.path().join("auv.sock");
  let discovery = directory.path().join("discovery.json");
  let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
  let mut daemon = ChildGuard(
    Command::new(env!("CARGO_BIN_EXE_auv"))
      .args([
        "serve",
        "--listen",
        &format!("unix://{}", socket.display()),
        "--listen",
        &format!("http://127.0.0.1:{port}"),
        "--pairing-store",
        pairing_path.to_str().unwrap(),
        "--store-root",
        store_root.to_str().unwrap(),
        "--discovery-file",
        discovery.to_str().unwrap(),
      ])
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::inherit())
      .spawn()
      .expect("spawn paired daemon"),
  );
  let token = wait_for_token(&mut daemon.0, auv_api_client::ConnectEndpoint::Unix(socket)).await;
  let endpoint = format!("http://127.0.0.1:{port}");
  let enrollment = wait_for_pairing(&mut daemon.0, endpoint.clone(), token).await;

  let profile_path = directory.path().join("device-profiles.json");
  fs::write(
    &profile_path,
    serde_json::to_vec(&serde_json::json!({
      "profiles": {
        "gpu": {
          "device_id": target_device_id,
          "device_name": "",
          "endpoint": endpoint.clone(),
          "device_credential": enrollment.device_credential,
        }
      }
    }))
    .unwrap(),
  )
  .unwrap();

  // The daemon name is platform-derived and therefore copied into the profile
  // after the first authenticated discovery call.
  let direct = GrpcClient::connect_paired(auv_api_client::PairedConnectConfig {
    endpoint: endpoint.parse().unwrap(),
    device_credential:
      serde_json::from_slice::<serde_json::Value>(&fs::read(&profile_path).unwrap()).unwrap()["profiles"]["gpu"]["device_credential"]
        .as_str()
        .unwrap()
        .to_string(),
  })
  .await
  .expect("connect bearer client");
  let target = direct.devices().list_devices().await.expect("discover target Device").remove(0);
  let target_name = target.name;
  let mut document: serde_json::Value = serde_json::from_slice(&fs::read(&profile_path).unwrap()).unwrap();
  document["profiles"]["gpu"]["device_name"] = target_name.clone().into();
  fs::write(&profile_path, serde_json::to_vec(&document).unwrap()).unwrap();

  let client = Client::from_context_with_profiles(
    AuvContext {
      config_profile: Some("gpu".to_string()),
      ..Default::default()
    },
    &ProfileStore::from_path(profile_path),
  )
  .await
  .expect("resolve paired profile");
  assert_eq!(client.context().and_then(|context| context.device_id.as_deref()), Some(target_device_id));
  assert_eq!(client.context().and_then(|context| context.device_name.as_deref()), (!target_name.is_empty()).then_some(target_name.as_str()));
  let serialized = serde_json::to_string(client.context().unwrap()).unwrap();
  assert!(!serialized.contains("device_credential"));
}

async fn wait_for_token(child: &mut Child, endpoint: auv_api_client::ConnectEndpoint) -> String {
  let deadline = Instant::now() + Duration::from_secs(15);
  loop {
    match GrpcClient::connect(endpoint.clone()).await {
      Ok(client) => match client.pairing().create_pairing_token(daemon_proto::CreatePairingTokenRequest { ttl: None }).await {
        Ok(response) => return response.token,
        Err(_) if Instant::now() < deadline => {}
        Err(error) => panic!("local daemon did not create a pairing token: {error}"),
      },
      Err(_) if Instant::now() < deadline => {}
      Err(error) => panic!("local daemon did not become ready: {error}"),
    }
    if let Some(status) = child.try_wait().expect("inspect paired daemon") {
      panic!("paired daemon exited before readiness: {status}");
    }
    tokio::time::sleep(Duration::from_millis(50)).await;
  }
}

async fn wait_for_pairing(child: &mut Child, endpoint: String, token: String) -> daemon_proto::PairDeviceResponse {
  let deadline = Instant::now() + Duration::from_secs(15);
  loop {
    match auv_api_client::protocol::grpc::clients::daemon::v1::pairing::Client::pair_device(
      endpoint.parse().expect("paired endpoint URI"),
      daemon_proto::PairDeviceRequest {
        token: token.clone(),
        device_id: "device_test_client".to_string(),
        label: "test client".to_string(),
      },
    )
    .await
    {
      Ok(enrollment) => return enrollment,
      Err(_) if Instant::now() < deadline => {
        if let Some(status) = child.try_wait().expect("inspect paired daemon") {
          panic!("paired daemon exited before readiness: {status}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
      }
      Err(error) => panic!("paired daemon did not become ready: {error}"),
    }
  }
}
