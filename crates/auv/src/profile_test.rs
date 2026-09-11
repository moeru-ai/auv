use super::*;

#[test]
fn profile_debug_output_redacts_bearer_and_remote_network_is_accepted() {
  let input = DeviceProfileInput {
    device_id: "device_test".to_string(),
    device_name: "test".to_string(),
    endpoint: "http://127.0.0.1:9847".to_string(),
    device_credential: "bearer-secret".to_string(),
  };
  let debug = format!("{input:?}");
  assert!(!debug.contains("bearer-secret"));
  assert!(debug.contains("[REDACTED]"));
  assert!(validate_remote_endpoint("http://[::1]:9847").is_ok());
  assert!(validate_remote_endpoint("http://192.0.2.10:9847").is_ok());
}

#[test]
fn profile_accepts_unknown_fields_and_default_permissions() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("profiles.json");
  std::fs::write(
      &path,
      br#"{"future_document_field":true,"profiles":{"studio":{"device_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","device_name":"Studio","endpoint":"http://localhost:9847","device_credential":"secret","future_profile_field":42}}}"#,
    )
    .unwrap();
  #[cfg(unix)]
  {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o666)).unwrap();
  }

  let store = ProfileStore::from_path(path);
  let listed = store.list_devices().unwrap();
  assert_eq!(listed[0].device_id(), "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
  let resolved = store
    .resolve(&AuvContext {
      config_profile: Some("studio".to_string()),
      ..Default::default()
    })
    .unwrap();
  assert_eq!(resolved.endpoint().to_string(), "http://localhost:9847/");
  assert_eq!(resolved.device_credential(), "secret");
}

#[test]
fn duplicate_device_names_report_canonical_candidate_ids() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("profiles.json");
  std::fs::write(
      &path,
      br#"{"profiles":{"a":{"device_id":"device_a","device_name":"Studio","endpoint":"http://localhost:9847","device_credential":"a-secret"},"b":{"device_id":"device_b","device_name":"Studio","endpoint":"http://localhost:9848","device_credential":"b-secret"}}}"#,
    )
    .unwrap();
  let error = ProfileStore::from_path(path)
    .resolve(&AuvContext {
      device_name: Some("Studio".to_string()),
      ..Default::default()
    })
    .unwrap_err();
  assert!(matches!(error, ProfileError::AmbiguousDevice(ids) if ids == "device_a, device_b"));
}

// https://github.com/moeru-ai/auv/actions/runs/31051684740/job/92460058236
// ROOT CAUSE:
//
// On Windows, opening the parent directory through File::open failed with
// Access Denied after the first profile document had already been published.
//
// Before the fix, profile CRUD only completed on Unix. The fix keeps atomic
// replacement and durability guarantees behind platform-specific operations.
#[test]
fn profile_crud_is_atomic_and_stores_the_opaque_bearer_inline() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("config/profiles.json");
  let store = ProfileStore::from_path(&path);
  store
    .create(
      "studio",
      DeviceProfileInput {
        device_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        device_name: "Studio".into(),
        endpoint: "http://localhost:9847".into(),
        device_credential: "secret-1".into(),
      },
    )
    .unwrap();
  assert_eq!(store.get_device("studio").unwrap().device_name(), "Studio");
  assert!(std::fs::read_to_string(&path).unwrap().contains("secret-1"));
  store
    .update(
      "studio",
      DeviceProfileInput {
        device_id: "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".into(),
        device_name: "Studio 2".into(),
        endpoint: "http://localhost:9848".into(),
        device_credential: "secret-2".into(),
      },
    )
    .unwrap();
  assert_eq!(store.get_device("studio").unwrap().device_name(), "Studio 2");
  store.delete("studio").unwrap();
  assert!(matches!(store.get_device("studio"), Err(ProfileError::UnknownConfigProfile(_))));
}

#[test]
fn damaged_profile_store_is_not_rewritten() {
  let directory = tempfile::tempdir().unwrap();
  let path = directory.path().join("profiles.json");
  std::fs::write(&path, b"damaged").unwrap();
  let error = ProfileStore::from_path(&path)
    .create(
      "studio",
      DeviceProfileInput {
        device_id: "device_studio".into(),
        device_name: "Studio".into(),
        endpoint: "http://localhost:9847".into(),
        device_credential: "secret".into(),
      },
    )
    .unwrap_err();
  assert!(matches!(error, ProfileError::Decode { .. }));
  assert_eq!(std::fs::read(path).unwrap(), b"damaged");
}
