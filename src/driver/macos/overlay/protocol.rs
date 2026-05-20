use serde::{Deserialize, Serialize};

use super::super::*;

#[derive(Debug, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum OverlayDaemonCommand {
  ShowCursor { x: f64, y: f64, label: String },
  HideCursor,
  Shutdown,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct OverlayDaemonAck {
  pub(crate) ok: bool,
  pub(crate) event: String,
  #[serde(default)]
  pub(crate) error: Option<String>,
}

pub(crate) fn serialize_overlay_command(command: &OverlayDaemonCommand) -> AuvResult<String> {
  serde_json::to_string(command)
    .map_err(|error| format!("failed to encode overlay command: {error}"))
}

pub(crate) fn parse_overlay_ack(raw: &str) -> AuvResult<OverlayDaemonAck> {
  serde_json::from_str(raw.trim()).map_err(|error| {
    format!(
      "failed to parse overlay daemon ack {:?}: {error}",
      raw.trim()
    )
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn show_cursor_payload_is_stable_json() {
    let payload = serialize_overlay_command(&OverlayDaemonCommand::ShowCursor {
      x: 600.0,
      y: 300.5,
      label: "AUV".to_string(),
    })
    .expect("payload should encode");

    assert_eq!(
      payload,
      r#"{"type":"show_cursor","x":600.0,"y":300.5,"label":"AUV"}"#
    );
  }

  #[test]
  fn hide_cursor_payload_is_stable_json() {
    let payload =
      serialize_overlay_command(&OverlayDaemonCommand::HideCursor).expect("payload should encode");

    assert_eq!(payload, r#"{"type":"hide_cursor"}"#);
  }

  #[test]
  fn parse_overlay_ack_accepts_success() {
    let ack = parse_overlay_ack(r#"{"ok":true,"event":"shown"}"#).expect("ack should parse");

    assert!(ack.ok);
    assert_eq!(ack.event, "shown");
    assert_eq!(ack.error, None);
  }
}
