use std::fs;
use std::path::Path;

use auv_tracing::{Context, RunId};

use super::infra::{AppProbeBackend, AppProbeOperation, AppProbeStepOutput, AppProbeStepRequest, resolve_probe_path};
use super::{AppIdentity, probe_app_with_backend};

#[test]
fn resolve_probe_path_accepts_probe_directory() {
  let root = std::env::temp_dir().join(format!("auv-app-probe-path-{}", crate::model::now_millis()));
  fs::create_dir_all(&root).expect("fixture directory");
  fs::write(root.join("probe.json"), "{}").expect("fixture probe");
  assert_eq!(resolve_probe_path(&root).expect("directory should resolve"), root.join("probe.json"));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn resolve_probe_path_rejects_missing_path() {
  let path = std::env::temp_dir().join(format!("auv-app-probe-missing-{}", crate::model::now_millis()));
  assert!(resolve_probe_path(&path).expect_err("missing path").contains("does not exist"));
}

#[derive(Default)]
struct ProbeBackend {
  calls: Vec<AppProbeOperation>,
  failures: Vec<AppProbeOperation>,
  capture_pre_activation_requested: bool,
}

impl AppProbeBackend for ProbeBackend {
  fn perform(&mut self, request: &AppProbeStepRequest, output_dir: &Path) -> crate::model::AuvResult<AppProbeStepOutput> {
    self.calls.push(request.operation);
    if request.operation == AppProbeOperation::CaptureWindow {
      self.capture_pre_activation_requested = request.inputs.get("activate_target_before_capture").is_some_and(|value| value == "true");
    }
    if self.failures.contains(&request.operation) {
      return Err(format!("fixture failure for {}", request.id));
    }
    let artifact_paths = match request.operation {
      AppProbeOperation::ListWindows => {
        let path = output_dir.join("window-list.txt");
        fs::write(
          &path,
          "observedAt=1\nfrontmostAppName=Fixture\nfrontmostAppBundleId=com.example.fixture\nfrontmostWindowTitle=Fixture Window\nwindowCount=0\n",
        )
        .expect("window fixture");
        vec![path]
      }
      AppProbeOperation::CaptureAxTree => {
        let path = output_dir.join("ax-tree.txt");
        fs::write(&path, "windowTitle=Fixture AX Window\n").expect("AX fixture");
        vec![path]
      }
      _ => Vec::new(),
    };
    Ok(AppProbeStepOutput {
      summary: format!("fixture completed {}", request.id),
      artifact_paths,
    })
  }
}

fn fixture_app() -> AppIdentity {
  AppIdentity {
    bundle_id: "com.example.fixture".to_string(),
    app_name: "Fixture".to_string(),
    app_path: None,
    main_executable_path: None,
    version: "1.0".to_string(),
    build_version: "1".to_string(),
    url_schemes: Vec::new(),
    apple_script_addressable: true,
    launch_services_resolved: true,
    resolution_notes: Vec::new(),
  }
}

#[test]
fn probe_app_executes_and_reports_the_advertised_surface_steps() {
  let project_root = std::env::temp_dir().join(format!("auv-app-probe-task22-{}", crate::model::now_millis()));
  let output_dir = project_root.join("probe");
  fs::create_dir_all(&output_dir).expect("probe output directory");
  let mut backend = ProbeBackend::default();
  let run_id = RunId::new();
  let context = Context::root(run_id);
  let probe = context
    .in_scope(|| probe_app_with_backend(&project_root, fixture_app(), output_dir.clone(), &mut backend))
    .expect("probe should execute direct capabilities");

  assert_eq!(
    backend.calls,
    vec![
      AppProbeOperation::ProbePermissions,
      AppProbeOperation::ListDisplays,
      AppProbeOperation::ActivateTargetApp,
      AppProbeOperation::ListWindows,
      AppProbeOperation::CaptureAxTree,
      AppProbeOperation::CaptureWindow,
      AppProbeOperation::OcrSample,
    ]
  );
  assert_eq!(
    probe.steps.iter().map(|step| step.id.as_str()).collect::<Vec<_>>(),
    vec![
      "probe-permissions",
      "list-displays",
      "activate-target-app",
      "list-windows",
      "capture-ax-tree",
      "capture-window",
      "ocr-sample",
    ]
  );
  assert!(probe.steps.iter().all(|step| step.status == "completed"));
  assert!(probe.steps.iter().all(|step| step.run_id == run_id.to_string()));
  assert!(probe.steps.iter().all(|step| step.artifacts.is_empty()));
  assert!(backend.capture_pre_activation_requested);
  assert_eq!(probe.steps.last().expect("OCR step").inputs["query"], "Fixture Window");
  assert!(output_dir.join("probe.json").is_file());

  let _ = fs::remove_dir_all(project_root);
}

#[test]
fn probe_app_records_optional_surface_failure_and_continues() {
  let project_root = std::env::temp_dir().join(format!("auv-app-probe-partial-{}", crate::model::now_millis()));
  let output_dir = project_root.join("probe");
  fs::create_dir_all(&output_dir).expect("probe output directory");
  let mut backend = ProbeBackend {
    failures: vec![AppProbeOperation::CaptureAxTree],
    ..ProbeBackend::default()
  };

  let probe = probe_app_with_backend(&project_root, fixture_app(), output_dir, &mut backend).expect("optional failure remains a probe fact");
  let failed = probe.steps.iter().find(|step| step.id == "capture-ax-tree").expect("failed AX step");

  assert_eq!(failed.status, "failed");
  assert!(failed.failure_message.as_deref().is_some_and(|message| message.contains("fixture failure")));
  assert_eq!(backend.calls.last(), Some(&AppProbeOperation::OcrSample));

  let _ = fs::remove_dir_all(project_root);
}

#[test]
fn probe_app_propagates_required_surface_failure() {
  let project_root = std::env::temp_dir().join(format!("auv-app-probe-required-{}", crate::model::now_millis()));
  let output_dir = project_root.join("probe");
  fs::create_dir_all(&output_dir).expect("probe output directory");
  let mut backend = ProbeBackend {
    failures: vec![AppProbeOperation::ListDisplays],
    ..ProbeBackend::default()
  };

  let error = probe_app_with_backend(&project_root, fixture_app(), output_dir, &mut backend).expect_err("required failure must abort");

  assert!(error.contains("list-displays"));
  assert_eq!(
    backend.calls,
    vec![
      AppProbeOperation::ProbePermissions,
      AppProbeOperation::ListDisplays
    ]
  );

  let _ = fs::remove_dir_all(project_root);
}
