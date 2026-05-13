use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::model::{AuvResult, DriverCall, DriverDescriptor, DriverResponse};

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
  DriverRegistry::new(vec![Box::new(FixtureObserveDriver)])
}

struct FixtureObserveDriver;

impl Driver for FixtureObserveDriver {
  fn descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: "fixture.observe",
      summary: "Non-UI fixture driver that proves invoke -> run -> inspect without platform side effects.",
      capabilities: &["observe.fixture"],
      donor_boundary: "AUV-native fixture driver; validate the shared execution substrate before platform drivers land.",
    }
  }

  fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
    if call.operation != "observe_fixture_scene" {
      return Err(format!(
        "driver fixture.observe does not support operation {}",
        call.operation
      ));
    }

    let target = call
      .target
      .application_id
      .clone()
      .unwrap_or_else(|| "fixture://default".to_string());
    let label = call
      .inputs
      .get("label")
      .cloned()
      .unwrap_or_else(|| "fixture-observation".to_string());

    Ok(DriverResponse {
      summary: format!(
        "Observed deterministic fixture scene for target {} with label {}.",
        target, label
      ),
      backend: Some("fixture.static".to_string()),
      notes: vec![
        "This command does not touch the real desktop.".to_string(),
        "Use it to validate implicit run creation, artifact plumbing, and inspect output."
          .to_string(),
      ],
      artifacts: Vec::new(),
    })
  }
}

pub fn copy_file(source: &PathBuf, destination: &PathBuf) -> AuvResult<()> {
  if let Some(parent) = destination.parent() {
    fs::create_dir_all(parent).map_err(|error| {
      format!(
        "failed to create artifact directory {}: {error}",
        parent.display()
      )
    })?;
  }

  fs::copy(source, destination).map_err(|error| {
    format!(
      "failed to copy artifact from {} to {}: {error}",
      source.display(),
      destination.display()
    )
  })?;

  Ok(())
}

pub fn sanitized_artifact_name(raw: &str) -> String {
  let sanitized = raw
    .chars()
    .map(|character| match character {
      'a'..='z' | 'A'..='Z' | '0'..='9' | '-' | '_' => character,
      _ => '-',
    })
    .collect::<String>()
    .trim_matches('-')
    .to_string();

  if sanitized.is_empty() {
    "artifact".to_string()
  } else {
    sanitized
  }
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::path::PathBuf;

  use super::{Driver, DriverRegistry, FixtureObserveDriver, sanitized_artifact_name};
  use crate::model::{DriverCall, ExecutionTarget, now_millis};

  #[test]
  fn sanitize_file_component_removes_invalid_characters() {
    assert_eq!(sanitized_artifact_name("My App!"), "My-App");
    assert_eq!(sanitized_artifact_name("../../etc/passwd"), "etc-passwd");
    assert_eq!(sanitized_artifact_name(""), "artifact");
  }

  #[test]
  fn driver_registry_stores_and_retrieves_drivers() {
    let registry = DriverRegistry::new(vec![Box::new(FixtureObserveDriver)]);
    assert!(registry.get("fixture.observe").is_some());
    assert!(registry.get("missing").is_none());
    assert_eq!(registry.descriptors().len(), 1);
    assert_eq!(registry.descriptors()[0].id, "fixture.observe");
  }

  #[test]
  fn fixture_driver_rejects_unknown_operations() {
    let driver = FixtureObserveDriver;
    let error = driver
      .invoke(&DriverCall {
        operation: "unknown".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        working_directory: PathBuf::from("."),
      })
      .expect_err("unknown operation should fail");
    assert!(error.contains("does not support operation"));
  }

  #[test]
  fn fixture_driver_produces_deterministic_summary() {
    let driver = FixtureObserveDriver;
    let mut inputs = BTreeMap::new();
    inputs.insert("label".to_string(), format!("fixture-{}", now_millis()));
    let response = driver
      .invoke(&DriverCall {
        operation: "observe_fixture_scene".to_string(),
        target: ExecutionTarget {
          application_id: Some("fixture://example".to_string()),
        },
        inputs,
        working_directory: PathBuf::from("."),
      })
      .expect("fixture call should succeed");

    assert!(response.summary.contains("fixture://example"));
    assert_eq!(response.backend.as_deref(), Some("fixture.static"));
    assert!(response.artifacts.is_empty());
  }
}
