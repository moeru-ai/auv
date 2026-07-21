use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use opentelemetry::Context;
use opentelemetry::InstrumentationScope;
use opentelemetry_sdk::error::{OTelSdkError, OTelSdkResult};
use opentelemetry_sdk::logs::{LogBatch, LogExporter, LogProcessor, SdkLogRecord};
use opentelemetry_sdk::trace::{Span, SpanData, SpanExporter, SpanProcessor};

pub const MAX_EXPORTED_ITEMS: usize = 64;

#[derive(Clone, Debug, Default)]
pub struct BoundedSpanExporter {
  state: Arc<SpanExporterState>,
}

#[derive(Debug, Default)]
struct SpanExporterState {
  spans: Mutex<Vec<SpanData>>,
  shutdowns: AtomicUsize,
}

impl BoundedSpanExporter {
  pub fn spans(&self) -> Vec<SpanData> {
    self.state.spans.lock().unwrap().clone()
  }

  pub fn shutdown_count(&self) -> usize {
    self.state.shutdowns.load(Ordering::SeqCst)
  }
}

impl SpanExporter for BoundedSpanExporter {
  async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
    let mut spans = self.state.spans.lock().map_err(|_| OTelSdkError::InternalFailure("bounded span exporter state poisoned".into()))?;
    if spans.len() + batch.len() > MAX_EXPORTED_ITEMS {
      return Err(OTelSdkError::InternalFailure("bounded span exporter capacity exceeded".into()));
    }
    spans.extend(batch);
    Ok(())
  }

  fn shutdown_with_timeout(&self, _timeout: std::time::Duration) -> OTelSdkResult {
    self.state.shutdowns.fetch_add(1, Ordering::SeqCst);
    Ok(())
  }
}

#[derive(Clone, Debug)]
pub struct FlushProbeSpanProcessor {
  state: Arc<FlushProbeState>,
}

impl FlushProbeSpanProcessor {
  pub fn failing() -> Self {
    let processor = Self {
      state: Arc::new(FlushProbeState::default()),
    };
    processor.state.fail_flush.store(true, Ordering::SeqCst);
    processor
  }

  pub fn force_flush_count(&self) -> usize {
    self.state.force_flushes.load(Ordering::SeqCst)
  }

  pub fn shutdown_count(&self) -> usize {
    self.state.shutdowns.load(Ordering::SeqCst)
  }
}

impl SpanProcessor for FlushProbeSpanProcessor {
  fn on_start(&self, _span: &mut Span, _context: &Context) {}

  fn on_end(&self, _span: SpanData) {}

  fn force_flush(&self) -> OTelSdkResult {
    self.state.force_flushes.fetch_add(1, Ordering::SeqCst);
    if self.state.fail_flush.load(Ordering::SeqCst) {
      Err(OTelSdkError::InternalFailure("test span flush failure".into()))
    } else {
      Ok(())
    }
  }

  fn shutdown_with_timeout(&self, _timeout: std::time::Duration) -> OTelSdkResult {
    self.state.shutdowns.fetch_add(1, Ordering::SeqCst);
    Ok(())
  }
}

#[derive(Clone, Debug, Default)]
pub struct BoundedLogExporter {
  state: Arc<LogExporterState>,
}

#[derive(Debug, Default)]
struct LogExporterState {
  logs: Mutex<Vec<SdkLogRecord>>,
  shutdowns: AtomicUsize,
}

impl BoundedLogExporter {
  pub fn logs(&self) -> Vec<SdkLogRecord> {
    self.state.logs.lock().unwrap().clone()
  }

  pub fn shutdown_count(&self) -> usize {
    self.state.shutdowns.load(Ordering::SeqCst)
  }
}

impl LogExporter for BoundedLogExporter {
  async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
    let mut logs = self.state.logs.lock().map_err(|_| OTelSdkError::InternalFailure("bounded log exporter state poisoned".into()))?;
    let batch = batch.iter().map(|(record, _)| record.clone()).collect::<Vec<_>>();
    if logs.len() + batch.len() > MAX_EXPORTED_ITEMS {
      return Err(OTelSdkError::InternalFailure("bounded log exporter capacity exceeded".into()));
    }
    logs.extend(batch);
    Ok(())
  }

  fn shutdown_with_timeout(&self, _timeout: std::time::Duration) -> OTelSdkResult {
    self.state.shutdowns.fetch_add(1, Ordering::SeqCst);
    Ok(())
  }
}

#[derive(Clone, Debug)]
pub struct FlushProbeLogProcessor {
  state: Arc<FlushProbeState>,
}

#[derive(Debug, Default)]
struct FlushProbeState {
  force_flushes: AtomicUsize,
  shutdowns: AtomicUsize,
  fail_flush: AtomicBool,
}

impl FlushProbeLogProcessor {
  pub fn failing() -> Self {
    let processor = Self {
      state: Arc::new(FlushProbeState::default()),
    };
    processor.state.fail_flush.store(true, Ordering::SeqCst);
    processor
  }

  pub fn force_flush_count(&self) -> usize {
    self.state.force_flushes.load(Ordering::SeqCst)
  }

  pub fn shutdown_count(&self) -> usize {
    self.state.shutdowns.load(Ordering::SeqCst)
  }
}

impl LogProcessor for FlushProbeLogProcessor {
  fn emit(&self, _data: &mut SdkLogRecord, _instrumentation: &InstrumentationScope) {}

  fn force_flush(&self) -> OTelSdkResult {
    self.state.force_flushes.fetch_add(1, Ordering::SeqCst);
    if self.state.fail_flush.load(Ordering::SeqCst) {
      Err(OTelSdkError::InternalFailure("test log flush failure".into()))
    } else {
      Ok(())
    }
  }

  fn shutdown_with_timeout(&self, _timeout: std::time::Duration) -> OTelSdkResult {
    self.state.shutdowns.fetch_add(1, Ordering::SeqCst);
    Ok(())
  }
}
