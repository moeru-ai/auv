use std::collections::HashMap;

use crate::model::{AuvResult, DriverCall, DriverDescriptor, DriverResponse};

use self::fixture::FixtureObserveDriver;
use self::macos::MacOsDesktopDriver;
pub(crate) use self::macos::{
  ObservedAxNode, ObservedAxTreeSnapshot, ObservedDisplay, ObservedDisplaySnapshot, ObservedOcrRow,
  ObservedRect, ObservedWindow, OcrTextSnapshot, clear_stale_lock_file, compute_combined_bounds,
  copy_file, describe_lock_owner, group_ocr_matches_into_rows, parse_observed_ax_tree,
  parse_ocr_text_snapshot, parse_window_line, report_value, sanitized_artifact_name,
};

mod fixture;
mod macos;

pub trait Driver {
  fn descriptor(&self) -> DriverDescriptor;
  fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse>;
}

pub struct DriverRegistry {
  drivers: HashMap<String, Box<dyn Driver>>,
}

impl DriverRegistry {
  pub fn new(drivers: Vec<Box<dyn Driver>>) -> Self {
    let mut registry = HashMap::new();
    for driver in drivers {
      let descriptor = driver.descriptor();
      registry.insert(descriptor.id.to_string(), driver);
    }
    Self { drivers: registry }
  }

  pub fn get(&self, driver_id: &str) -> Option<&dyn Driver> {
    self.drivers.get(driver_id).map(Box::as_ref)
  }

  pub fn descriptors(&self) -> Vec<DriverDescriptor> {
    let mut descriptors = self
      .drivers
      .values()
      .map(|driver| driver.descriptor())
      .collect::<Vec<_>>();
    descriptors.sort_by(|left, right| left.id.cmp(right.id));
    descriptors
  }
}

pub fn default_driver_registry() -> DriverRegistry {
  DriverRegistry::new(vec![
    Box::new(FixtureObserveDriver),
    Box::new(MacOsDesktopDriver),
  ])
}
