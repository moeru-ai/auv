use std::fs;
use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

fn private_file(path: &std::path::Path, contents: &[u8]) {
  fs::write(path, contents).unwrap();
  #[cfg(unix)]
  fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn profile_crud_and_offline_device_listing_use_non_secret_summaries() {
  let directory = tempfile::tempdir().unwrap();
  let config = directory.path().join("profiles.json");
  let credentials = directory.path().join("credentials.json");
  let discovery = directory.path().join("missing-discovery.json");
  let ca = directory.path().join("ca.pem");
  let certificate = directory.path().join("client.pem");
  let key = directory.path().join("client-key.pem");
  for path in [&ca, &certificate, &key] {
    private_file(path, b"private fixture");
  }
  let base = || {
    let mut command = Command::new(env!("CARGO_BIN_EXE_auv"));
    command
      .env("AUV_CONFIG_PROFILES_FILE", &config)
      .env("AUV_CREDENTIAL_PROFILES_FILE", &credentials)
      .env("AUV_DISCOVERY_FILE", &discovery)
      .env_remove("AUV_ENDPOINT");
    command
  };

  let created = base()
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
      "https://studio.example:9847",
      "--server-name",
      "studio.example",
      "--credential-profile",
      "paired-studio",
      "--server-ca-certificate",
    ])
    .arg(&ca)
    .arg("--client-certificate")
    .arg(&certificate)
    .arg("--client-private-key")
    .arg(&key)
    .output()
    .unwrap();
  assert!(created.status.success(), "{}", String::from_utf8_lossy(&created.stderr));

  let listed = base().args(["devices", "list", "--json"]).output().unwrap();
  assert!(listed.status.success(), "{}", String::from_utf8_lossy(&listed.stderr));
  let devices: serde_json::Value = serde_json::from_slice(&listed.stdout).unwrap();
  assert_eq!(devices[0]["device_id"], "device_studio");
  assert_eq!(devices[0]["status"], "offline");
  assert_eq!(devices[0]["config_profile"], "studio");
  assert!(!String::from_utf8_lossy(&listed.stdout).contains("private fixture"));

  let updated = base()
    .args([
      "devices",
      "profiles",
      "update",
      "studio",
      "--device-id",
      "device_studio",
      "--device-name",
      "Studio 2",
      "--endpoint",
      "https://studio.example:9848",
      "--server-name",
      "studio.example",
      "--credential-profile",
      "paired-studio",
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
  let credentials = directory.path().join("credentials.json");
  private_file(&config, b"damaged");
  private_file(&credentials, br#"{"profiles":{}}"#);
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
      "https://studio.example:9847",
      "--server-name",
      "studio.example",
      "--credential-profile",
      "paired",
    ])
    .env("AUV_CONFIG_PROFILES_FILE", &config)
    .env("AUV_CREDENTIAL_PROFILES_FILE", &credentials)
    .output()
    .unwrap();
  assert!(!output.status.success());
  assert_eq!(fs::read(&config).unwrap(), b"damaged");
}
