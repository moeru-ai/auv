//! Private PipeWire client files shipped beside the glibc Linux release
//! executable.
//!
//! The release bundle carries its own `libpipewire-0.3.so.0`. That library
//! loads client modules, SPA plugins and `client.conf` at runtime, and by
//! default it looks for them under the host's `/usr/lib` and `/usr/share`.
//! Modules built for a newer host PipeWire need symbols the bundled library
//! does not have, so the context fails to start (for example
//! `undefined symbol: pw_log_topic_register` with Debian 13 PipeWire 1.4 or
//! Ubuntu 26.04 PipeWire 1.6). The bundle therefore ships the matching client
//! modules, plugins and configuration, and the process points libpipewire at
//! them. libpipewire reads these locations only from the environment.
//!
//! The variables are removed again for children that are not this bundle
//! (provider Runners and plugins); see [`injected_variables`].

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Lists the PipeWire variables this process injected, so child launches can
/// remove exactly those and keep values the user set.
pub const INJECTED_ENV: &str = "AUV_BUNDLED_PIPEWIRE_ENV";

/// Points libpipewire at the bundled client files when this executable runs
/// from a release bundle.
///
/// Variables the user already set are kept. Outside a release bundle (for
/// example a `cargo` build or a distribution package) this does nothing.
///
/// # Safety
///
/// This mutates the process environment. Call it at the start of `main`,
/// before any other thread exists, as `std::env::set_var` requires.
pub unsafe fn prepare_pipewire_environment() {
  let Ok(executable) = std::env::current_exe() else {
    return;
  };
  let assignments = pipewire_environment(&executable, |name| std::env::var_os(name), Path::is_file);
  if assignments.is_empty() {
    return;
  }
  let names = assignments.iter().map(|(name, _)| *name).collect::<Vec<_>>().join(",");
  for (name, value) in assignments {
    // SAFETY: the caller guarantees that no other thread is running.
    unsafe { std::env::set_var(name, value) };
  }
  // SAFETY: as above.
  unsafe { std::env::set_var(INJECTED_ENV, names) };
}

/// Returns the variables to remove before starting a child process that is not
/// this bundle: every PipeWire variable this process injected, plus the marker.
pub fn injected_variables() -> Vec<OsString> {
  injected_variables_from(std::env::var_os(INJECTED_ENV))
}

fn injected_variables_from(marker: Option<OsString>) -> Vec<OsString> {
  let Some(marker) = marker else {
    return Vec::new();
  };
  let mut names = marker
    .to_string_lossy()
    .split(',')
    .filter(|name| PIPEWIRE_VARIABLES.iter().any(|(known, _)| known == name))
    .map(OsString::from)
    .collect::<Vec<_>>();
  names.push(OsString::from(INJECTED_ENV));
  names
}

/// Variables libpipewire 0.3 reads, and the bundle directory each one names,
/// relative to the directory that contains the executable.
const PIPEWIRE_VARIABLES: [(&str, &[&str]); 3] = [
  ("PIPEWIRE_MODULE_DIR", &["lib", "auv", "pipewire-0.3"]),
  ("SPA_PLUGIN_DIR", &["lib", "auv", "spa-0.2"]),
  ("PIPEWIRE_CONFIG_DIR", &["share", "auv", "pipewire"]),
];

fn pipewire_environment(
  executable: &Path,
  existing: impl Fn(&str) -> Option<OsString>,
  is_file: impl Fn(&Path) -> bool,
) -> Vec<(&'static str, PathBuf)> {
  let Some(root) = executable.parent() else {
    return Vec::new();
  };
  let bundle_path = |components: &[&str]| components.iter().fold(root.to_path_buf(), |path, component| path.join(component));
  // A bundle without the native protocol module or the client configuration
  // cannot start a PipeWire context, so it is not treated as a bundle at all.
  let modules = bundle_path(PIPEWIRE_VARIABLES[0].1);
  let config = bundle_path(PIPEWIRE_VARIABLES[2].1);
  if !is_file(&modules.join("libpipewire-module-protocol-native.so")) || !is_file(&config.join("client.conf")) {
    return Vec::new();
  }
  PIPEWIRE_VARIABLES
    .iter()
    .filter(|(name, _)| existing(name).is_none_or(|value| value.is_empty()))
    .map(|(name, components)| (*name, bundle_path(components)))
    .collect()
}

#[cfg(test)]
#[path = "bundle_test.rs"]
mod tests;
