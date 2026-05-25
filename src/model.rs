// File: src/model.rs
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub type AuvResult<T> = Result<T, String>;

static RUN_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum DisturbanceClass {
  None,
  Focus,
  ForegroundApp,
  Keyboard,
  Clipboard,
  Pointer,
}

impl DisturbanceClass {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::None => "none",
      Self::Focus => "focus",
      Self::ForegroundApp => "foreground_app",
      Self::Keyboard => "keyboard",
      Self::Clipboard => "clipboard",
      Self::Pointer => "pointer",
    }
  }

  pub fn parse(raw: &str) -> AuvResult<Self> {
    match raw.trim() {
      "none" => Ok(Self::None),
      "focus" => Ok(Self::Focus),
      "foreground_app" => Ok(Self::ForegroundApp),
      "keyboard" => Ok(Self::Keyboard),
      "clipboard" => Ok(Self::Clipboard),
      "pointer" => Ok(Self::Pointer),
      other => Err(format!(
        "unknown disturbance class {other:?}; expected one of none, focus, foreground_app, keyboard, clipboard, pointer"
      )),
    }
  }
}

#[derive(Clone, Debug)]
pub struct CommandSpec {
  pub id: &'static str,
  pub summary: &'static str,
  pub driver_id: &'static str,
  pub operation: &'static str,
  pub disturbance_classes: &'static [DisturbanceClass],
  pub max_disturbance: DisturbanceClass,
}

#[derive(Clone, Debug, Default)]
pub struct ExecutionTarget {
  pub application_id: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InvokeRequest {
  pub command_id: String,
  pub target: ExecutionTarget,
  pub inputs: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStatus {
  Completed,
  Failed,
}

impl RunStatus {
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Completed => "completed",
      Self::Failed => "failed",
    }
  }
}

#[derive(Clone, Debug)]
pub struct InvokeResult {
  pub run_id: String,
  pub status: RunStatus,
  pub output_summary: String,
  pub signals: BTreeMap<String, String>,
  pub artifact_paths: Vec<PathBuf>,
  pub failure_message: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DriverDescriptor {
  pub id: &'static str,
  pub summary: &'static str,
  pub capabilities: &'static [&'static str],
  pub donor_boundary: &'static str,
}

/// Control-plane metadata the runtime injects into every `DriverCall` so drivers can
/// build evidence `ArtifactRef`s and emit `OperationResult`s tied to the active run/span
/// without smuggling identifiers through the user-facing `inputs` map.
#[derive(Clone, Debug, Default)]
pub struct DriverRunContext {
  pub run_id: String,
  pub span_id: String,
}

#[derive(Clone, Debug)]
pub struct DriverCall {
  pub operation: String,
  pub target: ExecutionTarget,
  pub inputs: BTreeMap<String, String>,
  pub working_directory: PathBuf,
  pub run_context: DriverRunContext,
}

#[derive(Clone, Debug)]
pub struct ProducedArtifact {
  pub kind: String,
  pub source_path: PathBuf,
  pub preferred_name: String,
  pub note: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DriverResponse {
  pub summary: String,
  pub backend: Option<String>,
  pub signals: BTreeMap<String, String>,
  pub notes: Vec<String>,
  pub artifacts: Vec<ProducedArtifact>,
}

pub fn now_millis() -> u128 {
  SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()
    .as_millis()
}

pub fn new_run_id() -> String {
  let sequence = RUN_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
  format!("run_{}_{}_{}", now_millis(), process::id(), sequence)
}

#[cfg(test)]
mod tests {
  use super::{DisturbanceClass, new_run_id};

  #[test]
  fn new_run_id_is_unique_within_process() {
    let first = new_run_id();
    let second = new_run_id();

    assert_ne!(first, second);
  }

  #[test]
  fn disturbance_class_parses_known_values() {
    assert_eq!(
      DisturbanceClass::parse("clipboard").expect("clipboard should parse"),
      DisturbanceClass::Clipboard
    );
    assert_eq!(
      DisturbanceClass::parse("pointer").expect("pointer should parse"),
      DisturbanceClass::Pointer
    );
  }
}
