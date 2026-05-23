use super::Driver;
use std::collections::BTreeMap;

use crate::model::{AuvResult, DriverCall, DriverDescriptor, DriverResponse};
use serde_json::json;

pub(crate) struct FixtureObserveDriver;

impl Driver for FixtureObserveDriver {
  fn descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: "fixture.observe",
      summary: "Non-UI fixture driver that proves invoke -> run -> inspect without platform side effects.",
      capabilities: &["observe.fixture"],
      donor_boundary: "AUV-native fixture driver; useful for validating the shared execution substrate before real app drivers land.",
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

    let mut signals = BTreeMap::new();
    if let Some(action) = call.inputs.get("hook_action") {
      signals.insert("last.scan.hook.action".to_string(), action.clone());
    }
    if let Some(reason) = call.inputs.get("hook_reason") {
      signals.insert("last.scan.hook.reason".to_string(), reason.clone());
    }
    if let Some(action) = call.inputs.get("hook_action") {
      let decision = json!({
        "hook_name": call
          .inputs
          .get("hook_name")
          .cloned()
          .unwrap_or_else(|| "fixture".to_string()),
        "stage": call
          .inputs
          .get("hook_stage")
          .cloned()
          .unwrap_or_else(|| "fixture".to_string()),
        "page_index": call
          .inputs
          .get("hook_page_index")
          .and_then(|value| value.parse::<usize>().ok()),
        "action": action,
        "reason": call
          .inputs
          .get("hook_reason")
          .cloned()
          .unwrap_or_else(|| "fixture observe default reason".to_string()),
        "annotations": call
          .inputs
          .get("hook_annotation")
          .map(|value| vec![value.clone()])
          .unwrap_or_default(),
        "evidence": call
          .inputs
          .get("hook_evidence")
          .map(|value| vec![value.clone()])
          .unwrap_or_default(),
      });
      signals.insert("last.scan.hook.decision".to_string(), decision.to_string());
    }
    if let Some(context) = call.inputs.get("context") {
      signals.insert("fixture.context".to_string(), context.clone());
    }

    Ok(DriverResponse {
      summary: format!(
        "Observed deterministic fixture scene for target {} with label {}.",
        target, label
      ),
      backend: Some("fixture.static".to_string()),
      signals,
      notes: vec![
        "This command does not touch the real desktop.".to_string(),
        "Use it to verify that implicit run creation and inspect output stay stable.".to_string(),
      ],
      artifacts: Vec::new(),
    })
  }
}
