//! Subprocess coverage for separate app binaries and removed root subcommands.
//!
//! Hermetic: only spawns built `auv` / donor bins with `--help` or bare donor
//! subcommands. No desktop input, no network, no durable writes.

use std::process::Command;

fn run(bin: &str, args: &[&str]) -> std::process::Output {
  Command::new(bin).args(args).output().unwrap_or_else(|error| panic!("failed to spawn {bin}: {error}"))
}

fn stdout(out: &std::process::Output) -> String {
  String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr(out: &std::process::Output) -> String {
  String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn root_donor_subcommands_are_tombstoned() {
  let auv = env!("CARGO_BIN_EXE_auv");
  for (arg, bin) in [
    ("minecraft", "auv-minecraft"),
    ("osu", "auv-osu"),
    ("godot", "auv-godot"),
  ] {
    let out = run(auv, &[arg]);
    assert_ne!(out.status.code(), Some(0), "`auv {arg}` must exit non-zero");
    let err = stderr(&out);
    assert!(err.contains(bin), "`auv {arg}` stderr must point at `{bin}`: {err}");
    assert!(err.contains("has been removed") || err.contains("tombstone"), "`auv {arg}` stderr should explain removal: {err}");
  }
}

#[test]
fn donor_bins_help_exit_zero_and_name_live_bins() {
  let cases = [
    (env!("CARGO_BIN_EXE_auv-minecraft"), "auv-minecraft", "auv minecraft "),
    (env!("CARGO_BIN_EXE_auv-osu"), "auv-osu", "auv osu "),
    (env!("CARGO_BIN_EXE_auv-godot"), "auv-godot", "auv godot "),
  ];

  for (bin, live_name, retired_prefix) in cases {
    let out = run(bin, &["--help"]);
    assert_eq!(out.status.code(), Some(0), "{bin} --help must exit 0; stderr={}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains(live_name), "{bin} --help stdout must name `{live_name}`:\n{text}");
    assert!(!text.contains(retired_prefix), "{bin} --help must not present `{retired_prefix}` as live usage:\n{text}");
    assert!(!stderr(&out).starts_with("error:"), "{bin} --help must not prefix help with error:");
  }
}
