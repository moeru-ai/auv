use std::fs;
use std::path::PathBuf;

use auv_game_minecraft::{
  ScenePacketInputs, ScenePacketOutput, SourceRunSummary, SpatialBundleInputs, SpatialBundleOutput,
  SpatialBundleSourceArtifact, TextureSweepInputs, TextureSweepPreparationInputs,
  TextureSweepPreparationOutput, TextureSweepReport, TextureSweepSampleBuildInputs,
  TextureSweepSampleBuildOutput, TextureSweepThresholds, TrainingPackageInputs,
  TrainingPackageOutput, build_texture_sweep_samples_from_bundles, evaluate_texture_sweep,
  export_3dgs_scene_packet, export_3dgs_training_package, export_spatial_bundle,
  prepare_texture_sweep_resource_packs,
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
pub const MINECRAFT_TEXTURE_SWEEP_PREP_ARTIFACT_ROLE: &str = "minecraft-texture-sweep-prep";
pub const MINECRAFT_TEXTURE_SWEEP_RUNBOOK_ARTIFACT_ROLE: &str = "minecraft-texture-sweep-runbook";
pub const MINECRAFT_3DGS_SCENE_PACKET_ARTIFACT_ROLE: &str = "minecraft-3dgs-scene-packet";
pub const MINECRAFT_3DGS_SCENE_PACKET_INSPECT_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-scene-packet-inspect";
pub const MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE: &str = "minecraft-3dgs-training-package";
pub const MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE: &str =
  "minecraft-3dgs-training-package-inspect";
pub const MINECRAFT_PROJECTION_CALIBRATION_ARTIFACT_ROLE: &str = "minecraft-projection-calibration";

pub fn run_minecraft_3dgs_scene_packet_export(
  recording: &RecordingHandle,
  bundle_manifest_paths: Vec<PathBuf>,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<ScenePacketOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.minecraft.export_3dgs_scene_packet"),
    "Minecraft export MC-7 3DGS scene packet",
    |context| {
      context.record_event(
        "minecraft.export_3dgs_scene_packet.inputs",
        Some(format!(
          "bundle_manifests={} output_dir={} trained_3dgs=false action_path=false",
          bundle_manifest_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(","),
          output_dir.display()
        )),
      );
      let result = export_3dgs_scene_packet(ScenePacketInputs {
        bundle_manifest_paths: bundle_manifest_paths.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("minecraft.export_3dgs_scene_packet.artifacts", |context| {
        context.stage_artifact_file(
          MINECRAFT_3DGS_SCENE_PACKET_ARTIFACT_ROLE,
          &result.manifest_path,
          "minecraft-3dgs-scene-packet-run.json",
          Some("MC-7 3DGS input scene packet manifest; offline inspect artifact only".to_string()),
        )?;
        context.stage_artifact_file(
          MINECRAFT_3DGS_SCENE_PACKET_INSPECT_ARTIFACT_ROLE,
          &result.inspect_report_path,
          "minecraft-3dgs-scene-packet-inspect.json",
          Some(
            "MC-7 accepted-only scene packet inspect report; offline inspect artifact only"
              .to_string(),
          ),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_3dgs_training_package_export(
  recording: &RecordingHandle,
  scene_packet_manifest_path: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<TrainingPackageOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(
      RunType::Execute,
      "auv.minecraft.export_3dgs_training_package",
    ),
    "Minecraft export MC-7 D3 training-prep package",
    |context| {
      context.record_event(
        "minecraft.export_3dgs_training_package.inputs",
        Some(format!(
          "scene_packet_manifest={} output_dir={} trained_3dgs=false trainer_backend=false",
          scene_packet_manifest_path.display(),
          output_dir.display()
        )),
      );
      let result = export_3dgs_training_package(TrainingPackageInputs {
        scene_packet_manifest_path: scene_packet_manifest_path.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span(
        "minecraft.export_3dgs_training_package.artifacts",
        |context| {
          context.stage_artifact_file(
            MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE,
            &result.manifest_path,
            "minecraft-3dgs-training-package-run.json",
            Some(
              "MC-7 D3 canonical training-prep package manifest; offline inspect artifact only"
                .to_string(),
            ),
          )?;
          context.stage_artifact_file(
            MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE,
            &result.inspect_report_path,
            "minecraft-3dgs-training-package-inspect.json",
            Some(
              "MC-7 D3 training-prep inspect report plus Nerfstudio compatibility view status"
                .to_string(),
            ),
          )?;
          Ok::<_, String>(())
        },
      )?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_texture_sweep_preparation(
  recording: &RecordingHandle,
  sidecar_run_dir: PathBuf,
  output_dir: PathBuf,
) -> AuvResult<RecordedOperationOutput<TextureSweepPreparationOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(RunType::Execute, "auv.minecraft.prepare_texture_sweep"),
    "Minecraft prepare MC-6 texture sweep inputs",
    |context| {
      context.record_event(
        "minecraft.prepare_texture_sweep.inputs",
        Some(format!(
          "sidecar_run_dir={} output_dir={} live_chain=false",
          sidecar_run_dir.display(),
          output_dir.display()
        )),
      );
      let result = prepare_texture_sweep_resource_packs(TextureSweepPreparationInputs {
        sidecar_run_dir: sidecar_run_dir.clone(),
        output_dir: output_dir.clone(),
      })?;
      context.in_span("minecraft.prepare_texture_sweep.artifacts", |context| {
        context.stage_artifact_file(
          MINECRAFT_TEXTURE_SWEEP_PREP_ARTIFACT_ROLE,
          &result.manifest_path,
          "mc6-texture-sweep-prep.json",
          Some("MC-6 texture sweep preparation manifest".to_string()),
        )?;
        context.stage_artifact_file(
          MINECRAFT_TEXTURE_SWEEP_RUNBOOK_ARTIFACT_ROLE,
          &result.runbook_path,
          "mc6-texture-sweep-runbook.md",
          Some("MC-6 texture sweep manual runbook".to_string()),
        )?;
        Ok::<_, String>(())
      })?;
      Ok::<_, String>(result)
    },
  )
}

pub fn run_minecraft_texture_sweep_sample_build(
  recording: &RecordingHandle,
  bundle_manifest_paths: Vec<PathBuf>,
  output_path: PathBuf,
) -> AuvResult<RecordedOperationOutput<TextureSweepSampleBuildOutput>> {
  recording.run_recorded_operation(
    RunSpec::new(
      RunType::Execute,
      "auv.minecraft.build_texture_sweep_samples",
    ),
    "Minecraft build MC-6 texture sweep samples",
    |context| {
      context.record_event(
        "minecraft.build_texture_sweep_samples.inputs",
        Some(format!(
          "bundle_manifests={} output={}",
          bundle_manifest_paths
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(","),
          output_path.display()
        )),
      );
      let result = build_texture_sweep_samples_from_bundles(TextureSweepSampleBuildInputs {
        bundle_manifest_paths: bundle_manifest_paths.clone(),
        output_path: output_path.clone(),
      })?;
      context.in_span(
        "minecraft.build_texture_sweep_samples.artifacts",
        |context| {
          context.stage_artifact_file(
            MINECRAFT_TEXTURE_SWEEP_SAMPLES_ARTIFACT_ROLE,
            &result.output_path,
            "texture_sweep_samples.json",
            Some("MC-6 texture sweep samples built from spatial bundles".to_string()),
          )?;
          Ok::<_, String>(())
        },
      )?;
      Ok::<_, String>(result)
    },
  )
}

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

  fn identity_matrix() -> [f64; 16] {
    [
      1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]
  }

  fn write_sample_bundle(temp: &std::path::Path) -> PathBuf {
    let bundle_dir = temp.join("bundle");
    let screenshots_dir = bundle_dir.join("screenshots");
    let frames_dir = bundle_dir.join("spatial_frames");
    fs::create_dir_all(&screenshots_dir).expect("screenshots dir");
    fs::create_dir_all(&frames_dir).expect("frames dir");
    fs::write(screenshots_dir.join("artifact_0001-frame.png"), b"png").expect("screenshot");
    let frame = auv_game_minecraft::MinecraftSpatialFrame {
      spatial_frame_id: "frame-rich".to_string(),
      world_tick: 1,
      monotonic_timestamp_ms: 1_000,
      telemetry_session_id: None,
      viewport: auv_game_minecraft::Viewport::new(800, 600),
      view_matrix: identity_matrix(),
      projection_matrix: identity_matrix(),
      player_pose: auv_game_minecraft::PlayerPose {
        eye_position: auv_game_minecraft::Vec3::new(0.0, 0.0, 0.0),
        yaw: 0.0,
        pitch: 0.0,
      },
      raycast_hit: Some(auv_game_minecraft::RaycastHit {
        block_pos: auv_game_minecraft::BlockPosition::new(0, 0, 0),
        face: auv_game_minecraft::BlockFace::North,
        block_id: "minecraft:stone".to_string(),
      }),
      nearby_blocks: Vec::new(),
      nearby_entities: Vec::new(),
      inventory_summary: Vec::new(),
      screenshot_artifact_ref: Some("artifact://artifact_0001".to_string()),
      mc_capture_skew_ms: Some(10),
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: vec!["fabric".to_string(), "file/auv-mc6-rich".to_string()],
    };
    fs::write(
      frames_dir.join("artifact_0001-frame-rich.json"),
      serde_json::to_vec_pretty(&frame).expect("frame json"),
    )
    .expect("frame write");
    let manifest = auv_game_minecraft::SpatialBundleManifest {
      schema_version: auv_game_minecraft::SPATIAL_BUNDLE_SCHEMA_VERSION,
      source_run: auv_game_minecraft::SourceRunSummary {
        source_run_id: "run_1".to_string(),
        source_operation: "auv.minecraft.bridge".to_string(),
        source_run_type: "execute".to_string(),
        source_status: "ok".to_string(),
        generated_at_millis: 1,
        auv_git_commit: None,
        exporter_git_commit: None,
      },
      counts: auv_game_minecraft::SpatialBundleCounts {
        screenshots: 1,
        spatial_frames: 1,
        ..auv_game_minecraft::SpatialBundleCounts::default()
      },
      artifacts: vec![
        auv_game_minecraft::SpatialBundleArtifactRecord {
          artifact_id: "artifact_0001".to_string(),
          role: "minecraft-screenshot".to_string(),
          source_path: "artifacts/frame.png".to_string(),
          bundle_path: "screenshots/artifact_0001-frame.png".to_string(),
          directory: auv_game_minecraft::SpatialBundleDirectory::Screenshots,
          summary: None,
        },
        auv_game_minecraft::SpatialBundleArtifactRecord {
          artifact_id: "artifact_0002".to_string(),
          role: "minecraft-spatial-frame".to_string(),
          source_path: "artifacts/frame-rich.json".to_string(),
          bundle_path: "spatial_frames/artifact_0001-frame-rich.json".to_string(),
          directory: auv_game_minecraft::SpatialBundleDirectory::SpatialFrames,
          summary: None,
        },
      ],
      known_limits: Vec::new(),
    };
    let manifest_path = bundle_dir.join("run.json");
    fs::write(
      &manifest_path,
      serde_json::to_vec_pretty(&manifest).expect("manifest json"),
    )
    .expect("manifest write");
    manifest_path
  }

  #[test]
  fn three_dgs_scene_packet_export_records_manifest_artifact() {
    let temp = temp_dir("mc7-scene-packet");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let manifest_path = write_sample_bundle(&temp);

    let output = run_minecraft_3dgs_scene_packet_export(
      &recording,
      vec![manifest_path],
      temp.join("scene-packet"),
    )
    .expect("scene packet export");

    assert_eq!(output.value.manifest.counts.frames, 1);
    assert_eq!(output.value.manifest.counts.screenshots, 1);
    assert!(output.value.inspect_report_path.is_file());
    assert_eq!(output.value.inspect_report.counts.camera_records, 1);
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("scene packet run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_SCENE_PACKET_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_SCENE_PACKET_INSPECT_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn three_dgs_training_package_export_records_manifest_and_inspect_artifacts() {
    let temp = temp_dir("mc7-training-package");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let manifest_path = write_sample_bundle(&temp);
    let scene_packet = run_minecraft_3dgs_scene_packet_export(
      &recording,
      vec![manifest_path],
      temp.join("scene-packet"),
    )
    .expect("scene packet export");

    let output = run_minecraft_3dgs_training_package_export(
      &recording,
      scene_packet.value.manifest_path.clone(),
      temp.join("training-package"),
    )
    .expect("training package export");

    assert_eq!(output.value.manifest.counts.frames, 1);
    assert!(output.value.manifest_path.is_file());
    assert!(output.value.inspect_report_path.is_file());
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("training package run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_3DGS_TRAINING_PACKAGE_INSPECT_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
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
  fn texture_sweep_preparation_records_manifest_and_runbook() {
    let temp = temp_dir("mc6-texture-sweep-prep");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();

    let output = run_minecraft_texture_sweep_preparation(
      &recording,
      temp.join("sidecar-run"),
      temp.join("prep-output"),
    )
    .expect("texture sweep prep");

    assert_eq!(output.value.manifest.profiles.len(), 3);
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("prep run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_TEXTURE_SWEEP_PREP_ARTIFACT_ROLE)
    );
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_TEXTURE_SWEEP_RUNBOOK_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(temp);
  }

  #[test]
  fn texture_sweep_sample_build_records_samples_artifact() {
    let temp = temp_dir("mc6-texture-sweep-samples");
    let store = LocalStore::new(temp.join("store")).expect("store");
    let recording = RunRecordingBackend::new(store, Arc::new(NoopRunRecorder)).handle();
    let manifest_path = write_sample_bundle(&temp);

    let output = run_minecraft_texture_sweep_sample_build(
      &recording,
      vec![manifest_path],
      temp.join("samples.json"),
    )
    .expect("sample build");

    assert_eq!(output.value.sample_set.samples.len(), 1);
    let run = recording
      .read_run(output.run_id.as_str())
      .expect("sample build run should persist");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == MINECRAFT_TEXTURE_SWEEP_SAMPLES_ARTIFACT_ROLE)
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
            refusal_reason: None,
          },
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "flat-pack".to_string(),
            texture_profile: "flat_color".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 4.0,
            occlusion_iou: 0.92,
            refused_noise: false,
            refusal_reason: None,
          },
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "flat-pack".to_string(),
            texture_profile: "flat_color".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 20.0,
            occlusion_iou: 0.10,
            refused_noise: true,
            refusal_reason: Some(auv_game_minecraft::MismatchRefusalReason::MenuLoadingScreen),
          },
          auv_game_minecraft::TextureSweepSample {
            resource_pack: "repeat-pack".to_string(),
            texture_profile: "repetitive".to_string(),
            duration_seconds: 30.0,
            pose_error_px: 3.0,
            occlusion_iou: 0.93,
            refused_noise: false,
            refusal_reason: None,
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
