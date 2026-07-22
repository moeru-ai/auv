use std::collections::BTreeSet;
use std::fs;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::training_package::{
  TrainingCompatibilityStatus, TrainingCompatibilityViewReport, TrainingPackageCounts, TrainingPackageInspectReport, TrainingPackageManifest,
};

pub type TrainingLaunchPreparationResult<T> = Result<T, String>;

pub const TRAINING_LAUNCH_PLAN_SCHEMA_VERSION: u32 = 1;
pub const TRAINING_LAUNCH_INSPECT_REPORT_SCHEMA_VERSION: u32 = 1;

const NERFSTUDIO_VIEW_NAME: &str = "nerfstudio";
const TRAINER_BACKEND: &str = "nerfstudio.splatfacto";
const TRAINER_PROBE_COMMAND: &str = "ns-train --help";
// TODO(mc7-d6-trainer-backends): D5 intentionally fixes one trainer backend and one launch shape.
// Add backend selection only in an owner-approved follow-up that keeps D5 launch-prep evidence stable.
const TRAINER_LAUNCH_SUBCOMMAND: &str = "splatfacto";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingLaunchPreparationInputs {
  pub training_package_manifest_path: PathBuf,
  pub output_dir: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingLaunchPreparationOutput {
  pub output_dir: PathBuf,
  pub manifest_path: PathBuf,
  pub inspect_report_path: PathBuf,
  pub runbook_path: PathBuf,
  pub manifest: TrainingLaunchPlanManifest,
  pub inspect_report: TrainingLaunchInspectReport,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingLaunchPlanManifest {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub source_training_package_manifest_path: String,
  pub source_training_package_inspect_report_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub counts: TrainingPackageCounts,
  pub compatibility_view_name: String,
  pub trainer_backend: String,
  pub training_data_dir: String,
  #[serde(default)]
  pub transforms_path: Option<String>,
  pub export_report_path: String,
  pub suggested_output_dir: String,
  pub launch_command: String,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TrainingLaunchInspectReport {
  pub schema_version: u32,
  pub generated_at_millis: u64,
  pub training_launch_manifest_path: String,
  pub source_training_package_manifest_path: String,
  pub source_scene_packet_manifest_path: String,
  pub source_bundle_manifest_paths: Vec<String>,
  pub source_run_ids: Vec<String>,
  pub compatibility_status: TrainingCompatibilityStatus,
  pub trainer_readiness: TrainingLaunchReadiness,
  #[serde(default)]
  pub readiness_blocker: Option<TrainingLaunchReadinessBlocker>,
  pub probe_command: String,
  pub probe_succeeded: bool,
  pub exported_frame_count: usize,
  pub skipped_frame_count: usize,
  pub transforms_present: bool,
  pub warnings: Vec<String>,
  pub known_limits: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingLaunchReadiness {
  Ready,
  Blocked,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrainingLaunchReadinessBlocker {
  CompatibilityViewBlocked,
  TransformsMissing,
  TrainerCommandUnavailable,
}

pub fn prepare_3dgs_training_launch(
  inputs: TrainingLaunchPreparationInputs,
) -> TrainingLaunchPreparationResult<TrainingLaunchPreparationOutput> {
  prepare_3dgs_training_launch_with_probe(inputs, default_trainer_probe)
}

fn prepare_3dgs_training_launch_with_probe<F>(
  inputs: TrainingLaunchPreparationInputs,
  probe: F,
) -> TrainingLaunchPreparationResult<TrainingLaunchPreparationOutput>
where
  F: Fn(&str, &[&str]) -> bool,
{
  let training_package_manifest =
    read_json_file::<TrainingPackageManifest>(&inputs.training_package_manifest_path, "MC-7 D3 training package manifest")?;
  let training_package_dir = inputs.training_package_manifest_path.parent().ok_or_else(|| {
    format!("MC-7 D3 training package manifest {} has no parent directory", inputs.training_package_manifest_path.display())
  })?;

  let training_package_inspect_report_path = training_package_dir.join("inspect_report.json");
  let training_package_inspect_report =
    read_json_file::<TrainingPackageInspectReport>(&training_package_inspect_report_path, "MC-7 D3 training package inspect report")?;

  let manifest_view = find_compatibility_view(&training_package_manifest.compatibility_views, "MC-7 D3 training package manifest")?;
  let inspect_view =
    find_compatibility_view(&training_package_inspect_report.compatibility_views, "MC-7 D3 training package inspect report")?;

  let compatibility_export_report_path = training_package_dir.join(&inspect_view.export_report_path);
  ensure_file_readable(&compatibility_export_report_path, "MC-7 D3 Nerfstudio compatibility export report JSON")?;

  let transforms_path = inspect_view.transforms_path.as_ref().map(|path| training_package_dir.join(path));
  if inspect_view.transforms_path.is_some() {
    let declared_path = transforms_path.as_ref().expect("transforms path should exist when declared");
    ensure_file_readable(declared_path, "MC-7 D3 Nerfstudio transforms JSON")?;
  }

  let training_data_dir = training_package_dir.join("compat/nerfstudio");
  let compatibility_images_dir = training_data_dir.join("images");
  if inspect_view.exported_frame_count > 0 {
    ensure_directory_exists(&training_data_dir, "MC-7 D3 Nerfstudio compatibility data directory")?;
    ensure_directory_exists(&compatibility_images_dir, "MC-7 D3 Nerfstudio compatibility images directory")?;
  }

  let suggested_output_dir = inputs.output_dir.join("trainer-output/nerfstudio-splatfacto");
  let launch_command =
    format!("ns-train {TRAINER_LAUNCH_SUBCOMMAND} --data {} --output-dir {}", sh_quote(&training_data_dir), sh_quote(&suggested_output_dir));
  let probe_succeeded = probe("ns-train", &["--help"]);

  let transforms_present = transforms_path.is_some();
  let (trainer_readiness, readiness_blocker) = if inspect_view.status == TrainingCompatibilityStatus::Blocked {
    (TrainingLaunchReadiness::Blocked, Some(TrainingLaunchReadinessBlocker::CompatibilityViewBlocked))
  } else if inspect_view.exported_frame_count > 0 && !transforms_present {
    (TrainingLaunchReadiness::Blocked, Some(TrainingLaunchReadinessBlocker::TransformsMissing))
  } else if !probe_succeeded {
    (TrainingLaunchReadiness::Blocked, Some(TrainingLaunchReadinessBlocker::TrainerCommandUnavailable))
  } else {
    (TrainingLaunchReadiness::Ready, None)
  };

  let generated_at_millis = crate::run_read::now_millis();
  let manifest_path = inputs.output_dir.join("minecraft-3dgs-training-launch-plan.json");
  let inspect_report_path = inputs.output_dir.join("minecraft-3dgs-training-launch-inspect.json");
  let runbook_path = inputs.output_dir.join("mc7-training-launch-runbook.md");

  let mut warnings = BTreeSet::new();
  warnings.extend(training_package_inspect_report.warnings.iter().cloned());
  warnings.extend(inspect_view.warnings.iter().cloned());

  let mut known_limits = BTreeSet::new();
  known_limits.extend(training_package_manifest.known_limits.iter().cloned());
  known_limits.extend(training_package_inspect_report.known_limits.iter().cloned());
  known_limits.insert("MC-7 D5 training launch prep only; no trainer process is started and no trained splat is produced".to_string());

  let manifest = TrainingLaunchPlanManifest {
    schema_version: TRAINING_LAUNCH_PLAN_SCHEMA_VERSION,
    generated_at_millis,
    source_training_package_manifest_path: inputs.training_package_manifest_path.to_string_lossy().into_owned(),
    source_training_package_inspect_report_path: training_package_inspect_report_path.to_string_lossy().into_owned(),
    source_scene_packet_manifest_path: training_package_manifest.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: training_package_manifest.source_bundle_manifest_paths.clone(),
    source_run_ids: training_package_manifest.source_run_ids.clone(),
    counts: training_package_manifest.counts.clone(),
    compatibility_view_name: manifest_view.view_name.clone(),
    trainer_backend: TRAINER_BACKEND.to_string(),
    training_data_dir: training_data_dir.to_string_lossy().into_owned(),
    transforms_path: inspect_view.transforms_path.clone(),
    export_report_path: inspect_view.export_report_path.clone(),
    suggested_output_dir: suggested_output_dir.to_string_lossy().into_owned(),
    launch_command,
    known_limits: known_limits.iter().cloned().collect(),
  };
  write_json(&manifest_path, &manifest, "MC-7 D5 training launch plan JSON")?;

  let inspect_report = TrainingLaunchInspectReport {
    schema_version: TRAINING_LAUNCH_INSPECT_REPORT_SCHEMA_VERSION,
    generated_at_millis,
    training_launch_manifest_path: manifest_path.to_string_lossy().into_owned(),
    source_training_package_manifest_path: inputs.training_package_manifest_path.to_string_lossy().into_owned(),
    source_scene_packet_manifest_path: training_package_manifest.source_scene_packet_manifest_path.clone(),
    source_bundle_manifest_paths: training_package_manifest.source_bundle_manifest_paths.clone(),
    source_run_ids: training_package_manifest.source_run_ids.clone(),
    compatibility_status: inspect_view.status,
    trainer_readiness,
    readiness_blocker,
    probe_command: TRAINER_PROBE_COMMAND.to_string(),
    probe_succeeded,
    exported_frame_count: inspect_view.exported_frame_count,
    skipped_frame_count: inspect_view.skipped_frame_count,
    transforms_present,
    warnings: warnings.iter().cloned().collect(),
    known_limits: known_limits.iter().cloned().collect(),
  };
  write_json(&inspect_report_path, &inspect_report, "MC-7 D5 training launch inspect JSON")?;

  fs::create_dir_all(&inputs.output_dir)
    .map_err(|error| format!("failed to create MC-7 D5 training launch output directory {}: {error}", inputs.output_dir.display()))?;
  fs::write(&runbook_path, render_runbook(&manifest, &inspect_report).as_bytes())
    .map_err(|error| format!("failed to write MC-7 D5 training launch runbook {}: {error}", runbook_path.display()))?;

  Ok(TrainingLaunchPreparationOutput {
    output_dir: inputs.output_dir,
    manifest_path,
    inspect_report_path,
    runbook_path,
    manifest,
    inspect_report,
  })
}

fn default_trainer_probe(command: &str, arguments: &[&str]) -> bool {
  Command::new(command).args(arguments).status().map(|status| status.success()).unwrap_or(false)
}

fn render_runbook(manifest: &TrainingLaunchPlanManifest, inspect_report: &TrainingLaunchInspectReport) -> String {
  let mut output = String::new();
  output.push_str("# MC-7 training launch runbook\n\n");
  output.push_str("This is a preparation artifact. It does not start a trainer process and does not prove MC-7 training quality.\n\n");
  output.push_str(&format!(
    "- trainer backend: `{}`\n- compatibility view: `{}`\n- readiness: `{}`\n",
    manifest.trainer_backend,
    manifest.compatibility_view_name,
    match inspect_report.trainer_readiness {
      TrainingLaunchReadiness::Ready => "ready",
      TrainingLaunchReadiness::Blocked => "blocked",
    }
  ));
  if let Some(blocker) = inspect_report.readiness_blocker {
    output.push_str(&format!(
      "- readiness blocker: `{}`\n",
      match blocker {
        TrainingLaunchReadinessBlocker::CompatibilityViewBlocked => "compatibility_view_blocked",
        TrainingLaunchReadinessBlocker::TransformsMissing => "transforms_missing",
        TrainingLaunchReadinessBlocker::TrainerCommandUnavailable => {
          "trainer_command_unavailable"
        }
      }
    ));
  }
  output.push_str(&format!(
    "- exported frames: `{}`\n- skipped frames: `{}`\n- probe command: `{}`\n\n",
    inspect_report.exported_frame_count, inspect_report.skipped_frame_count, inspect_report.probe_command
  ));
  output.push_str("Suggested launch command:\n\n```bash\n");
  output.push_str(&manifest.launch_command);
  output.push_str("\n```\n\n");
  output.push_str("Notes:\n");
  output.push_str("- D5 keeps the D3 package authoritative; it does not copy training inputs into a second dataset tree.\n");
  output.push_str("- If readiness is `trainer_command_unavailable`, install a local Nerfstudio CLI and rerun `prepare-3dgs-training`.\n");
  output.push_str(
    "- If readiness is `compatibility_view_blocked`, regenerate D3 from a package that exports at least one Nerfstudio-compatible frame.\n",
  );
  output.push_str(
    "- If readiness is `transforms_missing`, treat the D3 package as corrupted input and rebuild D3 before attempting training.\n",
  );
  output
}

fn find_compatibility_view<'a>(
  views: &'a [TrainingCompatibilityViewReport],
  source: &str,
) -> TrainingLaunchPreparationResult<&'a TrainingCompatibilityViewReport> {
  views
    .iter()
    .find(|view| view.view_name == NERFSTUDIO_VIEW_NAME)
    .ok_or_else(|| format!("{source} has no {NERFSTUDIO_VIEW_NAME} compatibility view"))
}

fn ensure_file_readable(path: &Path, label: &str) -> TrainingLaunchPreparationResult<()> {
  fs::File::open(path).map(|_| ()).map_err(|error| format!("failed to open {label} {}: {error}", path.display()))
}

fn ensure_directory_exists(path: &Path, label: &str) -> TrainingLaunchPreparationResult<()> {
  let metadata = fs::metadata(path).map_err(|error| format!("failed to stat {label} {}: {error}", path.display()))?;
  if !metadata.is_dir() {
    return Err(format!("{label} {} is not a directory", path.display()));
  }
  Ok(())
}

fn sh_quote(path: &Path) -> String {
  format!("\"{}\"", path.to_string_lossy().replace('\\', "\\\\").replace('"', "\\\""))
}

fn write_json(path: &Path, value: &impl Serialize, label: &str) -> TrainingLaunchPreparationResult<()> {
  if let Some(parent) = path.parent() {
    fs::create_dir_all(parent).map_err(|error| format!("failed to create {label} directory {}: {error}", parent.display()))?;
  }
  let json = serde_json::to_string_pretty(value)
    .map(|mut json| {
      json.push('\n');
      json
    })
    .map_err(|error| format!("failed to serialize {label}: {error}"))?;
  fs::write(path, json.as_bytes()).map_err(|error| format!("failed to write {label} {}: {error}", path.display()))
}

fn read_json_file<T: DeserializeOwned>(path: &Path, label: &str) -> TrainingLaunchPreparationResult<T> {
  let file = fs::File::open(path).map_err(|error| format!("failed to open {label} {}: {error}", path.display()))?;
  serde_json::from_reader(BufReader::new(file)).map_err(|error| format!("failed to parse {label} {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
  use super::*;

  use image::{Rgba, RgbaImage};
  use tempfile::TempDir;

  use crate::training_package::{
    TrainingCompatibilityFrameDecision, TrainingCompatibilitySkipReason, TrainingCompatibilitySkipReasonCount, TrainingPackageFrameRecord,
  };

  #[test]
  fn prepares_training_launch_happy_path_when_probe_succeeds() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_training_package_fixture(
      &temp,
      TrainingPackageFixtureSpec {
        compatibility_status: TrainingCompatibilityStatus::Ready,
        exported_frame_count: 2,
        skipped_frame_count: 0,
        declare_transforms_path: true,
        create_transforms_file: true,
        create_inspect_report: true,
        create_export_report: true,
        warnings: vec![],
      },
    );

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
      },
      |_command, _arguments| true,
    )
    .expect("launch prep should succeed");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Ready);
    assert_eq!(output.inspect_report.readiness_blocker, None);
    assert!(output.manifest.launch_command.contains("compat/nerfstudio"));
    assert!(output.manifest_path.is_file());
    assert!(output.inspect_report_path.is_file());
    assert!(output.runbook_path.is_file());
  }

  #[test]
  fn blocked_when_trainer_probe_fails() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_training_package_fixture(
      &temp,
      TrainingPackageFixtureSpec {
        compatibility_status: TrainingCompatibilityStatus::Ready,
        exported_frame_count: 1,
        skipped_frame_count: 0,
        declare_transforms_path: true,
        create_transforms_file: true,
        create_inspect_report: true,
        create_export_report: true,
        warnings: vec![],
      },
    );

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
      },
      |_command, _arguments| false,
    )
    .expect("launch prep should still write blocked outputs");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Blocked);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchReadinessBlocker::TrainerCommandUnavailable));
    assert!(output.manifest_path.is_file());
    assert!(output.inspect_report_path.is_file());
    assert!(output.runbook_path.is_file());
  }

  #[test]
  fn partial_compatibility_can_still_be_ready_when_transforms_exist() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_training_package_fixture(
      &temp,
      TrainingPackageFixtureSpec {
        compatibility_status: TrainingCompatibilityStatus::Partial,
        exported_frame_count: 1,
        skipped_frame_count: 2,
        declare_transforms_path: true,
        create_transforms_file: true,
        create_inspect_report: true,
        create_export_report: true,
        warnings: vec!["one frame skipped".to_string()],
      },
    );

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
      },
      |_command, _arguments| true,
    )
    .expect("partial compatibility with transforms should still prepare");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Ready);
    assert!(output.inspect_report.warnings.iter().any(|value| value == "one frame skipped"));
  }

  #[test]
  fn blocked_compatibility_writes_outputs_and_sets_blocker() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_training_package_fixture(
      &temp,
      TrainingPackageFixtureSpec {
        compatibility_status: TrainingCompatibilityStatus::Blocked,
        exported_frame_count: 0,
        skipped_frame_count: 2,
        declare_transforms_path: false,
        create_transforms_file: false,
        create_inspect_report: true,
        create_export_report: true,
        warnings: vec!["no compatible frames".to_string()],
      },
    );

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
      },
      |_command, _arguments| true,
    )
    .expect("blocked compatibility should still produce D5 outputs");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Blocked);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchReadinessBlocker::CompatibilityViewBlocked));
    assert!(output.manifest_path.is_file());
    assert!(output.inspect_report_path.is_file());
    assert!(output.runbook_path.is_file());
  }

  #[test]
  fn hard_fails_when_inspect_report_is_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_training_package_fixture(
      &temp,
      TrainingPackageFixtureSpec {
        compatibility_status: TrainingCompatibilityStatus::Ready,
        exported_frame_count: 1,
        skipped_frame_count: 0,
        declare_transforms_path: true,
        create_transforms_file: true,
        create_inspect_report: false,
        create_export_report: true,
        warnings: vec![],
      },
    );

    let error = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
      },
      |_command, _arguments| true,
    )
    .expect_err("missing inspect report should fail");

    assert!(error.contains("failed to open MC-7 D3 training package inspect report"));
  }

  #[test]
  fn hard_fails_when_export_report_is_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_training_package_fixture(
      &temp,
      TrainingPackageFixtureSpec {
        compatibility_status: TrainingCompatibilityStatus::Ready,
        exported_frame_count: 1,
        skipped_frame_count: 0,
        declare_transforms_path: true,
        create_transforms_file: true,
        create_inspect_report: true,
        create_export_report: false,
        warnings: vec![],
      },
    );

    let error = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
      },
      |_command, _arguments| true,
    )
    .expect_err("missing export report should fail");

    assert!(error.contains("failed to open MC-7 D3 Nerfstudio compatibility export report JSON"));
  }

  #[test]
  fn hard_fails_when_declared_transforms_file_is_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_training_package_fixture(
      &temp,
      TrainingPackageFixtureSpec {
        compatibility_status: TrainingCompatibilityStatus::Ready,
        exported_frame_count: 1,
        skipped_frame_count: 0,
        declare_transforms_path: true,
        create_transforms_file: false,
        create_inspect_report: true,
        create_export_report: true,
        warnings: vec![],
      },
    );

    let error = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
      },
      |_command, _arguments| true,
    )
    .expect_err("missing declared transforms should fail");

    assert!(error.contains("failed to open MC-7 D3 Nerfstudio transforms JSON"));
  }

  #[derive(Clone)]
  struct TrainingPackageFixtureSpec {
    compatibility_status: TrainingCompatibilityStatus,
    exported_frame_count: usize,
    skipped_frame_count: usize,
    declare_transforms_path: bool,
    create_transforms_file: bool,
    create_inspect_report: bool,
    create_export_report: bool,
    warnings: Vec<String>,
  }

  fn write_training_package_fixture(temp: &TempDir, spec: TrainingPackageFixtureSpec) -> PathBuf {
    let training_dir = temp.path().join("training-package");
    let compat_dir = training_dir.join("compat/nerfstudio");
    let images_dir = compat_dir.join("images");
    fs::create_dir_all(training_dir.join("frames")).expect("frames dir");
    if spec.exported_frame_count > 0 {
      fs::create_dir_all(&images_dir).expect("images dir");
      for index in 1..=spec.exported_frame_count {
        write_png(&images_dir.join(format!("frame_{index:06}.png")));
      }
    }

    let transforms_path = spec.declare_transforms_path.then_some("compat/nerfstudio/transforms.json".to_string());
    if spec.create_export_report {
      write_json(
        &compat_dir.join("export_report.json"),
        &serde_json::json!({
          "view_name": NERFSTUDIO_VIEW_NAME,
          "status": spec.compatibility_status,
          "exported_frame_count": spec.exported_frame_count,
          "skipped_frame_count": spec.skipped_frame_count
        }),
        "MC-7 D5 fixture export report JSON",
      )
      .expect("export report write");
    }
    if spec.create_transforms_file {
      write_json(
        &compat_dir.join("transforms.json"),
        &serde_json::json!({
          "camera_model": "OPENCV",
          "frames": [{"file_path": "images/frame_000001.png", "transform_matrix": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]]}]
        }),
        "MC-7 D5 fixture transforms JSON",
      )
      .expect("transforms write");
    }

    let frame_records = vec![TrainingPackageFrameRecord {
      frame_index: 1,
      spatial_frame_id: "frame-1".to_string(),
      source_run_id: "run-1".to_string(),
      source_bundle_manifest_path: "/tmp/run-1/run.json".to_string(),
      source_scene_packet_frame_json_path: "frames/frame_000001.json".to_string(),
      canonical_frame_json_path: "frames/frame_000001.json".to_string(),
      canonical_image_path: Some("images/frame_000001.png".to_string()),
      screen_state: Some("in_game".to_string()),
      resource_pack_ids: vec!["fabric".to_string(), "file/auv-mc6-rich".to_string()],
      primary_file_resource_pack_id: Some("file/auv-mc6-rich".to_string()),
      compatibility_status: spec.compatibility_status,
      compatibility_skip_reasons: if spec.compatibility_status == TrainingCompatibilityStatus::Partial {
        vec![TrainingCompatibilitySkipReason::MissingScreenshot]
      } else {
        Vec::new()
      },
    }];
    let compatibility_view = TrainingCompatibilityViewReport {
      view_name: NERFSTUDIO_VIEW_NAME.to_string(),
      status: spec.compatibility_status,
      exported_frame_count: spec.exported_frame_count,
      skipped_frame_count: spec.skipped_frame_count,
      transforms_path: transforms_path.clone(),
      export_report_path: "compat/nerfstudio/export_report.json".to_string(),
      exported_frame_indices: (1..=spec.exported_frame_count).collect(),
      frame_decisions: vec![TrainingCompatibilityFrameDecision {
        frame_index: 1,
        spatial_frame_id: "frame-1".to_string(),
        source_run_id: "run-1".to_string(),
        status: if spec.compatibility_status == TrainingCompatibilityStatus::Blocked {
          TrainingCompatibilityStatus::Blocked
        } else {
          TrainingCompatibilityStatus::Ready
        },
        skip_reasons: if spec.compatibility_status == TrainingCompatibilityStatus::Partial {
          vec![TrainingCompatibilitySkipReason::MissingScreenshot]
        } else if spec.compatibility_status == TrainingCompatibilityStatus::Blocked {
          vec![TrainingCompatibilitySkipReason::MissingScreenshot]
        } else {
          Vec::new()
        },
      }],
      skip_reason_counts: if spec.compatibility_status == TrainingCompatibilityStatus::Ready {
        Vec::new()
      } else {
        vec![TrainingCompatibilitySkipReasonCount {
          reason: TrainingCompatibilitySkipReason::MissingScreenshot,
          count: spec.skipped_frame_count.max(1),
        }]
      },
      warnings: spec.warnings.clone(),
      used_legacy_view_translation_fallback_frame_indices: Vec::new(),
      known_limits: Vec::new(),
    };

    let manifest = TrainingPackageManifest {
      schema_version: 1,
      generated_at_millis: 1,
      source_scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
      source_bundle_manifest_paths: vec!["/tmp/run-1/run.json".to_string()],
      source_run_ids: vec!["run-1".to_string()],
      counts: TrainingPackageCounts {
        frames: 1,
        images: 1,
        compatibility_exported_frames: spec.exported_frame_count,
        compatibility_skipped_frames: spec.skipped_frame_count,
      },
      frames: frame_records,
      compatibility_views: vec![compatibility_view.clone()],
      known_limits: vec!["canonical package only; no trainer output".to_string()],
    };
    write_json(&training_dir.join("run.json"), &manifest, "MC-7 D5 fixture training package manifest JSON").expect("manifest write");

    if spec.create_inspect_report {
      let inspect_report = TrainingPackageInspectReport {
        schema_version: 1,
        generated_at_millis: 1,
        training_package_manifest_path: training_dir.join("run.json").to_string_lossy().into_owned(),
        scene_packet_manifest_path: "/tmp/scene-packet/run.json".to_string(),
        source_bundle_manifest_paths: vec!["/tmp/run-1/run.json".to_string()],
        source_run_ids: vec!["run-1".to_string()],
        counts: manifest.counts.clone(),
        compatibility_views: vec![compatibility_view],
        warnings: spec.warnings,
        known_limits: vec!["canonical package only; no trainer output".to_string()],
      };
      write_json(&training_dir.join("inspect_report.json"), &inspect_report, "MC-7 D5 fixture training package inspect report JSON")
        .expect("inspect write");
    }

    training_dir.join("run.json")
  }

  fn write_png(path: &Path) {
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent).expect("png parent");
    }
    let mut image = RgbaImage::new(2, 2);
    for pixel in image.pixels_mut() {
      *pixel = Rgba([255, 0, 0, 255]);
    }
    image.save(path).expect("png save");
  }
}
