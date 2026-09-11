use std::time::Duration;

use auv_api_proto::auv::api::daemon::v1 as proto;
use auv_api_proto::auv::api::daemon::v1::device_service_client::DeviceServiceClient;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::time::timeout;

const SERVER_READY_PREFIX: &str = "auv serve: ";

#[tokio::test]
async fn local_device_is_visible_through_discovered_and_explicit_endpoints() {
  let store = tempfile::tempdir().expect("temporary daemon store");
  let discovery_file = store.path().join("api-server.json");
  let profiles_file = store.path().join("device-profiles.json");
  std::fs::write(&profiles_file, br#"{"profiles":{}}"#).expect("isolated Device profiles");
  let mut child = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "api-server",
      "serve",
      "--host",
      "127.0.0.1",
      "--port",
      "0",
      "--store-root",
      store.path().to_str().expect("UTF-8 store path"),
      "--discovery-file",
      discovery_file.to_str().expect("UTF-8 discovery path"),
    ])
    .stdout(std::process::Stdio::piped())
    .stderr(std::process::Stdio::inherit())
    .kill_on_drop(true)
    .spawn()
    .expect("spawn API server");
  let stdout = child.stdout.take().expect("API server stdout");
  let endpoint = timeout(Duration::from_secs(30), async move {
    let mut lines = BufReader::new(stdout).lines();
    loop {
      let line = lines.next_line().await.expect("read API server stdout").expect("API server closed before ready");
      if let Some(endpoint) = line.strip_prefix(SERVER_READY_PREFIX) {
        return endpoint.to_string();
      }
    }
  })
  .await
  .expect("API server ready timeout");
  let mut client = DeviceServiceClient::connect(endpoint.clone()).await.expect("connect Device client");
  let device = client
    .list_devices(proto::ListDevicesRequest {})
    .await
    .expect("list Devices")
    .into_inner()
    .devices
    .into_iter()
    .next()
    .expect("local Device");
  let device_id = device.r#ref.expect("Device ref").device_id;

  let listed = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "list"])
    .env_remove("AUV_ENDPOINT")
    .env("AUV_DISCOVERY_FILE", &discovery_file)
    .env("AUV_CONFIG_PROFILES_FILE", &profiles_file)
    .output()
    .await
    .expect("list the discovered Device");
  assert!(listed.status.success(), "Device list stderr: {}", String::from_utf8_lossy(&listed.stderr));
  assert!(String::from_utf8_lossy(&listed.stdout).contains(&device_id[..12]));

  let explicit = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["devices", "list", "--endpoint", &endpoint])
    .env("AUV_ENDPOINT", "not-a-valid-endpoint")
    .env("AUV_DISCOVERY_FILE", store.path().join("missing.json"))
    .env("AUV_CONFIG_PROFILES_FILE", &profiles_file)
    .output()
    .await
    .expect("list through explicit endpoint precedence");
  assert!(explicit.status.success(), "explicit endpoint stderr: {}", String::from_utf8_lossy(&explicit.stderr));
  assert!(String::from_utf8_lossy(&explicit.stdout).contains(&device_id[..12]));

  child.kill().await.expect("stop API server");
  let _ = child.wait().await;
}
