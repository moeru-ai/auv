use std::fs;
use std::path::PathBuf;

use auv_game_minecraft::{
  SourceRunSummary, SpatialBundleInputs, SpatialBundleOutput, SpatialBundleSourceArtifact,
  TextureSweepInputs, TextureSweepReport, TextureSweepThresholds, evaluate_texture_sweep,
  export_spatial_bundle,
};
use auv_tracing_driver::RecordingHandle;
use auv_tracing_driver::recorded_operation::RecordedOperationOutput;
use auv_tracing_driver::run_builder::RunSpec;
use auv_tracing_driver::store::CanonicalRun;
use auv_tracing_driver::trace::{RunType, TraceStatusCode};

use crate::model::AuvResult;

pub const MINECRAFT_SPATIAL_FRAME_ARTIFACT_ROLE: &str = "minecraft-spatial-frame";
pub const MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE: &str = "minecraft-spatial-bundle";
pub const MINECRAFT_TEXTURE_SWEEP_SAMPLES_ARTIFACT_ROLE: &str = "minecraft-texture-sweep-samples";
pub const MINECRAFT_TEXTURE_SWEEP_ARTIFACT_ROLE: &str = "minecraft-texture-sweep";

pub fn run_minecraft_spatial_bundle_export(
  recording: &RecordingHandle,
  source_run_id: String,
  output_dir: PathBuf,
  git_commit: Option<String>,
) -> AuvResult<RecordedOperationOutput<SpatialBundleOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.minecraft.export_spatial_bundle"),
    "Minecraft export spatial dataset bundle",
    |context| {
      context.record_event(
        "minecraft.export_spatial_bundle.inputs",
        Some(format!(
          "source_run_id={} output_dir={}",
          source_run_id,
          output_dir.display()
        )),
      );
      let source_run = context.recording().read_run(&source_run_id)?;
      let source_run_dir = context.recording().run_dir(&source_run_id)?;
      let result = export_spatial_bundle(SpatialBundleInputs {
        output_dir: output_dir.clone(),
        source: source_run_summary(&source_run, git_commit.clone()),
        artifacts: source_bundle_artifacts(source_run_dir, &source_run),
      })?;
      context.in_span("minecraft.export_spatial_bundle.artifacts", |context| {
        let manifest_path = result.output_dir.join("run.json");
        context.stage_artifact_file(
          MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE,
          &manifest_path,
          "minecraft-spatial-bundle-run.json",
          Some("MC-6 spatial dataset bundle manifest".to_string()),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_texture_sweep_eval(
  recording: &RecordingHandle,
  samples_path: PathBuf,
  output_dir: PathBuf,
  require_real_source: bool,
) -> AuvResult<RecordedOperationOutput<TextureSweepReport>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.minecraft.eval_texture_sweep"),
    "Minecraft evaluate 2.5D texture sweep",
    |context| {
      context.record_event(
        "minecraft.eval_texture_sweep.inputs",
        Some(format!(
          "samples={} output_dir={} thresholds=mc6_v0 require_real_source={}",
          samples_path.display(),
          output_dir.display(),
          require_real_source
        )),
      );
      let report = evaluate_texture_sweep(&TextureSweepInputs {
        samples_path: samples_path.clone(),
        output_dir: output_dir.clone(),
        thresholds: TextureSweepThresholds::mc6_v0(),
        require_real_source,
      })?;
      context.in_span("minecraft.eval_texture_sweep.artifacts", |context| {
        context.stage_artifact_file(
          MINECRAFT_TEXTURE_SWEEP_SAMPLES_ARTIFACT_ROLE,
          &samples_path,
          "texture_sweep_samples.json",
          Some("MC-6 texture sweep input samples".to_string()),
        )?;
        let report_path = output_dir.join("texture_sweep_report.json");
        context.stage_artifact_file(
          MINECRAFT_TEXTURE_SWEEP_ARTIFACT_ROLE,
          &report_path,
          "texture_sweep_report.json",
          Some("MC-6 texture sweep p95/IoU report".to_string()),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(report)
    },
  )
}

pub fn current_git_commit() -> Option<String> {
  let output = std::process::Command::new("git")
    .args(["rev-parse", "HEAD"])
    .output()
    .ok()?;
  if !output.status.success() {
    return None;
  }
  let commit = String::from_utf8(output.stdout).ok()?.trim().to_string();
  (!commit.is_empty()).then_some(commit)
}

fn source_run_summary(source_run: &CanonicalRun, git_commit: Option<String>) -> SourceRunSummary {
  SourceRunSummary {
    source_run_id: source_run.run.run_id.as_str().to_string(),
    source_operation: source_run
      .spans
      .iter()
      .find(|span| span.span_id == source_run.run.root_span_id)
      .map(|span| span.name.clone())
      .unwrap_or_else(|| "unknown".to_string()),
    source_run_type: source_run.run.run_type.as_str().to_string(),
    source_status: source_run.run.status_code.as_str().to_string(),
    generated_at_millis: auv_tracing_driver::now_millis(),
    auv_git_commit: git_commit.clone(),
    exporter_git_commit: git_commit,
  }
}

fn source_bundle_artifacts(
  source_run_dir: PathBuf,
  source_run: &CanonicalRun,
) -> Vec<SpatialBundleSourceArtifact> {
  source_run
    .artifacts
    .iter()
    .map(|artifact| SpatialBundleSourceArtifact {
      artifact_id: artifact.artifact_id.as_str().to_string(),
      role: artifact.role.clone(),
      source_path: source_run_dir.join(&artifact.path),
      source_run_path: artifact.path.clone(),
      summary: artifact.summary.clone(),
    })
    .collect()
}

pub fn read_spatial_bundle_manifest(
  path: PathBuf,
) -> AuvResult<auv_game_minecraft::SpatialBundleManifest> {
  let bytes = fs::read(&path).map_err(|error| {
    format!(
      "failed to read minecraft spatial bundle manifest {}: {error}",
      path.display()
    )
  })?;
  serde_json::from_slice::<auv_game_minecraft::SpatialBundleManifest>(&bytes).map_err(|error| {
    format!(
      "failed to parse minecraft spatial bundle manifest {}: {error}",
      path.display()
    )
  })
}

pub fn texture_sweep_status(report: &TextureSweepReport) -> TraceStatusCode {
  if report.passed {
    TraceStatusCode::Ok
  } else {
    TraceStatusCode::Error
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_tracing_driver::RunRecordingBackend;
  use auv_tracing_driver::recording::NoopRunRecorder;
  use auv_tracing_driver::run_builder::RunSpec;
  use auv_tracing_driver::store::LocalStore;
  use auv_tracing_driver::trace::RunType;
  use std::sync::Arc;

  fn temp_dir(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("auv-{}-{}", label, crate::model::now_millis()));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir should be created");
    path
  }

  #[test]
  fn spatial_bundle_export_reads_source_run_and_records_manifest() {
    let temp = temp_dir("mc6-spatial-bundle");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let source_file = temp.join("frame.json");
    fs::write(&source_file, br#"{"spatial_frame_id":"frame-1"}"#).expect("source write");

    let source = recording
      .run_recorded_operation(
        RunSpec::new(RunType::Execute, "auv.minecraft.fixture"),
        "fixture source run",
        |context| {
          context.stage_artifact_file(
            MINECRAFT_SPATIAL_FRAME_ARTIFACT_ROLE,
            &source_file,
            "frame.json",
            Some("frame".to_string()),
          )?;
          Ok::<_, String>(())
        },
      )
      .expect("source run");

    let output_dir = temp.join("bundle");
    let output = run_minecraft_spatial_bundle_export(
      &recording,
      source.run_id.as_str().to_string(),
      output_dir.clone(),
      Some("abc123".to_string()),
    )
    .expect("bundle export");

    assert_eq!(output.value.manifest.counts.spatial_frames, 1);
    assert!(output_dir.join("run.json").is_file());
    let export_run = recording
      .read_run(output.run_id.as_str())
      .expect("export run should persist");
    assert!(
      export_run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_SPATIAL_BUNDLE_ARTIFACT_ROLE)
    );
    let manifests = crate::run_read::list_minecraft_spatial_bundle_manifests(
      recording.recording_backend().store(),
      output.run_id.as_str(),
    )
    .expect("spatial bundle manifests should list");
    assert_eq!(manifests.len(), 1);
    assert_eq!(
      manifests[0]
        .manifest
        .as_ref()
        .expect("manifest should parse")
        .source_run
        .source_run_id,
      source.run_id.as_str()
    );

    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn texture_sweep_eval_records_report_artifact() {
    let temp = temp_dir("mc6-texture-sweep");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let samples_path = temp.join("samples.json");
    fs::write(
      &samples_path,
      serde_json::to_vec_pretty(&auv_game_minecraft::TextureSweepSampleSet {
        source: None,
        samples: vec![
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "rich-pack".to_string(),
            texture_profile: "rich".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 2.0,
            occlusion_iou: 0.95,
            refused_noise: false,
          },
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "flat-pack".to_string(),
            texture_profile: "flat_color".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 4.0,
            occlusion_iou: 0.92,
            refused_noise: false,
          },
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "flat-pack".to_string(),
            texture_profile: "flat_color".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 20.0,
            occlusion_iou: 0.10,
            refused_noise: true,
          },
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "repeat-pack".to_string(),
            texture_profile: "repetitive".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 3.0,
            occlusion_iou: 0.93,
            refused_noise: false,
          },
        ],
      })
      .expect("samples json"),
    )
    .expect("samples write");

    let output =
      run_minecraft_texture_sweep_eval(&recording, samples_path, temp.join("sweep-output"), false)
        .expect("sweep eval");

    assert!(output.value.passed);
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_TEXTURE_SWEEP_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_TEXTURE_SWEEP_SAMPLES_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
  }
}
