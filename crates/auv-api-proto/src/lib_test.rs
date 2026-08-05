use crate::auv::api::daemon::v1::device_service_client::DeviceServiceClient;
use crate::auv::api::daemon::v1::discovery_service_client::DiscoveryServiceClient;
use crate::auv::api::daemon::v1::pairing_service_client::PairingServiceClient;
use crate::auv::api::daemon::v1::run_service_client::RunServiceClient;
use crate::auv::api::daemon::v1::runner_class_service_client::RunnerClassServiceClient;
use crate::auv::api::daemon::v1::runner_service_client::RunnerServiceClient;
use crate::auv::api::driver::macos::v1::media_control_service_client::MediaControlServiceClient;
use crate::auv::api::driver::v1::capture_service_client::CaptureServiceClient;
use crate::auv::api::driver::v1::display_service_client::DisplayServiceClient;
use crate::auv::api::driver::v1::input_service_client::InputServiceClient;
use crate::auv::api::driver::v1::overlay_service_client::OverlayServiceClient;
use crate::auv::api::driver::v1::text_recognition_service_client::TextRecognitionServiceClient;
use crate::auv::api::driver::v1::window_service_client::WindowServiceClient;
use crate::{FILE_DESCRIPTOR_SET, descriptor_set_for_service, descriptor_set_for_services};
use prost::Message;
use prost_reflect::{DescriptorPool, Value};
use prost_types::FileDescriptorSet;

#[test]
fn service_descriptor_closure_excludes_unserved_auv_services() {
  let encoded = descriptor_set_for_service("auv.api.driver.v1.DisplayService").expect("DisplayService exists");
  let set = FileDescriptorSet::decode(encoded.as_slice()).expect("closure is a descriptor set");
  let services = set
    .file
    .iter()
    .flat_map(|file| {
      let package = file.package.as_deref().unwrap_or_default();
      file.service.iter().map(move |service| format!("{package}.{}", service.name.as_deref().unwrap_or_default()))
    })
    .collect::<Vec<_>>();

  assert!(services.contains(&"auv.api.driver.v1.DisplayService".to_string()));
  assert!(!services.contains(&"auv.api.daemon.v1.DeviceService".to_string()));
}

#[test]
fn multi_service_descriptor_closure_contains_each_served_driver_service() {
  let encoded = descriptor_set_for_services(&[
    "auv.api.driver.v1.DisplayService",
    "auv.api.driver.v1.WindowService",
  ])
  .expect("driver services exist");
  let set = FileDescriptorSet::decode(encoded.as_slice()).expect("closure is a descriptor set");
  let services = set
    .file
    .iter()
    .flat_map(|file| {
      let package = file.package.as_deref().unwrap_or_default();
      file.service.iter().map(move |service| format!("{package}.{}", service.name.as_deref().unwrap_or_default()))
    })
    .collect::<Vec<_>>();
  assert!(services.contains(&"auv.api.driver.v1.DisplayService".to_string()));
  assert!(services.contains(&"auv.api.driver.v1.WindowService".to_string()));
}

#[test]
fn descriptor_closure_preserves_discovery_annotations() {
  let encoded = descriptor_set_for_service("auv.api.driver.v1.DisplayService").expect("DisplayService exists");
  let pool = DescriptorPool::decode(encoded.as_slice()).expect("closure retains dynamic options");
  let discoverable = pool.get_extension_by_name("auv.api.annotations.v1.discoverable").expect("discoverable extension descriptor");
  let effect = pool.get_extension_by_name("auv.api.annotations.v1.effect").expect("effect extension descriptor");
  let method = pool.get_service_by_name("auv.api.driver.v1.DisplayService").expect("DisplayService").methods().next().expect("ListDisplays");
  let options = method.options();
  assert_eq!(options.get_extension(&discoverable).as_ref(), &Value::Bool(true));
  assert_eq!(options.get_extension(&effect).as_ref(), &Value::EnumNumber(1));
}

#[test]
fn macos_media_control_service_has_typed_read_and_input_methods() {
  let encoded = descriptor_set_for_service("auv.api.driver.macos.v1.MediaControlService").expect("MediaControlService exists");
  let pool = DescriptorPool::decode(encoded.as_slice()).expect("media descriptor closure");
  let service = pool.get_service_by_name("auv.api.driver.macos.v1.MediaControlService").expect("MediaControlService");
  let methods = service.methods().collect::<Vec<_>>();
  assert_eq!(
    methods.iter().map(|method| method.name()).collect::<Vec<_>>(),
    [
      "GetNowPlaying",
      "Play",
      "Pause",
      "TogglePlayPause",
      "NextTrack",
      "PreviousTrack"
    ]
  );
  let discoverable = pool.get_extension_by_name("auv.api.annotations.v1.discoverable").expect("discoverable extension");
  let effect = pool.get_extension_by_name("auv.api.annotations.v1.effect").expect("effect extension");
  for (index, method) in methods.iter().enumerate() {
    assert!(method.input().get_field_by_name("lease").is_none());
    let options = method.options();
    assert_eq!(options.get_extension(&discoverable).as_ref(), &Value::Bool(true));
    assert_eq!(options.get_extension(&effect).as_ref(), &Value::EnumNumber(if index == 0 { 1 } else { 3 }));
  }

  fn assert_client<T>() {}
  assert_client::<MediaControlServiceClient<tonic::transport::Channel>>();
}

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
fn overlay_service_is_two_typed_mutations() {
  let encoded = descriptor_set_for_service("auv.api.driver.v1.OverlayService").expect("OverlayService exists");
  let pool = DescriptorPool::decode(encoded.as_slice()).expect("overlay descriptor closure");
  let service = pool.get_service_by_name("auv.api.driver.v1.OverlayService").expect("OverlayService");
  let methods = service.methods().collect::<Vec<_>>();
  assert_eq!(methods.iter().map(|method| method.name()).collect::<Vec<_>>(), ["ShowOverlay", "RemoveOverlay"]);
  let discoverable = pool.get_extension_by_name("auv.api.annotations.v1.discoverable").expect("discoverable");
  let effect = pool.get_extension_by_name("auv.api.annotations.v1.effect").expect("effect");
  for method in methods {
    assert_eq!(method.options().get_extension(&discoverable).as_ref(), &Value::Bool(true));
    assert_eq!(method.options().get_extension(&effect).as_ref(), &Value::EnumNumber(2));
    assert!(method.input().get_field_by_name("lease").is_none());
  }
  fn assert_client<T>() {}
  assert_client::<OverlayServiceClient<tonic::transport::Channel>>();
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

  assert_eq!(
    services,
    vec![
      ("DeviceService", vec!["ListDevices", "GetDevice"]),
      ("DiscoveryService", vec!["ListApiNamespaces", "GetApiNamespace", "GetApiGroupVersion"],),
      (
        "PairingService",
        vec![
          "CreatePairingToken",
          "PairDevice",
          "RevokeDeviceCredential",
          "SetPairedDeviceEnabled",
          "UnpairDevice"
        ],
      ),
      ("RunService", vec!["CreateRun", "ListRuns", "GetRun", "StopRun"]),
      ("RunnerClassService", vec!["ListRunnerClasses", "GetRunnerClass"]),
      ("RunnerService", vec!["CreateRunner", "ListRunners", "GetRunner", "DeleteRunner"],),
    ]
  );
  assert!(
    services.iter().flat_map(|(_, methods)| methods).all(|method| !method.starts_with("Watch")),
    "WATCH must remain absent until resource-version and reconnect semantics are implemented"
  );

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

#[test]
fn driver_display_service_is_typed_and_route_independent() {
  let descriptor_set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("decode FILE_DESCRIPTOR_SET");
  let file = descriptor_set
    .file
    .iter()
    .find(|file| file.name.as_deref() == Some("auv/api/driver/v1/display.proto"))
    .expect("Display driver descriptor");
  let service = file.service.iter().find(|service| service.name.as_deref() == Some("DisplayService")).expect("DisplayService");
  assert_eq!(service.method.iter().filter_map(|method| method.name.as_deref()).collect::<Vec<_>>(), vec!["ListDisplays"]);
  let request = file.message_type.iter().find(|message| message.name.as_deref() == Some("ListDisplaysRequest")).expect("request");
  assert!(!request.field.iter().any(|field| field.name.as_deref() == Some("lease")));

  fn assert_client<T>() {}
  assert_client::<DisplayServiceClient<tonic::transport::Channel>>();
  assert_client::<WindowServiceClient<tonic::transport::Channel>>();
}

#[test]
fn driver_capture_service_is_typed_and_route_independent() {
  let descriptor_set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("decode FILE_DESCRIPTOR_SET");
  let file = descriptor_set
    .file
    .iter()
    .find(|file| file.name.as_deref() == Some("auv/api/driver/v1/capture.proto"))
    .expect("Capture driver descriptor");
  let service = file.service.iter().find(|service| service.name.as_deref() == Some("CaptureService")).expect("CaptureService");
  assert_eq!(
    service.method.iter().filter_map(|method| method.name.as_deref()).collect::<Vec<_>>(),
    vec!["CaptureWindow", "CaptureDisplay", "CaptureRegion"]
  );
  let request = file.message_type.iter().find(|message| message.name.as_deref() == Some("CaptureWindowRequest")).expect("request");
  assert!(!request.field.iter().any(|field| field.name.as_deref() == Some("lease")));
  let window = request.field.iter().find(|field| field.name.as_deref() == Some("window")).expect("resolved Window ref");
  assert_eq!(window.type_name.as_deref(), Some(".auv.api.driver.v1.WindowRef"));
  let display_request =
    file.message_type.iter().find(|message| message.name.as_deref() == Some("CaptureDisplayRequest")).expect("display request");
  assert!(!display_request.field.iter().any(|field| field.name.as_deref() == Some("lease")));
  let display_selector = display_request.field.iter().find(|field| field.name.as_deref() == Some("selector")).expect("Display selector");
  assert_eq!(display_selector.type_name.as_deref(), Some(".auv.api.driver.v1.DisplaySelector"));

  fn assert_client<T>() {}
  assert_client::<CaptureServiceClient<tonic::transport::Channel>>();
}

#[test]
fn driver_text_recognition_service_is_typed_and_route_independent() {
  let descriptor_set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("decode FILE_DESCRIPTOR_SET");
  let file = descriptor_set
    .file
    .iter()
    .find(|file| file.name.as_deref() == Some("auv/api/driver/v1/text_recognition.proto"))
    .expect("TextRecognition driver descriptor");
  let service =
    file.service.iter().find(|service| service.name.as_deref() == Some("TextRecognitionService")).expect("TextRecognitionService");
  assert_eq!(
    service.method.iter().filter_map(|method| method.name.as_deref()).collect::<Vec<_>>(),
    vec!["RecognizeText", "FindWindowText", "FindDisplayText"]
  );
  for request_name in [
    "RecognizeTextRequest",
    "FindWindowTextRequest",
    "FindDisplayTextRequest",
  ] {
    let request = file.message_type.iter().find(|message| message.name.as_deref() == Some(request_name)).expect("request");
    assert!(!request.field.iter().any(|field| field.name.as_deref() == Some("lease")));
  }
  let recognize =
    file.message_type.iter().find(|message| message.name.as_deref() == Some("RecognizeTextRequest")).expect("recognize request");
  assert_eq!(
    recognize.field.iter().find(|field| field.name.as_deref() == Some("capture")).and_then(|field| field.type_name.as_deref()),
    Some(".auv.api.driver.v1.CapturedFrame")
  );
  let find_window =
    file.message_type.iter().find(|message| message.name.as_deref() == Some("FindWindowTextRequest")).expect("find window request");
  assert_eq!(
    find_window.field.iter().find(|field| field.name.as_deref() == Some("window")).and_then(|field| field.type_name.as_deref()),
    Some(".auv.api.driver.v1.WindowRef")
  );

  fn assert_client<T>() {}
  assert_client::<TextRecognitionServiceClient<tonic::transport::Channel>>();
}

#[test]
fn driver_input_service_is_route_independent_and_returns_delivery_evidence() {
  let descriptor_set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("decode FILE_DESCRIPTOR_SET");
  let file =
    descriptor_set.file.iter().find(|file| file.name.as_deref() == Some("auv/api/driver/v1/input.proto")).expect("Input driver descriptor");
  let service = file.service.iter().find(|service| service.name.as_deref() == Some("InputService")).expect("InputService");
  assert_eq!(
    service.method.iter().filter_map(|method| method.name.as_deref()).collect::<Vec<_>>(),
    vec![
      "ClickWindowPoint",
      "ClickScreenPoint",
      "MoveMouse",
      "StreamMouseMotion",
      "TypeText",
      "PasteText",
      "PressKey"
    ]
  );
  let complete = service.method.iter().find(|method| method.name.as_deref() == Some("MoveMouse")).expect("MoveMouse");
  assert!(!complete.client_streaming.unwrap_or(false));
  assert!(complete.server_streaming.unwrap_or(false));
  assert_eq!(complete.input_type.as_deref(), Some(".auv.api.driver.v1.MoveMouseRequest"));
  assert_eq!(complete.output_type.as_deref(), Some(".auv.api.driver.v1.MoveMouseStreamResponse"));
  let streaming = service.method.iter().find(|method| method.name.as_deref() == Some("StreamMouseMotion")).expect("StreamMouseMotion");
  assert!(streaming.client_streaming.unwrap_or(false));
  assert!(streaming.server_streaming.unwrap_or(false));
  assert_eq!(streaming.input_type.as_deref(), Some(".auv.api.driver.v1.StreamMouseMotionRequest"));
  assert_eq!(streaming.output_type.as_deref(), Some(".auv.api.driver.v1.StreamMouseMotionResponse"));
  for request_name in [
    "ClickWindowPointRequest",
    "ClickScreenPointRequest",
    "MoveMouseRequest",
    "StreamMouseMotionRequest",
    "TypeTextRequest",
    "PasteTextRequest",
    "PressKeyRequest",
  ] {
    let request = file.message_type.iter().find(|message| message.name.as_deref() == Some(request_name)).expect("request");
    assert!(!request.field.iter().any(|field| field.name.as_deref() == Some("lease")));
  }
  for response_name in [
    "ClickWindowPointResponse",
    "ClickScreenPointResponse",
    "MouseMotionCompleted",
    "TypeTextResponse",
    "PasteTextResponse",
    "PressKeyResponse",
  ] {
    let response = file.message_type.iter().find(|message| message.name.as_deref() == Some(response_name)).expect("response");
    let action = response.field.iter().find(|field| field.name.as_deref() == Some("action")).expect("InputActionResult");
    assert_eq!(action.type_name.as_deref(), Some(".auv.api.driver.v1.InputActionResult"));
  }
  let click_request = file.message_type.iter().find(|message| message.name.as_deref() == Some("ClickWindowPointRequest")).unwrap();
  assert_eq!(
    click_request.field.iter().find(|field| field.name.as_deref() == Some("window")).and_then(|field| field.type_name.as_deref()),
    Some(".auv.api.driver.v1.WindowRef")
  );
  let screen_click_request = file.message_type.iter().find(|message| message.name.as_deref() == Some("ClickScreenPointRequest")).unwrap();
  assert_eq!(
    screen_click_request.field.iter().find(|field| field.name.as_deref() == Some("point")).and_then(|field| field.type_name.as_deref()),
    Some(".auv.api.driver.v1.ScreenPoint")
  );
  assert_eq!(
    screen_click_request.field.iter().find(|field| field.name.as_deref() == Some("options")).and_then(|field| field.type_name.as_deref()),
    Some(".auv.api.driver.v1.ScreenClickOptions")
  );
  let paste_request = file.message_type.iter().find(|message| message.name.as_deref() == Some("PasteTextRequest")).unwrap();
  assert_eq!(
    paste_request.field.iter().find(|field| field.name.as_deref() == Some("options")).and_then(|field| field.type_name.as_deref()),
    Some(".auv.api.driver.v1.PasteTextOptions")
  );

  fn assert_client<T>() {}
  assert_client::<InputServiceClient<tonic::transport::Channel>>();
}

#[test]
fn macos_permission_service_is_a_typed_exact_projection() {
  use crate::auv::api::driver::macos::v1::permission_service_client::PermissionServiceClient;

  let descriptor_set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("decode FILE_DESCRIPTOR_SET");
  let file = descriptor_set
    .file
    .iter()
    .find(|file| file.name.as_deref() == Some("auv/api/driver/macos/v1/permission.proto"))
    .expect("macOS permission descriptor");
  assert_eq!(file.package.as_deref(), Some("auv.api.driver.macos.v1"));
  let service = file.service.iter().find(|service| service.name.as_deref() == Some("PermissionService")).expect("PermissionService");
  let method = service.method.iter().find(|method| method.name.as_deref() == Some("ProbePermissions")).expect("ProbePermissions");
  assert_eq!(method.input_type.as_deref(), Some(".auv.api.driver.macos.v1.ProbePermissionsRequest"));
  assert_eq!(method.output_type.as_deref(), Some(".auv.api.driver.macos.v1.ProbePermissionsResponse"));

  let request = file.message_type.iter().find(|message| message.name.as_deref() == Some("ProbePermissionsRequest")).expect("request");
  assert!(!request.field.iter().any(|field| field.name.as_deref() == Some("lease")));
  let response = file.message_type.iter().find(|message| message.name.as_deref() == Some("ProbePermissionsResponse")).expect("response");
  assert_eq!(
    response.field.iter().filter_map(|field| field.name.as_deref()).collect::<Vec<_>>(),
    [
      "screen_recording",
      "screen_capture_kit",
      "accessibility",
      "automation_to_system_events"
    ]
  );
  assert!(response.field.iter().all(|field| field.type_name.as_deref() == Some(".auv.api.driver.macos.v1.PermissionStatus")));

  fn assert_client<T>() {}
  assert_client::<PermissionServiceClient<tonic::transport::Channel>>();
}

#[test]
fn macos_application_service_preserves_typed_activation_verification() {
  use crate::auv::api::driver::macos::v1::application_service_client::ApplicationServiceClient;

  let descriptor_set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("decode FILE_DESCRIPTOR_SET");
  let file = descriptor_set
    .file
    .iter()
    .find(|file| file.name.as_deref() == Some("auv/api/driver/macos/v1/application.proto"))
    .expect("macOS application descriptor");
  let service = file.service.iter().find(|service| service.name.as_deref() == Some("ApplicationService")).expect("ApplicationService");
  let method = service.method.iter().find(|method| method.name.as_deref() == Some("ActivateBundleId")).expect("ActivateBundleId");
  assert_eq!(method.input_type.as_deref(), Some(".auv.api.driver.macos.v1.ActivateBundleIdRequest"));
  assert_eq!(method.output_type.as_deref(), Some(".auv.api.driver.macos.v1.ActivateBundleIdResponse"));

  let request = file.message_type.iter().find(|message| message.name.as_deref() == Some("ActivateBundleIdRequest")).expect("request");
  assert!(!request.field.iter().any(|field| field.name.as_deref() == Some("lease")));
  assert_eq!(
    request.field.iter().find(|field| field.name.as_deref() == Some("settle")).and_then(|field| field.type_name.as_deref()),
    Some(".google.protobuf.Duration")
  );
  let verification =
    file.message_type.iter().find(|message| message.name.as_deref() == Some("ApplicationActivationVerification")).expect("verification");
  assert_eq!(verification.oneof_decl.iter().filter_map(|oneof| oneof.name.as_deref()).collect::<Vec<_>>(), ["verification"]);
  assert_eq!(
    verification.field.iter().filter_map(|field| field.name.as_deref()).collect::<Vec<_>>(),
    ["verified_foreground", "foreground_mismatch", "unavailable"]
  );

  fn assert_client<T>() {}
  assert_client::<ApplicationServiceClient<tonic::transport::Channel>>();
}

#[test]
fn macos_accessibility_service_uses_one_typed_focus_selector() {
  use crate::auv::api::driver::macos::v1::accessibility_service_client::AccessibilityServiceClient;

  let descriptor_set = FileDescriptorSet::decode(FILE_DESCRIPTOR_SET).expect("decode FILE_DESCRIPTOR_SET");
  let file = descriptor_set
    .file
    .iter()
    .find(|file| file.name.as_deref() == Some("auv/api/driver/macos/v1/accessibility.proto"))
    .expect("macOS accessibility descriptor");
  let service = file.service.iter().find(|service| service.name.as_deref() == Some("AccessibilityService")).expect("AccessibilityService");
  assert_eq!(service.method.iter().filter_map(|method| method.name.as_deref()).collect::<Vec<_>>(), ["FocusText"]);
  let request = file.message_type.iter().find(|message| message.name.as_deref() == Some("FocusTextRequest")).expect("request");
  assert!(request.oneof_decl.iter().any(|oneof| oneof.name.as_deref() == Some("selector")));
  assert_eq!(
    request.field.iter().filter(|field| field.oneof_index == Some(0)).filter_map(|field| field.name.as_deref()).collect::<Vec<_>>(),
    ["query", "path"]
  );
  let result = file.message_type.iter().find(|message| message.name.as_deref() == Some("AxFocusResult")).expect("result");
  assert_eq!(
    result.field.iter().find(|field| field.name.as_deref() == Some("action")).and_then(|field| field.type_name.as_deref()),
    Some(".auv.api.driver.v1.InputActionResult")
  );

  fn assert_client<T>() {}
  assert_client::<AccessibilityServiceClient<tonic::transport::Channel>>();
}
