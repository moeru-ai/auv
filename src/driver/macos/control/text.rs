// File: src/driver/macos/control/text.rs
use super::super::*;
use super::common::activate_app_if_needed;

pub(super) fn clipboard_restore_signals(
  restored: bool,
) -> std::collections::BTreeMap<String, String> {
  std::collections::BTreeMap::from([("clipboard.restored".to_string(), restored.to_string())])
}

pub(crate) fn type_text(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let text = required_non_empty_string(call, "text")?;
  let replace_existing = optional_bool(call, "replace_existing")?.unwrap_or(false);
  let submit_key = optional_non_empty_string(call, "submit_key");
  let submit_settle_ms = optional_positive_u64(call, "submit_settle_ms")?.unwrap_or(0);
  let activate = should_activate_text_input(call)?;

  if activate {
    activate_app_if_needed(&app)?;
  }
  type_text_via_system_events(
    &text,
    replace_existing,
    submit_key.as_deref(),
    submit_settle_ms,
  )?;

  let report = render_type_text_report(&app, &text, replace_existing, submit_key.as_deref());
  let artifact = build_text_artifact(
    "type-text",
    "txt",
    &format!("type-text-{}", sanitize_file_component(&text)),
    report,
    "Typed text into the active macOS control through System Events.",
  )?;

  let mut notes = vec![
    format!("text={text}"),
    format!("textLength={}", text.chars().count()),
    format!("replaceExisting={replace_existing}"),
    format!("activatedApp={activate}"),
  ];
  if !app.is_empty() {
    notes.push(format!("app={app}"));
  }
  if let Some(submit_key) = submit_key.as_deref() {
    notes.push(format!("submitKey={submit_key}"));
  }
  if submit_settle_ms > 0 {
    notes.push(format!("submitSettleMs={submit_settle_ms}"));
  }

  Ok(DriverResponse {
    summary: match submit_key.as_deref() {
      Some(submit_key) => format!(
        "Typed {} character(s) into {} and submitted with {}.",
        text.chars().count(),
        if app.is_empty() {
          "the active app"
        } else {
          &app
        },
        submit_key
      ),
      None => format!(
        "Typed {} character(s) into {}.",
        text.chars().count(),
        if app.is_empty() {
          "the active app"
        } else {
          &app
        }
      ),
    },
    backend: Some("macos.system-events.type-text".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes,
    artifacts: vec![artifact],
  })
}

pub(crate) fn paste_text_preserve_clipboard(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let text = required_non_empty_string(call, "text")?;
  let replace_existing = optional_bool(call, "replace_existing")?.unwrap_or(false);
  let submit_key = optional_non_empty_string(call, "submit_key");
  let submit_settle_ms = optional_positive_u64(call, "submit_settle_ms")?.unwrap_or(0);
  let activate = should_activate_text_input(call)?;

  if activate {
    activate_app_if_needed(&app)?;
  }
  paste_text_preserving_clipboard(
    &text,
    replace_existing,
    submit_key.as_deref(),
    submit_settle_ms,
  )?;

  let artifact = build_text_artifact(
    "paste-text-preserve-clipboard",
    "txt",
    &format!(
      "paste-text-preserve-clipboard-{}",
      sanitize_file_component(&text)
    ),
    [
      format!("pastedAt={}", now_millis()),
      format!("app={app}"),
      format!("text={text}"),
      format!("textLength={}", text.chars().count()),
      format!("replaceExisting={replace_existing}"),
      format!("submitKey={}", submit_key.as_deref().unwrap_or("n/a")),
      format!("submitSettleMs={submit_settle_ms}"),
      format!("activatedApp={activate}"),
      "clipboardRestored=true".to_string(),
    ]
    .join("\n"),
    "Pasted text through the macOS clipboard, then restored the prior clipboard snapshot.",
  )?;

  let mut notes = vec![
    format!("text={text}"),
    format!("textLength={}", text.chars().count()),
    format!("replaceExisting={replace_existing}"),
    format!("activatedApp={activate}"),
    "clipboardRestored=true".to_string(),
  ];
  if !app.is_empty() {
    notes.push(format!("app={app}"));
  }
  if let Some(submit_key) = submit_key.as_deref() {
    notes.push(format!("submitKey={submit_key}"));
  }
  if submit_settle_ms > 0 {
    notes.push(format!("submitSettleMs={submit_settle_ms}"));
  }

  Ok(DriverResponse {
    summary: match submit_key.as_deref() {
      Some(submit_key) => format!(
        "Pasted {} character(s) into {} and submitted with {} while restoring the clipboard.",
        text.chars().count(),
        if app.is_empty() {
          "the active app"
        } else {
          &app
        },
        submit_key
      ),
      None => format!(
        "Pasted {} character(s) into {} while restoring the clipboard.",
        text.chars().count(),
        if app.is_empty() {
          "the active app"
        } else {
          &app
        }
      ),
    },
    backend: Some("macos.system-events.paste-text-preserve-clipboard".to_string()),
    signals: clipboard_restore_signals(true),
    notes,
    artifacts: vec![artifact],
  })
}

pub(crate) fn press_key(call: &DriverCall) -> AuvResult<DriverResponse> {
  let app = app_identifier(call).unwrap_or_default();
  let key = required_non_empty_string(call, "key")?;
  let settle_ms = optional_positive_u64(call, "settle_ms")?.unwrap_or(0);
  let activate = should_activate_text_input(call)?;

  if activate {
    activate_app_if_needed(&app)?;
  }
  send_key_input(&key, settle_ms)?;

  let artifact = build_text_artifact(
    "press-key",
    "txt",
    &format!("press-key-{}", sanitize_file_component(&key)),
    [
      format!("pressedAt={}", now_millis()),
      format!("app={app}"),
      format!("key={key}"),
      format!("settleMs={settle_ms}"),
      format!("activatedApp={activate}"),
    ]
    .join("\n"),
    "Pressed a keyboard key or shortcut through System Events.",
  )?;
  Ok(DriverResponse {
    summary: format!(
      "Pressed key {} in {}.",
      key,
      if app.is_empty() {
        "the active app"
      } else {
        &app
      }
    ),
    backend: Some("macos.system-events.press-key".to_string()),
    signals: std::collections::BTreeMap::new(),
    notes: vec![
      format!("key={key}"),
      format!("settleMs={settle_ms}"),
      format!("app={app}"),
      format!("activatedApp={activate}"),
    ],
    artifacts: vec![artifact],
  })
}

pub(super) fn should_activate_text_input(call: &DriverCall) -> AuvResult<bool> {
  Ok(optional_bool(call, "activate")?.unwrap_or(true))
}

#[cfg(test)]
mod tests {
  use super::{clipboard_restore_signals, should_activate_text_input};
  use crate::model::{DriverCall, ExecutionTarget};
  use std::collections::BTreeMap;
  use std::path::PathBuf;

  #[test]
  fn clipboard_restore_signals_uses_structured_namespace() {
    let signals = clipboard_restore_signals(true);

    assert_eq!(signals.get("clipboard.restored"), Some(&"true".to_string()));
  }

  #[test]
  fn text_input_activation_defaults_to_true() {
    let call = build_call([]);

    assert!(should_activate_text_input(&call).unwrap());
  }

  #[test]
  fn text_input_activation_can_be_disabled() {
    let call = build_call([("activate", "false")]);

    assert!(!should_activate_text_input(&call).unwrap());
  }

  fn build_call<const N: usize>(entries: [(&str, &str); N]) -> DriverCall {
    DriverCall {
      operation: "paste_text_preserve_clipboard".to_string(),
      target: ExecutionTarget {
        application_id: Some("com.netease.163music".to_string()),
      },
      inputs: entries
        .into_iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect::<BTreeMap<_, _>>(),
      working_directory: PathBuf::from("."),
      run_context: crate::model::DriverRunContext::default(),
    }
  }
}
