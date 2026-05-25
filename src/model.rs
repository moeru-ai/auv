// File: src/model.rs
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::trace::{ArtifactRecordV1Alpha1, SpanId};

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
  /// Future RPC method family this command projects into. Set explicitly per
  /// command rather than derived from the id so re-namings don't silently
  /// reshuffle the protocol surface. See [`CommandNamespace`].
  pub namespace: CommandNamespace,
}

/// Future RPC method family for a [`CommandSpec`]. AUV today exposes most
/// capability through `debug.*` command ids, which is fine for development
/// but is the wrong long-term surface. The namespace tag is the bridge: each
/// existing command declares which native RPC family it belongs to, so a
/// future RPC server can route by namespace + method without rewriting the
/// catalog or guessing from id prefixes.
///
/// **Provisional.** The taxonomy may grow; pure metadata today, no behavior
/// change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CommandNamespace {
  /// Read-only observation of the device surface (capture, find, list, probe,
  /// project, identify, wait, fixture).
  Observe,
  /// Mutating input or focus change (click, type, scroll-as-input, press,
  /// paste, activate, focus).
  Action,
  /// Assertion / verification commands.
  Verify,
  /// Multi-page structured observation (reserved; today scroll_scan is a
  /// runtime function, not a catalog command).
  Scan,
  /// Visual cursor / overlay presentation. Trust signal only; no semantic
  /// effect on the target app.
  Overlay,
  /// Domain-typed workflow that consumes structured candidates/evidence
  /// rather than dumping raw artifacts (e.g. `music.result.play`).
  Domain,
  /// Test fixture command. Should never appear in a production catalog.
  Test,
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
  pub producer_span_id: SpanId,
  pub status: RunStatus,
  pub output_summary: String,
  pub signals: BTreeMap<String, String>,
  pub artifacts: Vec<ArtifactRecordV1Alpha1>,
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
///
/// `device_id` / `session_id` identify the automation target and namespace. Today
/// they default to `"local"` / `"default"` since AUV runs only on the local
/// macOS host with one implicit session — drivers can ignore them. The fields
/// exist so future RPC/JS-SDK frontends can route to remote/VM devices and
/// scope state per session without changing the driver contract again.
#[derive(Clone, Debug)]
pub struct DriverRunContext {
  pub run_id: String,
  pub span_id: String,
  pub device_id: String,
  pub session_id: String,
}

impl Default for DriverRunContext {
  fn default() -> Self {
    Self {
      run_id: String::new(),
      span_id: String::new(),
      device_id: "local".to_string(),
      session_id: "default".to_string(),
    }
  }
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

pub fn now_millis() -> u64 {
  u64::try_from(
    SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .unwrap_or_default()
      .as_millis(),
  )
  .unwrap_or(u64::MAX)
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
