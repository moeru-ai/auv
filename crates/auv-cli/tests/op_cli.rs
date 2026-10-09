//! Process-level coverage for the `auv op run` binary entrypoint.
//!
//! Drives the real `auv` binary through the root CLI parser and dispatch
//! (`cli::run_root` -> `commands::op::run` -> `FastLoopScheduler` ->
//! executor), which the in-process `run_with_executor_inner` unit tests do
//! not exercise: argument parsing, subcommand routing, `--mode` handling,
//! and exit-code/JSON output reporting.
//!
//! `run_op` selects `WindowsProductionExecutor` on Windows, which would drive
//! real automation, while these tests rely on the fake operation executor.
//! They are therefore compiled out on Windows; the production executor path
//! is covered by the Windows live smoke runs instead.

// ROOT CAUSE (why this file exists):
//
// The e2e tests in auv-auto-loop drive the scheduler directly, and the op.rs
// unit tests drive `run_with_executor_inner` with hand-built `OpRunArgs`.
// Neither goes through the actual binary entrypoint, so a wiring mistake in
// root CLI parsing/dispatch (wrong subcommand route, dropped `--mode`,
// swallowed exit code) would pass every test and break production.

#![cfg(not(target_os = "windows"))]

use std::process::Command;

fn auv(args: &[&str]) -> std::process::Output {
  Command::new(env!("CARGO_BIN_EXE_auv")).args(args).output().expect("run auv binary")
}

fn stdout_json(output: &std::process::Output) -> serde_json::Value {
  let stdout = String::from_utf8_lossy(&output.stdout);
  serde_json::from_str(&stdout).unwrap_or_else(|err| panic!("stdout must be JSON (got {stdout:?}): {err}"))
}

#[test]
fn op_run_fast_mode_binary_entry_succeeds_unconfirmed() {
  let output = auv(&[
    "op",
    "run",
    "--operation",
    "qqmusic.prepare_playback_fast",
    "--mode",
    "fast",
    "--json",
  ]);
  assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

  let json = stdout_json(&output);
  assert_eq!(json["status"], "success");
  assert_eq!(json["execution_mode"], "fast");
  assert_eq!(json["confirmed"], false, "Fast must NEVER be confirmed:true");
  assert_eq!(json["reason_code"], "EXACT_KEY_MATCH");
}

#[test]
fn op_run_verified_mode_binary_entry_succeeds_confirmed() {
  let output = auv(&[
    "op",
    "run",
    "--operation",
    "qqmusic.prepare_playback",
    "--mode",
    "verified",
    "--json",
  ]);
  assert!(output.status.success(), "stderr: {}", String::from_utf8_lossy(&output.stderr));

  let json = stdout_json(&output);
  assert_eq!(json["status"], "success");
  assert_eq!(json["execution_mode"], "verified");
  assert_eq!(json["confirmed"], true, "Verified must be confirmed on gate pass");
  assert_eq!(json["reason_code"], "GATE_PASSED");
}

#[test]
fn op_run_mode_mismatch_binary_entry_fails_closed() {
  // qqmusic.prepare_playback is Verified-only; requesting Fast must be
  // rejected before any driver side effect.
  let output = auv(&[
    "op",
    "run",
    "--operation",
    "qqmusic.prepare_playback",
    "--mode",
    "fast",
    "--json",
  ]);
  assert!(!output.status.success(), "mode conflict must fail the process");

  let json = stdout_json(&output);
  assert_eq!(json["status"], "failed");
  assert_eq!(json["reason_code"], "MODE_MISMATCH_ESCALATE_VLM");
  assert_eq!(json["confirmed"], false);
}

#[test]
fn op_run_missing_mode_binary_entry_fails_closed() {
  let output = auv(&["op", "run", "--operation", "qqmusic.prepare_playback_fast"]);
  assert!(!output.status.success(), "missing --mode must fail the process");

  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("--mode"), "stderr must name the missing required flag: {stderr}");
}
