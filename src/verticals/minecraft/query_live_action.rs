use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use crate::contract::{
  ArtifactRef, FreshnessBasis, OPERATION_RESULT_API_VERSION, OperationOutput, OperationResult,
  OperationStatus, VerificationResult,
};
use crate::model::InvokeRequest;
use auv_driver::geometry::WindowPoint;
use auv_game_minecraft::{
  QueryActionWiringLineage, QueryActionWiringOutcome, QueryLiveClickExecutor,
};
use auv_tracing_driver::RunRecordingBackend;
use auv_tracing_driver::recorded_operation::RecordedOperationContext;
use auv_tracing_driver::trace::RunId;

pub const QUERY_WIRED_LIVE_ACTION_OPERATION_ID: &str = "auv.minecraft.query_wired_live_action";

// NOTICE(mc19-d4-known-limit): D4 closes non-stub `input.clickWindowPoint`
// dispatch for MC-19 wired live action. MC-20 D1 adds post-action verification
// in glue and removes this limit when verification claims are recorded; see
// `docs/ai/references/2026-06-30-minecraft-mc20-d1-query-wired-post-action-verification-design.md`.

pub fn invoke_click_at_window_point(
  recording: &RunRecordingBackend,
  context: &mut RecordedOperationContext<'_>,
  target_app: &str,
  target_title: &str,
  window_point: WindowPoint,
) -> Result<String, String> {
  let mut inputs = BTreeMap::new();
  inputs.insert("title".to_string(), target_title.to_string());
  inputs.insert("offset_x".to_string(), format!("{:.3}", window_point.0.x));
  inputs.insert("offset_y".to_string(), format!("{:.3}", window_point.0.y));

  let registry = auv_cli_invoke::default_registry();
  let command = registry
    .resolve("input.clickWindowPoint")
    .ok_or_else(|| "input.clickWindowPoint command is not registered".to_string())?;
  let parent = context.current_span().clone();
  let invoke_result = auv_cli_invoke::invoke_resolved_recorded_in_span(
    recording,
    context.run_mut(),
    &parent,
    command,
    InvokeRequest {
      command_id: "input.clickWindowPoint".to_string(),
      target: crate::model::ExecutionTarget {
        application_id: Some(target_app.to_string()),
        target_label: None,
      },
      inputs,
      dry_run: false,
    },
  )
  .map_err(|error| error.to_string())?;
  Ok(invoke_result.output_summary)
}

pub struct InvokeWindowPointClickExecutor<'ctx> {
  recording: *const RunRecordingBackend,
  context: *mut RecordedOperationContext<'ctx>,
  target_app: String,
  target_title: String,
}

impl<'ctx> InvokeWindowPointClickExecutor<'ctx> {
  pub fn new(
    context: &mut RecordedOperationContext<'ctx>,
    target_app: impl Into<String>,
    target_title: impl Into<String>,
  ) -> Self {
    Self {
      recording: context.recording() as *const RunRecordingBackend,
      context: context as *mut RecordedOperationContext<'ctx>,
      target_app: target_app.into(),
      target_title: target_title.into(),
    }
  }
}

impl QueryLiveClickExecutor for InvokeWindowPointClickExecutor<'_> {
  fn attempt_click(
    &self,
    window_point: WindowPoint,
    _lineage: &QueryActionWiringLineage,
  ) -> Result<String, String> {
    // NOTICE(mc19-d3-executor-borrow): wiring calls `attempt_click` synchronously
    // while the recorded-operation closure already holds `&mut context`; the raw
    // pointers are only valid for this non-reentrant dispatch.
    let recording = unsafe { &*self.recording };
    let context = unsafe { &mut *self.context };
    invoke_click_at_window_point(
      recording,
      context,
      &self.target_app,
      &self.target_title,
      window_point,
    )
  }
}

pub fn operation_status_and_message_from_wiring(
  wiring: &QueryActionWiringOutcome,
) -> (OperationStatus, String) {
  if wiring.attempted {
    if let Some(summary) = &wiring.click_summary {
      return (OperationStatus::Completed, summary.clone());
    }
    if let Some(refusal) = &wiring.refusal_reason {
      return (OperationStatus::Failed, refusal.clone());
    }
    return (
      OperationStatus::Failed,
      "query wired live action attempted without click summary or refusal".to_string(),
    );
  }

  let message = wiring
    .refusal_reason
    .clone()
    .unwrap_or_else(|| "query wired live action refused before dispatch".to_string());
  (OperationStatus::Completed, message)
}

pub fn build_query_wired_live_action_operation_result(
  run_id: &RunId,
  wiring: &QueryActionWiringOutcome,
  query_manifest_ref: Option<ArtifactRef>,
  verifications: Vec<VerificationResult>,
  witness_absent_limit_needed: bool,
) -> OperationResult {
  let (status, message) = operation_status_and_message_from_wiring(wiring);
  let freshness_basis = query_manifest_ref
    .as_ref()
    .map(|artifact_ref| FreshnessBasis {
      source_artifact: Some(artifact_ref.clone()),
      source_operation_id: Some("auv.minecraft.query_3dgs_training_result".to_string()),
      notes: vec!["MC-12 spatial query manifest staged in the same run".to_string()],
    });
  let known_limits = resolve_query_wired_live_action_known_limits(
    wiring,
    &verifications,
    witness_absent_limit_needed,
  );

  OperationResult {
    api_version: OPERATION_RESULT_API_VERSION.to_string(),
    run_id: run_id.clone(),
    status,
    operation_id: QUERY_WIRED_LIVE_ACTION_OPERATION_ID.to_string(),
    evidence_artifacts: query_manifest_ref.into_iter().collect(),
    output: OperationOutput::Acknowledged {
      message: Some(message),
    },
    verifications,
    freshness_basis,
    known_limits,
  }
}

fn resolve_query_wired_live_action_known_limits(
  wiring: &QueryActionWiringOutcome,
  verifications: &[VerificationResult],
  witness_absent_limit_needed: bool,
) -> Vec<String> {
  if !wiring.attempted || verifications.is_empty() {
    return wiring.known_limits.clone();
  }

  let mut known_limits = wiring
    .known_limits
    .iter()
    .filter(|limit| **limit != auv_game_minecraft::MC19_V1_D4_QUERY_WIRED_LIVE_ACTION_KNOWN_LIMIT)
    .cloned()
    .collect::<Vec<_>>();

  if witness_absent_limit_needed {
    known_limits
      .push(auv_game_minecraft::MC20_V1_QUERY_WIRED_WITNESS_ABSENT_KNOWN_LIMIT.to_string());
  }

  known_limits
}

pub fn stage_query_wired_live_action_operation_result(
  context: &mut RecordedOperationContext<'_>,
  operation_result: &OperationResult,
) -> Result<(PathBuf, ArtifactRef), String> {
  let artifact_json = serde_json::to_string_pretty(operation_result)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| {
      format!("failed to serialize query wired live action operation result: {error}")
    })?;
  let artifact_path = std::env::temp_dir().join(format!(
    "auv-minecraft-query-wired-live-action-operation-result-{}-{}.json",
    context.run_id(),
    crate::model::now_millis()
  ));
  fs::write(&artifact_path, artifact_json.as_bytes()).map_err(|error| {
    format!("failed to write query wired live action operation result: {error}")
  })?;
  let staged = context.stage_artifact_file_with_ref(
    "operation-result",
    &artifact_path,
    "operation-result.json",
    Some("MC-19 D4 query wired live action operation result with MC-12 query lineage".to_string()),
  );
  let _ = fs::remove_file(&artifact_path);
  staged.map_err(|error| error.to_string())
}
