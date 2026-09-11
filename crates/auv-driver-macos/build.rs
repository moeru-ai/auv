const MACOS_NATIVE_FFI_RS: &str = "src/native/binding.rs";
const MACOS_NATIVE_SWIFT_PACKAGE: &str = "native/swift/Package.swift";
const MACOS_NATIVE_SWIFT_TARGET_DIR: &str = "native/swift/Sources/AuvMacosNative";
const MACOS_NATIVE_SWIFT_MODULE: &str = "AuvMacosNative";

fn main() {
  let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
  if target_os != "macos" {
    return;
  }

  build_macos_native();
}

#[cfg(target_os = "macos")]
fn emit_swift_link_search_paths() {
  use std::env;
  use std::path::PathBuf;
  use std::process::Command;

  // NOTICE(swift-runtime-link-search): Nix's clang wrapper does not add the
  // Swift runtime or SDK Swift module directory to the final link search path.
  // Derive both from the selected swiftc toolchain; remove this workaround
  // when the toolchain wrapper propagates them automatically.
  let output = Command::new("swiftc").arg("-print-target-info").output().expect("query swiftc target info");
  if !output.status.success() {
    panic!("swiftc -print-target-info failed with status {}", output.status);
  }
  let target_info: serde_json::Value = serde_json::from_slice(&output.stdout).expect("parse swiftc target info");
  let mut search_paths = Vec::<PathBuf>::new();
  if let Some(paths) = target_info["paths"]["runtimeLibraryPaths"].as_array() {
    for path in paths.iter().filter_map(serde_json::Value::as_str).map(PathBuf::from) {
      if path.is_dir() && !search_paths.contains(&path) {
        search_paths.push(path);
      }
    }
  }
  if let Some(sdk_root) = env::var_os("SDKROOT") {
    let path = PathBuf::from(sdk_root).join("usr/lib/swift");
    if path.is_dir() && !search_paths.contains(&path) {
      search_paths.push(path);
    }
  }
  for path in search_paths {
    println!("cargo:rustc-link-search=native={}", path.display());
  }
}

#[cfg(target_os = "macos")]
fn configure_swift_target(command: &mut std::process::Command) {
  let architecture = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
    Ok("aarch64") => "arm64",
    Ok("x86_64") => "x86_64",
    _ => return,
  };
  let Ok(deployment_target) = std::env::var("MACOSX_DEPLOYMENT_TARGET") else {
    return;
  };
  command.arg("-target").arg(format!("{architecture}-apple-macosx{deployment_target}"));
}

#[cfg(target_os = "macos")]
fn build_macos_native() {
  use std::env;
  use std::fs;
  use std::path::PathBuf;
  use std::process::Command;

  println!("cargo:rerun-if-changed={MACOS_NATIVE_FFI_RS}");
  println!("cargo:rerun-if-changed={MACOS_NATIVE_SWIFT_PACKAGE}");
  println!("cargo:rerun-if-changed={MACOS_NATIVE_SWIFT_TARGET_DIR}");
  println!("cargo:rerun-if-env-changed=MACOSX_DEPLOYMENT_TARGET");
  println!("cargo:rerun-if-env-changed=SDKROOT");

  let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
  let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
  let generated_dir = out_dir.join("generated");
  let crate_bridge_dir = generated_dir.join("auv_driver_macos");
  let bridge_header = out_dir.join("native-bridging-header.h");
  let swift_lib = out_dir.join(format!("lib{MACOS_NATIVE_SWIFT_MODULE}.a"));
  let swift_target_dir = manifest_dir.join(MACOS_NATIVE_SWIFT_TARGET_DIR);
  let mut swift_sources = fs::read_dir(&swift_target_dir)
    .expect("read AuvMacosNative Swift sources")
    .map(|entry| entry.expect("read AuvMacosNative Swift source entry").path())
    .filter(|path| path.extension().is_some_and(|extension| extension == "swift"))
    .collect::<Vec<_>>();
  swift_sources.sort();
  for source in &swift_sources {
    println!("cargo:rerun-if-changed={}", source.display());
  }

  swift_bridge_build::parse_bridges(vec![manifest_dir.join(MACOS_NATIVE_FFI_RS)]).write_all_concatenated(&generated_dir, "auv_driver_macos");

  fs::write(
    &bridge_header,
    format!(
      "#include \"{}\"\n#include \"{}\"\n",
      generated_dir.join("SwiftBridgeCore.h").display(),
      crate_bridge_dir.join("auv_driver_macos.h").display()
    ),
  )
  .expect("write Swift bridge header");

  let mut command = Command::new("swiftc");
  configure_swift_target(&mut command);
  command
    .arg("-emit-library")
    .arg("-static")
    .arg("-parse-as-library")
    .arg("-module-name")
    .arg(MACOS_NATIVE_SWIFT_MODULE)
    .arg("-import-objc-header")
    .arg(&bridge_header)
    .arg(generated_dir.join("SwiftBridgeCore.swift"));
  for source in &swift_sources {
    command.arg(source);
  }
  let status = command.arg(crate_bridge_dir.join("auv_driver_macos.swift")).arg("-o").arg(&swift_lib).status().expect("spawn swiftc");

  if !status.success() {
    panic!("swiftc failed with status {status}");
  }

  println!("cargo:rustc-link-search=native={}", out_dir.display());
  emit_swift_link_search_paths();
  println!("cargo:rustc-link-lib=static={MACOS_NATIVE_SWIFT_MODULE}");
  println!("cargo:rustc-link-lib=dylib=swiftCore");
}

#[cfg(not(target_os = "macos"))]
fn build_macos_native() {
  panic!("building the macOS native Swift bridge requires a macOS host with swiftc available");
}
