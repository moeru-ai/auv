use std::process::Command;

#[test]
fn invoke_dry_run_writes_append_only_trace_records() {
  let store = tempfile::tempdir().expect("temporary tracing store");
  let output = Command::new(env!("CARGO_BIN_EXE_auv"))
    .args([
      "invoke",
      "scan.coverage",
      "--fixture-dir",
      "unused",
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
  assert!(records.iter().all(|envelope| envelope["version"] == 2));
  assert!(records.iter().all(|envelope| envelope["record"]["run_id"] == run_id));
  assert!(records.iter().any(|envelope| envelope["record"]["type"] == "event"));

  // The command is one span, with its inputs and outcome recorded inside it.
  let command = records
    .iter()
    .find(|envelope| envelope["record"]["type"] == "span_started" && envelope["record"]["name"] == "auv.invoke.command")
    .expect("command span");
  assert_eq!(command["record"]["attributes"]["command_id"], "scan.coverage");
  assert_eq!(command["record"]["attributes"]["dry_run"], true);
  let span_id = &command["record"]["span_id"];
  let event = |schema: &str| {
    records.iter().find(|envelope| envelope["record"]["schema"]["name"] == schema).unwrap_or_else(|| panic!("missing {schema}"))
  };
  let started = event("auv.invoke.command_started");
  assert_eq!(&started["record"]["span_id"], span_id);
  assert_eq!(started["record"]["payload"]["inputs"]["fixture-dir"], "unused");
  assert_eq!(event("auv.invoke.command_finished")["record"]["payload"]["status"], "succeeded");
  assert!(records.iter().any(|envelope| envelope["record"]["type"] == "span_ended" && &envelope["record"]["span_id"] == span_id));
}
