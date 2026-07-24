use auv_driver_common::{DriverDescriptor, PlatformKind};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MacosDriverDescriptor {
  pub id: &'static str,
  pub platform: PlatformKind,
  pub description: &'static str,
}

impl MacosDriverDescriptor {
  pub fn as_driver_descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: self.id,
      platform: self.platform,
      description: self.description,
    }
  }
}

pub fn macos_driver_descriptor() -> MacosDriverDescriptor {
  MacosDriverDescriptor {
    id: "macos.desktop",
    platform: PlatformKind::Macos,
    description: "macOS desktop primitives for capture, OCR, window resolution, AX tree inspection, and input control.",
  }
}
