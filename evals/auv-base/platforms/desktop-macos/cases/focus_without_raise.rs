//! Focus preparation scenarios and independent text/restoration assertions.

use serde_json::{Value, json};

pub struct Case {
  pub name: &'static str,
  pub initial: &'static str,
  pub commands: &'static [&'static [&'static str]],
  pub expected: &'static str,
}

pub const CASES: &[Case] = &[
  Case {
    name: "select_all",
    initial: "replace me",
    commands: &[&["hold", "cmd", "a"], &["input.typeText", "Z"]],
    expected: "Z",
  },
  Case {
    name: "emoji",
    initial: "",
    commands: &[&["input.typeText", "😀"]],
    expected: "😀",
  },
  Case {
    name: "bmp",
    initial: "",
    commands: &[&["input.typeText", "A猫"]],
    expected: "A猫",
  },
];

pub fn check(record: &Value, during: &[Value]) -> Value {
  let before = &record["before"];
  let anchor = &record["anchor_pid"];
  let target = &record["target_pid"];
  let restored = &record["restored"];
  let target_focused = restored["target"].get("documentFocused").unwrap_or(&restored["target"]["keyWindow"]);

  let mut checks = json!({
    "text": record["delivered"]["target"]["value"] == record["expected"],
    "no_raise": during.iter().all(|v| v["windowOrder"] == before["system"]["windowOrder"]),
    "workspace_front_unchanged": during.iter().all(|v| &v["workspaceFrontPid"] == anchor),
    "windowserver_front_unchanged": during.iter().all(|v| v["windowServerFrontPSN"] == before["system"]["windowServerFrontPSN"]),
    "restored_front": &restored["system"]["workspaceFrontPid"] == anchor,
    "restored_anchor_focus": restored["anchor"]["active"] == true && restored["anchor"]["keyWindow"] == true,
    "restored_target_unfocused": target_focused != &Value::Bool(true),
    "anchor_unchanged": restored["anchor"]["value"] == "ANCHOR",
    "valid": record["input_source_after"] == "com.apple.keylayout.ABC"
      && during.iter().all(|v| &v["workspaceFrontPid"] == anchor || &v["workspaceFrontPid"] == target),
  });

  if let Some(browser) = record.get("agent_browser_receipt") {
    checks["browser_agrees"] =
      json!(["value", "selection", "documentFocused"].iter().all(|key| browser[key] == record["delivered"]["target"][key]));
  }

  checks
}

pub fn passed(record: &Value) -> bool {
  let checks = &record["checks"];
  let mut required = vec![
    "text",
    "valid",
    "restored_front",
    "restored_anchor_focus",
    "restored_target_unfocused",
    "anchor_unchanged",
  ];

  if !record["mode"].as_str().unwrap_or("").starts_with("foreground") {
    required.extend([
      "no_raise",
      "workspace_front_unchanged",
      "windowserver_front_unchanged",
    ]);
  }

  record.get("error").is_none() && required.iter().all(|key| checks[key] == true) && checks["browser_agrees"] != false
}
