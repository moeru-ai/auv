use crate::{BoxFuture, DispatchFailure, ErrorCode, TraceRecord};

/// Receives trace records for a lossy external telemetry representation.
///
/// Exporters are separate from [`crate::TracingStore`]: a store preserves the
/// AUV record, while an exporter may intentionally omit payloads or attributes.
pub trait TraceExporter: Send + Sync {
  fn export(&self, record: TraceRecord) -> BoxFuture<'_, Result<(), ExportError>>;
  fn flush(&self) -> BoxFuture<'_, Result<(), ExportError>>;
}

/// Reports an external telemetry export failure.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("trace export failed: {code}")]
pub struct ExportError {
  code: ErrorCode,
}

impl ExportError {
  pub fn new(code: ErrorCode) -> Self {
    Self { code }
  }
  pub fn code(&self) -> &ErrorCode {
    &self.code
  }
}

/// Receives asynchronous instrumentation failures for diagnostics only.
pub trait DispatchErrorReporter: Send + Sync {
  fn report(&self, failure: &DispatchFailure);
}
