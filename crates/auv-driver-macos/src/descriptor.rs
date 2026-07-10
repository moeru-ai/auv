use auv_driver_common::{DriverDescriptor, PlatformKind};

pub const MACOS_DESKTOP_CAPABILITIES: &[&str] = &[
  "desktop.capture-display",
  "desktop.capture-region",
  "desktop.capture-window",
  "desktop.list-displays",
  "desktop.list-windows",
  "desktop.capture-ax-tree",
  "desktop.probe-permissions",
  "desktop.identify-point",
  "desktop.project-screenshot-point",
  "desktop.find-screen-text",
  "desktop.wait-screen-text",
  "desktop.find-screen-rows",
  "desktop.wait-screen-rows",
  "desktop.find-window-text",
  "desktop.wait-window-text",
  "desktop.find-window-rows",
  "desktop.wait-window-rows",
  "desktop.find-image-text",
  "desktop.verify-ax-text",
  "control.activate-app",
  "control.focus-text-input",
  "control.press-button",
  "control.type-text",
  "control.paste-text-preserve-clipboard",
  "control.press-key",
  "control.click-point",
  "control.click-window-point",
  "control.click-screen-text",
  "control.click-screen-row",
  "control.click-window-text",
  "control.click-window-row",
  "control.scroll-point",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacosDriverDescriptor {
  pub id: &'static str,
  pub platform: PlatformKind,
  pub summary: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
#[doc(hidden)]
pub struct MacosLegacyDescriptorMetadata {
  pub descriptor: MacosDriverDescriptor,
  pub capabilities: &'static [&'static str],
  pub donor_boundary: &'static str,
}

impl MacosDriverDescriptor {
  pub fn as_driver_descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: self.id,
      platform: self.platform,
      summary: self.summary,
    }
  }
}

pub fn macos_driver_descriptor() -> MacosDriverDescriptor {
  MacosDriverDescriptor {
    id: "macos.desktop",
    platform: PlatformKind::Macos,
    summary: "macOS desktop primitives for capture, OCR, window resolution, AX tree inspection, and input control.",
  }
}

#[doc(hidden)]
pub fn macos_legacy_descriptor_metadata() -> MacosLegacyDescriptorMetadata {
  MacosLegacyDescriptorMetadata {
    descriptor: macos_driver_descriptor(),
    capabilities: MACOS_DESKTOP_CAPABILITIES,
    donor_boundary: "Borrow host desktop primitives from platform APIs, but keep MCP tools, action executors, approval queues, and workflow shells out of AUV core.",
  }
}
