use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let protoc = protoc_bin_vendored::protoc_bin_path()?;
  // NOTICE(proto-build): Cargo builds use a vendored `protoc` so this crate can
  // compile outside the Nix dev shell; `nix develop` still provides `protobuf`
  // and `buf` for explicit schema work.
  unsafe {
    std::env::set_var("PROTOC", protoc);
  }

  let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
  let mut builder = tonic_prost_build::configure().file_descriptor_set_path(out_dir.join("auv.api.bin")).message_attribute(
    ".auv.api.daemon.v1",
    "#[derive(serde::Serialize, serde::Deserialize)] #[serde(default, rename_all = \"camelCase\", deny_unknown_fields)]",
  );
  // TODO(protojson-default-omission): non-Pairing daemon messages currently
  // emit some empty scalar/container fields through serde. Add descriptor-
  // driven skip attributes when the JSON contract requires byte-for-byte
  // canonical default omission; enum, uint64, Duration, and Timestamp wire
  // shapes are already explicit below.
  builder = builder
    .field_attribute(
      ".auv.api.daemon.v1.CreatePairingTokenRequest.ttl",
      "#[serde(default, skip_serializing_if = \"Option::is_none\", with = \"tonic_rest::serde::opt_duration\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.CreatePairingTokenResponse.expires_at",
      "#[serde(default, alias = \"expires_at\", skip_serializing_if = \"Option::is_none\", with = \"tonic_rest::serde::opt_timestamp\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.Run.created_at",
      "#[serde(default, skip_serializing_if = \"Option::is_none\", with = \"tonic_rest::serde::opt_timestamp\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.Run.completed_at",
      "#[serde(default, skip_serializing_if = \"Option::is_none\", with = \"tonic_rest::serde::opt_timestamp\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.Runner.idle_timeout",
      "#[serde(default, skip_serializing_if = \"Option::is_none\", with = \"tonic_rest::serde::opt_duration\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.Runner.created_at",
      "#[serde(default, skip_serializing_if = \"Option::is_none\", with = \"tonic_rest::serde::opt_timestamp\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.Runner.idle_deadline",
      "#[serde(default, skip_serializing_if = \"Option::is_none\", with = \"tonic_rest::serde::opt_timestamp\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.CreateRunnerRequest.idle_timeout",
      "#[serde(default, skip_serializing_if = \"Option::is_none\", with = \"tonic_rest::serde::opt_duration\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.DeleteRunnerRequest.grace_period",
      "#[serde(default, skip_serializing_if = \"Option::is_none\", with = \"tonic_rest::serde::opt_duration\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.ApiResource.operations",
      "#[serde(with = \"crate::json::api_resource_operation::repeated\", skip_serializing_if = \"Vec::is_empty\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.Device.platform",
      "#[serde(with = \"crate::json::device_platform\", skip_serializing_if = \"crate::json::is_zero_i32\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.Run.phase",
      "#[serde(with = \"crate::json::run_phase\", skip_serializing_if = \"crate::json::is_zero_i32\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.StopRunRequest.outcome",
      "#[serde(with = \"crate::json::run_outcome\", skip_serializing_if = \"crate::json::is_zero_i32\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.RunnerClass.supported_lifecycles",
      "#[serde(with = \"crate::json::runner_lifecycle::repeated\", skip_serializing_if = \"Vec::is_empty\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.Runner.lifecycle",
      "#[serde(with = \"crate::json::runner_lifecycle\", skip_serializing_if = \"crate::json::is_zero_i32\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.Runner.phase",
      "#[serde(with = \"crate::json::runner_phase\", skip_serializing_if = \"crate::json::is_zero_i32\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.CreateRunnerRequest.lifecycle",
      "#[serde(with = \"crate::json::runner_lifecycle\", skip_serializing_if = \"crate::json::is_zero_i32\")]",
    )
    .field_attribute(
      ".auv.api.daemon.v1.Runner.active_operations",
      "#[serde(with = \"crate::json::u64_string\", skip_serializing_if = \"crate::json::is_zero_u64\")]",
    );
  for field in [
    ".auv.api.daemon.v1.CreatePairingTokenResponse.token",
    ".auv.api.daemon.v1.PairDeviceRequest.token",
    ".auv.api.daemon.v1.PairDeviceRequest.label",
  ] {
    builder = builder.field_attribute(field, "#[serde(skip_serializing_if = \"String::is_empty\")]");
  }
  for field in [
    ".auv.api.daemon.v1.PairDeviceRequest.device_id",
    ".auv.api.daemon.v1.PairDeviceResponse.device_id",
    ".auv.api.daemon.v1.PairDeviceResponse.device_credential",
    ".auv.api.daemon.v1.RevokeDeviceCredentialRequest.device_id",
    ".auv.api.daemon.v1.SetPairedDeviceEnabledRequest.device_selector",
    ".auv.api.daemon.v1.UnpairDeviceRequest.device_selector",
  ] {
    let alias = field.rsplit_once('.').expect("field path has a field name").1;
    builder = builder.field_attribute(field, format!("#[serde(alias = \"{alias}\", skip_serializing_if = \"String::is_empty\")]"));
  }
  for field in [
    ".auv.api.daemon.v1.RevokeDeviceCredentialResponse.revoked",
    ".auv.api.daemon.v1.SetPairedDeviceEnabledRequest.enabled",
    ".auv.api.daemon.v1.SetPairedDeviceEnabledResponse.changed",
    ".auv.api.daemon.v1.UnpairDeviceResponse.removed",
  ] {
    builder = builder.field_attribute(field, "#[serde(skip_serializing_if = \"crate::is_false\")]");
  }
  builder.compile_protos(
    &[
      "../../proto/auv/api/daemon/v1/health.proto",
      "../../proto/auv/api/daemon/v1/discovery.proto",
      "../../proto/auv/api/daemon/v1/device.proto",
      "../../proto/auv/api/daemon/v1/pairing.proto",
      "../../proto/auv/api/annotations/v1/annotations.proto",
      "../../proto/auv/api/daemon/v1/run.proto",
      "../../proto/auv/api/daemon/v1/runner.proto",
      "../../proto/auv/api/transport/websocket/v1/websocket.proto",
      "../../proto/auv/api/driver/v1/capture.proto",
      "../../proto/auv/api/driver/v1/display.proto",
      "../../proto/auv/api/driver/v1/window.proto",
      "../../proto/auv/api/driver/v1/geometry.proto",
      "../../proto/auv/api/driver/v1/input.proto",
      "../../proto/auv/api/driver/v1/overlay.proto",
      "../../proto/auv/api/driver/v1/text_recognition.proto",
      "../../proto/auv/api/driver/macos/v1/permission.proto",
      "../../proto/auv/api/driver/macos/v1/accessibility.proto",
      "../../proto/auv/api/driver/macos/v1/application.proto",
      "../../proto/auv/api/driver/macos/v1/media_control.proto",
      "../../proto/auv/api/image/v1/image.proto",
      "../../proto/auv/api/image/v1/region.proto",
    ],
    &["../../proto", "../../proto/vendor"],
  )?;

  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/health.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/discovery.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/device.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/pairing.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/annotations/v1/annotations.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/run.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/runner.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/transport/websocket/v1/websocket.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/v1/capture.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/v1/display.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/v1/window.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/v1/geometry.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/v1/input.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/v1/overlay.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/v1/text_recognition.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/macos/v1/permission.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/macos/v1/accessibility.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/macos/v1/application.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/driver/macos/v1/media_control.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/image/v1/image.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/image/v1/region.proto");
  println!("cargo:rerun-if-changed=../../proto/vendor/google/api/annotations.proto");
  println!("cargo:rerun-if-changed=../../proto/vendor/google/api/http.proto");
  Ok(())
}
