use std::process::Command;

#[test]
fn invoke_dry_run_writes_append_only_trace_records() {
  let store = tempfile::tempdir().expect("temporary tracing store");
  let output = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "invoke",
      "scan.coverage",
      "--dry-run",
      "--json",
      "--store-root",
      store.path().to_str().expect("UTF-8 store path"),
    ])
    .output()
    .expect("run auv invoke");
  assert!(output.status.success(), "invoke failed: {}", String::from_utf8_lossy(&output.stderr));

  let direct: serde_json::Value = serde_json::from_slice(&output.stdout).expect("invoke JSON");
  let run_id = direct["run_id"].as_str().expect("run id");
  let records = std::fs::read_to_string(store.path().join("records.jsonl")).expect("trace records");
  let records =
    records.lines().map(|line| serde_json::from_str::<serde_json::Value>(line).expect("trace record envelope")).collect::<Vec<_>>();

  assert!(!records.is_empty());
  assert!(records.iter().all(|envelope| envelope["version"] == 1));
  assert!(records.iter().all(|envelope| envelope["record"]["run_id"] == run_id));
  assert!(records.iter().any(|envelope| envelope["record"]["type"] == "event"));
}
