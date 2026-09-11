use std::fs;
use std::process::Command;

#[test]
fn profile_crud_and_offline_device_listing_use_an_opaque_bearer() {
  const DEVICE_ID: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
  let directory = tempfile::tempdir().unwrap();
  let config = directory.path().join("profiles.json");
  let discovery = directory.path().join("missing-discovery.json");
  let base = || {
    let mut command = Command::new(env!("CARGO_BIN_EXE_auv"));
    command.env("AUV_CONFIG_PROFILES_FILE", &config).env("AUV_DISCOVERY_FILE", &discovery).env_remove("AUV_ENDPOINT");
    command
  };

  let created = base()
    .args([
      "devices",
      "profiles",
      "create",
      "studio",
      "--device-id",
      DEVICE_ID,
      "--device-name",
      "Studio",
      "--endpoint",
      "http://127.0.0.1:1",
      "--device-credential",
      "test-bearer",
    ])
    .output()
    .unwrap();
  assert!(created.status.success(), "{}", String::from_utf8_lossy(&created.stderr));

  let listed = base().args(["devices", "list", "--json"]).output().unwrap();
  assert!(listed.status.success(), "{}", String::from_utf8_lossy(&listed.stderr));
  let devices: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
  assert_eq!(devices[0]["device_id"], DEVICE_ID);
  assert_eq!(devices[0]["status"], "offline");
  assert_eq!(devices[0]["config_profile"], "studio");

  let updated = base()
    .args([
      "devices",
      "profiles",
      "update",
      "studio",
      "--device-id",
      DEVICE_ID,
      "--device-name",
      "Studio 2",
      "--endpoint",
      "http://localhost:9848",
      "--device-credential",
      "updated-bearer",
    ])
    .output()
    .unwrap();
  assert!(updated.status.success(), "{}", String::from_utf8_lossy(&updated.stderr));
  let got = base().args(["devices", "profiles", "get", "studio", "--json"]).output().unwrap();
  let profile: serde_json::Value = serde_json::from_slice(&got.stdout).unwrap();
  assert_eq!(profile["name"], "Studio 2");

  let deleted = base().args(["devices", "profiles", "delete", "studio"]).output().unwrap();
  assert!(deleted.status.success(), "{}", String::from_utf8_lossy(&deleted.stderr));
  let empty = base().args(["devices", "profiles", "list", "--json"]).output().unwrap();
  assert_eq!(serde_json::from_slice::<serde_json::Value>(&empty.stdout).unwrap(), serde_json::json!([]));
}

#[test]
fn damaged_profile_store_is_not_rewritten_by_cli() {
  let directory = tempfile::tempdir().unwrap();
  let config = directory.path().join("profiles.json");
  fs::write(&config, b"damaged").unwrap();
  let output = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "devices",
      "profiles",
      "create",
      "studio",
      "--device-id",
      "device_studio",
      "--device-name",
      "Studio",
      "--endpoint",
      "http://localhost:9847",
      "--device-credential",
      "test-bearer",
    ])
    .env("AUV_CONFIG_PROFILES_FILE", &config)
    .output()
    .unwrap();
  assert!(!output.status.success());
  assert_eq!(fs::read(&config).unwrap(), b"damaged");
}
