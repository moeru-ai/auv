#![cfg(unix)]

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::net::TcpListener;
use std::os::unix::fs::OpenOptionsExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use auv_api_client::placement::RunnerOptions;
use auv_api_client::profile::{ProfileError, ProfileStore};
use auv_api_client::{AuvContext, Client, ContextError};
use auv_api_proto::auv::api::core::v1::RunOutcome;
use auv_api_server::authority::{ApiScope, CertificateFingerprint, CredentialState, PairingCredential, PairingRecord, PairingStore};
use rcgen::{BasicConstraints, Certificate, CertificateParams, ExtendedKeyUsagePurpose, IsCa, Issuer, KeyPair, KeyUsagePurpose};

struct ChildGuard(Child);

impl Drop for ChildGuard {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

#[tokio::test]
async fn named_profile_selects_one_of_two_paired_daemons_and_preserves_typed_placement() {
  let directory = tempfile::tempdir().unwrap();
  let (ca, issuer) = certificate_authority();
  let (client_certificate, client_key) = leaf_certificate(&issuer, "paired-client", ExtendedKeyUsagePurpose::ClientAuth);
  let (server_certificate, server_key) = leaf_certificate(&issuer, "localhost", ExtendedKeyUsagePurpose::ServerAuth);
  let first_id = "device_018f1f00-0000-7000-8000-000000000001";
  let second_id = "device_018f1f00-0000-7000-8000-000000000002";
  let local_id = "device_018f1f00-0000-7000-8000-000000000003";
  let (mut local, discovery_path) = spawn_local_daemon(directory.path(), local_id, "Studio");
  wait_for_path(&mut local.0, &discovery_path);
  let (mut first, first_port) =
    spawn_daemon(directory.path(), "first", first_id, "Studio", &ca, &server_certificate, &server_key, &client_certificate);
  let (mut second, second_port) =
    spawn_daemon(directory.path(), "second", second_id, "Studio", &ca, &server_certificate, &server_key, &client_certificate);

  let ca_path = directory.path().join("ca.pem");
  let client_certificate_path = directory.path().join("client.pem");
  let client_key_path = directory.path().join("client-key.pem");
  write_private(&ca_path, ca.pem().as_bytes());
  write_private(&client_certificate_path, client_certificate.pem().as_bytes());
  write_private(&client_key_path, client_key.serialize_pem().as_bytes());
  let config_path = directory.path().join("device-profiles.json");
  let credential_path = directory.path().join("credential-profiles.json");
  write_private(
    &config_path,
    serde_json::to_string(&serde_json::json!({
      "profiles": {
        "first": {
          "device_id": first_id,
          "device_name": "Studio",
          "endpoint": format!("https://127.0.0.1:{first_port}"),
          "server_name": "localhost",
          "credential_profile": "paired-first"
        },
        "second": {
          "device_id": second_id,
          "device_name": "Studio",
          "endpoint": format!("https://127.0.0.1:{second_port}"),
          "server_name": "localhost",
          "credential_profile": "paired-second"
        }
      }
    }))
    .unwrap()
    .as_bytes(),
  );
  write_private(
    &credential_path,
    serde_json::to_string(&serde_json::json!({
      "profiles": {
        "paired-first": {
          "server_ca_certificate": ca_path,
          "client_certificate": client_certificate_path,
          "client_private_key": client_key_path
        },
        "paired-second": {
          "server_ca_certificate": directory.path().join("ca.pem"),
          "client_certificate": directory.path().join("client.pem"),
          "client_private_key": directory.path().join("client-key.pem")
        }
      }
    }))
    .unwrap()
    .as_bytes(),
  );
  let profiles = ProfileStore::from_paths(config_path, credential_path);

  let ambiguous = profiles
    .resolve(&AuvContext {
      device_name: Some("Studio".to_string()),
      ..Default::default()
    })
    .expect_err("duplicate remote names require a canonical ID or named profile");
  assert!(matches!(ambiguous, ProfileError::AmbiguousDevice(ids) if ids == format!("{first_id}, {second_id}")));

  let wrong_binding = Client::from_context_with_profiles(
    AuvContext {
      config_profile: Some("second".to_string()),
      credential_profile: Some("paired-first".to_string()),
      ..Default::default()
    },
    &profiles,
  )
  .await
  .expect_err("context cannot replace a Device profile's credential binding");
  assert!(matches!(
    wrong_binding,
    ContextError::Profile(ProfileError::ContextConflict {
      field: "credential_profile",
      ..
    })
  ));

  let first_client = wait_for_profile(&mut first.0, &profiles, "first").await;
  assert_eq!(first_client.context().and_then(|context| context.device_id.as_deref()), Some(first_id));
  let second_client = wait_for_profile(&mut second.0, &profiles, "second").await;

  let plugin = directory.path().join("auv-fixture-context");
  write_private(&plugin, b"#!/bin/sh\nprintf '%s\n' \"$AUV_CONTEXT\"\n");
  let mut plugin_permissions = fs::metadata(&plugin).unwrap().permissions();
  use std::os::unix::fs::PermissionsExt as _;
  plugin_permissions.set_mode(0o700);
  fs::set_permissions(&plugin, plugin_permissions).unwrap();
  let ambiguous_cli = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["--device", "Studio", "fixture-context"])
    .env("PATH", directory.path())
    .env("AUV_DISCOVERY_FILE", &discovery_path)
    .env("AUV_CONFIG_PROFILES_FILE", profiles.config_path())
    .env("AUV_CREDENTIAL_PROFILES_FILE", profiles.credential_path())
    .output()
    .expect("run ambiguous root Device selection");
  assert!(!ambiguous_cli.status.success());
  let ambiguous_stderr = String::from_utf8_lossy(&ambiguous_cli.stderr);
  for candidate in [local_id, first_id, second_id] {
    assert!(ambiguous_stderr.contains(candidate), "missing candidate {candidate} in {ambiguous_stderr}");
  }

  let remote_plugin = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["--device-id", second_id, "fixture-context"])
    .env("PATH", directory.path())
    .env("AUV_DISCOVERY_FILE", &discovery_path)
    .env("AUV_CONFIG_PROFILES_FILE", profiles.config_path())
    .env("AUV_CREDENTIAL_PROFILES_FILE", profiles.credential_path())
    .output()
    .expect("run plugin through paired Device profile");
  assert!(remote_plugin.status.success(), "stderr={}", String::from_utf8_lossy(&remote_plugin.stderr));
  let injected: serde_json::Value = serde_json::from_slice(&remote_plugin.stdout).expect("injected remote AUV_CONTEXT");
  assert_eq!(injected["device_id"], second_id);
  assert_eq!(injected["config_profile"], "second");
  assert_eq!(injected["credential_profile"], "paired-second");
  assert_eq!(injected["daemon_endpoint"], format!("https://127.0.0.1:{second_port}/"));
  assert!(injected["run_id"].as_str().is_some_and(|run_id| run_id.starts_with("run_")));
  assert!(!String::from_utf8_lossy(&remote_plugin.stdout).contains("PRIVATE KEY"));

  let context = second_client.context().expect("resolved profile context");
  assert_eq!(context.device_id.as_deref(), Some(second_id));
  assert_eq!(context.config_profile.as_deref(), Some("second"));
  assert_eq!(context.credential_profile.as_deref(), Some("paired-second"));
  assert!(!serde_json::to_string(context).unwrap().contains("PRIVATE KEY"));

  let repeated =
    Client::from_context_with_profiles(context.clone(), &profiles).await.expect("matching injected endpoint and profile reconnect");
  assert_eq!(repeated.context().and_then(|context| context.device_id.as_deref()), Some(second_id));
  let mismatch = Client::from_context_with_profiles(
    AuvContext {
      daemon_endpoint: Some(format!("https://127.0.0.1:{first_port}")),
      config_profile: Some("second".to_string()),
      ..Default::default()
    },
    &profiles,
  )
  .await
  .expect_err("injected endpoint must match the selected Device profile");
  assert!(matches!(mismatch, ContextError::ProfileEndpointMismatch { .. }));
  second_client.clone().placement().local().expect_err("paired remote transport cannot satisfy caller-local placement");

  let mut control = second_client.clone();
  let explicit_run = control
    .create_run(auv_api_proto::auv::api::core::v1::CreateRunRequest::default())
    .await
    .expect("create paired Run for run-only selection");
  let explicit_run_id = explicit_run.r#ref.as_ref().expect("Run ref").run_id.clone();
  let run_only_plugin = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["--run", &explicit_run_id, "fixture-context"])
    .env("PATH", directory.path())
    .env("AUV_DISCOVERY_FILE", &discovery_path)
    .env("AUV_CONFIG_PROFILES_FILE", profiles.config_path())
    .env("AUV_CREDENTIAL_PROFILES_FILE", profiles.credential_path())
    .output()
    .expect("resolve remote Run across configured daemon pool");
  assert!(run_only_plugin.status.success(), "stderr={}", String::from_utf8_lossy(&run_only_plugin.stderr));
  let run_only_context: serde_json::Value = serde_json::from_slice(&run_only_plugin.stdout).expect("run-only plugin context");
  assert_eq!(run_only_context["run_id"], explicit_run_id);
  assert_eq!(run_only_context["device_id"], second_id);
  assert_eq!(run_only_context["config_profile"], "second");
  control.stop_run(explicit_run_id, RunOutcome::Canceled).await.expect("stop paired run-only fixture");

  let execution = second_client.placement().runner(RunnerOptions::default()).await.expect("create remote Run and claim typed Runner");
  assert_eq!(execution.run().devices.first().map(|device| device.device_id.as_str()), Some(second_id));
  assert_eq!(execution.runner().and_then(|runner| runner.device.as_ref()).map(|device| device.device_id.as_str()), Some(second_id));
  let finished = execution.finish(RunOutcome::Succeeded).await.expect("release Runner and finish owned Run");
  assert_eq!(finished.phase, auv_api_proto::auv::api::core::v1::RunPhase::Succeeded as i32);
}

async fn wait_for_profile(child: &mut Child, profiles: &ProfileStore, name: &str) -> Client {
  let deadline = Instant::now() + Duration::from_secs(15);
  loop {
    match Client::from_context_with_profiles(
      AuvContext {
        config_profile: Some(name.to_string()),
        ..Default::default()
      },
      profiles,
    )
    .await
    {
      Ok(client) => return client,
      Err(error) if Instant::now() < deadline => {
        if let Some(status) = child.try_wait().expect("inspect paired daemon") {
          panic!("paired daemon exited before readiness: {status}; last client error: {error}");
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
      }
      Err(error) => panic!("paired daemon did not become ready: {error}"),
    }
  }
}

fn spawn_daemon(
  root: &Path,
  label: &str,
  device_id: &str,
  device_name: &str,
  ca: &Certificate,
  server_certificate: &Certificate,
  server_key: &KeyPair,
  client_certificate: &Certificate,
) -> (ChildGuard, u16) {
  let directory = root.join(label);
  let control = directory.join("store/control");
  fs::create_dir_all(&control).unwrap();
  write_private(&control.join("device-id"), format!("{device_id}\n").as_bytes());
  let server_certificate_path = directory.join("server.pem");
  let server_key_path = directory.join("server-key.pem");
  let client_ca_path = directory.join("client-ca.pem");
  let pairing_store_path = directory.join("pairings.json");
  write_private(&server_certificate_path, server_certificate.pem().as_bytes());
  write_private(&server_key_path, server_key.serialize_pem().as_bytes());
  write_private(&client_ca_path, ca.pem().as_bytes());
  let pairing_store = PairingStore::open(pairing_store_path.clone()).expect("open pairing store");
  pairing_store
    .insert(PairingRecord {
      pair_id: format!("paired-{label}"),
      label: format!("paired {label}"),
      enabled: true,
      scopes: vec![
        ApiScope::ControlInspect,
        ApiScope::ControlManage,
        ApiScope::OperationsExecute,
      ],
      credentials: vec![PairingCredential {
        certificate_fingerprint: CertificateFingerprint::from_der(client_certificate.der().as_ref()),
        state: CredentialState::Active,
      }],
    })
    .expect("pair test client");
  drop(pairing_store);
  let port = reserve_port();
  let child = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "api-server",
      "serve",
      "--remote-listen",
      "127.0.0.1",
      "--port",
      &port.to_string(),
      "--tls-certificate",
      server_certificate_path.to_str().unwrap(),
      "--tls-private-key",
      server_key_path.to_str().unwrap(),
      "--client-ca-certificate",
      client_ca_path.to_str().unwrap(),
      "--pairing-store",
      pairing_store_path.to_str().unwrap(),
      "--store-root",
      directory.join("store").to_str().unwrap(),
      "--no-discovery",
    ])
    .env("HOSTNAME", device_name)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::inherit())
    .spawn()
    .expect("spawn paired daemon");
  (ChildGuard(child), port)
}

fn spawn_local_daemon(root: &Path, device_id: &str, device_name: &str) -> (ChildGuard, std::path::PathBuf) {
  let directory = root.join("local");
  let control = directory.join("store/control");
  fs::create_dir_all(&control).unwrap();
  write_private(&control.join("device-id"), format!("{device_id}\n").as_bytes());
  let socket = directory.join("auv.sock");
  let discovery = directory.join("discovery.json");
  let child = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "serve",
      "--listen",
      &format!("unix://{}", socket.display()),
      "--store-root",
      directory.join("store").to_str().unwrap(),
      "--discovery-file",
      discovery.to_str().unwrap(),
    ])
    .env("HOSTNAME", device_name)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::inherit())
    .spawn()
    .expect("spawn local daemon");
  (ChildGuard(child), discovery)
}

fn wait_for_path(child: &mut Child, path: &Path) {
  let deadline = Instant::now() + Duration::from_secs(15);
  while Instant::now() < deadline {
    if path.exists() {
      return;
    }
    if let Some(status) = child.try_wait().expect("inspect local daemon") {
      panic!("local daemon exited before publishing {}: {status}", path.display());
    }
    std::thread::sleep(Duration::from_millis(25));
  }
  panic!("local daemon did not publish {}", path.display());
}

fn reserve_port() -> u16 {
  TcpListener::bind(("127.0.0.1", 0)).unwrap().local_addr().unwrap().port()
}

fn write_private(path: &Path, contents: &[u8]) {
  let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(path).expect("create private fixture");
  file.write_all(contents).expect("write private fixture");
}

fn certificate_authority() -> (Certificate, Issuer<'static, KeyPair>) {
  let mut params = CertificateParams::new(Vec::<String>::new()).expect("empty CA subject names");
  params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
  params.key_usages = vec![
    KeyUsagePurpose::DigitalSignature,
    KeyUsagePurpose::KeyCertSign,
    KeyUsagePurpose::CrlSign,
  ];
  let key = KeyPair::generate().expect("generate CA key");
  let certificate = params.self_signed(&key).expect("self-sign CA");
  (certificate, Issuer::new(params, key))
}

fn leaf_certificate(issuer: &Issuer<'static, KeyPair>, name: &str, purpose: ExtendedKeyUsagePurpose) -> (Certificate, KeyPair) {
  let mut params = CertificateParams::new(vec![name.to_string()]).expect("valid test subject name");
  params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
  params.extended_key_usages = vec![purpose];
  let key = KeyPair::generate().expect("generate leaf key");
  let certificate = params.signed_by(&key, issuer).expect("sign leaf certificate");
  (certificate, key)
}
