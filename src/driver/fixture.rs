// File: src/driver/fixture.rs
use super::Driver;
use std::collections::BTreeMap;

use auv_steam::{LibraryQuery, Steam, build_library_ls_json_output};

use crate::driver::macos::support::artifacts::build_text_artifact;
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
    if call.operation == "steam_library_list" {
      return steam_library_list(call);
    }

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

fn steam_library_list(_call: &DriverCall) -> AuvResult<DriverResponse> {
  let query = LibraryQuery::default();
  let steam = Steam::locate()
    .map_err(|error| format!("steam.library.list.v0 failed to locate Steam: {error}"))?;
  let result = steam.library_apps(query).map_err(|diagnostic| {
    format!(
      "steam.library.list.v0 failed: [{}] {}",
      diagnostic.code, diagnostic.message
    )
  })?;

  let output = build_library_ls_json_output(&result);
  let json = serde_json::to_string_pretty(&output)
    .map_err(|error| format!("failed to serialize steam library result: {error}"))?;
  let report = build_text_artifact(
    "steam-library-list",
    "json",
    "steam-library-list",
    format!("{json}\n"),
    "Structured installed Steam library listing from auv-steam.",
  )?;

  let mut signals = BTreeMap::new();
  signals.insert(
    "steam.library.source".to_string(),
    result.resolved_scope.source.clone(),
  );
  signals.insert("steam.library.status".to_string(), "installed".to_string());
  signals.insert(
    "steam.library.app_count".to_string(),
    result.apps.len().to_string(),
  );
  if let Some(first) = result.apps.first() {
    signals.insert(
      "steam.library.first_app.name".to_string(),
      first.name.clone(),
    );
    signals.insert(
      "steam.library.first_app.appid".to_string(),
      first.appid.to_string(),
    );
  }

  Ok(DriverResponse {
    summary: format!(
      "Listed {} installed Steam app(s) through auv-steam local appmanifest grounding.",
      result.apps.len()
    ),
    backend: Some("steam.local_appmanifest.library-list".to_string()),
    signals,
    notes: vec![
      format!("resolvedSource={}", result.resolved_scope.source),
      format!("appCount={}", result.apps.len()),
      "Capability implemented through auv-steam library reuse, not duplicated Steam parsing."
        .to_string(),
    ],
    artifacts: vec![report],
  })
}
