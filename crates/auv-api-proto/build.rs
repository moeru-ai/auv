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
  tonic_prost_build::configure().file_descriptor_set_path(out_dir.join("auv.api.bin")).compile_protos(
    &[
      "../../proto/auv/api/daemon/v1/discovery.proto",
      "../../proto/auv/api/daemon/v1/device.proto",
      "../../proto/auv/api/daemon/v1/pairing.proto",
      "../../proto/auv/api/annotations/v1/annotations.proto",
      "../../proto/auv/api/daemon/v1/run.proto",
      "../../proto/auv/api/daemon/v1/runner.proto",
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
      "../../proto/auv/api/inference/v1/object_detection.proto",
      "../../proto/auv/api/image/v1/image.proto",
      "../../proto/auv/api/image/v1/region.proto",
    ],
    &["../../proto"],
  )?;

  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/discovery.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/device.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/pairing.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/annotations/v1/annotations.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/run.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/daemon/v1/runner.proto");
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
  println!("cargo:rerun-if-changed=../../proto/auv/api/inference/v1/object_detection.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/image/v1/image.proto");
  println!("cargo:rerun-if-changed=../../proto/auv/api/image/v1/region.proto");
  Ok(())
}
