use std::collections::BTreeMap;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::model::now_millis;

pub const RUN_API_VERSION: &str = "auv.run.v1alpha1";
pub const SPAN_API_VERSION: &str = "auv.span.v1alpha1";
pub const EVENT_API_VERSION: &str = "auv.event.v1alpha1";
pub const ARTIFACT_API_VERSION: &str = "auv.artifact.v1alpha1";

static TRACE_COUNTER: AtomicU64 = AtomicU64::new(0);
static RUN_COUNTER: AtomicU64 = AtomicU64::new(0);
static SPAN_COUNTER: AtomicU64 = AtomicU64::new(0);
static EVENT_COUNTER: AtomicU64 = AtomicU64::new(0);

macro_rules! id_type {
  ($name:ident) => {
    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct $name(String);

    impl $name {
      pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
      }

      pub fn as_str(&self) -> &str {
        &self.0
      }
    }

    impl std::fmt::Display for $name {
      fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
      }
    }

    impl AsRef<str> for $name {
      fn as_ref(&self) -> &str {
        self.as_str()
      }
    }
  };
}

id_type!(RunId);
id_type!(TraceId);
id_type!(SpanId);
id_type!(EventId);
id_type!(ArtifactId);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunType {
  Command,
  Execute,
  Probe,
  Analyze,
  Distill,
  Validate,
}

impl RunType {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Command => "command",
      Self::Execute => "execute",
      Self::Probe => "probe",
      Self::Analyze => "analyze",
      Self::Distill => "distill",
      Self::Validate => "validate",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceState {
  Running,
  Ended,
}

impl TraceState {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Running => "running",
      Self::Ended => "ended",
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceStatusCode {
  Unset,
  Ok,
  Error,
}

impl TraceStatusCode {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Unset => "unset",
      Self::Ok => "ok",
      Self::Error => "error",
    }
  }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TraceFailure {
  pub message: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RunRecordV1Alpha1 {
  pub api_version: String,
  pub run_id: RunId,
  pub trace_id: TraceId,
  pub run_type: RunType,
  pub state: TraceState,
  pub status_code: TraceStatusCode,
  pub started_at_millis: u128,
  pub finished_at_millis: Option<u128>,
  pub root_span_id: SpanId,
  pub attributes: BTreeMap<String, serde_json::Value>,
  pub summary: Option<String>,
  pub failure: Option<TraceFailure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpanRecordV1Alpha1 {
  pub api_version: String,
  pub span_id: SpanId,
  pub parent_span_id: Option<SpanId>,
  pub name: String,
  pub state: TraceState,
  pub status_code: TraceStatusCode,
  pub started_at_millis: u128,
  pub finished_at_millis: Option<u128>,
  pub attributes: BTreeMap<String, serde_json::Value>,
  pub summary: Option<String>,
  pub failure: Option<TraceFailure>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EventRecordV1Alpha1 {
  pub api_version: String,
  pub event_id: EventId,
  pub span_id: SpanId,
  pub name: String,
  pub timestamp_millis: u128,
  pub attributes: BTreeMap<String, serde_json::Value>,
  pub message: Option<String>,
  pub artifact_ids: Vec<ArtifactId>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ArtifactRecordV1Alpha1 {
  pub api_version: String,
  pub artifact_id: ArtifactId,
  pub span_id: SpanId,
  pub event_id: Option<EventId>,
  pub role: String,
  pub mime_type: String,
  pub path: String,
  pub sha256: Option<String>,
  pub attributes: BTreeMap<String, serde_json::Value>,
  pub summary: Option<String>,
}

pub fn new_run_id() -> RunId {
  let sequence = RUN_COUNTER.fetch_add(1, Ordering::Relaxed);
  RunId::new(format!(
    "run_{}_{}_{}",
    now_millis(),
    process::id(),
    sequence
  ))
}

pub fn new_trace_id() -> TraceId {
  let sequence = TRACE_COUNTER.fetch_add(1, Ordering::Relaxed);
  TraceId::new(format_trace_id(
    now_millis() as u64,
    process::id(),
    sequence,
  ))
}

pub fn new_span_id() -> SpanId {
  let sequence = SPAN_COUNTER.fetch_add(1, Ordering::Relaxed);
  SpanId::new(format!("{:016x}", sequence + 1))
}

pub fn new_event_id() -> EventId {
  let sequence = EVENT_COUNTER.fetch_add(1, Ordering::Relaxed);
  EventId::new(format!("event_{}_{}", now_millis(), sequence))
}

pub fn string_attr(value: impl Into<String>) -> serde_json::Value {
  serde_json::Value::String(value.into())
}

fn format_trace_id(timestamp_millis: u64, process_id: u32, sequence: u64) -> String {
  format!(
    "{:012x}{:08x}{:012x}",
    timestamp_millis & 0x0000_ffff_ffff_ffff,
    process_id,
    sequence & 0x0000_ffff_ffff_ffff
  )
}

#[cfg(test)]
mod tests {
  use super::*;
  use serde_json::json;

  #[test]
  fn api_versions_are_v1alpha1() {
    assert_eq!(RUN_API_VERSION, "auv.run.v1alpha1");
    assert_eq!(SPAN_API_VERSION, "auv.span.v1alpha1");
    assert_eq!(EVENT_API_VERSION, "auv.event.v1alpha1");
    assert_eq!(ARTIFACT_API_VERSION, "auv.artifact.v1alpha1");
  }

  #[test]
  fn generated_ids_are_prefixed_and_distinct() {
    let first_run = new_run_id();
    let second_run = new_run_id();
    let trace_id = new_trace_id();
    let span_id = new_span_id();
    let event_id = new_event_id();

    assert!(first_run.as_str().starts_with("run_"));
    assert_ne!(first_run, second_run);
    assert_eq!(trace_id.as_str().len(), 32);
    assert_eq!(span_id.as_str().len(), 16);
    assert!(event_id.as_str().starts_with("event_"));
  }

  #[test]
  fn trace_id_format_uses_timestamp_process_and_counter_bits() {
    let trace_id = format_trace_id(0x1234_5678_9abc, 0xdef0_1234, 0x5678_9abc_def0);

    assert_eq!(trace_id, "123456789abcdef0123456789abcdef0");
    assert_eq!(trace_id.len(), 32);
    assert!(
      trace_id
        .chars()
        .all(|character| character.is_ascii_hexdigit())
    );
    assert!(
      trace_id
        .chars()
        .all(|character| !character.is_ascii_uppercase())
    );
    assert_ne!(
      format_trace_id(0x1234_5678_9abc, 0xdef0_1234, 0),
      format_trace_id(0x1234_5678_9abc, 0xdef0_1235, 0)
    );
  }

  #[test]
  fn status_codes_match_otel_words() {
    assert_eq!(TraceStatusCode::Unset.as_str(), "unset");
    assert_eq!(TraceStatusCode::Ok.as_str(), "ok");
    assert_eq!(TraceStatusCode::Error.as_str(), "error");
  }

  #[test]
  fn run_record_serializes_versioned_json_contract() {
    let record = RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: RunId::new("run_contract"),
      trace_id: TraceId::new("123456789abcdef0123456789abcdef0"),
      run_type: RunType::Command,
      state: TraceState::Ended,
      status_code: TraceStatusCode::Ok,
      started_at_millis: 100,
      finished_at_millis: Some(200),
      root_span_id: SpanId::new("0000000000000001"),
      attributes: BTreeMap::from([("target".to_string(), string_attr("ExampleEditor"))]),
      summary: Some("completed".to_string()),
      failure: None,
    };

    let value = serde_json::to_value(&record).expect("run record should serialize");
    assert_eq!(
      value,
      json!({
        "api_version": "auv.run.v1alpha1",
        "run_id": "run_contract",
        "trace_id": "123456789abcdef0123456789abcdef0",
        "run_type": "command",
        "state": "ended",
        "status_code": "ok",
        "started_at_millis": 100,
        "finished_at_millis": 200,
        "root_span_id": "0000000000000001",
        "attributes": {
          "target": "ExampleEditor"
        },
        "summary": "completed",
        "failure": null
      })
    );

    let decoded: RunRecordV1Alpha1 =
      serde_json::from_value(value).expect("run record should deserialize");
    assert_eq!(decoded.api_version, RUN_API_VERSION);
    assert_eq!(decoded.run_id.as_str(), "run_contract");
    assert_eq!(decoded.run_type, RunType::Command);
    assert_eq!(decoded.state, TraceState::Ended);
    assert_eq!(decoded.status_code, TraceStatusCode::Ok);
  }

  #[test]
  fn span_record_serializes_versioned_json_contract() {
    let record = SpanRecordV1Alpha1 {
      api_version: SPAN_API_VERSION.to_string(),
      span_id: SpanId::new("0000000000000002"),
      parent_span_id: Some(SpanId::new("0000000000000001")),
      name: "driver.invoke".to_string(),
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: 110,
      finished_at_millis: None,
      attributes: BTreeMap::from([("driver".to_string(), string_attr("macos"))]),
      summary: None,
      failure: Some(TraceFailure {
        message: "pending".to_string(),
      }),
    };

    let value = serde_json::to_value(&record).expect("span record should serialize");
    assert_eq!(
      value,
      json!({
        "api_version": "auv.span.v1alpha1",
        "span_id": "0000000000000002",
        "parent_span_id": "0000000000000001",
        "name": "driver.invoke",
        "state": "running",
        "status_code": "unset",
        "started_at_millis": 110,
        "finished_at_millis": null,
        "attributes": {
          "driver": "macos"
        },
        "summary": null,
        "failure": {
          "message": "pending"
        }
      })
    );

    let decoded: SpanRecordV1Alpha1 =
      serde_json::from_value(value).expect("span record should deserialize");
    assert_eq!(decoded.api_version, SPAN_API_VERSION);
    assert_eq!(decoded.span_id.as_str(), "0000000000000002");
    assert_eq!(
      decoded.parent_span_id.expect("parent span").as_str(),
      "0000000000000001"
    );
    assert_eq!(decoded.state, TraceState::Running);
    assert_eq!(decoded.status_code, TraceStatusCode::Unset);
  }

  #[test]
  fn event_record_serializes_versioned_json_contract() {
    let record = EventRecordV1Alpha1 {
      api_version: EVENT_API_VERSION.to_string(),
      event_id: EventId::new("event_contract"),
      span_id: SpanId::new("0000000000000002"),
      name: "artifact.captured".to_string(),
      timestamp_millis: 120,
      attributes: BTreeMap::from([("kind".to_string(), string_attr("screenshot"))]),
      message: Some("captured screenshot".to_string()),
      artifact_ids: vec![ArtifactId::new("artifact_contract")],
    };

    let value = serde_json::to_value(&record).expect("event record should serialize");
    assert_eq!(
      value,
      json!({
        "api_version": "auv.event.v1alpha1",
        "event_id": "event_contract",
        "span_id": "0000000000000002",
        "name": "artifact.captured",
        "timestamp_millis": 120,
        "attributes": {
          "kind": "screenshot"
        },
        "message": "captured screenshot",
        "artifact_ids": ["artifact_contract"]
      })
    );

    let decoded: EventRecordV1Alpha1 =
      serde_json::from_value(value).expect("event record should deserialize");
    assert_eq!(decoded.api_version, EVENT_API_VERSION);
    assert_eq!(decoded.event_id.as_str(), "event_contract");
    assert_eq!(decoded.span_id.as_str(), "0000000000000002");
    assert_eq!(decoded.artifact_ids[0].as_str(), "artifact_contract");
  }

  #[test]
  fn artifact_record_serializes_versioned_json_contract() {
    let record = ArtifactRecordV1Alpha1 {
      api_version: ARTIFACT_API_VERSION.to_string(),
      artifact_id: ArtifactId::new("artifact_contract"),
      span_id: SpanId::new("0000000000000002"),
      event_id: Some(EventId::new("event_contract")),
      role: "driver.output".to_string(),
      mime_type: "text/plain".to_string(),
      path: "artifacts/output.txt".to_string(),
      sha256: Some("abc123".to_string()),
      attributes: BTreeMap::from([("encoding".to_string(), string_attr("utf-8"))]),
      summary: Some("output".to_string()),
    };

    let value = serde_json::to_value(&record).expect("artifact record should serialize");
    assert_eq!(
      value,
      json!({
        "api_version": "auv.artifact.v1alpha1",
        "artifact_id": "artifact_contract",
        "span_id": "0000000000000002",
        "event_id": "event_contract",
        "role": "driver.output",
        "mime_type": "text/plain",
        "path": "artifacts/output.txt",
        "sha256": "abc123",
        "attributes": {
          "encoding": "utf-8"
        },
        "summary": "output"
      })
    );

    let decoded: ArtifactRecordV1Alpha1 =
      serde_json::from_value(value).expect("artifact record should deserialize");
    assert_eq!(decoded.api_version, ARTIFACT_API_VERSION);
    assert_eq!(decoded.artifact_id.as_str(), "artifact_contract");
    assert_eq!(decoded.span_id.as_str(), "0000000000000002");
    assert_eq!(
      decoded.event_id.expect("artifact event").as_str(),
      "event_contract"
    );
  }
}
