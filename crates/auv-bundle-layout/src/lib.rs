//! File layout of the self-contained Linux release bundle.
//!
//! `auv-release` builds and verifies the bundle from these paths, and
//! `auv-driver-linux` finds its private PipeWire client files and Tesseract
//! models through them at runtime. Keeping one definition means the archive
//! cannot drift from what the executable looks for.
//!
//! Directories are path segments relative to the directory that holds the
//! executable. Join them with [`path`] for the file system, or with
//! [`relative`] for the forward-slash paths recorded in the manifest.

use std::path::{Path, PathBuf};

/// The executable, at the bundle root.
pub const EXECUTABLE: &str = "auv";
/// Release manifest at the bundle root: files, hashes and their packages.
pub const MANIFEST: &str = "manifest.json";

/// Private shared libraries the executable links.
pub const LIBRARY_DIR: &[&str] = &["lib", "auv"];
/// PipeWire client modules that `client.conf` loads.
pub const PIPEWIRE_MODULE_DIR: &[&str] = &["lib", "auv", "pipewire-0.3"];
/// SPA plugins that `client.conf` loads, one directory per plugin family.
pub const SPA_PLUGIN_DIR: &[&str] = &["lib", "auv", "spa-0.2"];
/// PipeWire client configuration directory.
pub const PIPEWIRE_CONFIG_DIR: &[&str] = &["share", "auv", "pipewire"];
/// Tesseract models.
pub const TESSDATA_DIR: &[&str] = &["share", "auv", "tessdata"];
/// The AUV license.
pub const PROJECT_LICENSE_DIR: &[&str] = &["licenses", "auv"];
/// Debian copyright files of every bundled system package.
pub const SYSTEM_LICENSE_DIR: &[&str] = &["licenses", "system"];

/// Client configuration file inside [`PIPEWIRE_CONFIG_DIR`].
pub const PIPEWIRE_CLIENT_CONF: &str = "client.conf";
/// The module a PipeWire client needs to reach the daemon socket. A bundle
/// without it cannot start a context.
pub const PIPEWIRE_NATIVE_PROTOCOL_MODULE: &str = "libpipewire-module-protocol-native.so";

/// Joins `segments` onto `root` with the platform separator.
pub fn path(root: &Path, segments: &[&str]) -> PathBuf {
  segments.iter().fold(root.to_path_buf(), |path, segment| path.join(segment))
}

/// Joins `segments` with `/`, as manifest and archive paths are written.
pub fn relative(segments: &[&str]) -> String {
  segments.join("/")
}
