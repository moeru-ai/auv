use std::time::Duration;

use auv_driver::{
  Driver as TypedDriver, InputPolicy, PasteTextOptions, Point, Scroll, ScrollOptions, TextSubmit,
};

use crate::model::AuvResult;

// Explicit compatibility shims from the legacy root driver into
// `auv-driver-macos`.
//
// The legacy `macos.desktop` command adapter remains the default runtime
// surface. When it needs to borrow typed crate behavior, keep that borrowing
// centralized here instead of letting command handlers talk to
// `auv-driver-macos` directly.

pub(crate) mod descriptor {
  pub(crate) fn legacy_descriptor_metadata() -> auv_driver_macos::MacosLegacyDescriptorMetadata {
    auv_driver_macos::macos_legacy_descriptor_metadata()
  }
}

pub(crate) mod observe {
  pub(crate) use auv_driver_macos::observe::{
    find_ax_text_node, ocr_detection_signals, permission_probe_report, preferred_ax_signal_text,
    render_window_list_json, render_window_snapshot_report, row_detection_signals,
    verify_ax_text_signals, verify_now_playing_title_signals, wait_ocr_detection_signals,
    wait_row_detection_signals,
  };
}

pub(crate) mod session {
  use super::*;

  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub(crate) enum PasteTextBridgeOutcome {
    UsedTypedSession,
    NeedsLegacyFallback { reason: &'static str },
  }

  #[derive(Clone, Copy, Debug, PartialEq, Eq)]
  pub(crate) struct ScrollPointBridgeOutcome {
    pub(crate) input_bridge: &'static str,
    pub(crate) selected_path: &'static str,
    pub(crate) input_policy: &'static str,
  }

  impl ScrollPointBridgeOutcome {
    pub(crate) fn used_typed_session(policy: InputPolicy) -> Self {
      Self {
        input_bridge: "typed-session",
        selected_path: "foreground_system_events",
        input_policy: input_policy_name(policy),
      }
    }
  }

  impl PasteTextBridgeOutcome {
    pub(crate) fn input_bridge(self) -> &'static str {
      match self {
        Self::UsedTypedSession => "typed-session",
        Self::NeedsLegacyFallback { .. } => "legacy-clipboard",
      }
    }

    pub(crate) fn reason(self) -> &'static str {
      match self {
        Self::UsedTypedSession => "typed-submit-supported",
        Self::NeedsLegacyFallback { reason } => reason,
      }
    }

    pub(crate) fn needs_legacy_fallback(self) -> bool {
      matches!(self, Self::NeedsLegacyFallback { .. })
    }
  }

  pub(crate) fn paste_text_preserve_clipboard_bridge(
    text: &str,
    replace_existing: bool,
    submit: Option<TextSubmit>,
    submit_settle_ms: u64,
  ) -> AuvResult<PasteTextBridgeOutcome> {
    let Some(submit) = submit else {
      return Ok(PasteTextBridgeOutcome::NeedsLegacyFallback {
        reason: "unsupported-submit-key",
      });
    };
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    session
      .input()
      .paste_text(PasteTextOptions {
        text: text.to_string(),
        replace_existing,
        submit,
        settle: Duration::from_millis(submit_settle_ms),
      })
      .map_err(|error| format!("typed macOS paste_text adapter failed: {error}"))?;
    Ok(PasteTextBridgeOutcome::UsedTypedSession)
  }

  pub(crate) fn scroll_point_bridge(
    x: f64,
    y: f64,
    delta_x: f64,
    delta_y: f64,
    policy: InputPolicy,
    settle_ms: u64,
  ) -> AuvResult<ScrollPointBridgeOutcome> {
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    session
      .input()
      .scroll_at(
        Point::new(x, y),
        Scroll::new(delta_x, delta_y),
        ScrollOptions {
          policy,
          settle: Duration::from_millis(settle_ms),
        },
      )
      .map_err(|error| format!("typed macOS scroll_at adapter failed: {error}"))?;
    Ok(ScrollPointBridgeOutcome::used_typed_session(policy))
  }

  pub(crate) fn input_policy_name(policy: InputPolicy) -> &'static str {
    match policy {
      InputPolicy::BackgroundOnly => "background_only",
      InputPolicy::BackgroundPreferred => "background_preferred",
      InputPolicy::ForegroundPreferred => "foreground_preferred",
    }
  }

  #[cfg(test)]
  mod tests {
    use auv_driver::InputPolicy;

    use super::{PasteTextBridgeOutcome, ScrollPointBridgeOutcome};

    #[test]
    fn paste_text_bridge_outcome_exposes_signal_values() {
      let typed = PasteTextBridgeOutcome::UsedTypedSession;
      assert_eq!(typed.input_bridge(), "typed-session");
      assert_eq!(typed.reason(), "typed-submit-supported");
      assert!(!typed.needs_legacy_fallback());

      let fallback = PasteTextBridgeOutcome::NeedsLegacyFallback {
        reason: "unsupported-submit-key",
      };
      assert_eq!(fallback.input_bridge(), "legacy-clipboard");
      assert_eq!(fallback.reason(), "unsupported-submit-key");
      assert!(fallback.needs_legacy_fallback());
    }

    #[test]
    fn scroll_point_bridge_outcome_exposes_signal_values() {
      let outcome = ScrollPointBridgeOutcome::used_typed_session(InputPolicy::ForegroundPreferred);

      assert_eq!(outcome.input_bridge, "typed-session");
      assert_eq!(outcome.selected_path, "foreground_system_events");
      assert_eq!(outcome.input_policy, "foreground_preferred");
    }
  }
}
