use auv_driver_common::{DriverDescriptor, PlatformKind};

/// Capabilities the Windows desktop driver currently exposes.
///
/// This list grows as capability slices land. Today the implemented surface is
/// system OCR over a provided image, display/region/window capture, top-level
/// window enumeration/resolution, foreground pointer/keyboard input, and text
/// clipboard snapshot/restore/set.
// TODO(windows-driver): extend as further slices land, mirroring
// `MACOS_DESKTOP_CAPABILITIES`. Window mutation (move/resize/minimize) is
// exposed on WindowApi but, like the macOS driver, is not represented as a
// capability string here until a consumer needs to gate on it.
pub const WINDOWS_DESKTOP_CAPABILITIES: &[&str] = &[
  "desktop.recognize-image-text",
  "desktop.find-image-text",
  "desktop.list-displays",
  "desktop.capture-display",
  "desktop.capture-region",
  "desktop.capture-window",
  "desktop.list-windows",
  "control.click-point",
  "control.scroll-point",
  "control.type-text",
  "control.press-key",
  "control.copy",
  "control.paste",
  "clipboard.snapshot",
  "clipboard.restore",
  "clipboard.set-text",
  "desktop.probe-permissions",
  "desktop.capture-ax-tree",
  "control.activate-window",
  "control.focus-ax-node",
  "control.select-ax-node",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowsDriverDescriptor {
  pub id: &'static str,
  pub platform: PlatformKind,
  pub summary: &'static str,
}

impl WindowsDriverDescriptor {
  pub fn as_driver_descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: self.id,
      platform: self.platform,
      summary: self.summary,
    }
  }
}

pub fn windows_driver_descriptor() -> WindowsDriverDescriptor {
  WindowsDriverDescriptor {
    id: "windows.desktop",
    platform: PlatformKind::Windows,
    summary: "Windows desktop driver: system OCR, display/region/window capture, window enumeration and mutation, foreground input, clipboard, permission probe, and UIA accessibility tree observation/focus.",
  }
}
