use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let protoc = protoc_bin_vendored::protoc_bin_path()?;
  // SAFETY: Cargo executes this build script serially for the package before
  // prost reads PROTOC; no application threads exist in the build process.
  unsafe {
    std::env::set_var("PROTOC", protoc);
  }
  let out_dir = PathBuf::from(std::env::var("OUT_DIR")?);
  tonic_prost_build::configure()
    .extern_path(".auv.api.image.v1", "::auv_api_proto::auv::api::image::v1")
    .file_descriptor_set_path(out_dir.join("auv.game.balatro.api.bin"))
    .compile_protos(&["proto/auv/game/balatro/v1/object_detection.proto"], &["proto", "../../../proto"])?;
  println!("cargo:rerun-if-changed=proto/auv/game/balatro/v1/object_detection.proto");
  println!("cargo:rerun-if-changed=../../../proto/auv/api/annotations/v1/annotations.proto");
  println!("cargo:rerun-if-changed=../../../proto/auv/api/image/v1/image.proto");
  Ok(())
}
