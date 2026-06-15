// File: src/driver/macos/control/text.rs
use auv_driver::TextSubmit;

use super::super::support::runtime::paste_text_preserving_clipboard;
use super::super::support::{
  artifacts::{build_text_artifact, render_type_text_report, sanitize_file_component},
  call::{
    app_identifier, optional_bool, optional_non_empty_string, optional_positive_u64,
    required_non_empty_string,
  },
};
use super::super::{DriverCall, DriverResponse, now_millis};
use super::common::activate_app_if_needed;
use crate::model::AuvResult;

pub(super) fn clipboard_restore_signals(
  restored: bool,
) -> std::collections::BTreeMap<String, String> {
  std::collections::BTreeMap::from([("clipboard.restored".to_string(), restored.to_string())])
}

fn paste_text_signals(
  restored: bool,
  input_bridge: &str,
  input_bridge_reason: &str,
) -> std::collections::BTreeMap<String, String> {
  let mut signals = clipboard_restore_signals(restored);
  signals.insert("input.bridge".to_string(), input_bridge.to_string());
  signals.insert(
    "input.bridge.reason".to_string(),
    input_bridge_reason.to_string(),
  );
  signals
}

fn input_action_signals(
  outcome: &super::super::typed::session::InputActionBridgeOutcome,
) -> std::collections::BTreeMap<String, String> {
  let mut signals = std::collections::BTreeMap::new();
  signals.insert("input.bridge".to_string(), outcome.input_bridge.to_string());
  signals.insert(
    "input.bridge.selectedPath".to_string(),
    outcome.selected_path.to_string(),
  );
  signals.insert(
    "input.bridge.policy".to_string(),
    outcome.input_policy.to_string(),
  );
  if let Some(reason) = &outcome.fallback_reason {
    signals.insert("input.bridge.fallbackReason".to_string(), reason.clone());
  }
  signals
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
  let bridge_outcome = super::super::typed::session::type_text_bridge(
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
    backend: Some(format!(
      "macos.input.type-text.{}",
      bridge_outcome.input_bridge
    )),
    signals: input_action_signals(&bridge_outcome),
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
  let bridge_outcome = super::super::typed::session::paste_text_preserve_clipboard_bridge(
    &text,
    replace_existing,
    typed_submit_for_legacy_submit_key(submit_key.as_deref()),
    submit_settle_ms,
  )?;
  if bridge_outcome.needs_legacy_fallback() {
    paste_text_preserving_clipboard(
      &text,
      replace_existing,
      submit_key.as_deref(),
      submit_settle_ms,
    )?;
  }
  let input_bridge = bridge_outcome.input_bridge();
  let input_bridge_reason = bridge_outcome.reason();

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
      format!("inputBridge={input_bridge}"),
      format!("inputBridgeReason={input_bridge_reason}"),
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
    format!("inputBridge={input_bridge}"),
    format!("inputBridgeReason={input_bridge_reason}"),
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
    backend: Some(format!(
      "macos.input.paste-text-preserve-clipboard.{input_bridge}"
    )),
    signals: paste_text_signals(true, input_bridge, input_bridge_reason),
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
  let bridge_outcome = super::super::typed::session::press_key_bridge(&key, settle_ms)?;

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
    backend: Some(format!(
      "macos.input.press-key.{}",
      bridge_outcome.input_bridge
    )),
    signals: input_action_signals(&bridge_outcome),
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

fn typed_submit_for_legacy_submit_key(submit_key: Option<&str>) -> Option<TextSubmit> {
  match submit_key.map(|value| value.trim().to_ascii_lowercase()) {
    None => Some(TextSubmit::No),
    Some(submit) if submit.is_empty() => Some(TextSubmit::No),
    Some(submit) if submit == "return" => Some(TextSubmit::Return),
    _ => None,
  }
}

#[cfg(test)]
mod tests {
  use auv_driver::TextSubmit;

  use super::{
    clipboard_restore_signals, input_action_signals, paste_text_signals,
    should_activate_text_input, typed_submit_for_legacy_submit_key,
  };
  use crate::model::{DriverCall, ExecutionTarget};
  use std::collections::BTreeMap;
  use std::path::PathBuf;

  #[test]
  fn clipboard_restore_signals_uses_structured_namespace() {
    let signals = clipboard_restore_signals(true);

    assert_eq!(signals.get("clipboard.restored"), Some(&"true".to_string()));
  }

  #[test]
  fn paste_text_signals_include_input_bridge() {
    let signals = paste_text_signals(true, "typed-session", "typed-submit-supported");

    assert_eq!(signals.get("clipboard.restored"), Some(&"true".to_string()));
    assert_eq!(
      signals.get("input.bridge"),
      Some(&"typed-session".to_string())
    );
    assert_eq!(
      signals.get("input.bridge.reason"),
      Some(&"typed-submit-supported".to_string())
    );
  }

  #[test]
  fn input_action_signals_include_typed_bridge_metadata() {
    let outcome = crate::driver::macos::typed::session::InputActionBridgeOutcome {
      input_bridge: "typed-session",
      selected_path: "foreground_system_events",
      input_policy: "foreground_preferred",
      fallback_reason: None,
    };

    let signals = input_action_signals(&outcome);

    assert_eq!(
      signals.get("input.bridge"),
      Some(&"typed-session".to_string())
    );
    assert_eq!(
      signals.get("input.bridge.selectedPath"),
      Some(&"foreground_system_events".to_string())
    );
    assert_eq!(
      signals.get("input.bridge.policy"),
      Some(&"foreground_preferred".to_string())
    );
    assert!(!signals.contains_key("input.bridge.fallbackReason"));
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

  #[test]
  fn typed_submit_mapping_uses_safe_shared_subset_only() {
    assert_eq!(
      typed_submit_for_legacy_submit_key(None),
      Some(TextSubmit::No)
    );
    assert_eq!(
      typed_submit_for_legacy_submit_key(Some("return")),
      Some(TextSubmit::Return)
    );
    assert_eq!(
      typed_submit_for_legacy_submit_key(Some(" Return ")),
      Some(TextSubmit::Return)
    );
  }

  #[test]
  fn typed_submit_mapping_falls_back_for_legacy_only_submit_keys() {
    assert_eq!(typed_submit_for_legacy_submit_key(Some("enter")), None);
    assert_eq!(typed_submit_for_legacy_submit_key(Some("tab")), None);
    assert_eq!(typed_submit_for_legacy_submit_key(Some("space")), None);
  }

  fn build_call<const N: usize>(entries: [(&str, &str); N]) -> DriverCall {
    DriverCall {
      operation: "paste_text_preserve_clipboard".to_string(),
      target: ExecutionTarget {
        application_id: Some("com.netease.163music".to_string()),
        target_label: None,
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
