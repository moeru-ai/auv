// File: src/xtask.rs
use std::path::{Path, PathBuf};

pub(crate) fn generate_swift_bridge_for_ide(project_root: &Path) -> Result<PathBuf, String> {
  generate_swift_bridge_for_ide_impl(project_root)
}

#[cfg(target_os = "macos")]
fn generate_swift_bridge_for_ide_impl(project_root: &Path) -> Result<PathBuf, String> {
  let ffi_rs = project_root.join("crates/auv-driver-macos/src/native/binding.rs");
  let generated_dir =
    project_root.join("crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Generated");
  std::fs::create_dir_all(&generated_dir).map_err(|error| {
    format!(
      "failed to create Swift bridge generated directory {}: {error}",
      generated_dir.display()
    )
  })?;

  swift_bridge_build::parse_bridges(vec![ffi_rs])
    .write_all_concatenated(&generated_dir, "auv_driver_macos");
  std::fs::write(
    generated_dir.join("native-bridging-header.h"),
    "#include \"SwiftBridgeCore.h\"\n#include \"auv_driver_macos/auv_driver_macos.h\"\n",
  )
  .map_err(|error| {
    format!(
      "failed to write Swift bridge IDE header {}: {error}",
      generated_dir.join("native-bridging-header.h").display()
    )
  })?;

  Ok(generated_dir)
}

#[cfg(not(target_os = "macos"))]
fn generate_swift_bridge_for_ide_impl(_project_root: &Path) -> Result<PathBuf, String> {
  Err("generating the macOS Swift bridge for IDE indexing requires macOS".to_string())
}
