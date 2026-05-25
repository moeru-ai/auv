const MACOS_OVERLAY_FFI_RS: &str = "src/native/binding.rs";
const MACOS_OVERLAY_SWIFT_PACKAGE: &str = "native/swift/Package.swift";
const MACOS_OVERLAY_SWIFT_TARGET_DIR: &str = "native/swift/Sources/AuvMacosOverlayNative";
const MACOS_OVERLAY_SWIFT_MODULE: &str = "AuvMacosOverlayNative";

fn main() {
  let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
  if target_os != "macos" {
    return;
  }

  build_macos_overlay_native();
}

#[cfg(target_os = "macos")]
fn build_macos_overlay_native() {
  use std::env;
  use std::fs;
  use std::path::PathBuf;
  use std::process::Command;

  println!("cargo:rerun-if-changed={MACOS_OVERLAY_FFI_RS}");
  println!("cargo:rerun-if-changed={MACOS_OVERLAY_SWIFT_PACKAGE}");
  println!("cargo:rerun-if-changed={MACOS_OVERLAY_SWIFT_TARGET_DIR}");

  let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
  let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
  let generated_dir = out_dir.join("generated");
  let crate_bridge_dir = generated_dir.join("auv_overlay_macos");
  let bridge_header = out_dir.join("native-bridging-header.h");
  let swift_lib = out_dir.join(format!("lib{MACOS_OVERLAY_SWIFT_MODULE}.a"));
  let swift_target_dir = manifest_dir.join(MACOS_OVERLAY_SWIFT_TARGET_DIR);
  let mut swift_sources = fs::read_dir(&swift_target_dir)
    .expect("read AuvMacosOverlayNative Swift sources")
    .map(|entry| {
      entry
        .expect("read AuvMacosOverlayNative Swift source entry")
        .path()
    })
    .filter(|path| {
      path
        .extension()
        .is_some_and(|extension| extension == "swift")
    })
    .collect::<Vec<_>>();
  swift_sources.sort();
  for source in &swift_sources {
    println!("cargo:rerun-if-changed={}", source.display());
  }

  swift_bridge_build::parse_bridges(vec![manifest_dir.join(MACOS_OVERLAY_FFI_RS)])
    .write_all_concatenated(&generated_dir, "auv_overlay_macos");

  fs::write(
    &bridge_header,
    format!(
      "#include \"{}\"\n#include \"{}\"\n",
      generated_dir.join("SwiftBridgeCore.h").display(),
      crate_bridge_dir.join("auv_overlay_macos.h").display()
    ),
  )
  .expect("write Swift bridge header");

  let mut command = Command::new("swiftc");
  command
    .arg("-emit-library")
    .arg("-static")
    .arg("-parse-as-library")
    .arg("-module-name")
    .arg(MACOS_OVERLAY_SWIFT_MODULE)
    .arg("-import-objc-header")
    .arg(&bridge_header)
    .arg(generated_dir.join("SwiftBridgeCore.swift"));
  for source in &swift_sources {
    command.arg(source);
  }
  let status = command
    .arg(crate_bridge_dir.join("auv_overlay_macos.swift"))
    .arg("-o")
    .arg(&swift_lib)
    .status()
    .expect("spawn swiftc");

  if !status.success() {
    panic!("swiftc failed with status {status}");
  }

  println!("cargo:rustc-link-search=native={}", out_dir.display());
  println!("cargo:rustc-link-lib=static={MACOS_OVERLAY_SWIFT_MODULE}");
  println!("cargo:rustc-link-lib=dylib=swiftCore");
}

#[cfg(not(target_os = "macos"))]
fn build_macos_overlay_native() {
  panic!("building the macOS overlay Swift bridge requires a macOS host with swiftc available");
}
