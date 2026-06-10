// File: src/runtime.rs
//! Runtime execution engine.
//!
//! `Runtime` is the shared core used by CLI and other frontends: it resolves
//! command IDs via the catalog, invokes drivers, and records runs/spans/events
//! plus staged artifacts into the store.
//!
//! Boundary: this layer executes *given* requests. It is not a planner/LLM
//! agent, and it does not choose strategies beyond what the request/cmd
//! specifies.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::bundle::{SkillBundleCatalog, SkillBundleCatalogEntry, SkillBundleCommand};
use crate::catalog::CommandCatalog;
use crate::contract::ArtifactRef;
use crate::driver::DriverRegistry;
use crate::model::{
  AuvResult, CommandSpec, DriverCall, DriverDescriptor, DriverRunContext, InvokeRequest,
  InvokeResult, RunStatus, now_millis,
};
use crate::recording::{MemoryRunRecorder, RunRecorder, RunRecordingBackend, RunUpdate};
use crate::skill::{SkillCatalog, SkillRecipe, SkillRecipeOrigin, SkillRecipeRunner, SkillRunOptions};
use crate::store::{ArtifactFileSource, LocalStore};
use crate::trace::{
  EVENT_API_VERSION, EventRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType,
  SPAN_API_VERSION, SpanRecordV1Alpha1, TraceFailure, TraceState, TraceStatusCode, new_event_id,
  new_run_id, new_span_id, new_trace_id, string_attr,
};

pub struct Runtime {
  project_root: PathBuf,
  commands: CommandCatalog,
  bundles: SkillBundleCatalog,
  skills: SkillCatalog,
  drivers: DriverRegistry,
  recording: RunRecordingBackend,
}

impl Runtime {
  pub fn new(
    project_root: PathBuf,
    commands: CommandCatalog,
    drivers: DriverRegistry,
    store: LocalStore,
  ) -> Self {
    let bundles = SkillBundleCatalog::discover(&project_root).unwrap_or_else(|error| {
      panic!(
        "failed to discover bundle catalog from {}: {error}",
        project_root.display()
      )
    });
    let skills = SkillCatalog::discover(&project_root).unwrap_or_else(|error| {
      panic!(
        "failed to discover skill catalog from {}: {error}",
        project_root.display()
      )
    });
    Self::new_with_catalogs(project_root, commands, bundles, skills, drivers, store)
  }

  pub fn new_with_catalogs(
    project_root: PathBuf,
    commands: CommandCatalog,
    bundles: SkillBundleCatalog,
    skills: SkillCatalog,
    drivers: DriverRegistry,
    store: LocalStore,
  ) -> Self {
    Self {
      project_root,
      commands,
      bundles,
      skills,
      drivers,
      recording: RunRecordingBackend::new(store, Arc::new(MemoryRunRecorder::new())),
    }
  }

  pub fn list_commands(&self) -> &[crate::model::CommandSpec] {
    self.commands.all()
  }

  pub fn project_root(&self) -> &Path {
    &self.project_root
  }

  pub fn list_drivers(&self) -> Vec<DriverDescriptor> {
    self.drivers.descriptors()
  }

  pub fn inspect(&self, run_id: &str) -> AuvResult<String> {
    let canonical = self.recording.read_run(run_id)?;
    let verifications = crate::run_read::extract_verifications(self.recording.store(), &canonical)?;
    let observation_snapshots =
      crate::run_read::extract_observation_snapshots(self.recording.store(), &canonical)?;
    let detector_recognition_lineage =
      crate::run_read::extract_detector_recognition_lineage(self.recording.store(), &canonical)?;
    let candidate_promotion_lineage =
      crate::run_read::extract_candidate_promotion_lineage(self.recording.store(), &canonical)?;
    let candidate_action_decision_lineage =
      crate::run_read::extract_candidate_action_decision_lineage(
        self.recording.store(),
        &canonical,
      )?;
    let candidate_action_execution_lineage =
      crate::run_read::extract_candidate_action_execution_lineage(
        self.recording.store(),
        &canonical,
      )?;
    let validation_lineage =
      crate::run_read::extract_app_validation_lineage(self.recording.store(), &canonical)?;
    Ok(crate::inspect::render_text(
      &canonical,
      &verifications,
      &observation_snapshots,
      &detector_recognition_lineage,
      &candidate_promotion_lineage,
      &candidate_action_decision_lineage,
      &candidate_action_execution_lineage,
      &validation_lineage,
    ))
  }

  pub fn read_run(&self, run_id: &str) -> AuvResult<crate::store::CanonicalRun> {
    self.recording.read_run(run_id)
  }

  pub fn list_verifications(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::contract::VerificationResult>> {
    crate::run_read::list_verifications(self.recording.store(), run_id)
  }

  pub fn list_observation_snapshots(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::contract::ObservationSnapshot>> {
    crate::run_read::list_observation_snapshots(self.recording.store(), run_id)
  }

  pub fn list_app_validation_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::AppValidationLineage>> {
    crate::run_read::list_app_validation_lineage(self.recording.store(), run_id)
  }

  pub fn list_detector_recognition_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::DetectorRecognitionLineage>> {
    crate::run_read::list_detector_recognition_lineage(self.recording.store(), run_id)
  }

  pub fn list_candidate_promotion_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::CandidatePromotionLineage>> {
    crate::run_read::list_candidate_promotion_lineage(self.recording.store(), run_id)
  }

  pub fn list_candidate_action_decision_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::CandidateActionDecisionLineage>> {
    crate::run_read::list_candidate_action_decision_lineage(self.recording.store(), run_id)
  }

  pub fn record_candidate_action_decision(
    &self,
    promotion: &crate::candidate_promotion_recording::CandidatePromotionArtifact,
    request: crate::candidate_action_decision::CandidateActionDecisionRequest,
  ) -> AuvResult<
    crate::recorded_operation::RecordedOperationOutput<(
      ArtifactRef,
      crate::candidate_action_decision::CandidateActionDecisionArtifact,
    )>,
  > {
    self.run_recorded_operation(
      crate::run_builder::RunSpec::new(RunType::Execute, "auv.candidate.action.decide_only"),
      "Candidate action decide-only artifact recording",
      |context| {
        crate::candidate_action_decision::record_candidate_action_decision_artifact(
          context, promotion, &request,
        )
      },
    )
  }

  pub fn list_candidate_action_execution_lineage(
    &self,
    run_id: &str,
  ) -> AuvResult<Vec<crate::run_read::CandidateActionExecutionLineage>> {
    crate::run_read::list_candidate_action_execution_lineage(self.recording.store(), run_id)
  }

  pub fn run_candidate_action_command(
    &self,
    request: crate::candidate_action_command::CandidateActionCommandRequest,
  ) -> AuvResult<
    crate::recorded_operation::RecordedOperationOutput<
      crate::candidate_action_command::CandidateActionCommandOutput,
    >,
  > {
    self.run_recorded_operation(
      crate::run_builder::RunSpec::new(RunType::Execute, "auv.candidate.action.command"),
      "Consent-gated candidate action command",
      |context| {
        crate::candidate_action_command::execute_candidate_action_command(context, &request)
      },
    )
  }

  pub fn record_candidate_action_execution(
    &self,
    promotion: &crate::candidate_promotion_recording::CandidatePromotionArtifact,
    decision: &crate::candidate_action_decision::CandidateActionDecisionArtifact,
    request: crate::candidate_action_decision::CandidateActionExecutionRequest,
    input_action_result: auv_driver::InputActionResult,
  ) -> AuvResult<
    crate::recorded_operation::RecordedOperationOutput<(
      ArtifactRef,
      crate::candidate_action_decision::CandidateActionExecutionArtifact,
    )>,
  > {
    self.run_recorded_operation(
      crate::run_builder::RunSpec::new(RunType::Execute, "auv.candidate.action.execute_single"),
      "Candidate action execution artifact recording",
      |context| {
        crate::candidate_action_decision::record_candidate_action_execution_artifact(
          context,
          promotion,
          decision,
          &request,
          input_action_result,
        )
      },
    )
  }

  pub fn run_dir(&self, run_id: impl AsRef<str>) -> AuvResult<PathBuf> {
    self.recording.run_dir(run_id)
  }

  pub fn recorder(&self) -> Arc<dyn RunRecorder> {
    self.recording.recorder()
  }

  pub fn with_recording(mut self, recording: RunRecordingBackend) -> Self {
    self.recording = recording;
    self
  }

  pub fn with_recorder(mut self, recorder: Arc<dyn RunRecorder>) -> Self {
    let store = self.recording.store().clone();
    self.recording = RunRecordingBackend::new(store, recorder);
    self
  }

  pub fn start_run(
    &self,
    spec: crate::run_builder::RunSpec,
  ) -> AuvResult<crate::run_builder::RecordingRun> {
    let run_id = new_run_id();
    let root_span_id = new_span_id();
    let started = now_millis();
    let mut run_attributes = spec.attributes.clone();
    run_attributes.insert(
      crate::trace::RUN_ATTR_DEVICE_ID.to_string(),
      serde_json::Value::String(spec.device_id.as_str().to_string()),
    );
    run_attributes.insert(
      crate::trace::RUN_ATTR_SESSION_ID.to_string(),
      serde_json::Value::String(spec.session_id.as_str().to_string()),
    );
    let run = RunRecordV1Alpha1 {
      api_version: RUN_API_VERSION.to_string(),
      run_id: run_id.clone(),
      trace_id: new_trace_id(),
      run_type: spec.run_type,
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: started,
      finished_at_millis: None,
      root_span_id: root_span_id.clone(),
      attributes: run_attributes,
      summary: None,
      failure: None,
    };
    let root_span = SpanRecordV1Alpha1 {
      api_version: SPAN_API_VERSION.to_string(),
      span_id: root_span_id,
      parent_span_id: None,
      name: spec.root_span_name,
      state: TraceState::Running,
      status_code: TraceStatusCode::Unset,
      started_at_millis: started,
      finished_at_millis: None,
      attributes: spec.attributes,
      summary: None,
      failure: None,
    };
    let run = crate::run_builder::RecordingRun::new(run, root_span, self.recording.recorder());
    if self.recording.requires_successful_delivery() && !run.recording_errors().is_empty() {
      return Err(format!(
        "run recording delivery failed: {}",
        run.recording_errors().join("; ")
      ));
    }
    Ok(run)
  }

  pub fn finish_run(
    &self,
    run: crate::run_builder::RecordingRun,
    finish: crate::run_builder::RunFinish,
  ) -> AuvResult<RunId> {
    let failure = finish.failure.map(|message| TraceFailure { message });
    let recorded = run.finish(finish.status_code, finish.summary, failure);
    let run_id = recorded.snapshot.run.run_id.clone();
    let mut recording_errors = recorded.recording_errors;
    self.recording.write_run_snapshot(&recorded.snapshot)?;
    if let Err(error) = self.recording.record(RunUpdate::RunFinished {
      run_id: run_id.clone(),
      run: recorded.snapshot.run,
    }) && self.recording.requires_successful_delivery()
    {
      recording_errors.push(error);
    }
    if !recording_errors.is_empty() {
      return Err(format!(
        "run recording delivery failed: {}",
        recording_errors.join("; ")
      ));
    }
    Ok(run_id)
  }

  pub fn invoke(&self, request: InvokeRequest) -> AuvResult<InvokeResult> {
    let mut run = self.start_run(crate::run_builder::RunSpec::new(
      RunType::Command,
      "auv.command",
    ))?;
    let root = run.root_span();
    let result = match self.invoke_in_span(&mut run, &root, request) {
      Ok(result) => result,
      Err(error) => {
        if let Err(finish_error) = self.finish_run(
          run,
          crate::run_builder::RunFinish {
            status_code: TraceStatusCode::Error,
            summary: Some(format!(
              "Invocation failed. Inspect the run for details: {error}"
            )),
            failure: Some(error.clone()),
          },
        ) {
          return Err(format!(
            "{error}; additionally failed to persist failed run: {finish_error}"
          ));
        }
        return Err(error);
      }
    };
    let status_code = if result.status == RunStatus::Completed {
      TraceStatusCode::Ok
    } else {
      TraceStatusCode::Error
    };
    self.finish_run(
      run,
      crate::run_builder::RunFinish {
        status_code,
        summary: Some(result.output_summary.clone()),
        failure: result.failure_message.clone(),
      },
    )?;
    Ok(result)
  }

  pub fn invoke_in_span(
    &self,
    run: &mut crate::run_builder::RecordingRun,
    parent: &crate::run_builder::SpanRef,
    request: InvokeRequest,
  ) -> AuvResult<InvokeResult> {
    let command_id = request.command_id.clone();
    let bundle_command = self.resolve_bundle_command(&command_id)?;
    let direct_command = self.commands.resolve(&command_id);

    match (bundle_command, direct_command) {
      (Some(bundle_command), Some(direct_command)) => Err(format!(
        "ambiguous command {command_id}; matched direct command {}.{} and bundle command {}",
        direct_command.driver_id,
        direct_command.operation,
        render_bundle_command_match(&bundle_command)
      )),
      (Some(bundle_command), None) => {
        self.invoke_bundle_command_in_span(run, parent, request, bundle_command)
      }
      (None, Some(direct_command)) => {
        self.invoke_direct_command_in_span(run, parent, request, direct_command)
      }
      (None, None) => Err(format!(
        "unknown command {command_id}; use `list-commands` or `auv-cli skill bundle list` to inspect available entries"
      )),
    }
  }

  fn invoke_direct_command_in_span(
    &self,
    run: &mut crate::run_builder::RecordingRun,
    parent: &crate::run_builder::SpanRef,
    request: InvokeRequest,
    command: &CommandSpec,
  ) -> AuvResult<InvokeResult> {
    let driver = self.drivers.get(command.driver_id).ok_or_else(|| {
      format!(
        "command {} resolved to missing driver {}",
        command.id, command.driver_id
      )
    })?;

    let command_span = run.start_span(
      parent,
      span_record(
        "auv.command.invoke",
        command_attributes(
          command.id,
          command.driver_id,
          command.operation,
          request.target.application_id.as_deref(),
        ),
      ),
    )?;
    record_event(
      run,
      command_span.id(),
      "command.resolved",
      Some(format!(
        "resolved {} -> {}.{}",
        command.id, command.driver_id, command.operation
      )),
    );

    let driver_span = run.start_span(
      &command_span,
      span_record(
        "auv.driver.invoke",
        command_attributes(
          command.id,
          command.driver_id,
          command.operation,
          request.target.application_id.as_deref(),
        ),
      ),
    )?;
    record_event(
      run,
      driver_span.id(),
      "driver.invoke",
      Some(format!(
        "invoking {}.{}",
        command.driver_id, command.operation
      )),
    );

    let call = DriverCall {
      operation: command.operation.to_string(),
      target: request.target,
      inputs: request.inputs,
      working_directory: self.project_root.clone(),
      run_context: DriverRunContext {
        run_id: run.id().to_string(),
        span_id: driver_span.id().to_string(),
        device_id: run.device_id().as_str().to_string(),
        session_id: run.session_id().as_str().to_string(),
      },
    };

    let mut artifact_paths = Vec::new();
    let mut artifact_records = Vec::new();
    let mut response_signals = Default::default();

    let (status, output_summary, failure_message) = match driver.invoke(&call) {
      Ok(response) => {
        response_signals = response.signals.clone();
        if let Some(backend) = &response.backend {
          record_event(
            run,
            driver_span.id(),
            "driver.backend",
            Some(format!("backend={backend}")),
          );
        }

        for note in &response.notes {
          record_event(run, driver_span.id(), "driver.note", Some(note.clone()));
        }

        let mut artifact_failure = None;
        for artifact in response.artifacts {
          let event_id = new_event_id();
          match self.recording.stage_artifact(
            run.id(),
            run.artifact_count(),
            artifact,
            driver_span.id(),
            Some(event_id.clone()),
          ) {
            Ok(stored_artifact) => {
              let staged_path = self
                .recording
                .run_dir(run.id())?
                .join(&stored_artifact.path);
              record_event_with_id(
                run,
                driver_span.id(),
                event_id,
                "artifact.captured",
                Some(render_artifact_event(&stored_artifact)),
                vec![stored_artifact.artifact_id.clone()],
              );
              artifact_paths.push(staged_path.clone());
              run.record_artifact(stored_artifact.clone());
              artifact_records.push(stored_artifact.clone());
              if let Err(error) =
                self
                  .recording
                  .record_artifact_bytes(run.id(), &stored_artifact, &staged_path)
              {
                record_event(
                  run,
                  driver_span.id(),
                  "artifact.failed",
                  Some(format!("artifact upload failed: {error}")),
                );
                artifact_failure = Some(error);
              }
            }
            Err(error) => {
              record_event(
                run,
                driver_span.id(),
                "artifact.failed",
                Some(format!("artifact staging failed: {error}")),
              );
              artifact_failure = Some(error);
              break;
            }
          }
        }

        if let Some(error) = artifact_failure {
          let output_summary = format!(
            "Artifact handling failed after run creation. Inspect {} for the recorded trace.",
            run.id()
          );
          record_event(
            run,
            driver_span.id(),
            "run.failed",
            Some(format!(
              "artifact handling failed after driver success: {error}"
            )),
          );
          (RunStatus::Failed, output_summary, Some(error))
        } else {
          let output_summary = response.summary.clone();
          record_event(
            run,
            command_span.id(),
            "run.completed",
            Some(response.summary),
          );
          (RunStatus::Completed, output_summary, None)
        }
      }
      Err(error) => {
        let output_summary = format!(
          "Driver invocation failed after run creation. Inspect {} for the recorded trace.",
          run.id()
        );
        record_event(run, driver_span.id(), "driver.failed", Some(error.clone()));
        (RunStatus::Failed, output_summary, Some(error))
      }
    };

    let status_code = if status == RunStatus::Completed {
      TraceStatusCode::Ok
    } else {
      TraceStatusCode::Error
    };
    let span_failure = failure_message.clone();
    run.finish_span(
      &driver_span,
      crate::run_builder::SpanFinish {
        status_code,
        summary: Some(output_summary.clone()),
        failure: span_failure.clone(),
      },
    )?;
    run.finish_span(
      &command_span,
      crate::run_builder::SpanFinish {
        status_code,
        summary: Some(output_summary.clone()),
        failure: span_failure,
      },
    )?;

    Ok(InvokeResult {
      run_id: run.id().to_string(),
      producer_span_id: driver_span.id().clone(),
      status,
      output_summary,
      signals: response_signals,
      artifacts: artifact_records,
      artifact_paths,
      failure_message,
    })
  }

  fn invoke_bundle_command_in_span(
    &self,
    run: &mut crate::run_builder::RecordingRun,
    parent: &crate::run_builder::SpanRef,
    request: InvokeRequest,
    bundle_command: ResolvedBundleCommand<'_>,
  ) -> AuvResult<InvokeResult> {
    let mut command_overrides = request.inputs;
    // NOTICE: bundle-backed invoke currently maps the generic invoke target to
    // the conventional `app_id` recipe input. Broader target schemas are
    // deferred until owner-approved bundle target metadata exists.
    if let Some(application_id) = request.target.application_id.as_ref() {
      command_overrides.insert("app_id".to_string(), application_id.clone());
    }
    let target_application_id = command_overrides
      .get("app_id")
      .map(String::as_str)
      .or(request.target.application_id.as_deref());

    let command_span = run.start_span(
      parent,
      span_record(
        "auv.command.invoke",
        bundle_command_attributes(
          &request.command_id,
          &bundle_command.bundle.manifest.metadata.id,
          &bundle_command.command.recipe_id,
          target_application_id,
        ),
      ),
    )?;
    record_event(
      run,
      command_span.id(),
      "command.resolved",
      Some(format!(
        "resolved {} -> {}",
        request.command_id,
        render_bundle_command_match(&bundle_command)
      )),
    );

    let recipe_span = run.start_span(
      &command_span,
      span_record(
        "auv.recipe.invoke",
        bundle_command_attributes(
          &request.command_id,
          &bundle_command.bundle.manifest.metadata.id,
          &bundle_command.command.recipe_id,
          target_application_id,
        ),
      ),
    )?;
    record_event(
      run,
      recipe_span.id(),
      "recipe.invoke",
      Some(format!(
        "invoking recipe {} from bundle {}",
        bundle_command.command.recipe_id, bundle_command.bundle.manifest.metadata.id
      )),
    );

    let artifact_start_index = run.artifact_count();
    let recipe = SkillRecipe::from_manifest(
      bundle_command.recipe.manifest.clone(),
      SkillRecipeOrigin::CatalogPath(bundle_command.recipe.path.clone()),
    );
    let result = SkillRecipeRunner::new(self).run_into_existing_run(
      &recipe,
      SkillRunOptions {
        dry_run: request.dry_run,
        max_disturbance: None,
        overrides: command_overrides,
        quiet: true,
      },
      run,
      &recipe_span,
    );
    let run_dir = self.recording.run_dir(run.id())?;
    let snapshot = run.snapshot_preview();
    let new_artifacts = snapshot
      .artifacts
      .into_iter()
      .skip(artifact_start_index)
      .collect::<Vec<_>>();
    let artifact_paths = new_artifacts
      .iter()
      .map(|artifact| run_dir.join(&artifact.path))
      .collect::<Vec<_>>();

    let (status, output_summary, failure_message, response_signals) = match result {
      Ok(summary) => {
        let output_summary = if request.dry_run {
          format!(
            "Dry-ran bundle command {} via {} -> {}",
            request.command_id,
            bundle_command.bundle.manifest.metadata.id,
            bundle_command.command.recipe_id
          )
        } else {
          format!(
            "Executed bundle command {} via {} -> {}",
            request.command_id,
            bundle_command.bundle.manifest.metadata.id,
            bundle_command.command.recipe_id
          )
        };
        record_event(run, command_span.id(), "run.completed", Some(output_summary.clone()));
        (RunStatus::Completed, output_summary, None, summary.exported_variables)
      }
      Err(error) => {
        let output_summary = format!(
          "Bundle command failed after run creation. Inspect {} for the recorded trace.",
          run.id()
        );
        record_event(run, recipe_span.id(), "recipe.failed", Some(error.clone()));
        record_event(run, command_span.id(), "run.failed", Some(error.clone()));
        (RunStatus::Failed, output_summary, Some(error), Default::default())
      }
    };

    let status_code = if status == RunStatus::Completed {
      TraceStatusCode::Ok
    } else {
      TraceStatusCode::Error
    };
    let span_failure = failure_message.clone();
    run.finish_span(
      &recipe_span,
      crate::run_builder::SpanFinish {
        status_code,
        summary: Some(output_summary.clone()),
        failure: span_failure.clone(),
      },
    )?;
    run.finish_span(
      &command_span,
      crate::run_builder::SpanFinish {
        status_code,
        summary: Some(output_summary.clone()),
        failure: span_failure,
      },
    )?;

    Ok(InvokeResult {
      run_id: run.id().to_string(),
      producer_span_id: recipe_span.id().clone(),
      status,
      output_summary,
      signals: response_signals,
      artifacts: new_artifacts,
      artifact_paths,
      failure_message,
    })
  }

  pub fn stage_artifact_file(
    &self,
    run: &mut crate::run_builder::RecordingRun,
    span: &crate::run_builder::SpanRef,
    role: impl Into<String>,
    source_path: &Path,
    preferred_name: impl Into<String>,
    summary: Option<String>,
  ) -> AuvResult<PathBuf> {
    let event_id = new_event_id();
    let artifact = self.recording.stage_artifact_file(
      run.id(),
      run.artifact_count(),
      span.id(),
      Some(event_id.clone()),
      ArtifactFileSource {
        role: role.into(),
        source_path: source_path.to_path_buf(),
        preferred_name: preferred_name.into(),
        summary,
      },
    )?;
    record_event_with_id(
      run,
      span.id(),
      event_id,
      "artifact.captured",
      Some(render_artifact_event(&artifact)),
      vec![artifact.artifact_id.clone()],
    );
    let staged_path = self.recording.run_dir(run.id())?.join(&artifact.path);
    run.record_artifact(artifact.clone());
    self
      .recording
      .record_artifact_bytes(run.id(), &artifact, &staged_path)?;
    Ok(staged_path)
  }

  pub fn stage_artifact_file_with_ref(
    &self,
    run: &mut crate::run_builder::RecordingRun,
    span: &crate::run_builder::SpanRef,
    role: impl Into<String>,
    source_path: &Path,
    preferred_name: impl Into<String>,
    summary: Option<String>,
  ) -> AuvResult<(PathBuf, ArtifactRef)> {
    let event_id = new_event_id();
    let artifact = self.recording.stage_artifact_file(
      run.id(),
      run.artifact_count(),
      span.id(),
      Some(event_id.clone()),
      ArtifactFileSource {
        role: role.into(),
        source_path: source_path.to_path_buf(),
        preferred_name: preferred_name.into(),
        summary,
      },
    )?;
    record_event_with_id(
      run,
      span.id(),
      event_id.clone(),
      "artifact.captured",
      Some(render_artifact_event(&artifact)),
      vec![artifact.artifact_id.clone()],
    );
    let staged_path = self.recording.run_dir(run.id())?.join(&artifact.path);
    let artifact_ref = ArtifactRef {
      run_id: run.id().clone(),
      artifact_id: artifact.artifact_id.clone(),
      span_id: span.id().clone(),
      captured_event_id: Some(event_id),
    };
    run.record_artifact(artifact.clone());
    self
      .recording
      .record_artifact_bytes(run.id(), &artifact, &staged_path)?;
    Ok((staged_path, artifact_ref))
  }

  fn resolve_bundle_command(&self, command_id: &str) -> AuvResult<Option<ResolvedBundleCommand<'_>>> {
    let mut matches = Vec::new();
    for bundle in self.bundles.entries() {
      for command in &bundle.manifest.commands {
        if command.id != command_id {
          continue;
        }
        let recipe = self.skills.resolve_recipe_id(&command.recipe_id).map_err(|error| {
          format!(
            "bundle {} command {} references unknown recipe {}: {error}",
            bundle.manifest.metadata.id, command.id, command.recipe_id
          )
        })?;
        matches.push(ResolvedBundleCommand {
          bundle,
          command,
          recipe,
        });
      }
    }

    if matches.len() > 1 {
      let rendered = matches
        .iter()
        .map(render_bundle_command_match)
        .collect::<Vec<_>>()
        .join(", ");
      return Err(format!(
        "ambiguous command {command_id}; multiple bundle commands matched: {rendered}"
      ));
    }

    Ok(matches.pop())
  }
}

struct ResolvedBundleCommand<'a> {
  bundle: &'a SkillBundleCatalogEntry,
  command: &'a SkillBundleCommand,
  recipe: &'a crate::skill::SkillCatalogEntry,
}

fn span_record(
  name: impl Into<String>,
  attributes: crate::run_builder::Attributes,
) -> SpanRecordV1Alpha1 {
  SpanRecordV1Alpha1 {
    api_version: SPAN_API_VERSION.to_string(),
    span_id: new_span_id(),
    parent_span_id: None,
    name: name.into(),
    state: TraceState::Running,
    status_code: TraceStatusCode::Unset,
    started_at_millis: now_millis(),
    finished_at_millis: None,
    attributes,
    summary: None,
    failure: None,
  }
}

fn bundle_command_attributes(
  command_id: &str,
  bundle_id: &str,
  recipe_id: &str,
  target_application_id: Option<&str>,
) -> crate::run_builder::Attributes {
  let mut attributes = crate::run_builder::Attributes::new();
  attributes.insert("command_id".to_string(), string_attr(command_id));
  attributes.insert("bundle_id".to_string(), string_attr(bundle_id));
  attributes.insert("recipe_id".to_string(), string_attr(recipe_id));
  attributes.insert("auv.command.id".to_string(), string_attr(command_id));
  attributes.insert("auv.bundle.id".to_string(), string_attr(bundle_id));
  attributes.insert("auv.recipe.id".to_string(), string_attr(recipe_id));
  if let Some(target_application_id) = target_application_id {
    attributes.insert(
      "target_application_id".to_string(),
      string_attr(target_application_id),
    );
    attributes.insert(
      "auv.target.application_id".to_string(),
      string_attr(target_application_id),
    );
  }
  attributes
}

fn command_attributes(
  command_id: &str,
  driver_id: &str,
  operation: &str,
  target_application_id: Option<&str>,
) -> crate::run_builder::Attributes {
  let mut attributes = crate::run_builder::Attributes::new();
  attributes.insert("command_id".to_string(), string_attr(command_id));
  attributes.insert("driver_id".to_string(), string_attr(driver_id));
  attributes.insert("operation".to_string(), string_attr(operation));
  attributes.insert("auv.command.id".to_string(), string_attr(command_id));
  attributes.insert("auv.driver.id".to_string(), string_attr(driver_id));
  attributes.insert("auv.driver.operation".to_string(), string_attr(operation));
  if let Some(target_application_id) = target_application_id {
    attributes.insert(
      "target_application_id".to_string(),
      string_attr(target_application_id),
    );
    attributes.insert(
      "auv.target.application_id".to_string(),
      string_attr(target_application_id),
    );
  }
  attributes
}

fn record_event(
  run: &mut crate::run_builder::RecordingRun,
  span_id: &crate::trace::SpanId,
  name: &str,
  message: Option<String>,
) {
  record_event_with_id(run, span_id, new_event_id(), name, message, Vec::new());
}

fn record_event_with_id(
  run: &mut crate::run_builder::RecordingRun,
  span_id: &crate::trace::SpanId,
  event_id: crate::trace::EventId,
  name: &str,
  message: Option<String>,
  artifact_ids: Vec<crate::trace::ArtifactId>,
) {
  run.record_event(EventRecordV1Alpha1 {
    api_version: EVENT_API_VERSION.to_string(),
    event_id,
    span_id: span_id.clone(),
    name: name.to_string(),
    timestamp_millis: now_millis(),
    attributes: Default::default(),
    message,
    artifact_ids,
  });
}

fn render_artifact_event(artifact: &crate::trace::ArtifactRecordV1Alpha1) -> String {
  let note = artifact
    .summary
    .clone()
    .unwrap_or_else(|| "n/a".to_string());
  format!(
    "{} kind={} path={} note={}",
    artifact.artifact_id, artifact.role, artifact.path, note
  )
}

fn render_bundle_command_match(bundle_command: &ResolvedBundleCommand<'_>) -> String {
  format!(
    "{}:{} -> {}",
    bundle_command.bundle.manifest.metadata.id,
    bundle_command.command.id,
    bundle_command.command.recipe_id
  )
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::fs;
  use std::path::PathBuf;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  use serde_json::json;

  use crate::bundle::{SkillBundleCatalog, SkillBundleCommand, SkillBundleManifest, SkillBundleMetadata, SkillBundleTarget, SkillBundleVerification, SkillBundleVersions};
  use super::Runtime;
  use crate::catalog::CommandCatalog;
  use crate::driver::{Driver, DriverRegistry};
  use crate::model::{
    AuvResult, CommandSpec, DriverCall, DriverDescriptor, DriverResponse, ExecutionTarget,
    InvokeRequest, ProducedArtifact, RunStatus, now_millis,
  };
  use crate::skill::SkillCatalog;
  use crate::recording::{MemoryRunRecorder, RunRecorder, RunUpdate};
  use crate::store::LocalStore;

  struct ArtifactFailureDriver;
  struct ArtifactSuccessDriver;
  struct CountingDriver {
    calls: Arc<AtomicUsize>,
  }
  struct FailRunFinishedRecorder;
  struct RequiredFailRunStartedRecorder;
  struct RequiredFailRunFinishedRecorder;
  struct SuccessDriver;

  impl RunRecorder for FailRunFinishedRecorder {
    fn record(&self, update: RunUpdate) -> AuvResult<()> {
      match update {
        RunUpdate::RunFinished { .. } => Err("run finished recorder failure".to_string()),
        _ => Ok(()),
      }
    }
  }

  impl RunRecorder for RequiredFailRunStartedRecorder {
    fn record(&self, update: RunUpdate) -> AuvResult<()> {
      match update {
        RunUpdate::RunStarted { .. } => Err("run started recorder failure".to_string()),
        _ => Ok(()),
      }
    }

    fn requires_successful_delivery(&self) -> bool {
      true
    }
  }

  impl RunRecorder for RequiredFailRunFinishedRecorder {
    fn record(&self, update: RunUpdate) -> AuvResult<()> {
      match update {
        RunUpdate::RunFinished { .. } => Err("run finished recorder failure".to_string()),
        _ => Ok(()),
      }
    }

    fn requires_successful_delivery(&self) -> bool {
      true
    }
  }

  impl Driver for ArtifactFailureDriver {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test.driver",
        summary: "Test driver",
        capabilities: &["test.artifact-failure"],
        donor_boundary: "test-only",
      }
    }

    fn invoke(&self, _call: &DriverCall) -> AuvResult<DriverResponse> {
      Ok(DriverResponse {
        summary: "driver succeeded before artifact staging".to_string(),
        backend: Some("test.backend".to_string()),
        signals: BTreeMap::new(),
        notes: vec!["note".to_string()],
        artifacts: vec![ProducedArtifact {
          kind: "text".to_string(),
          source_path: PathBuf::from("/definitely/missing/artifact.txt"),
          preferred_name: "artifact.txt".to_string(),
          note: Some("missing".to_string()),
        }],
      })
    }
  }

  impl Driver for ArtifactSuccessDriver {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test.driver",
        summary: "Test driver",
        capabilities: &["test.artifact-success"],
        donor_boundary: "test-only",
      }
    }

    fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
      let artifact_path = call
        .working_directory
        .join(format!("auv-artifact-{}.txt", now_millis()));
      fs::write(&artifact_path, "artifact body").expect("artifact source should be writable");
      Ok(DriverResponse {
        summary: "driver captured artifact".to_string(),
        backend: Some("test.backend".to_string()),
        signals: BTreeMap::new(),
        notes: vec![],
        artifacts: vec![ProducedArtifact {
          kind: "text".to_string(),
          source_path: artifact_path,
          preferred_name: "artifact.txt".to_string(),
          note: Some("captured".to_string()),
        }],
      })
    }
  }

  impl Driver for CountingDriver {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test.driver",
        summary: "Counting driver",
        capabilities: &["test.counting"],
        donor_boundary: "test-only",
      }
    }

    fn invoke(&self, _call: &DriverCall) -> AuvResult<DriverResponse> {
      self.calls.fetch_add(1, Ordering::SeqCst);
      Ok(DriverResponse {
        summary: "driver counted".to_string(),
        backend: Some("test.backend".to_string()),
        signals: BTreeMap::new(),
        notes: vec![],
        artifacts: vec![],
      })
    }
  }

  impl Driver for SuccessDriver {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test.driver",
        summary: "Test driver",
        capabilities: &["test.success"],
        donor_boundary: "test-only",
      }
    }

    fn invoke(&self, _call: &DriverCall) -> AuvResult<DriverResponse> {
      Ok(DriverResponse {
        summary: "driver ok".to_string(),
        backend: Some("test.backend".to_string()),
        signals: BTreeMap::from([("explicitSignal".to_string(), "driver".to_string())]),
        notes: vec![
          "bestMatchText=driver ok".to_string(),
          "explicitSignal=stale-note".to_string(),
          "plain note".to_string(),
        ],
        artifacts: vec![],
      })
    }
  }

  #[test]
  fn invoke_in_span_adds_command_under_parent_span() {
    let project_root = temp_dir("runtime-recorded-project");
    let store_root = temp_dir("runtime-recorded-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let mut run = runtime
      .start_run(crate::run_builder::RunSpec::new(
        crate::trace::RunType::Execute,
        "auv.execute",
      ))
      .expect("run should start");
    let parent = run.root_span();
    let result = runtime
      .invoke_in_span(
        &mut run,
        &parent,
        InvokeRequest {
          command_id: "test.invoke".to_string(),
          target: ExecutionTarget::default(),
          inputs: BTreeMap::new(),
          dry_run: false,
        },
      )
      .expect("recorded invoke should succeed");
    assert_eq!(result.status, RunStatus::Completed);
    assert_eq!(
      result.signals.get("explicitSignal"),
      Some(&"driver".to_string())
    );
    assert!(!result.signals.contains_key("bestMatchText"));
    let run_id = runtime
      .finish_run(
        run,
        crate::run_builder::RunFinish {
          status_code: crate::trace::TraceStatusCode::Ok,
          summary: Some("done".to_string()),
          failure: None,
        },
      )
      .expect("run should finish");

    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    assert_eq!(canonical.run.run_type, crate::trace::RunType::Execute);
    assert!(
      canonical
        .spans
        .iter()
        .any(|span| span.name == "auv.command.invoke")
    );
    let command_span = canonical
      .spans
      .iter()
      .find(|span| span.name == "auv.command.invoke")
      .expect("command span should be recorded");
    assert_eq!(
      command_span.attributes.get("auv.command.id"),
      Some(&json!("test.invoke"))
    );
    assert_eq!(
      command_span.attributes.get("auv.driver.id"),
      Some(&json!("test.driver"))
    );
    assert_eq!(
      command_span.attributes.get("auv.driver.operation"),
      Some(&json!("test_operation"))
    );
    assert!(
      canonical
        .spans
        .iter()
        .any(|span| span.parent_span_id.as_ref() == Some(parent.id()))
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_unknown_command_finishes_failed_implicit_run() {
    let project_root = temp_dir("runtime-unknown-command-project");
    let store_root = temp_dir("runtime-unknown-command-store");
    let recorder = Arc::new(MemoryRunRecorder::new());
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone())
      .with_recorder(recorder.clone());

    let error = runtime
      .invoke(InvokeRequest {
        command_id: "test.missing".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect_err("unknown command should fail");

    assert!(error.contains("unknown command"));
    let run_id = recorder
      .drain_for_test()
      .into_iter()
      .find_map(|update| match update {
        RunUpdate::RunFinished { run, .. } => Some(run.run_id),
        _ => None,
      })
      .expect("failed implicit run should finish");
    let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
    assert_eq!(
      canonical.run.status_code,
      crate::trace::TraceStatusCode::Error
    );
    assert!(
      canonical
        .run
        .failure
        .expect("failure")
        .message
        .contains("unknown command")
    );
    assert!(
      canonical
        .spans
        .iter()
        .all(|span| span.state == crate::trace::TraceState::Ended)
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_succeeds_when_run_finished_recorder_delivery_fails_after_snapshot_write() {
    let project_root = temp_dir("runtime-run-finished-recorder-failure-project");
    let store_root = temp_dir("runtime-run-finished-recorder-failure-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone())
      .with_recorder(Arc::new(FailRunFinishedRecorder));

    let result = runtime
      .invoke(InvokeRequest {
        command_id: "test.invoke".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect("recorder failure after snapshot write should not fail invoke");

    let canonical = runtime
      .read_run(&result.run_id)
      .expect("snapshot should still be persisted");
    assert_eq!(canonical.run.status_code, crate::trace::TraceStatusCode::Ok);

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_aborts_before_driver_when_required_initial_recording_fails() {
    let project_root = temp_dir("runtime-required-initial-recorder-failure-project");
    let store_root = temp_dir("runtime-required-initial-recorder-failure-store");
    let calls = Arc::new(AtomicUsize::new(0));
    let runtime = Runtime::new(
      project_root.clone(),
      CommandCatalog::new(vec![CommandSpec {
        id: "test.invoke",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test invoke",
        driver_id: "test.driver",
        operation: "test_operation",
        disturbance_classes: &[crate::model::DisturbanceClass::None],
        max_disturbance: crate::model::DisturbanceClass::None,
      }]),
      DriverRegistry::new(vec![Box::new(CountingDriver {
        calls: calls.clone(),
      })]),
      LocalStore::new(store_root.clone()).expect("store should initialize"),
    )
    .with_recorder(Arc::new(RequiredFailRunStartedRecorder));

    let error = runtime
      .invoke(InvokeRequest {
        command_id: "test.invoke".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect_err("required initial recording failure should abort invoke");

    assert!(error.contains("run recording delivery failed"));
    assert!(error.contains("run started recorder failure"));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_fails_when_required_run_finished_recorder_delivery_fails() {
    let project_root = temp_dir("runtime-required-recorder-failure-project");
    let store_root = temp_dir("runtime-required-recorder-failure-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone())
      .with_recorder(Arc::new(RequiredFailRunFinishedRecorder));

    let error = runtime
      .invoke(InvokeRequest {
        command_id: "test.invoke".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect_err("required recorder delivery failure should fail invoke");

    assert!(error.contains("run recording delivery failed"));
    assert!(error.contains("run finished recorder failure"));
    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_links_artifact_capture_event_to_artifact_record() {
    let project_root = temp_dir("runtime-artifact-link-project");
    let store_root = temp_dir("runtime-artifact-link-store");
    let runtime = Runtime::new(
      project_root.clone(),
      CommandCatalog::new(vec![CommandSpec {
        id: "test.invoke",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test invoke",
        driver_id: "test.driver",
        operation: "test_operation",
        disturbance_classes: &[crate::model::DisturbanceClass::None],
        max_disturbance: crate::model::DisturbanceClass::None,
      }]),
      DriverRegistry::new(vec![Box::new(ArtifactSuccessDriver)]),
      LocalStore::new(store_root.clone()).expect("store should initialize"),
    );

    let result = runtime
      .invoke(InvokeRequest {
        command_id: "test.invoke".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect("artifact capture should succeed");

    let canonical = runtime.read_run(&result.run_id).expect("run should read");
    let artifact = canonical
      .artifacts
      .first()
      .expect("artifact should be recorded");
    let event_id = artifact
      .event_id
      .as_ref()
      .expect("artifact should point to event");
    let event = canonical
      .events
      .iter()
      .find(|event| event.event_id == *event_id)
      .expect("artifact event should be recorded");
    assert_eq!(event.name, "artifact.captured");
    assert_eq!(event.artifact_ids, vec![artifact.artifact_id.clone()]);
    assert_eq!(
      result.artifact_paths,
      vec![
        store_root
          .join("runs")
          .join(&result.run_id)
          .join(&artifact.path)
      ]
    );
    assert!(result.artifact_paths[0].exists());

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_persists_failed_run_when_artifact_staging_breaks() {
    let project_root = temp_dir("runtime-tests-project");
    let store_root = temp_dir("runtime-tests-store");
    let runtime = Runtime::new(
      project_root.clone(),
      CommandCatalog::new(vec![CommandSpec {
        id: "test.invoke",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test invoke",
        driver_id: "test.driver",
        operation: "test_operation",
        disturbance_classes: &[crate::model::DisturbanceClass::None],
        max_disturbance: crate::model::DisturbanceClass::None,
      }]),
      DriverRegistry::new(vec![Box::new(ArtifactFailureDriver)]),
      LocalStore::new(store_root.clone()).expect("store should initialize"),
    );

    let result = runtime
      .invoke(InvokeRequest {
        command_id: "test.invoke".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect("artifact staging failures should still return an inspectable run");

    assert_eq!(result.status, RunStatus::Failed);
    assert!(result.failure_message.is_some());

    let inspection = runtime
      .inspect(&result.run_id)
      .expect("failed run should still be inspectable");
    assert!(inspection.contains("Status: error"));
    assert!(inspection.contains("artifact staging failed"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn temp_dir(label: &str) -> PathBuf {
    let path = env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be creatable");
    path
  }

  /// Driver that stashes the run context of every call so tests can assert
  /// what the runtime actually threaded through `DriverRunContext`.
  struct ContextCapturingDriver {
    contexts: Arc<std::sync::Mutex<Vec<crate::model::DriverRunContext>>>,
  }

  impl Driver for ContextCapturingDriver {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test.driver",
        summary: "Captures the driver run context",
        capabilities: &["test.capture"],
        donor_boundary: "test-only",
      }
    }

    fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
      self
        .contexts
        .lock()
        .expect("context capture lock")
        .push(call.run_context.clone());
      Ok(DriverResponse {
        summary: "captured".to_string(),
        backend: None,
        signals: BTreeMap::new(),
        notes: vec![],
        artifacts: vec![],
      })
    }
  }

  fn runtime_with_context_capture(
    project_root: PathBuf,
    store_root: PathBuf,
    contexts: Arc<std::sync::Mutex<Vec<crate::model::DriverRunContext>>>,
  ) -> Runtime {
    Runtime::new(
      project_root,
      CommandCatalog::new(vec![CommandSpec {
        id: "test.invoke",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test invoke",
        driver_id: "test.driver",
        operation: "test_operation",
        disturbance_classes: &[crate::model::DisturbanceClass::None],
        max_disturbance: crate::model::DisturbanceClass::None,
      }]),
      DriverRegistry::new(vec![Box::new(ContextCapturingDriver { contexts })]),
      LocalStore::new(store_root).expect("store should initialize"),
    )
  }

  #[test]
  fn start_run_with_default_spec_stamps_local_default_attributes() {
    let project_root = temp_dir("runtime-default-device-project");
    let store_root = temp_dir("runtime-default-device-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let run = runtime
      .start_run(crate::run_builder::RunSpec::new(
        crate::trace::RunType::Command,
        "auv.command",
      ))
      .expect("default-spec run should start");
    assert_eq!(run.device_id().as_str(), "local");
    assert_eq!(run.session_id().as_str(), "default");

    runtime
      .finish_run(
        run,
        crate::run_builder::RunFinish {
          status_code: crate::trace::TraceStatusCode::Ok,
          summary: Some("default".to_string()),
          failure: None,
        },
      )
      .expect("default-spec run should finish");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn start_run_with_explicit_device_session_overrides_defaults() {
    let project_root = temp_dir("runtime-explicit-device-project");
    let store_root = temp_dir("runtime-explicit-device-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let spec = crate::run_builder::RunSpec::new(crate::trace::RunType::Command, "auv.command")
      .with_device(crate::trace::DeviceId::new("remote-mac"))
      .with_session(crate::trace::SessionId::new("music"));
    let run = runtime
      .start_run(spec)
      .expect("explicit-device run should start");
    assert_eq!(run.device_id().as_str(), "remote-mac");
    assert_eq!(run.session_id().as_str(), "music");

    runtime
      .finish_run(
        run,
        crate::run_builder::RunFinish {
          status_code: crate::trace::TraceStatusCode::Ok,
          summary: Some("explicit".to_string()),
          failure: None,
        },
      )
      .expect("explicit-device run should finish");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_threads_device_session_into_driver_run_context() {
    let project_root = temp_dir("runtime-driver-ctx-project");
    let store_root = temp_dir("runtime-driver-ctx-store");
    let contexts = Arc::new(std::sync::Mutex::new(Vec::new()));
    let runtime =
      runtime_with_context_capture(project_root.clone(), store_root.clone(), contexts.clone());

    runtime
      .invoke(InvokeRequest {
        command_id: "test.invoke".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: false,
      })
      .expect("default-target invoke should succeed");

    let captured = contexts.lock().expect("context capture lock").clone();
    assert_eq!(captured.len(), 1, "driver should be called exactly once");
    let ctx = &captured[0];
    assert_eq!(ctx.device_id, "local");
    assert_eq!(ctx.session_id, "default");
    assert!(!ctx.run_id.is_empty(), "run_id should be set");
    assert!(!ctx.span_id.is_empty(), "span_id should be set");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn run_snapshot_stores_device_session_in_attributes() {
    let project_root = temp_dir("runtime-attr-roundtrip-project");
    let store_root = temp_dir("runtime-attr-roundtrip-store");
    let runtime = runtime_with_success_driver(project_root.clone(), store_root.clone());

    let spec = crate::run_builder::RunSpec::new(crate::trace::RunType::Command, "auv.command")
      .with_device(crate::trace::DeviceId::new("local"))
      .with_session(crate::trace::SessionId::new("scan"));
    let run = runtime.start_run(spec).expect("run should start");
    let run_id = run.id().as_str().to_string();
    runtime
      .finish_run(
        run,
        crate::run_builder::RunFinish {
          status_code: crate::trace::TraceStatusCode::Ok,
          summary: Some("attr".to_string()),
          failure: None,
        },
      )
      .expect("run should finish");

    let canonical = runtime.read_run(&run_id).expect("run snapshot should read");
    let attrs = &canonical.run.attributes;
    assert_eq!(
      attrs.get(crate::trace::RUN_ATTR_DEVICE_ID),
      Some(&json!("local"))
    );
    assert_eq!(
      attrs.get(crate::trace::RUN_ATTR_SESSION_ID),
      Some(&json!("scan"))
    );

    // Old on-disk layout invariant: `.auv/runs/{run_id}/` directory, no
    // per-device or per-session subdir inserted.
    let run_dir = store_root.join("runs").join(&run_id);
    assert!(run_dir.exists(), "run dir must remain at runs/{{run_id}}");

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn runtime_with_success_driver(project_root: PathBuf, store_root: PathBuf) -> Runtime {
    Runtime::new(
      project_root,
      CommandCatalog::new(vec![CommandSpec {
        id: "test.invoke",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test invoke",
        driver_id: "test.driver",
        operation: "test_operation",
        disturbance_classes: &[crate::model::DisturbanceClass::None],
        max_disturbance: crate::model::DisturbanceClass::None,
      }]),
      DriverRegistry::new(vec![Box::new(SuccessDriver)]),
      LocalStore::new(store_root).expect("store should initialize"),
    )
  }

  #[test]
  fn invoke_rejects_ambiguous_bundle_and_direct_command_id() {
    let project_root = runtime_bundle_project_root("ambiguous-command");
    let store_root = temp_dir("runtime-ambiguous-bundle-store");
    let runtime = runtime_with_bundle_catalog(
      project_root.clone(),
      store_root.clone(),
      CommandCatalog::new(vec![CommandSpec {
        id: "test.bundle.command".to_string().leak(),
        namespace: crate::model::CommandNamespace::Test,
        summary: "Test direct invoke",
        driver_id: "test.driver",
        operation: "test_operation",
        disturbance_classes: &[crate::model::DisturbanceClass::None],
        max_disturbance: crate::model::DisturbanceClass::None,
      }]),
      skill_catalog_for_project(&project_root),
      bundle_catalog_with_command("test.bundle.command", "test.bundle.recipe"),
      DriverRegistry::new(vec![Box::new(SuccessDriver)]),
    );

    let error = runtime
      .invoke(InvokeRequest {
        command_id: "test.bundle.command".to_string(),
        target: ExecutionTarget::default(),
        inputs: BTreeMap::new(),
        dry_run: true,
      })
      .expect_err("ambiguous command should fail");

    assert!(error.contains("ambiguous command"));
    assert!(error.contains("direct command"));
    assert!(error.contains("bundle command"));

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  #[test]
  fn invoke_bundle_command_dry_run_executes_recipe_path() {
    let project_root = runtime_bundle_project_root("bundle-dry-run");
    let store_root = temp_dir("runtime-bundle-dry-run-store");
    let runtime = runtime_with_bundle_catalog(
      project_root.clone(),
      store_root.clone(),
      CommandCatalog::new(vec![CommandSpec {
        id: "test.skill.invoke",
        namespace: crate::model::CommandNamespace::Test,
        summary: "Recipe step command",
        driver_id: "test.driver",
        operation: "test_operation",
        disturbance_classes: &[crate::model::DisturbanceClass::None],
        max_disturbance: crate::model::DisturbanceClass::None,
      }]),
      skill_catalog_for_project(&project_root),
      bundle_catalog_with_command("test.bundle.command", "test.bundle.recipe"),
      DriverRegistry::new(vec![Box::new(SuccessDriver)]),
    );

    let result = runtime
      .invoke(InvokeRequest {
        command_id: "test.bundle.command".to_string(),
        target: ExecutionTarget {
          application_id: Some("com.example.BundleApp".to_string()),
        },
        inputs: BTreeMap::new(),
        dry_run: true,
      })
      .expect("bundle dry-run should succeed");

    assert_eq!(result.status, RunStatus::Completed);
    assert!(result.output_summary.contains("Dry-ran bundle command"));
    assert!(result.artifacts.is_empty());
    let canonical = runtime.read_run(&result.run_id).expect("run should read");
    assert!(
      canonical
        .spans
        .iter()
        .any(|span| span.name == "auv.recipe.invoke"),
      "bundle path should record recipe span"
    );
    let command_span = canonical
      .spans
      .iter()
      .find(|span| span.name == "auv.command.invoke")
      .expect("command span should exist");
    assert_eq!(
      command_span.attributes.get("auv.bundle.id"),
      Some(&json!("test.bundle.v0"))
    );
    assert_eq!(
      command_span.attributes.get("auv.recipe.id"),
      Some(&json!("test.bundle.recipe"))
    );
    assert_eq!(
      command_span.attributes.get("auv.target.application_id"),
      Some(&json!("com.example.BundleApp"))
    );

    let _ = fs::remove_dir_all(project_root);
    let _ = fs::remove_dir_all(store_root);
  }

  fn runtime_with_bundle_catalog(
    project_root: PathBuf,
    store_root: PathBuf,
    commands: CommandCatalog,
    skills: SkillCatalog,
    bundles: SkillBundleCatalog,
    drivers: DriverRegistry,
  ) -> Runtime {
    Runtime::new_with_catalogs(
      project_root,
      commands,
      bundles,
      skills,
      drivers,
      LocalStore::new(store_root).expect("store should initialize"),
    )
  }

  fn runtime_bundle_project_root(label: &str) -> PathBuf {
    let project_root = temp_dir(&format!("runtime-bundle-project-{label}"));
    let recipe_dir = project_root.join("recipes/test");
    fs::create_dir_all(&recipe_dir).expect("recipe dir should exist");
    let manifest = json!({
      "recipe_id": "test.bundle.recipe",
      "version": "0.1.0",
      "status": "experimental-recipe",
      "platform": "macOS",
      "target_app": { "bundle_id": "${app_id}", "display_mode": "fixture" },
      "strategy": {
        "family": "native-text",
        "grounding": "ax-text",
        "activation": "pointer-focus-clipboard-paste",
        "verificationContract": "verifyAxText"
      },
      "objective": "test bundle-backed invoke",
      "inputs": {
        "app_id": { "type": "string", "default": "com.example.BundleApp" }
      },
      "disturbance_policy": {
        "max_disturbance": "none",
        "declared_classes": ["none"]
      },
      "steps": [{
        "id": "first",
        "command_id": "test.skill.invoke",
        "disturbance": {
          "classes": ["none"],
          "max": "none"
        },
        "expect": {
          "output_must_contain": ["outcome=ok"]
        }
      }],
      "verification": {
        "expected_signals": ["signal"],
        "success_criteria": ["criteria"]
      }
    });
    fs::write(
      recipe_dir.join("bundle-recipe.v0.json"),
      serde_json::to_string_pretty(&manifest).expect("manifest should serialize"),
    )
    .expect("manifest should write");
    project_root
  }

  fn skill_catalog_for_project(project_root: &std::path::Path) -> SkillCatalog {
    SkillCatalog::discover(project_root).expect("skill catalog should load")
  }

  fn bundle_catalog_with_command(command_id: &str, recipe_id: &str) -> SkillBundleCatalog {
    let manifest = SkillBundleManifest {
      api_version: "auv.ai/v1alpha1".to_string(),
      kind: "SkillBundle".to_string(),
      metadata: SkillBundleMetadata {
        id: "test.bundle.v0".to_string(),
        name: "Test Bundle".to_string(),
        version: "0.1.0".to_string(),
        status: "working".to_string(),
      },
      commands: vec![SkillBundleCommand {
        id: command_id.to_string(),
        recipe_id: recipe_id.to_string(),
        summary: "Test bundle command".to_string(),
      }],
      target: SkillBundleTarget {
        application_family: "native-macos-apps".to_string(),
        platform: "macOS".to_string(),
      },
      versions: SkillBundleVersions {
        auv: ">=0.0.1, <0.1.0".to_string(),
        target_application: String::new(),
      },
      members: Vec::new(),
      verification: SkillBundleVerification {
        expected_signals: vec!["signal".to_string()],
        success_criteria: vec!["criteria".to_string()],
        non_goals: Vec::new(),
      },
      known_limits: Vec::new(),
    };
    SkillBundleCatalog::from_entries_for_test(vec![crate::bundle::SkillBundleCatalogEntry {
      manifest,
      path: PathBuf::from("/tmp/test-bundle.v0.json"),
    }])
  }
}
