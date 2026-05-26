// File: src/driver/macos/descriptor.rs
use super::*;

pub(crate) fn driver_descriptor() -> DriverDescriptor {
  let metadata = super::typed::descriptor::legacy_descriptor_metadata();
  let descriptor = metadata.descriptor;

  DriverDescriptor {
    id: descriptor.id,
    summary: descriptor.summary,
    capabilities: metadata.capabilities,
    donor_boundary: metadata.donor_boundary,
  }
}
