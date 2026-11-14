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

  embed_windows_manifest();
}

/// Links `auv.exe.manifest` into the `auv` binary: per-monitor-v2 DPI awareness from process
/// start, `asInvoker`. See NOTICE(windows-dpi-awareness) in `auv_driver_windows::dpi`.
fn embed_windows_manifest() {
  println!("cargo:rerun-if-changed=auv.exe.manifest");
  if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
    return;
  }
  // NOTICE: `winresource` is a build dependency only on Windows hosts (the release and check
  // jobs build Windows natively), so a cross build from another host links no manifest and `auv`
  // then warns at runtime that it is not per-monitor DPI aware.
  #[cfg(windows)]
  winresource::WindowsResource::new()
    .set_manifest_file("auv.exe.manifest")
    .compile_for("auv")
    .expect("embed the auv Windows application manifest");
  #[cfg(not(windows))]
  println!("cargo:warning=auv.exe is cross-built without its DPI awareness manifest");
}
