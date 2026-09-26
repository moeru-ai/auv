use std::path::PathBuf;

use serde_json::Value;

use super::{focus_cases, keyboard_cases};

#[test]
#[ignore = "requires local AUV_EVAL_RECEIPTS; does not launch GUI applications"]
fn migrated_assertions_match_recorded_receipts() {
  let root = PathBuf::from(std::env::var_os("AUV_EVAL_RECEIPTS").expect("set AUV_EVAL_RECEIPTS to the local keyboard-validation directory"));

  let mut keyboard_count = 0;
  for mode in ["background", "foreground", "background-supplement"] {
    for app in ["chrome", "electron"] {
      let records: Vec<Value> = serde_json::from_slice(
        &std::fs::read(root.join("2026-09-25-keyboard-modifier-validation").join(mode).join(format!("{app}-results.json"))).unwrap(),
      )
      .unwrap();

      for record in records.iter().filter(|r| r.get("checks").is_some()) {
        let cases = keyboard_cases::cases(true);
        let case = cases.iter().find(|c| record["case"] == c.name).unwrap();

        assert_eq!(keyboard_cases::check(case, record, mode == "foreground").unwrap(), record["checks"], "{mode}/{app}/{}", case.name);
        keyboard_count += 1;
      }
    }
  }

  let root = root.join("2026-09-26-no-raise-keyboard/restoration");
  let observations: Vec<Value> =
    std::fs::read_to_string(root.join("system-state.jsonl")).unwrap().lines().map(|s| serde_json::from_str(s).unwrap()).collect();
  let records: Vec<Value> = serde_json::from_slice(&std::fs::read(root.join("results.json")).unwrap()).unwrap();

  for record in &records {
    let mut during: Vec<_> = observations
      .iter()
      .filter(|v| {
        v["timeMs"].as_f64().unwrap() >= record["startMs"].as_f64().unwrap()
          && v["timeMs"].as_f64().unwrap() <= record["endMs"].as_f64().unwrap()
      })
      .cloned()
      .collect();

    for stage in ["prepared", "delivered", "restored"] {
      during.push(record[stage]["system"].clone());
    }

    assert_eq!(focus_cases::check(record, &during), record["checks"], "{}", record["caseId"]);
  }

  assert_eq!(keyboard_count, 186);
  assert_eq!(records.len(), 108);
}
