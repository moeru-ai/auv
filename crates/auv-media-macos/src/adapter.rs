//! Runtime bridge to the vendored mediaremote-adapter.
//!
//! The adapter framework (built from source by `build.rs`) and its perl driver
//! are embedded into this binary. On first use they are unpacked to a
//! content-keyed cache directory, then the now-playing read runs as
//! `/usr/bin/perl <script> <framework> get`. perl is an Apple platform binary
//! (`com.apple.perl`), which is what lets it read MediaRemote on macOS 15.4+
//! where an ad-hoc-signed binary cannot. See
//! `docs/ai/references/2026-06-04-media-macos-now-playing-design.md`.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::process::Command;

use crate::error::MediaError;

const FRAMEWORK_TAR: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/mediaremote-adapter.tar"));
const ADAPTER_PL: &str = include_str!(concat!(
  env!("CARGO_MANIFEST_DIR"),
  "/vendor/mediaremote-adapter/bin/mediaremote-adapter.pl"
));

const PERL: &str = "/usr/bin/perl";
const SCRIPT_NAME: &str = "mediaremote-adapter.pl";
const FRAMEWORK_NAME: &str = "MediaRemoteAdapter.framework";

fn cache_root() -> PathBuf {
  let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
  PathBuf::from(home).join("Library/Caches/auv/mediaremote-adapter")
}

/// Content key over the embedded assets, so a rebuilt adapter unpacks fresh.
fn asset_key() -> String {
  let mut hasher = DefaultHasher::new();
  FRAMEWORK_TAR.hash(&mut hasher);
  ADAPTER_PL.hash(&mut hasher);
  format!("{:016x}", hasher.finish())
}

/// Ensure the embedded adapter is unpacked; return (script_path, framework_path).
fn ensure_unpacked() -> Result<(PathBuf, PathBuf), MediaError> {
  let dir = cache_root().join(asset_key());
  let script = dir.join(SCRIPT_NAME);
  let framework = dir.join(FRAMEWORK_NAME);
  if script.is_file() && framework.is_dir() {
    return Ok((script, framework));
  }

  // Unpack into a private temp dir, then atomically rename into place so
  // concurrent readers never see a half-written cache.
  let staging = cache_root().join(format!("{}.tmp.{}", asset_key(), std::process::id()));
  let _ = std::fs::remove_dir_all(&staging);
  fs_err(
    std::fs::create_dir_all(&staging),
    "create adapter cache dir",
  )?;

  let tar_path = staging.join("framework.tar");
  fs_err(
    std::fs::write(&tar_path, FRAMEWORK_TAR),
    "write framework tar",
  )?;
  let status = Command::new("/usr/bin/tar")
    .arg("-xf")
    .arg(&tar_path)
    .arg("-C")
    .arg(&staging)
    .status()
    .map_err(|error| MediaError::native(format!("spawn tar: {error}"), None))?;
  if !status.success() {
    return Err(MediaError::native(
      format!("tar extraction failed with {status}"),
      None,
    ));
  }
  let _ = std::fs::remove_file(&tar_path);
  fs_err(
    std::fs::write(staging.join(SCRIPT_NAME), ADAPTER_PL),
    "write adapter script",
  )?;

  fs_err(
    std::fs::create_dir_all(cache_root()),
    "create adapter cache root",
  )?;
  if std::fs::rename(&staging, &dir).is_err() {
    // A concurrent process likely won the race; accept its result if valid.
    let _ = std::fs::remove_dir_all(&staging);
    if !(script.is_file() && framework.is_dir()) {
      return Err(MediaError::native(
        "failed to install adapter cache".to_string(),
        None,
      ));
    }
  }
  Ok((script, framework))
}

/// Run the adapter with `args` (after the script + framework path), returning
/// its trimmed stdout. Shared by `get`, `send`, and `seek`.
fn run_adapter(args: &[&str]) -> Result<String, MediaError> {
  let (script, framework) = ensure_unpacked()?;
  let output = Command::new(PERL)
    .arg(&script)
    .arg(&framework)
    .args(args)
    .output()
    .map_err(|error| {
      MediaError::native(
        format!("spawn {PERL}: {error}"),
        Some("ensure /usr/bin/perl exists (it ships with macOS)".to_string()),
      )
    })?;
  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    return Err(MediaError::native(
      format!(
        "mediaremote-adapter exited with {}: {stderr}",
        output.status
      ),
      Some("the adapter may not be entitled to use MediaRemote on this macOS".to_string()),
    ));
  }
  Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// Run the adapter `get` command, returning its raw JSON stdout (`null` when
/// nothing is playing).
pub(crate) fn run_now_playing_get() -> Result<String, MediaError> {
  run_adapter(&["get"])
}

/// Send a MediaRemote command by its numeric MRCommand id.
pub(crate) fn send_command(command_id: u8) -> Result<(), MediaError> {
  run_adapter(&["send", &command_id.to_string()]).map(|_| ())
}

/// Seek the now-playing app to `position_micros` microseconds.
pub(crate) fn seek(position_micros: u128) -> Result<(), MediaError> {
  run_adapter(&["seek", &position_micros.to_string()]).map(|_| ())
}

fn fs_err<T>(result: std::io::Result<T>, what: &str) -> Result<T, MediaError> {
  result.map_err(|error| MediaError::native(format!("{what}: {error}"), None))
}
