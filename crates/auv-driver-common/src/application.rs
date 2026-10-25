use serde::{Deserialize, Serialize};

use crate::error::DriverError;
use crate::window::WindowRef;

/// Which platform identity an application id names.
///
/// An application id is the opaque id that `window.list` reports for a
/// window's application on the same platform. The kind travels with every
/// activation result so callers can tell a macOS bundle id from a Windows
/// process image name without guessing from the string.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationIdKind {
  /// macOS bundle identifier, for example `com.apple.TextEdit`.
  BundleId,
  /// Windows process image name, as Windows `window.list` reports in
  /// `app_name`, for example `notepad.exe`.
  // TODO(windows-aumid-app-id): packaged apps also have an AppUserModelID.
  // It is deferred because `window.list` does not report it yet; add a kind
  // when a Windows window listing exposes AUMIDs.
  ProcessName,
  /// Linux Wayland desktop application id (the AT-SPI `AccessibleId` of the
  /// application). Defined for the contract; activation is not implemented.
  DesktopAppId,
  // TODO(x11-app-activate): an X11 `WM_CLASS` kind is added with the X11
  // driver, which main does not have yet.
}

impl ApplicationIdKind {
  /// Returns the canonical name used by human and structured output.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::BundleId => "bundle_id",
      Self::ProcessName => "process_name",
      Self::DesktopAppId => "desktop_app_id",
    }
  }
}

/// How a platform delivered the activation request.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApplicationActivationMethod {
  /// macOS `tell application id "<bundle-id>" to activate`.
  AppleScriptActivate,
  /// Resolve the application's main visible window and foreground it
  /// (Windows `SetForegroundWindow`).
  ForegroundWindow,
  // TODO(x11-app-activate): an EWMH `_NET_ACTIVE_WINDOW` method is added with
  // the X11 driver, which main does not have yet.
}

impl ApplicationActivationMethod {
  /// Returns the canonical name used by human and structured output.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::AppleScriptActivate => "apple_script_activate",
      Self::ForegroundWindow => "foreground_window",
    }
  }
}

/// Evidence returned after requesting foreground activation for one app.
///
// NOTICE: there is no focus-disturbance field. Unlike input delivery, where
// focus change is a side effect worth reporting, activation changes focus by
// definition; the verification below already says whether it happened.
// TODO(app-activate-attempts): every platform has a single strategy today, so
// there are no attempts or fallback reasons to record. Add them, in the shape
// of `InputActionResult.attempts`, when a platform gains a second strategy.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationActivationResult {
  /// Application id supplied to the activation request.
  pub requested_application_id: String,
  /// Which platform identity `requested_application_id` names.
  pub application_id_kind: ApplicationIdKind,
  /// How the activation request was delivered.
  pub method: ApplicationActivationMethod,
  /// The window the platform foregrounded, when activation went through one.
  pub activated_window: Option<WindowRef>,
  /// Evidence from the post-activation foreground observation.
  pub verification: ApplicationActivationVerification,
  /// What a post-activation window observation established about the
  /// activated application's windows (issue #324). `None` means this driver
  /// version did not observe windows — an absent fact, never an absence
  /// claim.
  pub window_observation: Option<ActivationWindowObservation>,
}

/// What a post-activation platform observation could establish.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ApplicationActivationVerification {
  /// The requested application was observed as the foreground application.
  VerifiedForeground {
    /// Application id returned by the foreground observation.
    observed_application_id: String,
  },
  /// Activation was delivered, but another application was observed in front.
  #[serde(rename = "activation_only_foreground_mismatch")]
  ForegroundMismatch {
    /// Application id returned by the foreground observation.
    observed_application_id: String,
  },
  /// Activation was delivered, but no foreground observation was available.
  #[serde(rename = "activation_only_verification_unavailable")]
  Unavailable {
    /// Why the platform could not provide foreground evidence.
    reason: String,
  },
}

impl ApplicationActivationVerification {
  /// Returns the canonical status used by human and structured output.
  pub fn status(&self) -> &'static str {
    match self {
      Self::VerifiedForeground { .. } => "verified_foreground",
      Self::ForegroundMismatch { .. } => "activation_only_foreground_mismatch",
      Self::Unavailable { .. } => "activation_only_verification_unavailable",
    }
  }

  /// Returns the observed foreground application id when one was available.
  pub fn observed_application_id(&self) -> Option<&str> {
    match self {
      Self::VerifiedForeground {
        observed_application_id,
      }
      | Self::ForegroundMismatch {
        observed_application_id,
      } => Some(observed_application_id),
      Self::Unavailable { .. } => None,
    }
  }
}

/// What a post-activation window observation established about the activated
/// application's windows (issue #324).
///
/// The three states are facts about the observation, not promises about
/// usability: a `Present` window may still be minimized, occluded, or
/// uncapturable, and only a window operation's own evidence proves a window
/// usable. `Absent` means the observation succeeded and the application owns
/// no matching window. `Unknown` means the observation itself failed and
/// carries no claim about the application either way.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ActivationWindowObservation {
  /// The application owns at least one window the observation path can see;
  /// `reference` names the window the driver resolves as the application's
  /// main window, in the same preference a main-visible window selector
  /// applies.
  #[serde(rename = "window_present")]
  Present {
    /// Window the driver would target next.
    reference: WindowRef,
  },
  /// The observation succeeded and found no window matching the activated
  /// application.
  #[serde(rename = "window_absent")]
  Absent,
  /// The window observation failed; `reason` names the failure. Distinct
  /// from every foreground-verification outcome: an unknown observation says
  /// nothing about which application owns the foreground.
  #[serde(rename = "window_unknown")]
  Unknown {
    /// Why the platform could not observe windows.
    reason: String,
  },
}

impl ActivationWindowObservation {
  /// Returns the canonical status used by human and structured output.
  pub fn status(&self) -> &'static str {
    match self {
      Self::Present { .. } => "window_present",
      Self::Absent => "window_absent",
      Self::Unknown { .. } => "window_unknown",
    }
  }
}

/// The typed error for a platform whose application ids are defined but whose
/// activation is not implemented.
///
/// Drivers return this before delivering anything, so an unsupported platform
/// fails the operation instead of reporting an activation-only result.
pub fn unsupported_activation(kind: ApplicationIdKind) -> DriverError {
  match kind {
    // TODO(wayland-app-activate): Wayland has no portable activation request;
    // a follow-up slice adds per-desktop backends (GNOME, KDE, wlroots), not a
    // GNOME-only path.
    ApplicationIdKind::DesktopAppId => DriverError::unsupported("application.activate on Linux Wayland (desktop_app_id)"),
    ApplicationIdKind::BundleId => DriverError::unsupported("application.activate by bundle_id on this platform"),
    ApplicationIdKind::ProcessName => DriverError::unsupported("application.activate by process_name on this platform"),
  }
}

#[cfg(test)]
#[path = "application_test.rs"]
mod tests;
