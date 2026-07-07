// Builds the vendored mediaremote-adapter framework from source (the git
// submodule under vendor/) via cmake, then tars the built framework into
// OUT_DIR so the crate can `include_bytes!` it. macOS-only: off-macOS the
// crate compiles without the embedded asset and `now_playing()` returns
// `Unsupported`.

use std::env;
use std::path::PathBuf;
use std::process::Command;

const SUBMODULE: &str = "vendor/mediaremote-adapter";
const FRAMEWORK: &str = "MediaRemoteAdapter.framework";

fn main() {
  let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
  if target_os != "macos" {
    return;
  }
  build_macos();
}

fn build_macos() {
  let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
  let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));
  let submodule = manifest_dir.join(SUBMODULE);

  if !submodule.join("CMakeLists.txt").exists() {
    panic!(
      "mediaremote-adapter submodule is not initialized at {} — run: \
       git submodule update --init --recursive",
      submodule.display()
    );
  }

  println!("cargo:rerun-if-changed={}", submodule.join("CMakeLists.txt").display());
  println!("cargo:rerun-if-changed={}", submodule.join("src").display());
  println!("cargo:rerun-if-changed={}", submodule.join("bin/mediaremote-adapter.pl").display());

  let build_dir = out_dir.join("mra-build");
  std::fs::create_dir_all(&build_dir).expect("create cmake build dir");

  run(Command::new("cmake").arg("-S").arg(&submodule).arg("-B").arg(&build_dir), "cmake configure");
  run(Command::new("cmake").arg("--build").arg(&build_dir), "cmake build");

  let framework = build_dir.join(FRAMEWORK);
  assert!(framework.exists(), "framework not built at {}", framework.display());

  // Tar (preserving the bundle's symlinks) into OUT_DIR for include_bytes!.
  let tar_path = out_dir.join("mediaremote-adapter.tar");
  run(Command::new("tar").arg("-C").arg(&build_dir).arg("-cf").arg(&tar_path).arg(FRAMEWORK), "tar framework");
}

fn run(command: &mut Command, label: &str) {
  let status = command.status().unwrap_or_else(|error| panic!("spawn {label}: {error}"));
  assert!(status.success(), "{label} failed with {status}");
}
