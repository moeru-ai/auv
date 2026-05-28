// File: src/driver/macos/support/runtime.rs
use std::env;
use std::fs;
use std::io::ErrorKind;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::Duration;

use super::super::*;

pub(crate) fn require_macos() -> AuvResult<()> {
  if env::consts::OS != "macos" {
    return Err("macos.desktop is only available on macOS".to_string());
  }

  Ok(())
}

pub(crate) fn activate_target_app(app: &str) -> AuvResult<()> {
  let command = if looks_like_bundle_identifier(app) {
    format!(
      "tell application id {} to activate",
      osascript_string_literal(app)
    )
  } else {
    format!(
      "tell application {} to activate",
      osascript_string_literal(app)
    )
  };
  let args = vec!["-e".to_string(), command];
  run_command(OSASCRIPT_BINARY, &args).map(|_| ())
}

pub(crate) struct ClipboardLock {
  path: PathBuf,
}

impl Drop for ClipboardLock {
  fn drop(&mut self) {
    let _ = fs::remove_file(&self.path);
  }
}

pub(crate) fn acquire_clipboard_lock(timeout_ms: u64) -> AuvResult<ClipboardLock> {
  let path = env::temp_dir().join("auv-macos-clipboard.lock");
  let started_at = now_millis();

  loop {
    match fs::OpenOptions::new()
      .write(true)
      .create_new(true)
      .open(&path)
    {
      Ok(mut file) => {
        let _ = writeln!(file, "pid={}", std::process::id());
        let _ = writeln!(file, "acquiredAt={}", started_at);
        return Ok(ClipboardLock { path });
      }
      Err(error) if error.kind() == ErrorKind::AlreadyExists => {
        clear_stale_lock_file(&path)?;
        if now_millis().saturating_sub(started_at) > timeout_ms {
          let owner = describe_lock_owner(&path).unwrap_or_else(|_| "unknown owner".to_string());
          return Err(format!(
            "timed out waiting for the global macOS clipboard lock after {} ms ({owner}; path={})",
            timeout_ms,
            path.display()
          ));
        }
        thread::sleep(Duration::from_millis(50));
      }
      Err(error) => {
        return Err(format!(
          "failed to acquire the global macOS clipboard lock {}: {error}",
          path.display()
        ));
      }
    }
  }
}

pub(crate) fn clear_stale_lock_file(path: &Path) -> AuvResult<()> {
  let Some(owner_pid) = read_lock_owner_pid(path)? else {
    return Ok(());
  };
  if process_is_alive(owner_pid) {
    return Ok(());
  }

  fs::remove_file(path).map_err(|error| {
    format!(
      "failed to clear stale lock {} owned by pid {}: {error}",
      path.display(),
      owner_pid
    )
  })?;
  Ok(())
}

pub(crate) fn read_lock_owner_pid(path: &Path) -> AuvResult<Option<u32>> {
  let content = match fs::read_to_string(path) {
    Ok(content) => content,
    Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
    Err(error) => {
      return Err(format!(
        "failed to read lock file {}: {error}",
        path.display()
      ));
    }
  };

  for line in content.lines() {
    if let Some(raw_pid) = line.trim().strip_prefix("pid=") {
      let pid = raw_pid
        .trim()
        .parse::<u32>()
        .map_err(|error| format!("invalid pid entry in lock file {}: {error}", path.display()))?;
      return Ok(Some(pid));
    }
  }

  Ok(None)
}

pub(crate) fn describe_lock_owner(path: &Path) -> AuvResult<String> {
  let content = match fs::read_to_string(path) {
    Ok(content) => content,
    Err(error) if error.kind() == ErrorKind::NotFound => {
      return Ok("lock file disappeared".to_string());
    }
    Err(error) => {
      return Err(format!(
        "failed to read lock file {}: {error}",
        path.display()
      ));
    }
  };

  let mut fragments = Vec::new();
  for line in content.lines() {
    let trimmed = line.trim();
    if !trimmed.is_empty() {
      fragments.push(trimmed.to_string());
    }
  }
  if fragments.is_empty() {
    Ok("empty lock file".to_string())
  } else {
    Ok(fragments.join(", "))
  }
}

pub(crate) fn process_is_alive(pid: u32) -> bool {
  if pid == 0 {
    return false;
  }

  let status = Command::new("/bin/kill")
    .args(["-0", &pid.to_string()])
    .status();
  matches!(status, Ok(status) if status.success())
}

pub(crate) fn capture_clipboard_snapshot() -> AuvResult<String> {
  auv_driver_macos::native::clipboard::capture_clipboard_snapshot()
}

pub(crate) fn restore_clipboard_snapshot(snapshot_payload: &str) -> AuvResult<()> {
  auv_driver_macos::native::clipboard::restore_clipboard_snapshot(snapshot_payload)
}

pub(crate) fn set_clipboard_text(text: &str) -> AuvResult<()> {
  auv_driver_macos::native::clipboard::set_clipboard_text(text)
}

pub(crate) fn type_text_via_system_events(
  text: &str,
  replace_existing: bool,
  submit_key: Option<&str>,
  submit_settle_ms: u64,
) -> AuvResult<()> {
  let mut lines = vec!["tell application \"System Events\"".to_string()];
  if replace_existing {
    lines.push("keystroke \"a\" using {command down}".to_string());
    lines.push("delay 0.05".to_string());
    lines.push("key code 51".to_string());
    lines.push("delay 0.05".to_string());
  }
  push_text_keystroke_lines(&mut lines, text);
  if let Some(submit_key) = submit_key {
    let key_code = special_key_code(submit_key)?;
    lines.push("delay 0.05".to_string());
    lines.push(format!("key code {key_code}"));
  }
  lines.push("end tell".to_string());
  run_osascript_lines(&lines)?;
  if submit_settle_ms > 0 {
    thread::sleep(Duration::from_millis(submit_settle_ms));
  }
  Ok(())
}

pub(crate) fn push_text_keystroke_lines(lines: &mut Vec<String>, text: &str) {
  for character in text.chars() {
    lines.push(format!(
      "keystroke {}",
      osascript_string_literal(&character.to_string())
    ));
    lines.push("delay 0.02".to_string());
  }
}

pub(crate) fn paste_text_preserving_clipboard(
  text: &str,
  replace_existing: bool,
  submit_key: Option<&str>,
  submit_settle_ms: u64,
) -> AuvResult<()> {
  let _clipboard_lock = acquire_clipboard_lock(5_000)?;
  let clipboard_snapshot = capture_clipboard_snapshot()?;
  let action_result = (|| {
    set_clipboard_text(text)?;
    let mut lines = vec!["tell application \"System Events\"".to_string()];
    if replace_existing {
      lines.push("keystroke \"a\" using {command down}".to_string());
      lines.push("delay 0.05".to_string());
      lines.push("key code 51".to_string());
      lines.push("delay 0.05".to_string());
    }
    lines.push("keystroke \"v\" using {command down}".to_string());
    lines.push("delay 0.15".to_string());
    if let Some(submit_key) = submit_key {
      let key_code = special_key_code(submit_key)?;
      lines.push("delay 0.05".to_string());
      lines.push(format!("key code {key_code}"));
    }
    lines.push("end tell".to_string());
    run_osascript_lines(&lines)?;
    if submit_settle_ms > 0 {
      thread::sleep(Duration::from_millis(submit_settle_ms));
    }
    Ok(())
  })();
  let restore_result = restore_clipboard_snapshot(&clipboard_snapshot);

  match (action_result, restore_result) {
    (Ok(()), Ok(())) => Ok(()),
    (Err(action_error), Ok(())) => Err(action_error),
    (Ok(()), Err(restore_error)) => Err(format!(
      "restored pasted text action but failed to restore clipboard: {restore_error}"
    )),
    (Err(action_error), Err(restore_error)) => Err(format!(
      "{action_error}; additionally failed to restore clipboard: {restore_error}"
    )),
  }
}

pub(crate) fn special_key_code(raw: &str) -> AuvResult<u32> {
  match raw.trim().to_ascii_lowercase().as_str() {
    "return" => Ok(36),
    "enter" => Ok(76),
    "tab" => Ok(48),
    "delete" | "backspace" => Ok(51),
    "escape" | "esc" => Ok(53),
    "space" => Ok(49),
    other => Err(format!(
      "invalid submit key {}; supported values are return, enter, tab, delete, backspace, escape, and space",
      other
    )),
  }
}

pub(crate) fn run_osascript_lines(lines: &[String]) -> AuvResult<CommandOutput> {
  let mut args = Vec::with_capacity(lines.len() * 2);
  for line in lines {
    args.push("-e".to_string());
    args.push(line.clone());
  }
  run_command(OSASCRIPT_BINARY, &args)
}

pub(crate) fn send_key_input(key: &str, settle_ms: u64) -> AuvResult<()> {
  if key.contains('+') {
    send_shortcut(key)?;
  } else if let Ok(key_code) = special_key_code(key) {
    run_osascript_lines(&[
      "tell application \"System Events\"".to_string(),
      format!("key code {key_code}"),
      "end tell".to_string(),
    ])?;
  } else if key.chars().count() == 1 {
    run_osascript_lines(&[format!(
      "tell application \"System Events\" to keystroke {}",
      osascript_string_literal(key)
    )])?;
  } else {
    return Err(format!(
      "invalid key {}; use a special key like Return, a shortcut like cmd+f, or debug.typeText for multi-character text",
      key
    ));
  }

  if settle_ms > 0 {
    thread::sleep(Duration::from_millis(settle_ms));
  }
  Ok(())
}

pub(crate) fn send_shortcut(shortcut: &str) -> AuvResult<()> {
  let parsed = parse_shortcut(shortcut)?;
  let line = if parsed.modifiers.is_empty() {
    format!(
      "tell application \"System Events\" to keystroke {}",
      osascript_string_literal(&parsed.key)
    )
  } else {
    format!(
      "tell application \"System Events\" to keystroke {} using {{{}}}",
      osascript_string_literal(&parsed.key),
      parsed.modifiers.join(", ")
    )
  };
  run_osascript_lines(&[line]).map(|_| ())
}

#[derive(Debug)]
pub(crate) struct ParsedShortcut {
  pub(crate) key: String,
  pub(crate) modifiers: Vec<&'static str>,
}

pub(crate) fn parse_shortcut(shortcut: &str) -> AuvResult<ParsedShortcut> {
  let raw_parts = shortcut
    .split('+')
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>();
  if raw_parts.len() < 2 {
    return Err(format!(
      "invalid shortcut {}; expected a form like cmd+f or cmd+shift+p",
      shortcut
    ));
  }

  let key = raw_parts
    .last()
    .map(|value| value.to_ascii_lowercase())
    .ok_or_else(|| format!("invalid shortcut {}; missing key", shortcut))?;
  if key.chars().count() != 1 {
    return Err(format!(
      "invalid shortcut {}; only single-character keys are currently supported",
      shortcut
    ));
  }

  let mut modifiers = Vec::new();
  for raw_modifier in &raw_parts[..raw_parts.len() - 1] {
    let modifier = match raw_modifier.to_ascii_lowercase().as_str() {
      "cmd" | "command" => "command down",
      "shift" => "shift down",
      "alt" | "option" => "option down",
      "ctrl" | "control" => "control down",
      other => {
        return Err(format!(
          "invalid shortcut {}; unsupported modifier {}",
          shortcut, other
        ));
      }
    };
    if !modifiers.contains(&modifier) {
      modifiers.push(modifier);
    }
  }

  Ok(ParsedShortcut { key, modifiers })
}
