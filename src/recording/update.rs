//! Run update events + camelCase HTTP wire shapes.
//!
//! `RunUpdate` is the canonical in-process event (uses internal record types).
//! `Api*Record` + `ApiRunUpdate` are the camelCase wire shapes for the inspect
//! server HTTP write API. The `From` impls convert between them, with
//! `api_millis()` clamping u128 timestamps to u64.

use crate::trace::{
  ArtifactId, ArtifactRecordV1Alpha1, EventRecordV1Alpha1, RunId, RunRecordV1Alpha1, SpanId,
  SpanRecordV1Alpha1,
};

#[derive(Clone, Debug, PartialEq)]
pub enum RunUpdate {
  RunStarted {
    run_id: RunId,
    run: RunRecordV1Alpha1,
  },
  SpanStarted {
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  EventAppended {
    run_id: RunId,
    event: EventRecordV1Alpha1,
  },
  ArtifactCreated {
    run_id: RunId,
    artifact: ArtifactRecordV1Alpha1,
  },
  SpanFinished {
    run_id: RunId,
    span: SpanRecordV1Alpha1,
  },
  RunFinished {
    run_id: RunId,
    run: RunRecordV1Alpha1,
  },
}

impl RunUpdate {
  pub fn run_id(&self) -> &RunId {
    match self {
      Self::RunStarted { run_id, .. }
      | Self::SpanStarted { run_id, .. }
      | Self::EventAppended { run_id, .. }
      | Self::ArtifactCreated { run_id, .. }
      | Self::SpanFinished { run_id, .. }
      | Self::RunFinished { run_id, .. } => run_id,
    }
  }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiRunRecord {
  pub api_version: String,
  pub run_id: RunId,
  pub trace_id: crate::trace::TraceId,
  pub run_type: crate::trace::RunType,
  pub state: crate::trace::TraceState,
  pub status_code: crate::trace::TraceStatusCode,
  pub started_at_millis: u64,
  pub finished_at_millis: Option<u64>,
  pub root_span_id: SpanId,
  pub attributes: crate::run_builder::Attributes,
  pub summary: Option<String>,
  pub failure: Option<crate::trace::TraceFailure>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiSpanRecord {
  pub api_version: String,
  pub span_id: SpanId,
  pub parent_span_id: Option<SpanId>,
  pub name: String,
  pub state: crate::trace::TraceState,
  pub status_code: crate::trace::TraceStatusCode,
  pub started_at_millis: u64,
  pub finished_at_millis: Option<u64>,
  pub attributes: crate::run_builder::Attributes,
  pub summary: Option<String>,
  pub failure: Option<crate::trace::TraceFailure>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiEventRecord {
  pub api_version: String,
  pub event_id: crate::trace::EventId,
  pub span_id: SpanId,
  pub name: String,
  pub timestamp_millis: u64,
  pub attributes: crate::run_builder::Attributes,
  pub message: Option<String>,
  pub artifact_ids: Vec<ArtifactId>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiArtifactRecord {
  pub api_version: String,
  pub artifact_id: ArtifactId,
  pub span_id: SpanId,
  pub event_id: Option<crate::trace::EventId>,
  pub role: String,
  pub mime_type: String,
  pub path: String,
  pub sha256: Option<String>,
  pub attributes: crate::run_builder::Attributes,
  pub summary: Option<String>,
}

impl From<RunRecordV1Alpha1> for ApiRunRecord {
  fn from(record: RunRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      run_id: record.run_id,
      trace_id: record.trace_id,
      run_type: record.run_type,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: api_millis(record.started_at_millis),
      finished_at_millis: record.finished_at_millis.map(api_millis),
      root_span_id: record.root_span_id,
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<ApiRunRecord> for RunRecordV1Alpha1 {
  fn from(record: ApiRunRecord) -> Self {
    Self {
      api_version: record.api_version,
      run_id: record.run_id,
      trace_id: record.trace_id,
      run_type: record.run_type,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: u128::from(record.started_at_millis),
      finished_at_millis: record.finished_at_millis.map(u128::from),
      root_span_id: record.root_span_id,
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<SpanRecordV1Alpha1> for ApiSpanRecord {
  fn from(record: SpanRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      span_id: record.span_id,
      parent_span_id: record.parent_span_id,
      name: record.name,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: api_millis(record.started_at_millis),
      finished_at_millis: record.finished_at_millis.map(api_millis),
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<ApiSpanRecord> for SpanRecordV1Alpha1 {
  fn from(record: ApiSpanRecord) -> Self {
    Self {
      api_version: record.api_version,
      span_id: record.span_id,
      parent_span_id: record.parent_span_id,
      name: record.name,
      state: record.state,
      status_code: record.status_code,
      started_at_millis: u128::from(record.started_at_millis),
      finished_at_millis: record.finished_at_millis.map(u128::from),
      attributes: record.attributes,
      summary: record.summary,
      failure: record.failure,
    }
  }
}

impl From<EventRecordV1Alpha1> for ApiEventRecord {
  fn from(record: EventRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      event_id: record.event_id,
      span_id: record.span_id,
      name: record.name,
      timestamp_millis: api_millis(record.timestamp_millis),
      attributes: record.attributes,
      message: record.message,
      artifact_ids: record.artifact_ids,
    }
  }
}

impl From<ApiEventRecord> for EventRecordV1Alpha1 {
  fn from(record: ApiEventRecord) -> Self {
    Self {
      api_version: record.api_version,
      event_id: record.event_id,
      span_id: record.span_id,
      name: record.name,
      timestamp_millis: u128::from(record.timestamp_millis),
      attributes: record.attributes,
      message: record.message,
      artifact_ids: record.artifact_ids,
    }
  }
}

fn api_millis(value: u128) -> u64 {
  u64::try_from(value).unwrap_or(u64::MAX)
}

impl From<ArtifactRecordV1Alpha1> for ApiArtifactRecord {
  fn from(record: ArtifactRecordV1Alpha1) -> Self {
    Self {
      api_version: record.api_version,
      artifact_id: record.artifact_id,
      span_id: record.span_id,
      event_id: record.event_id,
      role: record.role,
      mime_type: record.mime_type,
      path: record.path,
      sha256: record.sha256,
      attributes: record.attributes,
      summary: record.summary,
    }
  }
}

impl From<ApiArtifactRecord> for ArtifactRecordV1Alpha1 {
  fn from(record: ApiArtifactRecord) -> Self {
    Self {
      api_version: record.api_version,
      artifact_id: record.artifact_id,
      span_id: record.span_id,
      event_id: record.event_id,
      role: record.role,
      mime_type: record.mime_type,
      path: record.path,
      sha256: record.sha256,
      attributes: record.attributes,
      summary: record.summary,
    }
  }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ApiRunUpdate {
  RunStarted {
    #[serde(rename = "runId")]
    run_id: RunId,
    run: ApiRunRecord,
  },
  SpanStarted {
    #[serde(rename = "runId")]
    run_id: RunId,
    span: ApiSpanRecord,
  },
  EventAppended {
    #[serde(rename = "runId")]
    run_id: RunId,
    event: ApiEventRecord,
  },
  ArtifactCreated {
    #[serde(rename = "runId")]
    run_id: RunId,
    artifact: ApiArtifactRecord,
  },
  SpanFinished {
    #[serde(rename = "runId")]
    run_id: RunId,
    span: ApiSpanRecord,
  },
  RunFinished {
    #[serde(rename = "runId")]
    run_id: RunId,
    run: ApiRunRecord,
  },
}

impl From<RunUpdate> for ApiRunUpdate {
  fn from(update: RunUpdate) -> Self {
    match update {
      RunUpdate::RunStarted { run_id, run } => Self::RunStarted {
        run_id,
        run: run.into(),
      },
      RunUpdate::SpanStarted { run_id, span } => Self::SpanStarted {
        run_id,
        span: span.into(),
      },
      RunUpdate::EventAppended { run_id, event } => Self::EventAppended {
        run_id,
        event: event.into(),
      },
      RunUpdate::ArtifactCreated { run_id, artifact } => Self::ArtifactCreated {
        run_id,
        artifact: artifact.into(),
      },
      RunUpdate::SpanFinished { run_id, span } => Self::SpanFinished {
        run_id,
        span: span.into(),
      },
      RunUpdate::RunFinished { run_id, run } => Self::RunFinished {
        run_id,
        run: run.into(),
      },
    }
  }
}

impl From<ApiRunUpdate> for RunUpdate {
  fn from(update: ApiRunUpdate) -> Self {
    match update {
      ApiRunUpdate::RunStarted { run_id, run } => Self::RunStarted {
        run_id,
        run: run.into(),
      },
      ApiRunUpdate::SpanStarted { run_id, span } => Self::SpanStarted {
        run_id,
        span: span.into(),
      },
      ApiRunUpdate::EventAppended { run_id, event } => Self::EventAppended {
        run_id,
        event: event.into(),
      },
      ApiRunUpdate::ArtifactCreated { run_id, artifact } => Self::ArtifactCreated {
        run_id,
        artifact: artifact.into(),
      },
      ApiRunUpdate::SpanFinished { run_id, span } => Self::SpanFinished {
        run_id,
        span: span.into(),
      },
      ApiRunUpdate::RunFinished { run_id, run } => Self::RunFinished {
        run_id,
        run: run.into(),
      },
    }
  }
}

#[cfg(test)]
mod tests {
  use crate::trace::{
    RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SpanId, TraceId, TraceState,
    TraceStatusCode,
  };

  use super::{ApiRunUpdate, RunUpdate};

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
      attributes: Default::default(),
      summary: None,
      failure: None,
    }
  }

  #[test]
  fn run_update_serializes_public_shape_as_camel_case() {
    let update = RunUpdate::RunStarted {
      run_id: RunId::new("run_update_test"),
      run: test_run(),
    };

    let value = serde_json::to_value(ApiRunUpdate::from(update)).expect("update should serialize");
    assert_eq!(value["type"], "runStarted");
    assert_eq!(value["runId"], "run_update_test");
    assert_eq!(value["run"]["apiVersion"], "auv.run.v1alpha1");
    assert_eq!(value["run"]["rootSpanId"], "0000000000000001");
    assert!(value["run"].get("root_span_id").is_none());
  }
}
