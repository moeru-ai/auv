// File: src/driver/macos/support/artifacts.rs
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::super::*;
use crate::contract::ArtifactRef;
use crate::model::DriverRunContext;
use crate::trace::{ArtifactId, RunId, SpanId};

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn run_command(binary: &str, args: &[String]) -> AuvResult<CommandOutput> {
  let output = std::process::Command::new(binary)
    .args(args)
    .output()
    .map_err(|error| match error.kind() {
      ErrorKind::NotFound => format!("failed to spawn {}: command not found", binary),
      _ => format!("failed to spawn {}: {}", binary, error),
    })?;

  let stderr = String::from_utf8_lossy(&output.stderr).to_string();

  if !output.status.success() {
    let trimmed_stderr = stderr.trim();
    return Err(format!(
      "{} exited with status {}: {}",
      binary,
      output.status,
      if trimmed_stderr.is_empty() {
        "no stderr output"
      } else {
        trimmed_stderr
      }
    ));
  }

  Ok(CommandOutput {})
}

pub(crate) fn temp_file_path(label: &str, extension: &str) -> PathBuf {
  let sequence = TEMP_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
  env::temp_dir().join(format!(
    "auv-{}-{}-{}-{}.{}",
    sanitize_file_component(label),
    now_millis(),
    std::process::id(),
    sequence,
    extension
  ))
}

pub(crate) fn build_text_artifact(
  kind: &str,
  extension: &str,
  label: &str,
  content: String,
  note: &str,
) -> AuvResult<ProducedArtifact> {
  let source_path = temp_file_path(label, extension);
  fs::write(&source_path, content).map_err(|error| {
    format!(
      "failed to write artifact source {}: {error}",
      source_path.display()
    )
  })?;

  Ok(ProducedArtifact {
    kind: kind.to_string(),
    source_path,
    preferred_name: format!("{}.{}", sanitize_file_component(label), extension),
    note: Some(note.to_string()),
  })
}

pub(crate) fn screenshot_temp_path(label: &str) -> PathBuf {
  temp_file_path(label, "png")
}

pub(crate) fn render_type_text_report(
  app: &str,
  text: &str,
  replace_existing: bool,
  submit_key: Option<&str>,
) -> String {
  let mut lines = vec![
    format!("typedAt={}", now_millis()),
    format!("app={app}"),
    format!("text={text}"),
    format!("textLength={}", text.chars().count()),
    format!("replaceExisting={replace_existing}"),
  ];
  if let Some(submit_key) = submit_key {
    lines.push(format!("submitKey={submit_key}"));
  }
  lines.join("\n")
}

pub(crate) fn render_activate_app_report(app: &str, settle_ms: u64) -> String {
  [
    format!("activatedAt={}", now_millis()),
    format!("app={app}"),
    format!("settleMs={settle_ms}"),
  ]
  .join("\n")
}

pub(crate) fn looks_like_bundle_identifier(raw: &str) -> bool {
  raw.contains('.')
    && raw
      .chars()
      .all(|character| character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_'))
}

pub(crate) fn osascript_string_literal(raw: &str) -> String {
  let mut escaped = String::from("\"");
  for character in raw.chars() {
    match character {
      '\\' => escaped.push_str("\\\\"),
      '"' => escaped.push_str("\\\""),
      _ => escaped.push(character),
    }
  }
  escaped.push('"');
  escaped
}

pub(crate) fn launch_host_process() -> String {
  env::args()
    .next()
    .map(PathBuf::from)
    .as_ref()
    .and_then(|value| value.file_name())
    .and_then(|value| value.to_str())
    .unwrap_or("auv-cli")
    .to_string()
}

#[cfg(test)]
pub(crate) fn swift_string_literal(raw: &str) -> String {
  let mut escaped = String::from("\"");
  for character in raw.chars() {
    match character {
      '\\' => escaped.push_str("\\\\"),
      '"' => escaped.push_str("\\\""),
      '\n' => escaped.push_str("\\n"),
      '\r' => escaped.push_str("\\r"),
      '\t' => escaped.push_str("\\t"),
      _ => escaped.push(character),
    }
  }
  escaped.push('"');
  escaped
}

pub(crate) fn sanitize_file_component(raw: &str) -> String {
  let sanitized = raw
    .chars()
    .map(|character| match character {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
      _ => '-',
    })
    .collect::<String>()
    .trim_matches('-')
    .to_string();

  if sanitized.is_empty() {
    "artifact".to_string()
  } else {
    sanitized
  }
}

pub(crate) fn copy_file(source: &PathBuf, destination: &PathBuf) -> AuvResult<()> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create artifact directory {}: {error}",
        parent.display()
      )
    })?;
  }

  fs::copy(source, destination).map_err(|error| {
    format!(
      "failed to copy artifact from {} to {}: {error}",
      source.display(),
      destination.display()
    )
  })?;

  Ok(())
}

pub(crate) fn sanitized_artifact_name(raw: &str) -> String {
  sanitize_file_component(raw)
}

pub(crate) struct CommandOutput {}

/// Builds the `artifacts` vector for a `DriverResponse` while exposing typed
/// `ArtifactRef`s pointing at the IDs the runtime will assign once it stages
/// the response.
///
/// The runtime stages artifacts in order, minting IDs as `artifact_{:04}`
/// based on 1-indexed position. This builder ties ref minting to push position
/// so drivers can embed cross-artifact `ArtifactRef`s (e.g. evidence refs in
/// `OperationResult` JSON or `recognition_result_ref` in candidate evidence)
/// without hardcoding string constants like `"artifact_0001"` that drift
/// silently whenever the response shape changes.
pub(crate) struct DriverArtifactBuilder {
  run_context: DriverRunContext,
  artifacts: Vec<ProducedArtifact>,
}

impl DriverArtifactBuilder {
  pub(crate) fn new(run_context: &DriverRunContext) -> Self {
    Self {
      run_context: run_context.clone(),
      artifacts: Vec::new(),
    }
  }

  /// Returns the `ArtifactRef` the artifact at the given 0-indexed slot will
  /// receive after the runtime stages the response. Lets callers refer
  /// forward to artifacts before pushing them — necessary when building
  /// content that embeds refs pointing at later artifacts in the same
  /// response (e.g. an `OperationResult` whose evidence list cites a
  /// recognition artifact pushed after it).
  ///
  /// Callers are responsible for matching push order to reserved slots;
  /// `push` returns the same ref so the round-trip is checkable.
  pub(crate) fn ref_at(&self, slot: usize) -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new(self.run_context.run_id.as_str()),
      artifact_id: ArtifactId::new(&format!("artifact_{:04}", slot + 1)),
      span_id: SpanId::new(self.run_context.span_id.as_str()),
      captured_event_id: None,
    }
  }

  /// Pushes an artifact and returns its `ArtifactRef`. Equivalent to
  /// `ref_at(self.len())` before the push.
  pub(crate) fn push(&mut self, artifact: ProducedArtifact) -> ArtifactRef {
    let slot = self.artifacts.len();
    self.artifacts.push(artifact);
    self.ref_at(slot)
  }

  pub(crate) fn into_vec(self) -> Vec<ProducedArtifact> {
    self.artifacts
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn driver_artifact_builder_assigns_sequential_ids() {
    let ctx = DriverRunContext {
      run_id: "run_42".to_string(),
      span_id: "span_7".to_string(),
    };
    let mut builder = DriverArtifactBuilder::new(&ctx);
    let first = builder.push(ProducedArtifact {
      kind: "screenshot".to_string(),
      source_path: PathBuf::from("/tmp/a.png"),
      preferred_name: "a.png".to_string(),
      note: None,
    });
    let second = builder.push(ProducedArtifact {
      kind: "report".to_string(),
      source_path: PathBuf::from("/tmp/b.txt"),
      preferred_name: "b.txt".to_string(),
      note: None,
    });

    assert_eq!(first.artifact_id.as_str(), "artifact_0001");
    assert_eq!(second.artifact_id.as_str(), "artifact_0002");
    assert_eq!(first.run_id.as_str(), "run_42");
    assert_eq!(first.span_id.as_str(), "span_7");
    assert_eq!(builder.into_vec().len(), 2);
  }

  #[test]
  fn driver_artifact_builder_ref_at_matches_push_position() {
    let ctx = DriverRunContext::default();
    let mut builder = DriverArtifactBuilder::new(&ctx);
    // Forward-reference a slot before pushing it.
    let forward = builder.ref_at(2);
    builder.push(ProducedArtifact {
      kind: "x".to_string(),
      source_path: PathBuf::from("/tmp/0.txt"),
      preferred_name: "0.txt".to_string(),
      note: None,
    });
    builder.push(ProducedArtifact {
      kind: "y".to_string(),
      source_path: PathBuf::from("/tmp/1.txt"),
      preferred_name: "1.txt".to_string(),
      note: None,
    });
    let actual = builder.push(ProducedArtifact {
      kind: "z".to_string(),
      source_path: PathBuf::from("/tmp/2.txt"),
      preferred_name: "2.txt".to_string(),
      note: None,
    });
    assert_eq!(forward.artifact_id, actual.artifact_id);
  }
}
