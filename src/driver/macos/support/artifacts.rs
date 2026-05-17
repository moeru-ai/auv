use std::env;
use std::fs;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use super::super::*;

static TEMP_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) fn run_swift_script(source: &str) -> AuvResult<String> {
  let script_path = temp_file_path("swift-script", "swift");
  fs::write(&script_path, source).map_err(|error| {
    format!(
      "failed to write Swift script {}: {error}",
      script_path.display()
    )
  })?;

  let result = run_swift_script_with_fallback(&script_path);
  let _ = fs::remove_file(&script_path);
  result
}

pub(crate) fn run_swift_script_with_fallback(script_path: &PathBuf) -> AuvResult<String> {
  let xcrun_args = vec!["swift".to_string(), script_path.display().to_string()];
  match run_command(XCRUN_BINARY, &xcrun_args) {
    Ok(output) => Ok(output.stdout),
    Err(error) if error.contains("failed to spawn xcrun") => {
      let swift_args = vec![script_path.display().to_string()];
      Ok(run_command("swift", &swift_args)?.stdout)
    }
    Err(error) => Err(error),
  }
}

pub(crate) fn run_command(binary: &str, args: &[String]) -> AuvResult<CommandOutput> {
  let output = std::process::Command::new(binary)
    .args(args)
    .output()
    .map_err(|error| match error.kind() {
      ErrorKind::NotFound => format!("failed to spawn {}: command not found", binary),
      _ => format!("failed to spawn {}: {}", binary, error),
    })?;

  let stdout = String::from_utf8_lossy(&output.stdout).to_string();
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

  Ok(CommandOutput { stdout })
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

pub(crate) struct CommandOutput {
  pub(crate) stdout: String,
}
