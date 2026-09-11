#![cfg(unix)]

use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct ChildGuard(Child);

impl Drop for ChildGuard {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

#[test]
fn pair_connect_saves_the_credential_without_printing_it_and_consumes_the_token() {
  let directory = tempfile::tempdir().unwrap();
  let store = directory.path().join("pairings.json");
  let socket = directory.path().join("auv.sock");
  let discovery = directory.path().join("discovery.json");
  let profiles = directory.path().join("profiles.json");
  let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
  let auv = env!("CARGO_BIN_EXE_auv");
  let mut daemon = ChildGuard(
    Command::new(auv)
      .args([
        "serve",
        "--listen",
        &format!("unix://{}", socket.display()),
        "--listen",
        &format!("http://127.0.0.1:{port}"),
        "--pairing-store",
        store.to_str().unwrap(),
        "--store-root",
        directory.path().join("store").to_str().unwrap(),
        "--discovery-file",
        discovery.to_str().unwrap(),
      ])
      .stdin(Stdio::null())
      .stdout(Stdio::null())
      .stderr(Stdio::inherit())
      .spawn()
      .unwrap(),
  );
  wait_for_path(&mut daemon.0, &discovery);

  let created = Command::new(auv).args(["devices", "pair", "create-token"]).env("AUV_DISCOVERY_FILE", &discovery).output().unwrap();
  assert!(created.status.success(), "{}", String::from_utf8_lossy(&created.stderr));
  let token = String::from_utf8(created.stdout).unwrap().trim().to_string();
  assert_eq!(token.len(), 32);
  assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
  assert!(!std::fs::read_to_string(&store).unwrap().contains(&token));

  let endpoint = format!("http://127.0.0.1:{port}");
  let connected = Command::new(auv)
    .args([
      "-vv",
      "devices",
      "pair",
      "--endpoint",
      &endpoint,
      "connect",
      "--token",
      &token,
      "--label",
      "Tablet",
      "--profile",
      "gpu",
      "--json",
    ])
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
    .output()
    .unwrap();
  assert!(connected.status.success(), "{}", String::from_utf8_lossy(&connected.stderr));
  let result: serde_json::Value = serde_json::from_slice(&connected.stdout).unwrap();
  assert_eq!(result["profile"], "gpu");
  assert_eq!(result["credentials_file"], profiles.to_string_lossy().as_ref());
  let output = format!("{}{}", String::from_utf8_lossy(&connected.stdout), String::from_utf8_lossy(&connected.stderr));
  assert!(!output.contains(&token));
  assert!(!output.contains("device_credential"));
  assert!(String::from_utf8_lossy(&connected.stderr).contains("PairingService"));

  let mut profile_document: serde_json::Value = serde_json::from_slice(&std::fs::read(&profiles).unwrap()).unwrap();
  let bearer = profile_document["profiles"]["gpu"]["device_credential"].as_str().unwrap().to_string();
  assert_eq!(bearer.len(), 64);
  assert!(bearer.bytes().all(|byte| byte.is_ascii_hexdigit()));
  assert!(!std::fs::read_to_string(&store).unwrap().contains(&bearer));

  // ROOT CAUSE:
  //
  // If a daemon reports an empty or changed Device name, a stale profile name
  // previously made a valid canonical-ID connection appear invalid.
  // The fix treats configured names as display/selection hints and verifies
  // paired identity using the canonical Device ID.
  profile_document["profiles"]["gpu"]["device_name"] = "stale-profile-name".into();
  std::fs::write(&profiles, serde_json::to_vec_pretty(&profile_document).unwrap()).unwrap();

  let listed = Command::new(auv)
    .args(["-vv", "devices", "list", "--json"])
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
    .env("AUV_DISCOVERY_FILE", directory.path().join("no-local-daemon.json"))
    .env_remove("AUV_ENDPOINT")
    .output()
    .unwrap();
  assert!(listed.status.success(), "{}", String::from_utf8_lossy(&listed.stderr));
  let devices: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
  assert_eq!(devices[0]["config_profile"], "gpu");
  assert_eq!(devices[0]["status"], "online", "{}", String::from_utf8_lossy(&listed.stderr));
  let expected_platform = if cfg!(target_os = "macos") {
    "DEVICE_PLATFORM_MACOS"
  } else if cfg!(target_os = "linux") {
    "DEVICE_PLATFORM_LINUX"
  } else if cfg!(target_os = "windows") {
    "DEVICE_PLATFORM_WINDOWS"
  } else {
    "DEVICE_PLATFORM_UNSPECIFIED"
  };
  assert_eq!(devices[0]["platform"], expected_platform, "online profiles must retain remote Device facts");

  profile_document["profiles"]["gpu"]["device_credential"] = "invalid-credential".into();
  std::fs::write(&profiles, serde_json::to_vec_pretty(&profile_document).unwrap()).unwrap();
  let unauthorized = Command::new(auv)
    .args(["devices", "list", "--json"])
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
    .env("AUV_DISCOVERY_FILE", directory.path().join("no-local-daemon.json"))
    .env_remove("AUV_ENDPOINT")
    .output()
    .unwrap();
  assert!(unauthorized.status.success(), "{}", String::from_utf8_lossy(&unauthorized.stderr));
  let devices: serde_json::Value = serde_json::from_slice(&unauthorized.stdout).unwrap();
  assert_eq!(devices[0]["status"], "unauthorized");

  let reused = Command::new(auv)
    .args([
      "devices",
      "pair",
      "--endpoint",
      &endpoint,
      "connect",
      "--token",
      &token,
      "--device-id",
      "device_other",
      "--label",
      "Other",
    ])
    .env("AUV_CONFIG_PROFILES_FILE", &profiles)
    .output()
    .unwrap();
  assert!(!reused.status.success());
  assert!(String::from_utf8_lossy(&reused.stderr).contains("invalid, expired, or has already been consumed"));
}

fn wait_for_path(child: &mut Child, path: &std::path::Path) {
  let deadline = Instant::now() + Duration::from_secs(15);
  while Instant::now() < deadline {
    if path.exists() {
      return;
    }
    if let Some(status) = child.try_wait().unwrap() {
      panic!("daemon exited before readiness: {status}");
    }
    std::thread::sleep(Duration::from_millis(25));
  }
  panic!("daemon did not publish {}", path.display());
}
