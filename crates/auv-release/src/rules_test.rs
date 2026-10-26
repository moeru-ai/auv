use std::cmp::Ordering;

use super::*;

#[test]
fn runtime_libraries_stay_on_the_host() {
  for name in [
    "libc.so.6",
    "ld-linux-x86-64.so.2",
    "ld-linux-aarch64.so.1",
    "libm.so.6",
    "libnss_files.so.2",
    "libpthread.so.0",
  ] {
    assert!(is_runtime_provided(name), "{name}");
  }
  for name in [
    "libtesseract.so.5",
    "libpipewire-0.3.so.0",
    "libcrypto.so.3",
    "libmagic.so.1",
  ] {
    assert!(!is_runtime_provided(name), "{name}");
  }
}

#[test]
fn run_paths_follow_the_bundle_location() {
  assert_eq!(expected_run_path("auv"), Some("$ORIGIN/lib/auv".to_string()));
  assert_eq!(expected_run_path("lib/auv/libtesseract.so.5"), Some("$ORIGIN".to_string()));
  assert_eq!(expected_run_path("lib/auv/pipewire-0.3/libpipewire-module-protocol-native.so"), Some("$ORIGIN:$ORIGIN/..".to_string()));
  assert_eq!(expected_run_path("lib/auv/spa-0.2/support/libspa-support.so"), Some("$ORIGIN/../..".to_string()));
  assert_eq!(expected_run_path("lib/auv/spa-0.2/stray.so"), None);
  assert_eq!(expected_run_path("share/auv/tessdata/eng.traineddata"), None);
  assert_eq!(expected_run_path("manifest.json"), None);
}

#[test]
fn client_conf_references_skip_comments() {
  let conf = "\
context.modules = [
  { name = libpipewire-module-protocol-native }
  { name = libpipewire-module-client-node }
  #{ name = libpipewire-module-rt }
  { name = libpipewire-module-protocol-native } # libpipewire-module-x11-bell
]
context.spa-libs = {
  support.* = support/libspa-support
  audio.convert.* = audioconvert/libspa-audioconvert
}
";
  assert_eq!(
    client_conf_references(conf),
    ClientConfReferences {
      modules: vec![
        "libpipewire-module-client-node".into(),
        "libpipewire-module-protocol-native".into()
      ],
      plugins: vec![
        "audioconvert/libspa-audioconvert".into(),
        "support/libspa-support".into()
      ],
    }
  );
}

#[test]
fn glibc_versions_compare_numerically() {
  assert_eq!(compare_glibc_versions("2.36", "2.4"), Ordering::Greater);
  assert_eq!(compare_glibc_versions("2.17", "2.36"), Ordering::Less);
  assert_eq!(compare_glibc_versions("2.2.5", "2.2"), Ordering::Greater);
  assert_eq!(compare_glibc_versions("2.36", "2.36"), Ordering::Equal);
}
