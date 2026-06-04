use std::time::Duration;

use auv_driver::{
  ClickOptions, Driver as TypedDriver, InputPolicy, KeyPressOptions, PasteTextOptions, Point,
  Scroll, ScrollOptions, TextSubmit, TypeTextOptions, WindowClickStrategy, WindowPoint,
};

use crate::model::AuvResult;

// Explicit compatibility shims from the legacy root driver into
// `auv-driver-macos`.
//
// The legacy `macos.desktop` command adapter remains the default runtime
// surface. When it needs to borrow typed crate behavior, keep that borrowing
// centralized here instead of letting command handlers talk to
// `auv-driver-macos` directly.
//
// TODO(remove-legacy-driver-call-adapter): delete these compatibility shims
// when `Runtime::invoke` no longer routes macOS commands through legacy
// `DriverCall` handlers and can open typed `auv-driver` sessions directly.

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

  #[derive(Clone, Debug, PartialEq, Eq)]
  pub(crate) struct InputActionBridgeOutcome {
    pub(crate) input_bridge: &'static str,
    pub(crate) selected_path: &'static str,
    pub(crate) input_policy: &'static str,
    pub(crate) fallback_reason: Option<String>,
  }

  impl InputActionBridgeOutcome {
    pub(crate) fn from_result(policy: InputPolicy, result: &auv_driver::InputActionResult) -> Self {
      Self {
        input_bridge: "typed-session",
        selected_path: selected_path_name(result.selected_path),
        input_policy: input_policy_name(policy),
        fallback_reason: result.fallback_reason.clone(),
      }
    }
  }

  impl ScrollPointBridgeOutcome {
    pub(crate) fn from_result(policy: InputPolicy, result: &auv_driver::InputActionResult) -> Self {
      let outcome = InputActionBridgeOutcome::from_result(policy, result);
      Self {
        input_bridge: outcome.input_bridge,
        selected_path: outcome.selected_path,
        input_policy: outcome.input_policy,
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

  pub(crate) fn type_text_bridge(
    text: &str,
    replace_existing: bool,
    submit_key: Option<&str>,
    submit_settle_ms: u64,
  ) -> AuvResult<InputActionBridgeOutcome> {
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    let type_settle = if submit_key.is_none() {
      Duration::from_millis(submit_settle_ms)
    } else {
      Duration::ZERO
    };
    let result = session
      .input()
      .type_text(
        text,
        TypeTextOptions {
          policy: InputPolicy::ForegroundPreferred,
          replace_existing,
          submit: TextSubmit::No,
          inter_char_delay: Duration::from_millis(20),
          allow_clipboard_fallback: false,
          settle: type_settle,
        },
      )
      .map_err(|error| format!("typed macOS type_text adapter failed: {error}"))?;
    if let Some(submit_key) = submit_key {
      session
        .input()
        .press_key(KeyPressOptions {
          key: submit_key.to_string(),
          settle: Duration::from_millis(submit_settle_ms),
        })
        .map_err(|error| format!("typed macOS submit key adapter failed: {error}"))?;
    }

    Ok(InputActionBridgeOutcome::from_result(
      InputPolicy::ForegroundPreferred,
      &result,
    ))
  }

  pub(crate) fn press_key_bridge(key: &str, settle_ms: u64) -> AuvResult<InputActionBridgeOutcome> {
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    let result = session
      .input()
      .press_key(KeyPressOptions {
        key: key.to_string(),
        settle: Duration::from_millis(settle_ms),
      })
      .map_err(|error| format!("typed macOS press_key adapter failed: {error}"))?;

    Ok(InputActionBridgeOutcome::from_result(
      InputPolicy::ForegroundPreferred,
      &result,
    ))
  }

  pub(crate) fn list_windows_bridge() -> AuvResult<Vec<auv_driver::Window>> {
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    session
      .window()
      .list()
      .map_err(|error| format!("typed macOS window list adapter failed: {error}"))
  }

  pub(crate) fn list_displays_bridge() -> AuvResult<auv_driver::ObservedDisplays> {
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    session
      .display()
      .list()
      .map_err(|error| format!("typed macOS display list adapter failed: {error}"))
  }

  pub(crate) fn capture_display_bridge(
    display: Option<String>,
  ) -> AuvResult<auv_driver::DisplayCapture> {
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    session
      .display()
      .capture(auv_driver::CaptureOptions {
        display,
        ..auv_driver::CaptureOptions::default()
      })
      .map_err(|error| format!("typed macOS display capture adapter failed: {error}"))
  }

  pub(crate) fn capture_region_bridge(
    display: Option<String>,
    region: auv_driver::Rect,
  ) -> AuvResult<auv_driver::RegionCapture> {
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    session
      .display()
      .capture_region(auv_driver::CaptureOptions {
        display,
        region: Some(region),
        ..auv_driver::CaptureOptions::default()
      })
      .map_err(|error| format!("typed macOS region capture adapter failed: {error}"))
  }

  pub(crate) fn probe_permissions_bridge() -> AuvResult<auv_driver::PermissionProbe> {
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    session
      .permission()
      .probe()
      .map_err(|error| format!("typed macOS permission probe adapter failed: {error}"))
  }

  pub(crate) fn scroll_global_hid_bridge(
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
    let result = session
      .input()
      .scroll_global_hid(
        Point::new(x, y),
        Scroll::new(delta_x, delta_y),
        Duration::from_millis(settle_ms),
      )
      .map_err(|error| format!("typed macOS global scroll adapter failed: {error}"))?;
    Ok(ScrollPointBridgeOutcome::from_result(policy, &result))
  }

  pub(crate) fn scroll_window_point_bridge(
    window: auv_driver::Window,
    window_x: f64,
    window_y: f64,
    delta_x: f64,
    delta_y: f64,
    policy: InputPolicy,
    settle_ms: u64,
  ) -> AuvResult<ScrollPointBridgeOutcome> {
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    let result = session
      .window()
      .scroll(
        &window,
        WindowPoint::new(window_x, window_y),
        Scroll::new(delta_x, delta_y),
        ScrollOptions {
          policy,
          settle: Duration::from_millis(settle_ms),
          ..ScrollOptions::default()
        },
      )
      .map_err(|error| format!("typed macOS window scroll adapter failed: {error}"))?;
    Ok(ScrollPointBridgeOutcome::from_result(policy, &result))
  }

  pub(crate) fn click_window_point_bridge(
    window: auv_driver::Window,
    window_x: f64,
    window_y: f64,
    policy: InputPolicy,
    click: auv_driver::Click,
    window_strategy: WindowClickStrategy,
  ) -> AuvResult<InputActionBridgeOutcome> {
    let driver = auv_driver_macos::MacosDriver::new();
    let session = driver
      .open_local()
      .map_err(|error| format!("failed to open typed macOS driver session: {error}"))?;
    let result = session
      .window()
      .click(
        &window,
        WindowPoint::new(window_x, window_y),
        ClickOptions {
          policy,
          click,
          window_strategy,
        },
      )
      .map_err(|error| format!("typed macOS window click adapter failed: {error}"))?;
    Ok(InputActionBridgeOutcome::from_result(policy, &result))
  }

  pub(crate) fn input_policy_name(policy: InputPolicy) -> &'static str {
    match policy {
      InputPolicy::BackgroundOnly => "background_only",
      InputPolicy::BackgroundPreferred => "background_preferred",
      InputPolicy::ForegroundPreferred => "foreground_preferred",
    }
  }

  pub(crate) fn selected_path_name(path: auv_driver::InputDeliveryPath) -> &'static str {
    match path {
      auv_driver::InputDeliveryPath::Noop => "noop",
      auv_driver::InputDeliveryPath::AxPress => "ax_press",
      auv_driver::InputDeliveryPath::AxFocus => "ax_focus",
      auv_driver::InputDeliveryPath::AxSetValue => "ax_set_value",
      auv_driver::InputDeliveryPath::AxScroll => "ax_scroll",
      auv_driver::InputDeliveryPath::AxSelectedText => "ax_selected_text",
      auv_driver::InputDeliveryPath::WindowTargetedMouse => "window_targeted_mouse",
      auv_driver::InputDeliveryPath::WindowTargetedWheel => "window_targeted_wheel",
      auv_driver::InputDeliveryPath::WindowTargetedKeyboard => "window_targeted_keyboard",
      auv_driver::InputDeliveryPath::WindowTargetedKeyboardScroll => {
        "window_targeted_keyboard_scroll"
      }
      auv_driver::InputDeliveryPath::ClipboardPaste => "clipboard_paste",
      auv_driver::InputDeliveryPath::ForegroundSystemEvents => "foreground_system_events",
      auv_driver::InputDeliveryPath::Unsupported => "unsupported",
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
    fn scroll_global_hid_bridge_outcome_exposes_signal_values() {
      let result = auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::ForegroundSystemEvents,
      );
      let outcome =
        ScrollPointBridgeOutcome::from_result(InputPolicy::ForegroundPreferred, &result);

      assert_eq!(outcome.input_bridge, "typed-session");
      assert_eq!(outcome.selected_path, "foreground_system_events");
      assert_eq!(outcome.input_policy, "foreground_preferred");
    }

    #[test]
    fn scroll_bridge_outcome_uses_actual_selected_path() {
      let result = auv_driver::InputActionResult::single_success(
        auv_driver::InputDeliveryPath::WindowTargetedWheel,
      );

      let outcome =
        ScrollPointBridgeOutcome::from_result(InputPolicy::BackgroundPreferred, &result);

      assert_eq!(outcome.input_bridge, "typed-session");
      assert_eq!(outcome.selected_path, "window_targeted_wheel");
      assert_eq!(outcome.input_policy, "background_preferred");
    }
  }
}
