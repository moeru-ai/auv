// File: src/xtask.rs
use std::path::Path;

pub(crate) fn generate_swift_bridge_for_ide(project_root: &Path) -> Result<Vec<String>, String> {
  generate_swift_bridge_for_ide_impl(project_root)
}

#[cfg(target_os = "macos")]
fn generate_swift_bridge_for_ide_impl(project_root: &Path) -> Result<Vec<String>, String> {
  let driver_generated_dir = generate_one_swift_bridge_for_ide(
    project_root,
    "crates/auv-driver-macos/src/native/binding.rs",
    "crates/auv-driver-macos/native/swift/Sources/AuvMacosNative/Generated",
    "auv_driver_macos",
  )?;
  let overlay_generated_dir = generate_one_swift_bridge_for_ide(
    project_root,
    "crates/auv-overlay-macos/src/native/binding.rs",
    "crates/auv-overlay-macos/native/swift/Sources/AuvMacosOverlayNative/Generated",
    "auv_overlay_macos",
  )?;

  Ok(vec![driver_generated_dir, overlay_generated_dir])
}

#[cfg(target_os = "macos")]
fn generate_one_swift_bridge_for_ide(
  project_root: &Path,
  ffi_rs: &str,
  generated_dir: &str,
  crate_name: &str,
) -> Result<String, String> {
  let ffi_rs = project_root.join(ffi_rs);
  let generated_dir = project_root.join(generated_dir);
  std::fs::create_dir_all(&generated_dir).map_err(|error| {
    format!(
      "failed to create Swift bridge generated directory {}: {error}",
      generated_dir.display()
    )
  })?;

  swift_bridge_build::parse_bridges(vec![ffi_rs])
    .write_all_concatenated(&generated_dir, crate_name);
  std::fs::write(
    generated_dir.join("native-bridging-header.h"),
    format!("#include \"SwiftBridgeCore.h\"\n#include \"{crate_name}/{crate_name}.h\"\n"),
  )
  .map_err(|error| {
    format!(
      "failed to write Swift bridge IDE header {}: {error}",
      generated_dir.join("native-bridging-header.h").display()
    )
  })?;

  Ok(generated_dir.display().to_string())
}

#[cfg(not(target_os = "macos"))]
fn generate_swift_bridge_for_ide_impl(_project_root: &Path) -> Result<Vec<String>, String> {
  Err("generating the macOS Swift bridge for IDE indexing requires macOS".to_string())
}
