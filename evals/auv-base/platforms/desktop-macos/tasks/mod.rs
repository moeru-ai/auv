//! macOS evaluation options, native fixture setup, and case definitions.

use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};
use clap::Args;
use serde_json::Value;
use tokio::time::Instant;

use crate::support::{self, output, path, pause, read_json};

#[path = "../cases/focus_without_raise.rs"]
pub mod focus_cases;

#[path = "../cases/keyboard.rs"]
pub mod keyboard_cases;

#[cfg(test)]
#[path = "../cases/replay.rs"]
mod replay;

pub const ABC: &str = "com.apple.keylayout.ABC";

#[derive(Args)]
pub struct DesktopOptions {
  /// Fresh local output directory (use evals/auv-base/results/ or a temporary path).
  #[arg(long)]
  pub output: PathBuf,

  /// Installed Electron executable. TypeScript receivers must be built separately.
  #[arg(long)]
  pub electron: PathBuf,

  #[arg(
    long,
    default_value = "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
  )]
  pub chrome: PathBuf,

  #[arg(long, default_value_os_t = support::binary("auv"))]
  pub auv: PathBuf,
}

pub fn root() -> PathBuf {
  support::root().join("platforms/desktop-macos")
}

pub async fn compile(source: &Path, destination: &Path) -> Result<()> {
  output(Path::new("swiftc"), &[path(source), "-o".into(), path(destination)]).await?;

  Ok(())
}

/// Build only fixture-owned app bundles; separate names give anchor and target independent focus.
pub fn build_app(binary: &Path, directory: &Path, name: &str) -> Result<PathBuf> {
  let contents = directory.join(format!("{name}.app/Contents"));
  std::fs::create_dir_all(contents.join("MacOS"))?;

  let executable = contents.join("MacOS").join(name);
  std::fs::copy(binary, &executable)?;

  std::fs::write(
    contents.join("Info.plist"),
    format!(
      r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>{name}</string><key>CFBundleIdentifier</key><string>ai.auv.test.{name}</string>
<key>CFBundleName</key><string>{name}</string><key>CFBundlePackageType</key><string>APPL</string></dict></plist>"#
    ),
  )?;

  Ok(executable)
}

pub async fn wait_json(file: &Path, predicate: impl Fn(&Value) -> bool) -> Result<Value> {
  let end = Instant::now() + std::time::Duration::from_secs(15);

  loop {
    let value = read_json(file)?;
    if predicate(&value) {
      return Ok(value);
    }

    ensure!(Instant::now() < end, "timed out observing {}: {value}", file.display());
    pause(30).await?;
  }
}
