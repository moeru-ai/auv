//! Scenarios and receiver assertions, independent of application startup.

use anyhow::{Context, Result};
use serde_json::{Value, json};

pub struct Case {
  pub name: &'static str,
  pub initial: &'static str,
  pub commands: Vec<Vec<&'static str>>,
  pub expected: &'static str,
  inputs: usize,
  keys: Option<usize>,
}

impl Case {
  fn new(
    name: &'static str,
    initial: &'static str,
    commands: &[&[&'static str]],
    expected: &'static str,
    inputs: usize,
    keys: Option<usize>,
  ) -> Self {
    Self {
      name,
      initial,
      commands: commands.iter().map(|c| c.to_vec()).collect(),
      expected,
      inputs,
      keys,
    }
  }
}

pub fn cases(held: bool) -> Vec<Case> {
  if !held {
    return vec![
      Case::new("unicode_bmp", "", &[&["input.typeText", "A猫"]], "A猫", 2, Some(2)),
      Case::new("unicode_emoji", "", &[&["input.typeText", "😀"]], "😀", 1, Some(1)),
      Case::new("plain_key", "", &[&["input.keys", "b"]], "b", 1, Some(1)),
      Case::new("shift_key", "", &[&["input.keys", "shift", "b"]], "B", 1, Some(1)),
      Case::new("repeat_key", "", &[&["input.keys", "x", "--count", "3", "--interval-ms", "50"]], "xxx", 3, Some(3)),
      Case::new("select_all_replace", "replace me", &[&["input.keys", "cmd", "a"], &["input.typeText", "Z"]], "Z", 1, None),
      Case::new("arrow_left", "ab", &[&["input.keys", "left"], &["input.typeText", "X"]], "aXb", 1, Some(2)),
      Case::new("return", "ab", &[&["input.keys", "return"]], "ab\n", 1, Some(1)),
      Case::new("modifier_release", "", &[&["input.keys", "shift", "b"], &["input.keys", "c"]], "Bc", 2, Some(2)),
    ];
  }

  vec![
    Case::new("hold_b", "", &[&["hold", "b"]], "b", 1, Some(1)),
    Case::new("hold_shift_b", "", &[&["hold", "shift", "b"]], "B", 1, Some(1)),
    Case::new("split_b", "", &[&["down", "b"], &["wait", "200"], &["up"]], "b", 1, Some(1)),
    Case::new("timeout_b", "", &[&["timeout", "b"], &["wait", "350"], &["up"]], "b", 1, Some(1)),
    Case::new("drop_b", "", &[&["down", "b"], &["wait", "200"], &["drop"]], "b", 1, Some(1)),
    // ROOT CAUSE: ordinary macOS presses lost an existing held Shift. Assert
    // modifier inheritance and cleanup on the next unmodified key.
    Case::new(
      "held_shift_press_b",
      "",
      &[
        &["down", "shift"],
        &["wait", "200"],
        &["input.keys", "b"],
        &["up"],
        &["input.keys", "c"],
      ],
      "Bc",
      2,
      Some(2),
    ),
    Case::new(
      "held_shift_nested_press",
      "",
      &[
        &["down", "shift"],
        &["wait", "200"],
        &["input.keys", "shift", "b"],
        &["input.keys", "c"],
        &["up"],
        &["input.keys", "d"],
      ],
      "BCd",
      3,
      Some(3),
    ),
    Case::new(
      "timeout_shift_then_b",
      "",
      &[
        &["timeout", "shift"],
        &["wait", "350"],
        &["input.keys", "b"],
        &["up"],
      ],
      "b",
      1,
      Some(1),
    ),
    Case::new(
      "drop_shift_then_b",
      "",
      &[
        &["down", "shift"],
        &["wait", "200"],
        &["drop"],
        &["input.keys", "b"],
      ],
      "b",
      1,
      Some(1),
    ),
    Case::new("hold_select_all", "replace me", &[&["hold", "cmd", "a"], &["input.typeText", "Z"]], "Z", 1, None),
    Case::new("persistent_press_b", "", &[&["input.keys", "b"]], "b", 1, Some(1)),
    Case::new("persistent_unicode_bmp", "", &[&["input.typeText", "A猫"]], "A猫", 2, Some(2)),
    Case::new("persistent_emoji", "", &[&["input.typeText", "😀"]], "😀", 1, Some(1)),
  ]
}

pub fn check(case: &Case, record: &Value, foreground: bool) -> Result<Value> {
  let receipt = &record["receipt"];
  let events = receipt["events"].as_array().context("receiver events missing")?;
  let base: Vec<_> =
    events.iter().filter(|e| !["Shift", "Control", "Alt", "Meta", "CapsLock"].contains(&e["key"].as_str().unwrap_or(""))).collect();
  let downs: Vec<_> = base.iter().filter(|e| e["type"] == "keydown").collect();
  let ups: Vec<_> = base.iter().filter(|e| e["type"] == "keyup").collect();
  let responses = record["driver_results"].as_array().context("driver results missing")?;

  let mut checks = json!({
    "text": receipt["value"] == case.expected,
    "input_count": events.iter().filter(|e| e["type"] == "input").count() == case.inputs,
    "key_pairs": case.keys.is_none_or(|count| downs.len() == count && ups.len() == count),
    "focus": record["foreground_before"] == record["foreground_after"] && events.iter().all(|e| e["documentFocused"] == foreground),
    "trusted_events": !events.is_empty() && events.iter().all(|e| e["trusted"] == true),
    "submission_unverified": responses.iter().filter(|r| !r["result"].is_null()).all(|r| r["result"]["verified"] == false),
  });

  if ["hold_b", "hold_shift_b", "split_b", "timeout_b", "drop_b"].contains(&case.name) {
    checks["hold_duration"] = json!(
      downs.len() == 1
        && ups.len() == 1
        && downs[0]["receivedMs"]
          .as_f64()
          .zip(ups[0]["receivedMs"].as_f64())
          .is_some_and(|(down, up)| (150.0..=1500.0).contains(&(up - down)))
    );
  }

  if [
    "split_b",
    "drop_b",
    "held_shift_press_b",
    "held_shift_nested_press",
    "drop_shift_then_b",
  ]
  .contains(&case.name)
  {
    let held = record["intermediate"][0]["events"].as_array().context("intermediate receipt missing")?;
    checks["down_before_release"] = json!(held.iter().any(|e| e["type"] == "keydown") && !held.iter().any(|e| e["type"] == "keyup"));
  }

  if ["timeout_b", "timeout_shift_then_b"].contains(&case.name) {
    let held = record["intermediate"][0]["events"].as_array().context("timeout receipt missing")?;
    checks["timeout_released"] = json!(held.iter().any(|e| e["type"] == "keyup"));
  }

  let shifts: Option<&[bool]> = match case.name {
    "held_shift_press_b" => Some(&[true, false]),
    "held_shift_nested_press" => Some(&[true, true, false]),
    "timeout_shift_then_b" | "drop_shift_then_b" => Some(&[false]),
    _ => None,
  };

  if let Some(shifts) = shifts {
    checks["modifier_flags"] = json!(
      downs.iter().map(|e| e["shift"].as_bool()).collect::<Vec<_>>() == shifts.iter().copied().map(Some).collect::<Vec<_>>()
        && ups.iter().map(|e| e["shift"].as_bool()).collect::<Vec<_>>() == shifts.iter().copied().map(Some).collect::<Vec<_>>()
    );
  }

  Ok(checks)
}
