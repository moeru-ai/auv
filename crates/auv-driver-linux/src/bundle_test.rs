use std::ffi::OsString;
use std::path::{Path, PathBuf};

use super::*;

fn bundle_file(path: &Path) -> bool {
  path == Path::new("/opt/auv/lib/auv/pipewire-0.3/libpipewire-module-protocol-native.so")
    || path == Path::new("/opt/auv/share/auv/pipewire/client.conf")
}

// ROOT CAUSE:
//
// If the release bundle's libpipewire 0.3.65 ran on a host with PipeWire 1.4
// or newer, it loaded the host's client modules, which need symbols such as
// `pw_log_topic_register`, and the context failed to start.
//
// Before the fix, libpipewire always searched the host module, plugin and
// configuration directories.
// The fix points all three at the files shipped in the bundle.
#[test]
fn points_pipewire_at_the_bundled_client_files() {
  let environment = pipewire_environment(Path::new("/opt/auv/auv"), |_| None, bundle_file);

  assert_eq!(
    environment,
    vec![
      ("PIPEWIRE_MODULE_DIR", PathBuf::from("/opt/auv/lib/auv/pipewire-0.3")),
      ("SPA_PLUGIN_DIR", PathBuf::from("/opt/auv/lib/auv/spa-0.2")),
      ("PIPEWIRE_CONFIG_DIR", PathBuf::from("/opt/auv/share/auv/pipewire")),
    ]
  );
}

#[test]
fn keeps_pipewire_variables_the_user_set() {
  let environment =
    pipewire_environment(Path::new("/opt/auv/auv"), |name| (name == "SPA_PLUGIN_DIR").then(|| OsString::from("/srv/spa")), bundle_file);

  let names = environment.iter().map(|(name, _)| *name).collect::<Vec<_>>();
  assert_eq!(names, ["PIPEWIRE_MODULE_DIR", "PIPEWIRE_CONFIG_DIR"]);
}

#[test]
fn leaves_the_environment_alone_outside_a_release_bundle() {
  let environment = pipewire_environment(Path::new("/home/user/auv/target/debug/auv"), |_| None, |_| false);

  assert!(environment.is_empty());
}

#[test]
fn removes_only_injected_variables_from_children() {
  let names = injected_variables_from(Some(OsString::from("PIPEWIRE_MODULE_DIR,PIPEWIRE_CONFIG_DIR,LD_PRELOAD")));

  assert_eq!(names, ["PIPEWIRE_MODULE_DIR", "PIPEWIRE_CONFIG_DIR", INJECTED_ENV]);
}

#[test]
fn removes_nothing_when_this_process_injected_nothing() {
  assert!(injected_variables_from(None).is_empty());
}
