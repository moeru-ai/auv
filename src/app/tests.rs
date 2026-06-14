use super::analysis::{
  build_app_analysis, candidate_compatibility, recommended_strategy, summarize_failed_probe_steps,
};
use super::infra::{invoke_probe_step, resolve_probe_path};
use super::*;
use crate::contract::{
  ArtifactRef, CandidateQuery, SelectorScope, SurfaceSelector, SurfaceSelectorClause,
};
use crate::driver::{Driver, DriverRegistry};
use crate::model::RunStatus;
use crate::model::{DriverCall, DriverDescriptor, DriverResponse, ProducedArtifact};
use crate::run_builder::RunSpec;
use crate::store::LocalStore;
use crate::trace::RunType;

struct TestProbeDriver;

impl Driver for TestProbeDriver {
  fn descriptor(&self) -> DriverDescriptor {
    DriverDescriptor {
      id: "fixture.observe",
      summary: "Test probe driver",
      capabilities: &["test"],
      donor_boundary: "test",
    }
  }

  fn invoke(&self, call: &DriverCall) -> AuvResult<DriverResponse> {
    if call
      .inputs
      .get("test_mode")
      .is_some_and(|mode| mode == "artifact")
    {
      let first_path = call.working_directory.join("probe-first-artifact.txt");
      let second_path = call.working_directory.join("probe-second-artifact.txt");
      fs::write(&first_path, "first artifact").expect("first artifact should write");
      fs::write(&second_path, "second artifact").expect("second artifact should write");
      return Ok(DriverResponse {
        summary: "artifact ok".to_string(),
        artifacts: vec![
          ProducedArtifact {
            kind: "text".to_string(),
            source_path: first_path,
            preferred_name: "first.txt".to_string(),
            note: Some("first".to_string()),
          },
          ProducedArtifact {
            kind: "text".to_string(),
            source_path: second_path,
            preferred_name: "second.txt".to_string(),
            note: Some("second".to_string()),
          },
        ],
        notes: Vec::new(),
        signals: BTreeMap::from([("outcome".to_string(), "ok".to_string())]),
        backend: None,
      });
    }

    Ok(DriverResponse {
      summary: format!("{} ok", call.operation),
      artifacts: Vec::new(),
      notes: Vec::new(),
      signals: BTreeMap::from([("outcome".to_string(), "ok".to_string())]),
      backend: None,
    })
  }
}

#[test]
fn parse_probe_directory_resolves_probe_json() {
  let root = temp_dir("app-probe-resolve");
  fs::write(root.join("probe.json"), "{}").expect("probe.json should be writable");
  let resolved = resolve_probe_path(&root).expect("directory should resolve");
  assert_eq!(resolved, root.join("probe.json"));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn recommended_strategy_uses_stable_taxonomy_id() {
  let strategy = recommended_strategy(
    "search-entry",
    "ax-text-input",
    "clipboard-submit",
    "captureEvidence",
    AssessmentStatus::Candidate,
    "test rationale",
  )
  .expect("taxonomy should be valid");
  assert_eq!(
    strategy.taxonomy_id,
    "search-entry.ax-text-input.clipboard-submit.capture-evidence"
  );
}

#[test]
fn recommended_native_text_strategy_uses_ax_backed_taxonomy_id() {
  let strategy = recommended_strategy(
    "native-text",
    "ax-text",
    "ax-perform-action-clipboard-paste",
    "verifyAxText",
    AssessmentStatus::Candidate,
    "test rationale",
  )
  .expect("taxonomy should be valid");
  assert_eq!(strategy.taxonomy_id, NATIVE_TEXT_CANONICAL_TAXONOMY_ID);
}

#[test]
fn invoke_probe_steps_share_parent_probe_run_id() {
  let root = temp_dir("probe-step-parent-run");
  let runtime = test_runtime(root.clone());
  let mut run = runtime
    .start_run(RunSpec::new(RunType::Probe, "auv.probe"))
    .expect("probe run should start");
  let root_span = run.root_span();

  let first = invoke_probe_step(
    &runtime,
    &mut run,
    &root_span,
    "first",
    "fixture.observe",
    None,
    BTreeMap::new(),
    false,
  )
  .expect("first step should complete");
  let second = invoke_probe_step(
    &runtime,
    &mut run,
    &root_span,
    "second",
    "fixture.observe",
    None,
    BTreeMap::new(),
    false,
  )
  .expect("second step should complete");

  assert_eq!(first.run_id, run.id().as_str());
  assert_eq!(second.run_id, run.id().as_str());
  assert_eq!(first.run_id, second.run_id);

  let run_id = runtime
    .finish_run(
      run,
      RunFinish {
        status_code: TraceStatusCode::Ok,
        summary: Some("probe complete".to_string()),
        failure: None,
      },
    )
    .expect("probe run should finish");
  let canonical = runtime.read_run(run_id.as_str()).expect("run should read");
  let first_probe_span = canonical
    .spans
    .iter()
    .find(|span| span.name == "auv.probe.step")
    .expect("first probe step span should be recorded");
  assert_eq!(
    first_probe_span.attributes.get("auv.probe.step_id"),
    Some(&serde_json::json!("first"))
  );
  assert_eq!(
    first_probe_span.attributes.get("auv.step.kind"),
    Some(&serde_json::json!("probe"))
  );
  assert!(!first_probe_span.attributes.contains_key("auv.step.index"));

  let _ = fs::remove_dir_all(root);
}

#[test]
fn invoke_probe_step_preserves_artifact_metadata_order() {
  let root = temp_dir("probe-step-artifact-metadata");
  let runtime = test_runtime(root.clone());
  let mut run = runtime
    .start_run(RunSpec::new(RunType::Probe, "auv.probe"))
    .expect("probe run should start");
  let root_span = run.root_span();

  let step = invoke_probe_step(
    &runtime,
    &mut run,
    &root_span,
    "artifact-step",
    "fixture.observe",
    None,
    BTreeMap::from([("test_mode".to_string(), "artifact".to_string())]),
    false,
  )
  .expect("artifact step should complete");

  assert_eq!(step.artifact_paths.len(), 2);
  assert_eq!(step.artifacts.len(), 2);
  assert_eq!(step.artifacts[0].artifact_id, "artifact_0001");
  assert_eq!(step.artifacts[1].artifact_id, "artifact_0002");
  assert_eq!(step.artifacts[0].path, step.artifact_paths[0]);
  assert_eq!(step.artifacts[1].path, step.artifact_paths[1]);
  assert_eq!(step.artifacts[0].role, "text");
  assert_eq!(step.artifacts[1].role, "text");
  assert_eq!(step.artifacts[0].span_id, step.artifacts[1].span_id);
  assert_ne!(step.artifacts[0].span_id, step.span_id);

  let _ = runtime
    .finish_run(
      run,
      RunFinish {
        status_code: TraceStatusCode::Ok,
        summary: Some("probe complete".to_string()),
        failure: None,
      },
    )
    .expect("probe run should finish");
  let _ = fs::remove_dir_all(root);
}

#[test]
fn resolve_probe_ocr_sample_query_prefers_frontmost_window_or_app_name() {
  let root = temp_dir("probe-ocr-query");
  let windows_path = root.join("observe-windows.txt");
  let ax_path = root.join("observe-window-tree.txt");
  fs::write(
    &windows_path,
    "frontmostAppName=Netease Music\nfrontmostWindowTitle=\nobservedAt=2026-05-20T00:00:00Z\nwindowCount=0\n",
  )
  .expect("window report should write");
  fs::write(
    &ax_path,
    "observedAt=2026-05-20T00:00:00Z\nappName=Netease Music\nbundleId=com.netease.163music\nwindowTitle=\nrootRole=AXWindow\nnodeCount=0\n",
  )
  .expect("ax report should write");

  let steps = vec![
    probe_step_fixture("observe-windows", "window.list", vec![windows_path]),
    probe_step_fixture("observe-window-tree", "window.captureAxTree", vec![ax_path]),
  ];
  let app = app_identity_fixture("com.netease.163music", "NeteaseMusic");

  assert_eq!(
    resolve_probe_ocr_sample_query(&app, &steps),
    "Netease Music"
  );
  let _ = fs::remove_dir_all(root);
}

#[test]
fn report_renders_expected_sections() {
  let analysis = report_analysis_fixture();

  let report = render_app_analysis_report(&analysis);
  assert!(report.contains("## 1. App Basic Information"));
  assert!(report.contains("## 2. Available Surfaces"));
  assert!(report.contains("## 3. Grounding Assessment"));
  assert!(report.contains("## 4. Candidate / Annotation Layer"));
  assert!(report.contains("coordinateSpace"));
  assert!(report.contains("candidateQuery"));
  assert!(report.contains("sources=`ax`"));
  assert!(report.contains("evidenceRefs"));
  assert!(report.contains("promotionGate: `action_grade_candidate`"));
  assert!(report.contains("inputBindings"));
  assert!(report.contains("## 5. Control Strategy"));
  assert!(report.contains("## 6. Verification Assessment"));
  assert!(report.contains("Recommended Candidate Strategies"));
  assert!(!report.contains("recipe:"));
  assert!(!report.contains("case matrix"));
}

#[test]
fn build_app_analysis_tolerates_partial_probe_failures() {
  let root = temp_dir("app-analysis-partial-probe");
  let probe_path = root.join("probe.json");
  let probe = AppProbe {
    probe_version: APP_PROBE_VERSION.to_string(),
    created_at_millis: 0,
    project_root: root.clone(),
    output_dir: root.clone(),
    app: app_identity_fixture("com.example.Partial", "Partial"),
    steps: vec![
      failed_probe_step_fixture(
        "probe-permissions",
        "app.probePermissions",
        "permission denied",
      ),
      failed_probe_step_fixture("list-displays", "display.list", "display unavailable"),
      failed_probe_step_fixture("capture-ax-tree", "window.captureAxTree", "AX unavailable"),
    ],
  };

  let analysis = build_app_analysis(&probe_path, &probe).expect("partial probe should analyze");
  assert_eq!(analysis.app_identity.bundle_id, "com.example.Partial");
  assert!(
    analysis
      .known_boundaries
      .iter()
      .any(|note| note.contains("probe-permissions"))
  );
  assert!(
    analysis
      .known_boundaries
      .iter()
      .any(|note| note.contains("AX snapshot was unavailable or partial"))
  );
  let _ = fs::remove_dir_all(root);
}

#[test]
fn summarize_failed_probe_steps_uses_failure_message() {
  let probe = AppProbe {
    probe_version: APP_PROBE_VERSION.to_string(),
    created_at_millis: 0,
    project_root: PathBuf::from("/tmp/project"),
    output_dir: PathBuf::from("/tmp/project/.auv/app-probes/example"),
    app: app_identity_fixture("com.example.App", "Example"),
    // Historical fixture ids: this test exercises probe report summarization,
    // not generic invoke command resolution.
    steps: vec![
      probe_step_fixture("ok", "debug.ok", Vec::new()),
      failed_probe_step_fixture("failed", "debug.failed", "explicit failure"),
    ],
  };

  let notes = summarize_failed_probe_steps(&probe);
  assert_eq!(notes.len(), 1);
  assert!(notes[0].contains("explicit failure"));
}

fn report_analysis_fixture() -> AppAnalysis {
  AppAnalysis {
    analysis_version: APP_ANALYSIS_VERSION.to_string(),
    created_at_millis: 0,
    probe_path: PathBuf::from("/tmp/probe.json"),
    app_identity: AppIdentity {
      bundle_id: "com.example.App".to_string(),
      app_name: "Example".to_string(),
      app_path: Some(PathBuf::from("/Applications/Example.app")),
      main_executable_path: None,
      version: "1.0".to_string(),
      build_version: "100".to_string(),
      url_schemes: vec!["example".to_string()],
      apple_script_addressable: true,
      launch_services_resolved: true,
      resolution_notes: vec![],
    },
    window_context: AppWindowContext {
      observed_window_count: 1,
      observed_at: "2026-05-18T00:00:00Z".to_string(),
      frontmost_app_name: "Example".to_string(),
      frontmost_window_title: "Example".to_string(),
      primary_window_title: "Example".to_string(),
      primary_window_bounds: Some(AppRect {
        x: 0,
        y: 0,
        width: 100,
        height: 100,
      }),
      primary_window_display_scale: Some(2.0),
    },
    permissions: AppPermissionState {
      screen_recording: "granted".to_string(),
      accessibility: "granted".to_string(),
      automation_to_system_events: "granted".to_string(),
      launch_host_process: "Atlas".to_string(),
    },
    available_surfaces: AppAvailableSurfaces {
      accessibility_tree: AssessmentStatus::Available,
      menu_surface: AssessmentStatus::Unknown,
      shortcut_surface: AssessmentStatus::Candidate,
      apple_script_surface: AssessmentStatus::Available,
      url_scheme_surface: AssessmentStatus::Available,
      keyboard_first_surface: AssessmentStatus::Candidate,
      pointer_fallback_surface: AssessmentStatus::Likely,
    },
    grounding_assessment: AppGroundingAssessment {
      ocr_sample_query: "Example".to_string(),
      ocr_sample_status: AssessmentStatus::Candidate,
      ocr_sample_match_count: 2,
      stable_anchor_candidates: vec!["appName: Example".to_string()],
      stable_region_candidates: vec!["primaryWindow=0,0,100,100".to_string()],
      overlay_debug_artifacts_recommended: false,
    },
    control_assessment: AppControlAssessment {
      preferred_path: "non-pointer first".to_string(),
      non_pointer_path: AssessmentStatus::Candidate,
      keyboard_path: AssessmentStatus::Candidate,
      pointer_fallback: AssessmentStatus::Likely,
      notes: vec!["test note".to_string()],
    },
    verification_assessment: AppVerificationAssessment {
      ax_verify: AssessmentStatus::Candidate,
      image_verify: AssessmentStatus::Candidate,
      ui_state_verify: AssessmentStatus::Candidate,
      semantic_success: AssessmentStatus::Unknown,
      notes: vec!["verification note".to_string()],
    },
    disturbance_profile: AppDisturbanceProfile {
      observation: vec!["none".to_string()],
      non_pointer_control: vec!["keyboard".to_string()],
      pointer_fallback: vec!["pointer".to_string()],
    },
    annotation_candidates: vec![AppSurfaceCandidate {
      candidate_id: "search-entry-focus-ax".to_string(),
      area: "search-entry".to_string(),
      kind: "focus-query".to_string(),
      source: "ax".to_string(),
      status: AssessmentStatus::Candidate,
      primary_text: "Search".to_string(),
      secondary_text: "role=AXTextField path=0.1".to_string(),
      query_value: "Search".to_string(),
      coordinate_space: "global-logical".to_string(),
      bounds: Some(AppRect {
        x: 10,
        y: 10,
        width: 80,
        height: 20,
      }),
      click_point: Some(AppPoint { x: 50, y: 20 }),
      confidence: None,
      evidence_step_id: "capture-ax-tree".to_string(),
      candidate_query: Some(CandidateQuery {
        query_id: "search-entry-focus-ax".to_string(),
        selector: SurfaceSelector {
          any_of: vec![SurfaceSelectorClause::Ax {
            role: Some("AXTextField".to_string()),
            label: Some("Search".to_string()),
            path: Some("0.1".to_string()),
            enabled: None,
            visible: Some(true),
          }],
          within: SelectorScope::TargetWindow,
          require_visible: true,
        },
        output_kind: Some("focus-query".to_string()),
        known_limits: vec!["test query".to_string()],
      }),
      evidence_refs: vec![ArtifactRef {
        run_id: crate::trace::RunId::new("run_probe"),
        span_id: crate::trace::SpanId::new("span_probe"),
        artifact_id: crate::trace::ArtifactId::new("artifact_0001"),
        captured_event_id: Some(crate::trace::EventId::new("event_probe")),
      }],
      promotion_gate: Some(AppCandidatePromotionGate {
        status: AppCandidatePromotionStatus::ActionGradeCandidate,
        missing_gates: Vec::new(),
        notes: vec!["Sample candidate satisfies the v0 search-entry promotion seam.".to_string()],
      }),
      input_bindings: BTreeMap::from([("focus_query".to_string(), "Search".to_string())]),
      compatibility: candidate_compatibility(
        &["search-entry.ax-text-input.clipboard-submit.capture-evidence"],
        &[],
      ),
      notes: vec!["sample note".to_string()],
    }],
    known_boundaries: vec!["one boundary".to_string()],
    recommended_strategies: vec![
      recommended_strategy(
        "search-entry",
        "ax-text-input",
        "clipboard-submit",
        "captureEvidence",
        AssessmentStatus::Candidate,
        "test rationale",
      )
      .expect("strategy should render"),
    ],
  }
}

fn temp_dir(label: &str) -> PathBuf {
  let path = std::env::temp_dir().join(format!("auv-{}-{}", label, now_millis()));
  let _ = fs::remove_dir_all(&path);
  fs::create_dir_all(&path).expect("temp dir should be creatable");
  path
}

fn app_identity_fixture(bundle_id: &str, app_name: &str) -> AppIdentity {
  AppIdentity {
    bundle_id: bundle_id.to_string(),
    app_name: app_name.to_string(),
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

fn probe_step_fixture(id: &str, command_id: &str, artifact_paths: Vec<PathBuf>) -> AppProbeStep {
  AppProbeStep {
    id: id.to_string(),
    command_id: command_id.to_string(),
    target_application_id: None,
    inputs: BTreeMap::new(),
    run_id: "run_fixture".to_string(),
    span_id: "span_fixture".to_string(),
    status: RunStatus::Completed.as_str().to_string(),
    output_summary: "ok".to_string(),
    artifacts: artifact_paths
      .iter()
      .enumerate()
      .map(|(index, path)| AppProbeArtifact {
        artifact_id: format!("artifact_{:04}", index + 1),
        span_id: "span_fixture".to_string(),
        path: path.clone(),
        role: path
          .extension()
          .and_then(|value| value.to_str())
          .map(|extension| format!("fixture-{extension}"))
          .unwrap_or_else(|| "fixture".to_string()),
        captured_event_id: None,
      })
      .collect(),
    artifact_paths,
    failure_message: None,
  }
}

fn failed_probe_step_fixture(id: &str, command_id: &str, error: &str) -> AppProbeStep {
  AppProbeStep {
    id: id.to_string(),
    command_id: command_id.to_string(),
    target_application_id: None,
    inputs: BTreeMap::new(),
    run_id: "run_fixture".to_string(),
    span_id: "span_fixture".to_string(),
    status: RunStatus::Failed.as_str().to_string(),
    output_summary: format!("Probe step {id} failed"),
    artifact_paths: Vec::new(),
    artifacts: Vec::new(),
    failure_message: Some(error.to_string()),
  }
}

fn test_runtime(project_root: PathBuf) -> Runtime {
  let drivers = DriverRegistry::new(vec![Box::new(TestProbeDriver)]);
  Runtime::new(
    project_root.clone(),
    drivers,
    LocalStore::new(project_root).expect("store should initialize"),
  )
}
