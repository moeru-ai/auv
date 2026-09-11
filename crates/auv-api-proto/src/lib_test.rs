use crate::FILE_DESCRIPTOR_SET;
use crate::auv::api::daemon::v1::device_service_client::DeviceServiceClient;
use crate::auv::api::daemon::v1::discovery_service_client::DiscoveryServiceClient;
use crate::auv::api::daemon::v1::pairing_service_client::PairingServiceClient;
use crate::auv::api::daemon::v1::run_service_client::RunServiceClient;
use crate::auv::api::daemon::v1::runner_class_service_client::RunnerClassServiceClient;
use crate::auv::api::daemon::v1::runner_service_client::RunnerServiceClient;
use prost::Message;
use prost_reflect::{DescriptorPool, Value};
use prost_types::FileDescriptorSet;

#[test]
fn every_discoverable_method_declares_a_non_unspecified_effect() {
  let pool = DescriptorPool::decode(FILE_DESCRIPTOR_SET).expect("decode descriptor pool with extensions");
  let discoverable = pool.get_extension_by_name("auv.api.annotations.v1.discoverable").expect("discoverable extension descriptor");
  let effect = pool.get_extension_by_name("auv.api.annotations.v1.effect").expect("effect extension descriptor");

  for service in pool.services() {
    for method in service.methods() {
      let options = method.options();
      if options.get_extension(&discoverable).as_ref() == &Value::Bool(true) {
        assert!(options.has_extension(&effect), "{} omits the effect annotation", method.full_name());
        assert_ne!(options.get_extension(&effect).as_ref(), &Value::EnumNumber(0), "{} has unspecified effect", method.full_name());
      }
    }
  }
}

#[test]
fn daemon_json_uses_proto_enum_well_known_and_uint64_shapes() {
  use crate::auv::api::daemon::v1::{Runner, RunnerLifecycle, RunnerPhase};

  let runner = Runner {
    lifecycle: RunnerLifecycle::UnlessShutdown as i32,
    phase: RunnerPhase::Ready as i32,
    created_at: Some(prost_types::Timestamp {
      seconds: 0,
      nanos: 0,
    }),
    active_operations: u64::MAX,
    ..Default::default()
  };
  let json = serde_json::to_value(&runner).unwrap();
  assert_eq!(json["lifecycle"], "RUNNER_LIFECYCLE_UNLESS_SHUTDOWN");
  assert_eq!(json["phase"], "RUNNER_PHASE_READY");
  assert_eq!(json["createdAt"], "1970-01-01T00:00:00+00:00");
  assert_eq!(json["activeOperations"], u64::MAX.to_string());

  let decoded: Runner = serde_json::from_value(json).unwrap();
  assert_eq!(decoded.lifecycle, runner.lifecycle);
  assert_eq!(decoded.phase, runner.phase);
  assert_eq!(decoded.created_at, runner.created_at);
  assert_eq!(decoded.active_operations, runner.active_operations);
}

#[test]
fn daemon_control_services_are_typed_and_do_not_claim_watch() {
  let descriptor_set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("decode FILE_DESCRIPTOR_SET");
  let mut services = descriptor_set
    .file
    .iter()
    .filter(|file| file.package.as_deref() == Some("auv.api.daemon.v1"))
    .flat_map(|file| file.service.iter())
    .map(|service| {
      (service.name.as_deref().expect("service name"), service.method.iter().filter_map(|method| method.name.as_deref()).collect::<Vec<_>>())
    })
    .collect::<Vec<_>>();
  services.sort_by_key(|(name, _)| *name);

  let runner_file =
    descriptor_set.file.iter().find(|file| file.name.as_deref() == Some("auv/api/daemon/v1/runner.proto")).expect("Runner descriptor");
  let runner = runner_file.message_type.iter().find(|message| message.name.as_deref() == Some("Runner")).expect("Runner message");
  assert_eq!(
    runner.field.iter().filter_map(|field| field.name.as_deref()).collect::<Vec<_>>(),
    vec![
      "ref",
      "device",
      "runner_class",
      "labels",
      "lifecycle",
      "idle_timeout",
      "phase",
      "created_at",
      "process_id",
      "active_operations",
      "idle_deadline",
    ]
  );
  for (message_name, field_names) in [("Runner", &["active_operations"][..])] {
    let message = runner_file.message_type.iter().find(|message| message.name.as_deref() == Some(message_name)).expect(message_name);
    for field_name in field_names {
      assert_eq!(
        message.field.iter().find(|field| field.name.as_deref() == Some(field_name)).and_then(|field| field.r#type),
        Some(prost_types::field_descriptor_proto::Type::Uint64 as i32),
        "{message_name}.{field_name} must remain a uint64 counter"
      );
    }
  }
  assert!(!runner_file.message_type.iter().any(|message| message.name.as_deref() == Some("RunnerCapability")));

  fn assert_client<T>() {}
  assert_client::<DeviceServiceClient<tonic::transport::Channel>>();
  assert_client::<DiscoveryServiceClient<tonic::transport::Channel>>();
  assert_client::<PairingServiceClient<tonic::transport::Channel>>();
  assert_client::<RunServiceClient<tonic::transport::Channel>>();
  assert_client::<RunnerServiceClient<tonic::transport::Channel>>();
  assert_client::<RunnerClassServiceClient<tonic::transport::Channel>>();
}
