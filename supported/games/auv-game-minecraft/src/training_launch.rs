use std::collections::BTreeSet;
use std::fs;
use std::io::BufReader;
use std::path::{Component, Path, PathBuf};
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
// Source snapshot used to review the offline OpenSplat CLI and Nerfstudio input contract.
// The local `--help` probe checks command availability only, not this revision.
const OPEN_SPLAT_CONTRACT_REVISION: &str = "9fb62fde8b7b8c416121d3cbdcda278ffd9682f7";
// Release tag whose prebuilt `brush-app-aarch64-apple-darwin` asset and CLI surface were
// reviewed for this contract. Pinned to a tag rather than a commit because the launch plan
// targets the published macOS binary; the local `--help` probe does not verify the version.
const BRUSH_CONTRACT_REVISION: &str = "v0.3.0";
// Brush substitutes `{iter}` with the zero-padded training step
// (`brush-process/src/train_stream.rs`: `export_name.replace("{iter}", ...)`). Keeping the
// placeholder matters because Brush exports every `--export-every` steps in addition to the
// last step, so a fixed filename would let an unfinished checkpoint occupy the path a
// downstream collector would otherwise read as the final splat.
const BRUSH_EXPORT_NAME_TEMPLATE: &str = "splat_{iter}.ply";

/// Trainer command contract selected for offline launch preparation.
// TODO(3dgs-training-cli-restore): no CLI frontend selects this backend yet. The
// pre-#130 `auv-minecraft prepare-3dgs-training --trainer-backend` wiring (stranded
// commit 9c9296ef) patches a training CLI command family that #130 deleted and the
// restore lane has not brought back; port the CLI surface only in an owner-approved
// slice that restores that family onto the current frontend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrainingBackend {
  // NOTICE: not reachable on Apple Silicon. `splatfacto.py` hardcodes `.cuda()`
  // (L208, L535) and its rasterizer `gsplat` builds `CUDAExtension`
  // (setup.py L203/L226/L249), so the probe can succeed while training can never
  // run on that host. Kept because it stays correct on CUDA hosts, and because
  // launch evidence must still name the backend it prepared for. See
  // docs/ai/references/apps/minecraft/2026-07-26-minecraft-3dgs-trainer-backend-evidence.md.
  NerfstudioSplatfacto,
  OpenSplat,
  /// wgpu/Metal trainer (ArthurBrussee/brush): the only backend here that can
  /// actually train on the Apple Silicon target.
  // NOTICE: unlike OpenSplat, Brush does not fail when the seed point cloud is
  // unreadable. `brush-dataset/src/formats/nerfstudio.rs` wraps the load in
  // `if let Ok(ply_data)` with no else branch, leaving `init_splat = None`, and
  // `brush-process/src/train_stream.rs` then takes `else { // Default: just use
  // random splats }`, announcing it only through `log::info!`. A silently
  // seedless run therefore exits 0 and writes a plausible splat that ignores the
  // exported Minecraft geometry, so seed presence must be a launch-time
  // readiness precondition rather than something inferred from exit status.
  Brush,
  // TODO(3dgs-seed-consumption-verification): readiness proves the seed cloud
  // *exists and resolves inside the dataset*, not that the trainer parsed it.
  // Verifying real consumption needs either a post-run splat-count/extent check
  // against the seed, or scraping the trainer's info log for the random-init
  // line; both require actually running a trainer, which this offline
  // preparation module deliberately does not do. Unlocks when an owner names a
  // slice that runs a real training job and asserts on its output.
}

impl TrainingBackend {
  /// Stable backend name persisted in launch evidence.
  pub fn manifest_name(self) -> &'static str {
    match self {
      Self::NerfstudioSplatfacto => "nerfstudio.splatfacto",
      Self::OpenSplat => "opensplat",
      Self::Brush => "brush",
    }
  }

  fn probe_program(self) -> &'static str {
    match self {
      Self::NerfstudioSplatfacto => "ns-train",
      Self::OpenSplat => "opensplat",
      Self::Brush => "brush",
    }
  }

  /// Whether the backend consumes the Nerfstudio `ply_file_path` seed cloud, and so
  /// must not be reported Ready without one. True for both reasons a backend can
  /// need it: OpenSplat aborts without it, Brush silently trains on random
  /// initialization instead.
  fn consumes_seed_point_cloud(self) -> bool {
    match self {
      // NOTICE: splatfacto seeds from the dataparser's own point cloud and supports
      // `random_init`, so a missing seed cloud is not a launch-time blocker here.
      Self::NerfstudioSplatfacto => false,
      Self::OpenSplat | Self::Brush => true,
    }
  }

  fn probe_arguments(self) -> &'static [&'static str] {
    &["--help"]
  }

  fn probe_command(self) -> String {
    format!("{} {}", self.probe_program(), self.probe_arguments().join(" "))
  }

  fn suggested_output_dir(self, output_dir: &Path) -> PathBuf {
    match self {
      Self::NerfstudioSplatfacto => output_dir.join("trainer-output/nerfstudio-splatfacto"),
      Self::OpenSplat => output_dir.join("trainer-output/opensplat"),
      Self::Brush => output_dir.join("trainer-output/brush"),
    }
  }

  fn launch_command(self, training_data_dir: &Path, suggested_output_dir: &Path) -> String {
    match self {
      Self::NerfstudioSplatfacto => {
        format!("ns-train splatfacto --data {} --output-dir {}", sh_quote(training_data_dir), sh_quote(suggested_output_dir))
      }
      Self::OpenSplat => {
        let output_file = suggested_output_dir.join("splat.ply");
        format!("opensplat {} --output {}", sh_quote(training_data_dir), sh_quote(&output_file))
      }
      // NOTICE: Brush takes the dataset as a positional argument and disables its viewer on
      // its own once one is present (`with_viewer` is declared
      // `default_value_if("source", ArgPredicate::IsPresent, "false")`), so the plan must not
      // pass an explicit headless flag. `--export-path` is a directory and `--export-name` is
      // a filename joined onto it, not a second path (`train_stream.rs`:
      // `export_path.join(&export_name)`).
      Self::Brush => {
        format!(
          "brush {} --export-path {} --export-name {}",
          sh_quote(training_data_dir),
          sh_quote(suggested_output_dir),
          sh_quote(Path::new(BRUSH_EXPORT_NAME_TEMPLATE))
        )
      }
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrainingLaunchPreparationInputs {
  pub training_package_manifest_path: PathBuf,
  pub output_dir: PathBuf,
  pub trainer_backend: TrainingBackend,
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
  SeedPointCloudMissing,
  TrainerCommandUnavailable,
}

#[derive(Debug, Deserialize)]
struct NerfstudioTransformsSeedCloud {
  #[serde(default)]
  ply_file_path: Option<String>,
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
    read_json_file::<TrainingPackageManifest>(&inputs.training_package_manifest_path, "training package manifest")?;
  let training_package_dir = inputs
    .training_package_manifest_path
    .parent()
    .ok_or_else(|| format!("training package manifest {} has no parent directory", inputs.training_package_manifest_path.display()))?;

  let training_package_inspect_report_path = training_package_dir.join("inspect_report.json");
  let training_package_inspect_report =
    read_json_file::<TrainingPackageInspectReport>(&training_package_inspect_report_path, "training package inspect report")?;

  let manifest_view = find_compatibility_view(&training_package_manifest.compatibility_views, "training package manifest")?;
  let inspect_view = find_compatibility_view(&training_package_inspect_report.compatibility_views, "training package inspect report")?;

  let compatibility_export_report_path = training_package_dir.join(&inspect_view.export_report_path);
  ensure_file_readable(&compatibility_export_report_path, "Nerfstudio compatibility export report JSON")?;

  let transforms_path = inspect_view.transforms_path.as_ref().map(|path| training_package_dir.join(path));
  if inspect_view.transforms_path.is_some() {
    let declared_path = transforms_path.as_ref().expect("transforms path should exist when declared");
    ensure_file_readable(declared_path, "Nerfstudio transforms JSON")?;
  }

  let training_data_dir = training_package_dir.join("compat/nerfstudio");
  let compatibility_images_dir = training_data_dir.join("images");
  if inspect_view.exported_frame_count > 0 {
    ensure_directory_exists(&training_data_dir, "Nerfstudio compatibility data directory")?;
    ensure_directory_exists(&compatibility_images_dir, "Nerfstudio compatibility images directory")?;
  }

  let seed_point_cloud_present = if inputs.trainer_backend.consumes_seed_point_cloud() {
    transforms_path.as_ref().map(|path| resolve_seed_point_cloud_path(path)).transpose()?.flatten().is_some()
  } else {
    false
  };
  let suggested_output_dir = inputs.trainer_backend.suggested_output_dir(&inputs.output_dir);
  let launch_command = inputs.trainer_backend.launch_command(&training_data_dir, &suggested_output_dir);
  let probe_succeeded = probe(inputs.trainer_backend.probe_program(), inputs.trainer_backend.probe_arguments());

  let transforms_present = transforms_path.is_some();
  let (trainer_readiness, readiness_blocker) = if inspect_view.status == TrainingCompatibilityStatus::Blocked {
    (TrainingLaunchReadiness::Blocked, Some(TrainingLaunchReadinessBlocker::CompatibilityViewBlocked))
  } else if inspect_view.exported_frame_count > 0 && !transforms_present {
    (TrainingLaunchReadiness::Blocked, Some(TrainingLaunchReadinessBlocker::TransformsMissing))
  } else if inspect_view.exported_frame_count > 0 && inputs.trainer_backend.consumes_seed_point_cloud() && !seed_point_cloud_present {
    (TrainingLaunchReadiness::Blocked, Some(TrainingLaunchReadinessBlocker::SeedPointCloudMissing))
  } else if !probe_succeeded {
    (TrainingLaunchReadiness::Blocked, Some(TrainingLaunchReadinessBlocker::TrainerCommandUnavailable))
  } else {
    (TrainingLaunchReadiness::Ready, None)
  };

  let generated_at_millis = crate::now_millis();
  let manifest_path = inputs.output_dir.join("minecraft-3dgs-training-launch-plan.json");
  let inspect_report_path = inputs.output_dir.join("minecraft-3dgs-training-launch-inspect.json");
  let runbook_path = inputs.output_dir.join("training-launch-runbook.md");

  let mut warnings = BTreeSet::new();
  warnings.extend(training_package_inspect_report.warnings.iter().cloned());
  warnings.extend(inspect_view.warnings.iter().cloned());

  let mut known_limits = BTreeSet::new();
  known_limits.extend(training_package_manifest.known_limits.iter().cloned());
  known_limits.extend(training_package_inspect_report.known_limits.iter().cloned());
  known_limits.insert("training launch preparation only; no trainer process is started and no trained splat is produced".to_string());
  match inputs.trainer_backend {
    TrainingBackend::NerfstudioSplatfacto => {}
    TrainingBackend::OpenSplat => {
      known_limits.insert(format!(
        "OpenSplat readiness targets upstream source revision {OPEN_SPLAT_CONTRACT_REVISION} and checks only the local launch contract: command probe, transforms.json, and referenced seed point cloud; binary version and real training are not verified"
      ));
    }
    TrainingBackend::Brush => {
      known_limits.insert(format!(
        "Brush readiness targets upstream release {BRUSH_CONTRACT_REVISION} and checks only the local launch contract: command probe, transforms.json, and referenced seed point cloud; binary version and real training are not verified"
      ));
      // Both limits below exist because Brush degrades instead of failing, so a zero exit
      // status is weaker evidence here than it is for OpenSplat.
      known_limits.insert(
        "Brush does not fail when the referenced seed point cloud is unreadable; it reports the fallback at info level and trains from random initialization, so a completed run does not prove the exported Minecraft geometry was used"
          .to_string(),
      );
      known_limits.insert(
        "Brush treats a failed splat export as a warning rather than a fatal error, so a zero exit status does not prove an exported ply exists".to_string(),
      );
    }
  }

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
    trainer_backend: inputs.trainer_backend.manifest_name().to_string(),
    training_data_dir: training_data_dir.to_string_lossy().into_owned(),
    transforms_path: inspect_view.transforms_path.clone(),
    export_report_path: inspect_view.export_report_path.clone(),
    suggested_output_dir: suggested_output_dir.to_string_lossy().into_owned(),
    launch_command,
    known_limits: known_limits.iter().cloned().collect(),
  };
  write_json(&manifest_path, &manifest, "training launch plan JSON")?;

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
    probe_command: inputs.trainer_backend.probe_command(),
    probe_succeeded,
    exported_frame_count: inspect_view.exported_frame_count,
    skipped_frame_count: inspect_view.skipped_frame_count,
    transforms_present,
    warnings: warnings.iter().cloned().collect(),
    known_limits: known_limits.iter().cloned().collect(),
  };
  write_json(&inspect_report_path, &inspect_report, "training launch inspect JSON")?;

  fs::create_dir_all(&inputs.output_dir)
    .map_err(|error| format!("failed to create training launch output directory {}: {error}", inputs.output_dir.display()))?;
  fs::write(&runbook_path, render_runbook(&manifest, &inspect_report).as_bytes())
    .map_err(|error| format!("failed to write training launch runbook {}: {error}", runbook_path.display()))?;

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

fn resolve_seed_point_cloud_path(transforms_path: &Path) -> TrainingLaunchPreparationResult<Option<PathBuf>> {
  let transforms = read_json_file::<NerfstudioTransformsSeedCloud>(transforms_path, "Nerfstudio transforms JSON")?;
  let Some(relative_path) = transforms.ply_file_path.filter(|path| !path.trim().is_empty()) else {
    return Ok(None);
  };
  let relative_path = Path::new(&relative_path);
  if relative_path.is_absolute()
    || relative_path.components().any(|component| !matches!(component, Component::CurDir | Component::Normal(_)))
  {
    return Err(format!(
      "OpenSplat seed point cloud path {:?} in {} must stay relative to the training dataset",
      relative_path,
      transforms_path.display()
    ));
  }
  let dataset_root = transforms_path.parent().unwrap_or(Path::new("."));
  let resolved_path = dataset_root.join(relative_path);
  let Ok(metadata) = fs::symlink_metadata(&resolved_path) else {
    return Ok(None);
  };
  if metadata.file_type().is_symlink() {
    ensure_path_stays_within_dataset_root(dataset_root, &resolved_path)?;
    return Err(format!(
      "OpenSplat seed point cloud {} must not resolve through symlinks inside the training dataset",
      resolved_path.display()
    ));
  }
  if !metadata.is_file() {
    return Ok(None);
  }
  ensure_path_stays_within_dataset_root(dataset_root, &resolved_path)?;
  Ok(Some(resolved_path))
}

fn ensure_path_stays_within_dataset_root(dataset_root: &Path, resolved_path: &Path) -> TrainingLaunchPreparationResult<()> {
  let canonical_dataset_root = fs::canonicalize(dataset_root)
    .map_err(|error| format!("failed to canonicalize training dataset root {}: {error}", dataset_root.display()))?;
  let canonical_resolved_path = fs::canonicalize(resolved_path)
    .map_err(|error| format!("failed to canonicalize OpenSplat seed point cloud {}: {error}", resolved_path.display()))?;
  if !canonical_resolved_path.starts_with(&canonical_dataset_root) {
    return Err(format!(
      "OpenSplat seed point cloud {} must stay within the training dataset rooted at {}",
      resolved_path.display(),
      dataset_root.display()
    ));
  }

  let relative_path = resolved_path.strip_prefix(dataset_root).unwrap_or(resolved_path);
  let mut current_path = dataset_root.to_path_buf();
  for component in relative_path.components() {
    current_path.push(component.as_os_str());
    if fs::symlink_metadata(&current_path).map(|metadata| metadata.file_type().is_symlink()).unwrap_or(false) {
      return Err(format!(
        "OpenSplat seed point cloud {} must not resolve through symlinks inside the training dataset",
        resolved_path.display()
      ));
    }
  }
  Ok(())
}

fn render_runbook(manifest: &TrainingLaunchPlanManifest, inspect_report: &TrainingLaunchInspectReport) -> String {
  let mut output = String::new();
  output.push_str("# 3DGS training launch runbook\n\n");
  output.push_str("This is a preparation artifact. It does not start a trainer process and does not prove training quality.\n\n");
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
        TrainingLaunchReadinessBlocker::SeedPointCloudMissing => "seed_point_cloud_missing",
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
  output.push_str("- The training package remains authoritative; this step does not copy inputs into a second dataset tree.\n");
  if manifest.trainer_backend == TrainingBackend::OpenSplat.manifest_name() {
    output.push_str("- If readiness is `trainer_command_unavailable`, install a local OpenSplat CLI and rerun `prepare-3dgs-training`.\n");
    output.push_str(
      "- If readiness is `seed_point_cloud_missing`, regenerate the training package from captures that include raycast hits so transforms.json can reference a seed point cloud.\n",
    );
  } else if manifest.trainer_backend == TrainingBackend::Brush.manifest_name() {
    output.push_str(
      "- If readiness is `trainer_command_unavailable`, install the prebuilt Brush binary for this platform and rerun `prepare-3dgs-training`.\n",
    );
    output.push_str(
      "- If readiness is `seed_point_cloud_missing`, regenerate the training package from captures that include raycast hits so transforms.json can reference a seed point cloud. Brush will not surface this itself: it reports the dropped seed at info level and trains from random initialization instead of failing.\n",
    );
    output.push_str(
      "- A completed Brush run is not evidence on its own. It exits successfully both when the seed cloud was ignored and when the splat export failed, so confirm the exported ply exists under the suggested output directory before treating the result as trained on this scene.\n",
    );
  } else {
    output.push_str("- If readiness is `trainer_command_unavailable`, install a local Nerfstudio CLI and rerun `prepare-3dgs-training`.\n");
  }
  output.push_str(
    "- If readiness is `compatibility_view_blocked`, regenerate the training package from captures that export at least one Nerfstudio-compatible frame.\n",
  );
  output.push_str(
    "- If readiness is `transforms_missing`, treat the training package as corrupted input and rebuild it before attempting training.\n",
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
        trainer_backend: TrainingBackend::NerfstudioSplatfacto,
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
        trainer_backend: TrainingBackend::NerfstudioSplatfacto,
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
        trainer_backend: TrainingBackend::NerfstudioSplatfacto,
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
        trainer_backend: TrainingBackend::NerfstudioSplatfacto,
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
        trainer_backend: TrainingBackend::NerfstudioSplatfacto,
      },
      |_command, _arguments| true,
    )
    .expect_err("missing inspect report should fail");

    assert!(error.contains("failed to open training package inspect report"));
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
        trainer_backend: TrainingBackend::NerfstudioSplatfacto,
      },
      |_command, _arguments| true,
    )
    .expect_err("missing export report should fail");

    assert!(error.contains("failed to open Nerfstudio compatibility export report JSON"));
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
        trainer_backend: TrainingBackend::NerfstudioSplatfacto,
      },
      |_command, _arguments| true,
    )
    .expect_err("missing declared transforms should fail");

    assert!(error.contains("failed to open Nerfstudio transforms JSON"));
  }

  #[test]
  fn opensplat_ready_when_seed_point_cloud_exists_and_probe_succeeds() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_ready_training_package_fixture(&temp);
    set_seed_point_cloud_fixture(&manifest_path, "points3d.ply", true);

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
        trainer_backend: TrainingBackend::OpenSplat,
      },
      |command, arguments| {
        assert_eq!(command, "opensplat");
        assert_eq!(arguments, ["--help"]);
        true
      },
    )
    .expect("launch prep should succeed");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Ready);
    assert_eq!(output.inspect_report.readiness_blocker, None);
    assert_eq!(output.manifest.trainer_backend, "opensplat");
    assert_eq!(output.inspect_report.probe_command, "opensplat --help");
    assert!(output.manifest.launch_command.starts_with("opensplat "));
    assert!(output.manifest.launch_command.contains("--output"));
    assert!(output.manifest.launch_command.contains("trainer-output/opensplat/splat.ply"));
    assert!(
      output.manifest.known_limits.iter().any(|limit| limit.contains(OPEN_SPLAT_CONTRACT_REVISION)),
      "launch evidence should pin the reviewed OpenSplat contract revision"
    );
    assert!(output.runbook_path.ends_with("training-launch-runbook.md"));
  }

  #[test]
  fn opensplat_blocks_when_command_probe_fails() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_ready_training_package_fixture(&temp);
    set_seed_point_cloud_fixture(&manifest_path, "points3d.ply", true);

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
        trainer_backend: TrainingBackend::OpenSplat,
      },
      |command, arguments| {
        assert_eq!(command, "opensplat");
        assert_eq!(arguments, ["--help"]);
        false
      },
    )
    .expect("unavailable trainer should still write blocked outputs");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Blocked);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchReadinessBlocker::TrainerCommandUnavailable));
  }

  #[test]
  fn opensplat_blocks_when_transforms_have_no_seed_point_cloud_reference() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_ready_training_package_fixture(&temp);

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
        trainer_backend: TrainingBackend::OpenSplat,
      },
      |_command, _arguments| true,
    )
    .expect("launch prep should still write blocked outputs");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Blocked);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchReadinessBlocker::SeedPointCloudMissing));
  }

  #[test]
  fn opensplat_blocks_when_seed_point_cloud_file_is_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_ready_training_package_fixture(&temp);
    set_seed_point_cloud_fixture(&manifest_path, "points3d.ply", false);

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
        trainer_backend: TrainingBackend::OpenSplat,
      },
      |_command, _arguments| true,
    )
    .expect("launch prep should still write blocked outputs");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Blocked);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchReadinessBlocker::SeedPointCloudMissing));
  }

  #[test]
  fn opensplat_rejects_seed_point_cloud_path_outside_dataset() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_ready_training_package_fixture(&temp);
    set_seed_point_cloud_fixture(&manifest_path, "../outside.ply", false);

    let error = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
        trainer_backend: TrainingBackend::OpenSplat,
      },
      |_command, _arguments| true,
    )
    .expect_err("seed point cloud reference must remain inside the dataset");

    assert!(error.contains("must stay relative to the training dataset"), "unexpected error: {error}");
  }

  #[cfg(unix)]
  #[test]
  fn opensplat_rejects_seed_point_cloud_symlink_outside_dataset() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_ready_training_package_fixture(&temp);
    set_seed_point_cloud_fixture(&manifest_path, "points3d.ply", false);

    let outside_dir = temp.path().join("outside");
    fs::create_dir_all(&outside_dir).expect("outside dir");
    let outside_seed = outside_dir.join("points3d.ply");
    fs::write(
      &outside_seed,
      b"ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n1.0 1.0 1.0 128 128 128\n",
    )
    .expect("outside seed write");
    let linked_seed = manifest_path.parent().expect("training package directory").join("compat/nerfstudio/points3d.ply");
    symlink(&outside_seed, &linked_seed).expect("seed symlink");

    let error = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
        trainer_backend: TrainingBackend::OpenSplat,
      },
      |_command, _arguments| true,
    )
    .expect_err("seed point cloud symlink must be rejected");

    assert!(error.contains("must stay within the training dataset"), "unexpected error: {error}");
  }

  #[test]
  fn brush_ready_when_seed_point_cloud_exists_and_probe_succeeds() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_ready_training_package_fixture(&temp);
    set_seed_point_cloud_fixture(&manifest_path, "points3d.ply", true);

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
        trainer_backend: TrainingBackend::Brush,
      },
      |command, arguments| {
        assert_eq!(command, "brush");
        assert_eq!(arguments, ["--help"]);
        true
      },
    )
    .expect("launch prep should succeed");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Ready);
    assert_eq!(output.inspect_report.readiness_blocker, None);
    assert_eq!(output.manifest.trainer_backend, "brush");
    assert_eq!(output.inspect_report.probe_command, "brush --help");
    // Brush takes the dataset as a positional path and runs headless as soon as one
    // is present, so the plan must not pass a `--source`/`--with-viewer` pair.
    assert!(output.manifest.launch_command.starts_with("brush "), "unexpected command: {}", output.manifest.launch_command);
    assert!(!output.manifest.launch_command.contains("--with-viewer"));
    assert!(output.manifest.launch_command.contains("--export-path"));
    // `--export-name` is a filename that Brush joins onto `--export-path`, not a path of
    // its own, and it must keep the `{iter}` placeholder so the guaranteed last-step
    // export does not overwrite, or get overwritten by, a periodic one.
    assert!(
      output.manifest.launch_command.contains(&format!("--export-name \"{BRUSH_EXPORT_NAME_TEMPLATE}\"")),
      "unexpected command: {}",
      output.manifest.launch_command
    );
    assert!(!output.manifest.launch_command.contains("trainer-output/brush/splat"));
    assert!(
      output.manifest.known_limits.iter().any(|limit| limit.contains(BRUSH_CONTRACT_REVISION)),
      "launch evidence should pin the reviewed Brush contract revision"
    );
    assert!(
      output.manifest.known_limits.iter().any(|limit| limit.contains("random")),
      "launch evidence must record that a dropped seed cloud silently degrades to random initialization"
    );
  }

  // ROOT CAUSE:
  //
  // If the trainer backend silently tolerates a missing seed point cloud, readiness
  // computed from the command probe alone reports Ready for a run that will train on
  // random initialization instead of the exported Minecraft geometry.
  //
  // Before the fix, only OpenSplat gated on the seed cloud, because OpenSplat aborts
  // without one (`nerfstudio.cpp`: `if (t.plyFilePath.empty()) throw`).
  // Brush instead falls back without an error: `nerfstudio.rs:377` wraps the load in
  // `if let Ok(ply_data)` with no else branch, and `train_stream.rs:124-130` takes the
  // `else { // Default: just use random splats }` path, logging only at info level.
  // The fix keeps the seed cloud a readiness precondition for every backend that
  // consumes one, so evidence cannot claim Ready for a silently seedless run.
  #[test]
  fn brush_blocks_when_seed_point_cloud_is_missing_despite_silent_trainer_fallback() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_ready_training_package_fixture(&temp);
    set_seed_point_cloud_fixture(&manifest_path, "points3d.ply", false);

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
        trainer_backend: TrainingBackend::Brush,
      },
      |_command, _arguments| true,
    )
    .expect("launch prep should still write blocked outputs");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Blocked);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchReadinessBlocker::SeedPointCloudMissing));
  }

  #[test]
  fn brush_blocks_when_transforms_have_no_seed_point_cloud_reference() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_ready_training_package_fixture(&temp);

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
        trainer_backend: TrainingBackend::Brush,
      },
      |_command, _arguments| true,
    )
    .expect("launch prep should still write blocked outputs");

    assert_eq!(output.inspect_report.trainer_readiness, TrainingLaunchReadiness::Blocked);
    assert_eq!(output.inspect_report.readiness_blocker, Some(TrainingLaunchReadinessBlocker::SeedPointCloudMissing));
  }

  #[test]
  fn brush_runbook_explains_silent_seed_fallback_recovery() {
    let temp = tempfile::tempdir().expect("temp dir");
    let manifest_path = write_ready_training_package_fixture(&temp);
    set_seed_point_cloud_fixture(&manifest_path, "points3d.ply", true);

    let output = prepare_3dgs_training_launch_with_probe(
      TrainingLaunchPreparationInputs {
        training_package_manifest_path: manifest_path,
        output_dir: temp.path().join("launch"),
        trainer_backend: TrainingBackend::Brush,
      },
      |_command, _arguments| true,
    )
    .expect("launch prep should succeed");

    let runbook = fs::read_to_string(&output.runbook_path).expect("runbook should be written");
    assert!(runbook.contains("brush"), "runbook should name the Brush backend");
    assert!(
      runbook.contains("random"),
      "runbook must warn that Brush degrades to random initialization instead of failing when the seed cloud is unreadable"
    );
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

  fn write_ready_training_package_fixture(temp: &TempDir) -> PathBuf {
    write_training_package_fixture(
      temp,
      TrainingPackageFixtureSpec {
        compatibility_status: TrainingCompatibilityStatus::Ready,
        exported_frame_count: 1,
        skipped_frame_count: 0,
        declare_transforms_path: true,
        create_transforms_file: true,
        create_inspect_report: true,
        create_export_report: true,
        warnings: Vec::new(),
      },
    )
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
        "training launch fixture export report JSON",
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
        "training launch fixture transforms JSON",
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
    write_json(&training_dir.join("run.json"), &manifest, "training launch fixture package manifest JSON").expect("manifest write");

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
      write_json(&training_dir.join("inspect_report.json"), &inspect_report, "training launch fixture package inspect report JSON")
        .expect("inspect write");
    }

    training_dir.join("run.json")
  }

  fn set_seed_point_cloud_fixture(manifest_path: &Path, relative_path: &str, create_file: bool) {
    let transforms_path = manifest_path.parent().expect("training package directory").join("compat/nerfstudio/transforms.json");
    let mut transforms: serde_json::Value =
      serde_json::from_slice(&fs::read(&transforms_path).expect("read transforms")).expect("parse transforms");
    transforms
      .as_object_mut()
      .expect("transforms object")
      .insert("ply_file_path".to_string(), serde_json::Value::String(relative_path.to_string()));
    write_json(&transforms_path, &transforms, "training launch fixture transforms JSON").expect("rewrite transforms");

    if create_file {
      fs::write(
        transforms_path.parent().expect("transforms directory").join(relative_path),
        b"ply\nformat ascii 1.0\nelement vertex 1\nproperty float x\nproperty float y\nproperty float z\nproperty uchar red\nproperty uchar green\nproperty uchar blue\nend_header\n0.5 0.5 0.5 128 128 128\n",
      )
      .expect("seed point cloud write");
    }
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
