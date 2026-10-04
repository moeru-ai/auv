use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
  println!("cargo:rerun-if-env-changed=AUV_WINDOWS_HELPER_EXE_PATH");

  let output = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR")).join("embedded_windows_helper.rs");
  let source = match (env::var_os("CARGO_CFG_TARGET_OS").as_deref(), env::var_os("AUV_WINDOWS_HELPER_EXE_PATH")) {
    (Some(target), Some(path)) if target == "windows" => {
      let path = PathBuf::from(path);
      println!("cargo:rerun-if-changed={}", path.display());
      let payload = PathBuf::from(env::var_os("OUT_DIR").expect("Cargo must provide OUT_DIR")).join("auv-helper.exe");
      fs::copy(&path, &payload).unwrap_or_else(|error| panic!("failed to stage {}: {error}", path.display()));
      "pub(super) const EXECUTABLE: Option<&[u8]> = Some(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/auv-helper.exe\")));\n"
    }
    _ => "pub(super) const EXECUTABLE: Option<&[u8]> = None;\n",
  };

  fs::write(output, source).expect("write embedded Windows helper module");
}
