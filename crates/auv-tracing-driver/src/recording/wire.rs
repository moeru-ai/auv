//! camelCase wire view for the inspect server HTTP write API.
//!
//! `RunUpdate` serializes canonically as snake_case. The HTTP write API
//! (`POST /write/runs/{runId}/updates`) negotiates a camelCase shape with
//! external callers. This module wraps `RunUpdate` in a newtype whose
//! `Serialize`/`Deserialize` impls rename keys through `serde_json::Value`,
//! keeping the Rust field list defined exactly once in [`super::update`]
//! and [`crate::trace`].
//!
//! Key transformation rules:
//! - Recurse into the value tree; rename every JSON object key.
//! - Skip recursion into values under `attributes` — those are user-defined
//!   key/value pairs (e.g., `auv.step.id` → arbitrary JSON) that must round-trip
//!   verbatim.

use serde::de::Error as DeError;
use serde::ser::Error as SerError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use super::update::RunUpdate;

#[derive(Clone, Debug, PartialEq)]
pub struct WireUpdate(pub RunUpdate);

impl Serialize for WireUpdate {
  fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
    let mut value = serde_json::to_value(&self.0).map_err(S::Error::custom)?;
    rename_keys(&mut value, snake_to_camel);
    rename_discriminator(&mut value, snake_to_camel);
    value.serialize(serializer)
  }
}

impl<'de> Deserialize<'de> for WireUpdate {
  fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
    let mut value = Value::deserialize(deserializer)?;
    rename_keys(&mut value, camel_to_snake);
    rename_discriminator(&mut value, camel_to_snake);
    serde_json::from_value(value)
      .map(WireUpdate)
      .map_err(D::Error::custom)
  }
}

/// `RunUpdate` uses serde tag = "type" with snake_case variant names. On the
/// camelCase wire, the discriminator string also needs renaming (e.g.
/// `"run_started"` ↔ `"runStarted"`).
fn rename_discriminator(value: &mut Value, transform: fn(&str) -> String) {
  if let Value::Object(map) = value
    && let Some(Value::String(name)) = map.get_mut("type")
  {
    *name = transform(name);
  }
}

/// Recursively rename JSON object keys from camelCase to snake_case in place.
///
/// User-defined keys under `attributes` are preserved verbatim. Useful when
/// decoding camelCase HTTP write fixtures into canonical record types.
pub fn camel_case_keys_to_snake(value: &mut Value) {
  rename_keys(value, camel_to_snake);
}

fn rename_keys(value: &mut Value, transform: fn(&str) -> String) {
  match value {
    Value::Object(map) => {
      let entries: Vec<(String, Value)> = std::mem::take(map).into_iter().collect();
      for (key, mut nested) in entries {
        if key != "attributes" {
          rename_keys(&mut nested, transform);
        }
        let renamed = transform(&key);
        map.insert(renamed, nested);
      }
    }
    Value::Array(items) => {
      for item in items {
        rename_keys(item, transform);
      }
    }
    _ => {}
  }
}

fn snake_to_camel(input: &str) -> String {
  let mut out = String::with_capacity(input.len());
  let mut upper_next = false;
  for ch in input.chars() {
    if ch == '_' {
      upper_next = true;
      continue;
    }
    if upper_next {
      out.extend(ch.to_uppercase());
      upper_next = false;
    } else {
      out.push(ch);
    }
  }
  out
}

fn camel_to_snake(input: &str) -> String {
  let mut out = String::with_capacity(input.len() + 4);
  for (index, ch) in input.chars().enumerate() {
    if ch.is_ascii_uppercase() {
      if index > 0 {
        out.push('_');
      }
      out.extend(ch.to_lowercase());
    } else {
      out.push(ch);
    }
  }
  out
}

#[cfg(test)]
mod tests {
  use crate::trace::{
    RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SpanId, TraceId, TraceState,
    TraceStatusCode,
  };

  use super::super::update::RunUpdate;
  use super::{WireUpdate, camel_to_snake, snake_to_camel};

  fn test_run() -> RunRecordV1Alpha1 {
    RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: RunId::new("run_update_test"),
      trace_id: TraceId::new("00000000000000000000000000000001"),
      run_type: RunType::Execute,
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: 100,
      finished_at_millis: None,
      root_span_id: SpanId::new("0000000000000001"),
      attributes: [(
        "auv.step.id".to_string(),
        serde_json::Value::String("captureScreenshot".to_string()),
      )]
      .into_iter()
      .collect(),
      summary: None,
      failure: None,
    }
  }

  #[test]
  fn case_helpers_round_trip_common_field_names() {
    for snake in ["run_id", "started_at_millis", "api_version", "root_span_id"] {
      let camel = snake_to_camel(snake);
      let back = camel_to_snake(&camel);
      assert_eq!(snake, back, "round trip {snake} -> {camel} -> {back}");
    }
  }

  #[test]
  fn wire_serializes_run_update_as_camel_case() {
    let update = RunUpdate::RunStarted {
      run_id: RunId::new("run_update_test"),
      run: test_run(),
    };

    let value = serde_json::to_value(WireUpdate(update)).expect("update should serialize");
    assert_eq!(value["type"], "runStarted");
    assert_eq!(value["runId"], "run_update_test");
    assert_eq!(value["run"]["apiVersion"], "auv.run.v1alpha1");
    assert_eq!(value["run"]["rootSpanId"], "0000000000000001");
    assert!(value["run"].get("root_span_id").is_none());
  }

  #[test]
  fn wire_preserves_attribute_keys_verbatim() {
    let update = RunUpdate::RunStarted {
      run_id: RunId::new("run_update_test"),
      run: test_run(),
    };

    let value = serde_json::to_value(WireUpdate(update)).expect("update should serialize");
    let attributes = value["run"]["attributes"]
      .as_object()
      .expect("attributes object");
    assert_eq!(
      attributes.get("auv.step.id").and_then(|v| v.as_str()),
      Some("captureScreenshot"),
      "user-defined attribute keys must not be transformed"
    );
  }

  #[test]
  fn wire_round_trips_via_camel_case() {
    let original = RunUpdate::RunStarted {
      run_id: RunId::new("run_update_test"),
      run: test_run(),
    };
    let json = serde_json::to_string(&WireUpdate(original.clone())).expect("serialize");
    let decoded: WireUpdate = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(decoded.0, original);
  }
}
