use std::sync::{Arc, Mutex};

use opentelemetry_sdk::error::{OTelSdkError, OTelSdkResult};
use opentelemetry_sdk::logs::{LogBatch, LogExporter, SdkLogRecord};
use opentelemetry_sdk::trace::{SpanData, SpanExporter};

#[derive(Clone, Debug, Default)]
pub struct BoundedSpanExporter(Arc<Mutex<Vec<SpanData>>>);

impl BoundedSpanExporter {
  pub fn spans(&self) -> Vec<SpanData> {
    self.0.lock().unwrap().clone()
  }
}

impl SpanExporter for BoundedSpanExporter {
  async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
    self.0.lock().map_err(|_| OTelSdkError::InternalFailure("span exporter state poisoned".into()))?.extend(batch);
    Ok(())
  }
}

#[derive(Clone, Debug, Default)]
pub struct BoundedLogExporter(Arc<Mutex<Vec<SdkLogRecord>>>);

impl BoundedLogExporter {
  pub fn logs(&self) -> Vec<SdkLogRecord> {
    self.0.lock().unwrap().clone()
  }
}

impl LogExporter for BoundedLogExporter {
  async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
    self
      .0
      .lock()
      .map_err(|_| OTelSdkError::InternalFailure("log exporter state poisoned".into()))?
      .extend(batch.iter().map(|(record, _)| record.clone()));
    Ok(())
  }
}
