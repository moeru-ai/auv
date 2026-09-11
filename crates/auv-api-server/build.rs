use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
  let config = tonic_rest_build::RestCodegenConfig::new()
    .package("auv.api.daemon.v1", "auv::api::daemon::v1")
    .proto_root("auv_api_proto")
    .runtime_crate("crate::rest")
    .extension_type("crate::control::CallerId")
    .public_methods(&["Check", "PairDevice"]);
  let generated = tonic_rest_build::generate(auv_api_proto::FILE_DESCRIPTOR_SET, &config)?;
  std::fs::write(PathBuf::from(std::env::var("OUT_DIR")?).join("daemon_rest.rs"), generated)?;
  Ok(())
}
