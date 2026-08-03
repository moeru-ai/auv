//! Subprocess coverage for the root AUV CLI boundary.

use std::process::Command;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn run(args: &[&str]) -> std::process::Output {
  Command::new(env!("CARGO_BIN_EXE_auv")).args(args).output().expect("run root auv binary")
}

fn stdout(output: &std::process::Output) -> String {
  String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &std::process::Output) -> String {
  String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn root_version_exits_zero_and_names_the_package_version() {
  let output = run(&["--version"]);

  assert_eq!(output.status.code(), Some(0), "auv --version must exit 0; stderr={}", stderr(&output));
  assert_eq!(stdout(&output), format!("auv {}\n", env!("CARGO_PKG_VERSION")));
  assert!(stderr(&output).is_empty(), "auv --version must not write stderr: {}", stderr(&output));
}

#[test]
fn root_help_does_not_advertise_supported_app_or_game_frontends() {
  let output = run(&["--help"]);

  assert_eq!(output.status.code(), Some(0), "auv --help must exit 0; stderr={}", stderr(&output));
  let help = stdout(&output);
  for removed_surface in [
    "auv-godot",
    "auv-osu",
    "auv-minecraft",
    "app.textedit.document.write",
  ] {
    assert!(!help.contains(removed_surface), "root help must not advertise {removed_surface}:\n{help}");
  }
  assert!(
    !help.lines().any(|line| line.trim_start().starts_with("permissions ")),
    "root help must not advertise the removed permissions command:\n{help}"
  );
  assert!(
    !help.lines().any(|line| line.trim_start().starts_with("session ")),
    "root help must not advertise the retired session command:\n{help}"
  );

  for expected in [
    "Commands:",
    "doctor",
    "invoke",
    "mcp",
    "plugin",
    "Examples:",
  ] {
    assert!(help.contains(expected), "root help must contain {expected:?}:\n{help}");
  }
}

#[test]
fn invoke_command_help_uses_typed_arguments_and_inline_examples() {
  let output = run(&["invoke", "screen.findText", "--help"]);

  assert_eq!(output.status.code(), Some(0), "invoke help must exit 0; stderr={}", stderr(&output));
  let help = stdout(&output);
  assert!(help.contains("Usage: auv invoke screen.findText [OPTIONS] <TEXT>"), "unexpected invoke help:\n{help}");
  assert!(help.contains("Arguments:"), "unexpected invoke help:\n{help}");
  assert!(help.contains("<TEXT>"), "unexpected invoke help:\n{help}");
  assert!(help.contains("Examples:"), "unexpected invoke help:\n{help}");
  assert!(help.contains("auv invoke screen.findText \"Settings\""), "unexpected invoke help:\n{help}");
  assert!(!help.contains("--target com.apple.TextEdit"), "help must not claim unsupported target activation:\n{help}");
}

#[cfg(unix)]
#[test]
fn unknown_top_level_command_executes_matching_auv_plugin() {
  let temp = tempfile::tempdir().expect("create plugin directory");
  let plugin = temp.path().join("auv-fixture");
  std::fs::write(
    &plugin,
    "#!/bin/sh\nprintf 'args=%s|%s\\n' \"$1\" \"$2\"\nprintf 'auv_path=%s\\n' \"$AUV_PATH\"\nprintf 'plugin stderr\\n' >&2\nexit 23\n",
  )
  .expect("write fixture plugin");
  let mut permissions = std::fs::metadata(&plugin).expect("read plugin metadata").permissions();
  permissions.set_mode(0o755);
  std::fs::set_permissions(&plugin, permissions).expect("make plugin executable");

  let output = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["fixture", "child", "--value"])
    .env("PATH", temp.path())
    .output()
    .expect("run root auv binary");

  assert_eq!(output.status.code(), Some(23));
  assert_eq!(stdout(&output), format!("args=child|--value\nauv_path={}\n", env!("CARGO_BIN_EXE_auv")));
  assert_eq!(stderr(&output), "plugin stderr\n");
}

#[cfg(unix)]
#[test]
fn plugin_list_reports_path_order_shadowing_and_builtin_collisions() {
  let first = tempfile::tempdir().expect("create first plugin directory");
  let second = tempfile::tempdir().expect("create second plugin directory");
  for path in [
    first.path().join("auv-demo"),
    second.path().join("auv-demo"),
    second.path().join("auv-invoke"),
  ] {
    std::fs::write(&path, "#!/bin/sh\n").expect("write fixture plugin");
    let mut permissions = std::fs::metadata(&path).expect("read plugin metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(&path, permissions).expect("make plugin executable");
  }
  let path = std::env::join_paths([first.path(), second.path()]).expect("join fixture PATH");

  let output = Command::new(env!("CARGO_BIN_EXE_auv")).args(["plugin", "list"]).env("PATH", path).output().expect("list plugins");

  assert_eq!(output.status.code(), Some(1), "plugin warnings must produce a failing status");
  assert!(stdout(&output).contains(&first.path().join("auv-demo").display().to_string()));
  let diagnostics = stderr(&output);
  assert!(diagnostics.contains("shadowed"), "missing shadow warning:\n{diagnostics}");
  assert!(diagnostics.contains("collides with built-in command `invoke`"), "missing collision warning:\n{diagnostics}");
}

#[cfg(unix)]
#[test]
fn builtins_take_precedence_and_compound_plugin_names_are_not_probed() {
  let temp = tempfile::tempdir().expect("create plugin directory");
  for name in ["auv-doctor", "auv-demo-child"] {
    let path = temp.path().join(name);
    std::fs::write(&path, "#!/bin/sh\nprintf 'plugin-ran\\n'\n").expect("write fixture plugin");
    let mut permissions = std::fs::metadata(&path).expect("read plugin metadata").permissions();
    permissions.set_mode(0o755);
    std::fs::set_permissions(path, permissions).expect("make plugin executable");
  }

  let builtin = Command::new(env!("CARGO_BIN_EXE_auv")).args(["doctor", "--help"]).env("PATH", temp.path()).output().expect("run builtin");
  assert!(builtin.status.success());
  assert!(!stdout(&builtin).contains("plugin-ran"));

  let compound = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args(["demo", "child"])
    .env("PATH", temp.path())
    .output()
    .expect("run missing single-name plugin");
  assert!(!compound.status.success());
  assert!(stderr(&compound).contains("auv-demo"));
  assert!(!stdout(&compound).contains("plugin-ran"));
}

#[cfg(unix)]
#[test]
fn plugin_list_reports_non_executable_candidates_and_empty_paths() {
  let empty = tempfile::tempdir().expect("create empty PATH directory");
  let empty_output =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["plugin", "list"]).env("PATH", empty.path()).output().expect("list empty plugin path");
  assert!(empty_output.status.success());
  assert!(stdout(&empty_output).contains("No AUV plugins were found"));

  let candidate = empty.path().join("auv-disabled");
  std::fs::write(&candidate, "#!/bin/sh\n").expect("write non-executable candidate");
  let warning_output =
    Command::new(env!("CARGO_BIN_EXE_auv")).args(["plugin", "list"]).env("PATH", empty.path()).output().expect("list non-executable plugin");
  assert!(!warning_output.status.success());
  assert!(stderr(&warning_output).contains("not executable"));
}

#[test]
fn typed_invoke_values_are_rejected_before_execution() {
  let output = run(&[
    "invoke",
    "screen.captureRegion",
    "--x",
    "not-a-number",
    "--y",
    "0",
    "--width",
    "10",
    "--height",
    "10",
    "--dry-run",
  ]);

  assert!(!output.status.success());
  assert!(stderr(&output).contains("invalid value 'not-a-number'"), "unexpected diagnostic:\n{}", stderr(&output));
}

#[test]
fn typed_invoke_ranges_are_rejected_by_the_handler() {
  let output = run(&[
    "invoke",
    "input.clickWindowPoint",
    "--relative-x",
    "2",
    "--relative-y",
    "0.5",
    "--dry-run",
  ]);

  assert!(!output.status.success());
  assert!(stdout(&output).contains("within 0..=1"), "unexpected diagnostic:\n{}", stdout(&output));
}

#[test]
fn invoke_store_root_cannot_consume_the_next_flag() {
  let output = run(&[
    "invoke",
    "scan.coverage",
    "--fixture-dir",
    "unused",
    "--store-root",
    "--dry-run",
  ]);

  assert!(!output.status.success());
  assert!(stderr(&output).contains("--store-root <PATH>"), "unexpected diagnostic:\n{}", stderr(&output));
}
