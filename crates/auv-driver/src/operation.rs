#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OperationDisturbance {
  None,
  Focus,
  ForegroundApp,
  Keyboard,
  Clipboard,
  Pointer,
}

impl OperationDisturbance {
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

  pub fn parse(raw: &str) -> Result<Self, String> {
    match raw.trim() {
      "none" => Ok(Self::None),
      "focus" => Ok(Self::Focus),
      "foreground_app" => Ok(Self::ForegroundApp),
      "keyboard" => Ok(Self::Keyboard),
      "clipboard" => Ok(Self::Clipboard),
      "pointer" => Ok(Self::Pointer),
      other => Err(format!(
        "unknown operation disturbance {other:?}; expected one of none, focus, foreground_app, keyboard, clipboard, pointer"
      )),
    }
  }
}

#[derive(Clone, Debug)]
pub struct OperationSpec {
  pub id: &'static str,
  pub summary: &'static str,
  pub driver_id: &'static str,
  pub operation: &'static str,
  pub disturbance_classes: &'static [OperationDisturbance],
  pub max_disturbance: OperationDisturbance,
  /// Future RPC method family this operation projects into. Set explicitly per
  /// operation rather than derived from ids so renamings do not silently
  /// reshuffle the protocol surface. See [`OperationNamespace`].
  pub namespace: OperationNamespace,
}

/// Future RPC method family for an [`OperationSpec`]. Generic invoke exposes
/// capability-oriented command ids such as `display.*`, `window.*`, `input.*`,
/// `app.*`, `overlay.*`, and `mediaControl.*`, but this namespace is driver
/// metadata: it records the operation family independently from any CLI id.
///
/// **Provisional.** The taxonomy may grow; pure metadata today, no behavior
/// change.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationNamespace {
  /// Read-only observation of the device surface (capture, find, list, probe,
  /// project, identify, wait, fixture).
  Observe,
  /// Mutating input or focus change (click, type, scroll-as-input, press,
  /// paste, activate, focus).
  Action,
  /// Assertion / verification operations.
  Verify,
  /// Multi-page structured observation (reserved; today scroll_scan is a
  /// runtime function, not an invoke command).
  Scan,
  /// Visual cursor / overlay presentation. Trust signal only; no semantic
  /// effect on the target app.
  Overlay,
  /// Domain-typed workflow that consumes structured candidates/evidence.
  /// Not part of the generic invoke registry.
  Domain,
  /// Test fixture operation. Should not be exposed as production metadata.
  Test,
}

#[cfg(test)]
mod tests {
  use super::OperationDisturbance;

  #[test]
  fn operation_disturbance_parses_known_values() {
    assert_eq!(
      OperationDisturbance::parse("clipboard").expect("clipboard should parse"),
      OperationDisturbance::Clipboard
    );
    assert_eq!(
      OperationDisturbance::parse("pointer").expect("pointer should parse"),
      OperationDisturbance::Pointer
    );
    assert!(OperationDisturbance::parse("network").is_err());
  }
}
