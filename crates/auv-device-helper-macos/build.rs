use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
  println!("cargo:rerun-if-env-changed=AUV_MACOS_HELPER_APP_ARCHIVE_PATH");

  let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR")).join("embedded_macos_helper.rs");
  let source = match env::var_os("AUV_MACOS_HELPER_APP_ARCHIVE_PATH") {
    Some(path) if env::var_os("CARGO_FEATURE_SETUP").is_some() => {
      let path = PathBuf::from(path);
      println!("cargo:rerun-if-changed={}", path.display());
      let payload = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR")).join("AUV Helper.zip");
      fs::copy(&path, &payload).unwrap_or_else(|error| panic!("failed to stage {}: {error}", path.display()));
      "pub(super) const ARCHIVE: Option<&[u8]> = Some(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/AUV Helper.zip\")));\n"
    }
    _ => "pub(super) const ARCHIVE: Option<&[u8]> = None;\n",
  };

  fs::write(output, source).expect("write embedded macOS helper module");
}
