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
  tonic_prost_build::configure()
    .file_descriptor_set_path(out_dir.join("auv.api.v1.session.bin"))
    .compile_protos(&["../../proto/auv/api/v1/session.proto"], &["../../proto"])?;

  println!("cargo:rerun-if-changed=../../proto/auv/api/v1/session.proto");
  Ok(())
}
