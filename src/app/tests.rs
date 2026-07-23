use super::analysis::{build_app_analysis, candidate_compatibility, recommended_strategy, summarize_failed_probe_steps};
use super::infra::{invoke_probe_step, resolve_probe_path};
use super::*;
use crate::contract::{ArtifactRef, CandidateQuery, SelectorScope, SurfaceSelector, SurfaceSelectorClause};
use crate::model::RunStatus;
use auv_tracing_driver::run_builder::RunSpec;
use auv_tracing_driver::store::LocalStore;
use auv_tracing_driver::trace::RunType;

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
  assert_eq!(strategy.taxonomy_id, "search-entry.ax-text-input.clipboard-submit.capture-evidence");
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

fn scan_coverage_fixture_dir() -> String {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("crates/auv-scan/tests/fixtures/scan/coverage/coverage_stable_v0").display().to_string()
}

#[test]
fn invoke_probe_steps_share_parent_probe_run_id() {
  let root = temp_dir("probe-step-parent-run");
  let runtime = test_runtime(root.clone());
  let mut run = runtime.recording().handle().start_run(RunSpec::new(RunType::Probe, "auv.probe")).expect("probe run should start");
  let root_span = run.root_span();

  let inputs = BTreeMap::from([("fixture-dir".to_string(), scan_coverage_fixture_dir())]);
  let first = invoke_probe_step(&runtime, &mut run, &root_span, "first", "scan.coverage", None, inputs.clone(), false)
    .expect("first step should complete");
  let second =
    invoke_probe_step(&runtime, &mut run, &root_span, "second", "scan.coverage", None, inputs, false).expect("second step should complete");

  assert_eq!(first.run_id, run.id().as_str());
  assert_eq!(second.run_id, run.id().as_str());
  assert_eq!(first.run_id, second.run_id);

  let run_id = runtime
    .recording()
    .handle()
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
  let first_probe_span =
    canonical.spans.iter().find(|span| span.name == "auv.probe.step").expect("first probe step span should be recorded");
  assert_eq!(first_probe_span.attributes.get("auv.probe.step_id"), Some(&serde_json::json!("first")));
  assert_eq!(first_probe_span.attributes.get("auv.step.kind"), Some(&serde_json::json!("probe")));
  assert!(!first_probe_span.attributes.contains_key("auv.step.index"));

  let _ = fs::remove_dir_all(root);
}

#[test]
fn invoke_probe_step_preserves_direct_command_artifact_boundary() {
  let root = temp_dir("probe-step-direct-command-artifact-boundary");
  let runtime = test_runtime(root.clone());
  let mut run = runtime.recording().handle().start_run(RunSpec::new(RunType::Probe, "auv.probe")).expect("probe run should start");
  let root_span = run.root_span();

  let inputs = BTreeMap::from([("fixture-dir".to_string(), scan_coverage_fixture_dir())]);
  let step = invoke_probe_step(&runtime, &mut run, &root_span, "artifact-step", "scan.coverage", None, inputs, false)
    .expect("direct invoke step should complete");

  assert!(step.output_summary.starts_with("scan coverage produced from fixture "));
  assert!(!step.artifact_paths.is_empty());
  assert!(!step.artifacts.is_empty());

  let _ = runtime
    .recording()
    .handle()
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
fn resolve_probe_ocr_sample_query_supports_legacy_step_ids() {
  let root = temp_dir("probe-ocr-query");
  let window_step =
    report_probe_step_fixture(&root, "observe-windows", "window.list", "frontmostAppName=Netease Music\nfrontmostWindowTitle=\n");
  let ax_step = report_probe_step_fixture(
    &root,
    "observe-window-tree",
    "window.captureAxTree",
    "observedAt=2026-05-20T00:00:00Z\nappName=Netease Music\nbundleId=com.netease.163music\nwindowTitle=\nrootRole=AXWindow\nnodeCount=0\n",
  );
  let app = app_identity_fixture("com.netease.163music", "NeteaseMusic");

  assert_eq!(resolve_probe_ocr_sample_query(&app, &[window_step]), "Netease Music");
  assert_eq!(resolve_probe_ocr_sample_query(&app, &[ax_step]), "Netease Music");
  let _ = fs::remove_dir_all(root);
}

// ROOT CAUSE:
//
// If app probe reached the OCR sample, its query fell back to app identity
// because the resolver only recognized retired probe step ids.
//
// Before the fix, the live `list-windows` and `capture-ax-tree` steps were
// ignored. The fix prefers their evidence while retaining legacy probe reads.
#[test]
fn resolve_probe_ocr_sample_query_prefers_current_probe_step_ids() {
  let root = temp_dir("probe-ocr-query-current-ids");
  let window_steps = vec![
    report_probe_step_fixture(&root, "observe-windows", "window.list", "frontmostAppName=Legacy Music\nfrontmostWindowTitle=\n"),
    report_probe_step_fixture(&root, "list-windows", "window.list", "frontmostAppName=Netease Music\nfrontmostWindowTitle=\n"),
  ];
  let ax_steps = vec![
    report_probe_step_fixture(&root, "observe-window-tree", "window.captureAxTree", "appName=Legacy Music\nwindowTitle=\n"),
    report_probe_step_fixture(&root, "capture-ax-tree", "window.captureAxTree", "appName=Netease Music\nwindowTitle=\n"),
  ];
  let app = app_identity_fixture("com.netease.163music", "NeteaseMusic");

  assert_eq!(resolve_probe_ocr_sample_query(&app, &window_steps), "Netease Music");
  assert_eq!(resolve_probe_ocr_sample_query(&app, &ax_steps), "Netease Music");
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
      failed_probe_step_fixture("probe-permissions", "app.probePermissions", "permission denied"),
      failed_probe_step_fixture("list-displays", "display.list", "display unavailable"),
      failed_probe_step_fixture("capture-ax-tree", "window.captureAxTree", "AX unavailable"),
    ],
  };

  let analysis = build_app_analysis(&probe_path, &probe).expect("partial probe should analyze");
  assert_eq!(analysis.app_identity.bundle_id, "com.example.Partial");
  assert!(analysis.known_boundaries.iter().any(|note| note.contains("probe-permissions")));
  assert!(analysis.known_boundaries.iter().any(|note| note.contains("AX snapshot was unavailable or partial")));
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
        run_id: auv_tracing_driver::trace::RunId::new("run_probe"),
        span_id: auv_tracing_driver::trace::SpanId::new("span_probe"),
        artifact_id: auv_tracing_driver::trace::ArtifactId::new("artifact_0001"),
        captured_event_id: Some(auv_tracing_driver::trace::EventId::new("event_probe")),
      }],
      promotion_gate: Some(AppCandidatePromotionGate {
        status: AppCandidatePromotionStatus::ActionGradeCandidate,
        missing_gates: Vec::new(),
        notes: vec!["Sample candidate satisfies the v0 search-entry promotion seam.".to_string()],
      }),
      input_bindings: BTreeMap::from([("focus_query".to_string(), "Search".to_string())]),
      compatibility: candidate_compatibility(&["search-entry.ax-text-input.clipboard-submit.capture-evidence"], &[]),
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

fn report_probe_step_fixture(root: &Path, id: &str, command_id: &str, report: &str) -> AppProbeStep {
  let path = root.join(format!("{id}.txt"));
  fs::write(&path, report).expect("probe report should write");
  probe_step_fixture(id, command_id, vec![path])
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
  Runtime::new(project_root.clone(), LocalStore::new(project_root).expect("store should initialize"))
}
