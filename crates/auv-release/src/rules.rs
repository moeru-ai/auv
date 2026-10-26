//! Rules that both packaging and verification apply, so the two cannot drift.

use std::sync::LazyLock;

use auv_bundle_layout as layout;

/// Oldest glibc the Linux release runs on: Debian 12, the build container.
pub const GLIBC_BASELINE: &str = "2.36";

/// Whether the C runtime provides `name`. These stay on the host: the bundle
/// targets [`GLIBC_BASELINE`] and must not carry its own libc or loader.
pub fn is_runtime_provided(name: &str) -> bool {
  const PREFIXES: [&str; 11] = [
    "ld-linux",
    "libanl.so",
    "libc.so",
    "libdl.so",
    "libm.so",
    "libnss_",
    "libpthread.so",
    "libresolv.so",
    "librt.so",
    "libthread_db.so",
    "libutil.so",
  ];
  PREFIXES.iter().any(|prefix| name.starts_with(prefix)) && name.contains(".so")
}

/// The run path an ELF file at `relative` (a `/`-separated bundle path) must
/// carry, or `None` when the bundle does not patch it.
///
/// Shared libraries sit directly in the library directory; PipeWire modules one
/// level and SPA plugins two levels below it. Modules also find the modules
/// they link beside themselves.
pub fn expected_run_path(relative: &str) -> Option<String> {
  let libraries = format!("{}/", layout::relative(layout::LIBRARY_DIR));
  let modules = format!("{}/", layout::relative(layout::PIPEWIRE_MODULE_DIR));
  let plugins = format!("{}/", layout::relative(layout::SPA_PLUGIN_DIR));
  let run_path = if relative == layout::EXECUTABLE {
    format!("$ORIGIN/{}", layout::relative(layout::LIBRARY_DIR))
  } else if relative.starts_with(&modules) {
    "$ORIGIN:$ORIGIN/..".to_string()
  } else if let Some(plugin) = relative.strip_prefix(&plugins) {
    plugin.contains('/').then(|| "$ORIGIN/../..".to_string())?
  } else {
    relative.strip_prefix(&libraries).filter(|library| !library.contains('/')).map(|_| "$ORIGIN".to_string())?
  };
  Some(run_path)
}

/// Modules and SPA plugins that a PipeWire `client.conf` loads, sorted and
/// without duplicates. Comments are ignored.
pub fn client_conf_references(client_conf: &str) -> ClientConfReferences {
  static MODULE: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"libpipewire-module-[a-z0-9-]+").unwrap());
  static PLUGIN: LazyLock<regex::Regex> = LazyLock::new(|| regex::Regex::new(r"[a-z0-9-]+/libspa-[a-z0-9-]+").unwrap());
  let active = client_conf.lines().map(|line| line.split('#').next().unwrap_or_default()).collect::<Vec<_>>().join("\n");
  let collect = |pattern: &regex::Regex| {
    let mut found = pattern.find_iter(&active).map(|found| found.as_str().to_string()).collect::<Vec<_>>();
    found.sort();
    found.dedup();
    found
  };
  ClientConfReferences {
    modules: collect(&MODULE),
    plugins: collect(&PLUGIN),
  }
}

#[derive(Debug, PartialEq, Eq)]
pub struct ClientConfReferences {
  /// Module names without `.so`, such as `libpipewire-module-protocol-native`.
  pub modules: Vec<String>,
  /// Plugin paths below the SPA directory without `.so`, such as
  /// `support/libspa-support`.
  pub plugins: Vec<String>,
}

/// Compares dotted glibc versions such as `2.36` and `2.4` numerically.
pub fn compare_glibc_versions(left: &str, right: &str) -> std::cmp::Ordering {
  let parts = |version: &str| version.split('.').map(|part| part.parse::<u32>().unwrap_or(0)).collect::<Vec<_>>();
  parts(left).cmp(&parts(right))
}

#[cfg(test)]
#[path = "rules_test.rs"]
mod tests;
