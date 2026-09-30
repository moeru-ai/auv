use super::*;

fn device(id: &str, name: &str, local: bool) -> proto::Device {
  proto::Device {
    r#ref: Some(proto::DeviceRef {
      device_id: id.to_string(),
    }),
    name: name.to_string(),
    local,
    ..Default::default()
  }
}

#[test]
fn canonical_device_identity_does_not_depend_on_its_display_name() {
  let context = AuvContext {
    device_id: Some("device_a".to_string()),
    device_name: Some("configured alias".to_string()),
    ..Default::default()
  };
  assert!(context_matches_canonical_device(&context, &device("device_a", "", false)).is_ok());
}

#[test]
fn duplicate_names_report_stable_candidate_ids() {
  let devices = [
    device("device_a", "studio", true),
    device("device_b", "studio", false),
  ];
  let error = select_device(&devices, &DeviceSelector::by_name("studio"), PlacementConstraint::Automatic, None).unwrap_err();
  assert!(error.to_string().contains("device_a, device_b"));
}

#[test]
fn local_constraint_rejects_an_explicit_remote_device() {
  let devices = [
    device("device_local", "local", true),
    device("device_remote", "remote", false),
  ];
  let error = select_device(&devices, &DeviceSelector::by_id("device_remote"), PlacementConstraint::LocalOnly, None).unwrap_err();
  assert!(error.to_string().contains("conflicts with remote Device"));
}

#[test]
fn resource_selectors_accept_unique_short_ids_and_reject_ambiguity() {
  let devices = [
    device("0123456789abaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "a", true),
    device("fedcba987654bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "b", false),
  ];
  let selected = select_device(&devices, &DeviceSelector::by_id("0123456789ab"), PlacementConstraint::Automatic, None).unwrap();
  assert_eq!(device_id(&selected), device_id(&devices[0]));

  let run_ids = [
    "abc00000000000000000000000000000",
    "abc11111111111111111111111111111",
  ];
  let selector = RunSelector::parse("abc").unwrap();
  let error = resolve_run_id(&selector, run_ids.into_iter()).unwrap_err();
  assert!(error.to_string().contains("ambiguous"));
}

#[test]
fn automatic_existing_multi_device_run_defers_placement_to_the_runner_route() {
  let devices = [
    device("device_a", "a", true),
    device("device_b", "b", false),
  ];
  let allowed = [
    proto::DeviceRef {
      device_id: "device_a".to_string(),
    },
    proto::DeviceRef {
      device_id: "device_b".to_string(),
    },
  ];
  assert!(select_default_device(&devices, PlacementConstraint::Automatic, Some(&allowed)).unwrap().is_none());
  assert_eq!(
    select_default_device(&devices, PlacementConstraint::LocalOnly, Some(&allowed))
      .unwrap()
      .and_then(|device| device.r#ref)
      .map(|reference| reference.device_id),
    Some("device_a".to_string())
  );
}

#[cfg(unix)]
#[tokio::test]
async fn stale_local_discovery_does_not_block_a_single_paired_device() {
  let directory = tempfile::tempdir().expect("temporary device selection directory");
  let device_id = "018f1f00000070008000000000000001018f1f00000070008000000000000001";
  let profiles = profile::ProfileStore::from_path(directory.path().join("profiles.json"));
  profiles
    .create(
      "remote",
      profile::DeviceProfileInput {
        device_id: device_id.to_string(),
        device_name: "remote".to_string(),
        endpoint: "http://127.0.0.1:0".to_string(),
        device_credential: "test-credential".to_string(),
      },
    )
    .expect("store remote profile");

  let missing_socket = directory.path().join("stale-local.sock");
  let local_endpoint = Some(ConnectEndpoint::Unix(missing_socket));

  let selected = Client::resolve_device_context(
    AuvContext {
      device_id: Some(device_id.to_string()),
      ..Default::default()
    },
    &profiles,
    local_endpoint.clone(),
  )
  .await;

  assert!(matches!(selected, Err(ContextError::PairedConnect(_))), "expected the paired profile to be tried: {selected:?}");

  // ROOT CAUSE:
  //
  // The Device list displays ID prefixes, but profile routing compared the
  // selected prefix with the full ID. A stale local endpoint then failed
  // before the paired profile could be reached.
  let selected_prefix = Client::resolve_device_context(
    AuvContext {
      device_id: Some(device_id[..12].to_string()),
      ..Default::default()
    },
    &profiles,
    local_endpoint.clone(),
  )
  .await;

  assert!(
    matches!(selected_prefix, Err(ContextError::PairedConnect(_))),
    "expected the ID prefix to route to the paired profile: {selected_prefix:?}"
  );

  let unmatched = Client::resolve_device_context(
    AuvContext {
      device_id: Some("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_string()),
      ..Default::default()
    },
    &profiles,
    local_endpoint,
  )
  .await;

  assert!(matches!(unmatched, Err(ContextError::Connect(_))), "an unmatched profile cannot hide local connection errors: {unmatched:?}");
}
