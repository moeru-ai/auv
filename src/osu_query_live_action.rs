use std::fs;
use std::path::PathBuf;

use crate::contract::{
  ArtifactRef, FreshnessBasis, OPERATION_RESULT_API_VERSION, OperationOutput, OperationResult,
  OperationStatus,
};
use crate::minecraft_query_live_action::invoke_click_at_window_point;
use auv_driver::geometry::WindowPoint;
use auv_game_osu::{
  VisualTruthQueryActionWiringLineage, VisualTruthQueryActionWiringOutcome,
  VisualTruthQueryLiveClickExecutor,
};
use auv_tracing_driver::RunRecordingBackend;
use auv_tracing_driver::recorded_operation::RecordedOperationContext;
use auv_tracing_driver::trace::RunId;

pub const QUERY_WIRED_LIVE_ACTION_OPERATION_ID: &str =
  "auv.osu.visual_truth_query_wired_live_action";

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

impl VisualTruthQueryLiveClickExecutor for InvokeWindowPointClickExecutor<'_> {
  fn attempt_click(
    &self,
    window_point: WindowPoint,
    _lineage: &VisualTruthQueryActionWiringLineage,
  ) -> Result<String, String> {
    // NOTICE(osu-d3-executor-borrow): wiring calls `attempt_click` synchronously
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
  wiring: &VisualTruthQueryActionWiringOutcome,
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
      "osu query wired live action attempted without click summary or refusal".to_string(),
    );
  }

  let message = wiring
    .refusal_reason
    .clone()
    .unwrap_or_else(|| "osu query wired live action refused before dispatch".to_string());
  (OperationStatus::Completed, message)
}

pub fn build_osu_query_wired_live_action_operation_result(
  run_id: &RunId,
  wiring: &VisualTruthQueryActionWiringOutcome,
  query_manifest_ref: Option<ArtifactRef>,
) -> OperationResult {
  let (status, message) = operation_status_and_message_from_wiring(wiring);
  let freshness_basis = query_manifest_ref
    .as_ref()
    .map(|artifact_ref| FreshnessBasis {
      source_artifact: Some(artifact_ref.clone()),
      source_operation_id: Some("auv.osu.query_visual_truth_spatial".to_string()),
      notes: vec!["osu visual truth spatial query manifest staged in the same run".to_string()],
    });

  OperationResult {
    api_version: OPERATION_RESULT_API_VERSION.to_string(),
    run_id: run_id.clone(),
    status,
    operation_id: QUERY_WIRED_LIVE_ACTION_OPERATION_ID.to_string(),
    evidence_artifacts: query_manifest_ref.into_iter().collect(),
    output: OperationOutput::Acknowledged {
      message: Some(message),
    },
    verifications: Vec::new(),
    freshness_basis,
    known_limits: wiring.known_limits.clone(),
  }
}

pub fn stage_osu_query_wired_live_action_operation_result(
  context: &mut RecordedOperationContext<'_>,
  operation_result: &OperationResult,
) -> Result<(PathBuf, ArtifactRef), String> {
  let artifact_json = serde_json::to_string_pretty(operation_result)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| {
      format!("failed to serialize osu query wired live action operation result: {error}")
    })?;
  let artifact_path = std::env::temp_dir().join(format!(
    "auv-osu-query-wired-live-action-operation-result-{}-{}.json",
    context.run_id(),
    crate::model::now_millis()
  ));
  fs::write(&artifact_path, artifact_json.as_bytes()).map_err(|error| {
    format!("failed to write osu query wired live action operation result: {error}")
  })?;
  let staged = context.stage_artifact_file_with_ref(
    "operation-result",
    &artifact_path,
    "operation-result.json",
    Some(
      "osu visual truth query wired live action operation result with spatial query lineage"
        .to_string(),
    ),
  );
  let _ = fs::remove_file(&artifact_path);
  staged.map_err(|error| error.to_string())
}
