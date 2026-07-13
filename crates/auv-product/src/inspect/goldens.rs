//! Inspect goldens (test-only).
//!
//! Classification: test-only
//! Non-goals: no production control-flow changes.
//!
//! Set `AUV_UPDATE_INSPECT_GOLDENS=1` to refresh committed fixtures under
//! `tests/fixtures/inspect_goldens/`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use auv_tracing_driver::ArtifactFileSource;
use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::{
  ARTIFACT_API_VERSION, ArtifactRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION, SpanId,
  SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
};
use serde::Serialize;
use serde_json::json;

use crate::inspect::{build_product_inspect_composer, inspect_run, inspect_run_with};
use auv_cli::contract::{
  OBSERVATION_SNAPSHOT_API_VERSION, OPERATION_RESULT_API_VERSION, ObservationSnapshot, ObservationSource, OperationOutput, OperationResult,
  OperationStatus, RecognitionScope, RecognitionSurface, VERIFICATION_RESULT_API_VERSION, VerificationMethod, VerificationResult,
};
use auv_cli::scroll_scan::{
  CollectionObservation, CompletenessClaim, HookDecisionRecord, ObservationCluster, ScanPageRecord, ScanRegion, ScanTarget,
  ScrollBoundaryCandidate, ScrollScanArtifact, SectionCandidate, StopEvidence, StopPolicy, StopReason,
};

fn golden_dir() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/inspect_goldens")
}

fn normalize_inspect_text(raw: &str, roots: &[&Path]) -> String {
  let mut out = raw.to_string();
  for root in roots {
    let root_str = root.to_string_lossy();
    out = out.replace(root_str.as_ref(), "<TMP>");
  }
  if let Ok(tmp) = std::env::temp_dir().canonicalize() {
    out = out.replace(&tmp.to_string_lossy().into_owned(), "<TMPHOST>");
  }
  // Event ids embed wall-clock millis; span ids are process counters.
  let mut normalized = String::with_capacity(out.len());
  let bytes = out.as_bytes();
  let mut i = 0;
  while i < bytes.len() {
    if bytes[i..].starts_with(b"event_") {
      let mut j = i + "event_".len();
      while j < bytes.len() && (bytes[j].is_ascii_digit() || bytes[j] == b'_') {
        j += 1;
      }
      if j > i + "event_".len() {
        normalized.push_str("<EVENT_ID>");
        i = j;
        continue;
      }
    }
    if is_hex_digit(bytes[i]) {
      let mut j = i;
      while j < bytes.len() && is_hex_digit(bytes[j]) {
        j += 1;
      }
      let len = j - i;
      let boundary_before = i == 0 || !is_hex_digit(bytes[i - 1]);
      let boundary_after = j == bytes.len() || !is_hex_digit(bytes[j]);
      if len == 16 && boundary_before && boundary_after {
        normalized.push_str("<SPAN_ID>");
        i = j;
        continue;
      }
    }
    normalized.push(bytes[i] as char);
    i += 1;
  }
  normalized
}

fn is_hex_digit(b: u8) -> bool {
  b.is_ascii_digit() || (b'a'..=b'f').contains(&b) || (b'A'..=b'F').contains(&b)
}

fn assert_or_update_golden(name: &str, normalized: &str) {
  let path = golden_dir().join(name);
  // Only refresh when explicitly set to `1` (empty/"0"/other values stay read-only).
  if std::env::var("AUV_UPDATE_INSPECT_GOLDENS").as_deref() == Ok("1") {
    fs::create_dir_all(golden_dir()).expect("golden dir");
    fs::write(&path, normalized).expect("write golden");
    return;
  }
  let expected = fs::read_to_string(&path).unwrap_or_else(|error| panic!("missing golden {}: {error}", path.display()));
  assert_eq!(normalized, expected, "golden mismatch for {name}");
}

fn stamp() -> u128 {
  SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos()
}

fn dummy_run(run_id: &str) -> RunRecordV1Alpha1 {
  RunRecordV1Alpha1 {
    api_version: RUN_API_VERSION.to_string(),
    run_id: RunId::new(run_id),
    trace_id: TraceId::new("trace_golden"),
    run_type: RunType::Command,
    state: TraceState::Ended,
    status_code: TraceStatusCode::Ok,
    started_at_millis: 1,
    finished_at_millis: Some(2),
    root_span_id: SpanId::new("span_root"),
    attributes: BTreeMap::new(),
    summary: Some("golden run".to_string()),
    failure: None,
  }
}

fn dummy_span(span_id: &SpanId) -> SpanRecordV1Alpha1 {
  SpanRecordV1Alpha1 {
    api_version: SPAN_API_VERSION.to_string(),
    span_id: span_id.clone(),
    parent_span_id: None,
    name: "auv.inspect.span".to_string(),
    state: TraceState::Ended,
    status_code: TraceStatusCode::Ok,
    started_at_millis: 1,
    finished_at_millis: Some(2),
    attributes: BTreeMap::new(),
    summary: None,
    failure: None,
  }
}

fn stage_json_artifact<T: Serialize>(
  store: &LocalStore,
  root: &Path,
  run_id: &RunId,
  span_id: &SpanId,
  index: usize,
  role: &str,
  preferred_name: &str,
  value: &T,
) -> ArtifactRecordV1Alpha1 {
  let source_path = root.join(format!("source-{index}-{preferred_name}"));
  let rendered = serde_json::to_string_pretty(value).expect("artifact json should serialize") + "\n";
  fs::write(&source_path, rendered).expect("artifact source should write");
  store
    .stage_artifact_file(
      run_id,
      index,
      span_id,
      None,
      ArtifactFileSource {
        role: role.to_string(),
        source_path,
        preferred_name: preferred_name.to_string(),
        summary: None,
      },
    )
    .expect("artifact should stage")
}

fn dummy_observation_snapshot(run_id: &RunId, span_id: &SpanId) -> ObservationSnapshot {
  ObservationSnapshot {
    api_version: OBSERVATION_SNAPSHOT_API_VERSION.to_string(),
    snapshot_id: "snapshot_golden".to_string(),
    run_id: run_id.clone(),
    span_id: span_id.clone(),
    captured_at_millis: 150,
    source: ObservationSource::Visual,
    scope: RecognitionScope {
      surface: RecognitionSurface::Window,
      display_ref: None,
      native_display_id: None,
      app_bundle_id: Some("com.example.app".to_string()),
      window_title: Some("Example".to_string()),
      window_number: None,
      region_hint: None,
      capture_artifact: None,
      capture_contract_artifact: None,
    },
    capture_contract_ref: None,
    evidence: Vec::new(),
    nodes: Vec::new(),
    detail: json!({"producer": "scroll_scan"}),
    known_limits: vec!["visual only".to_string()],
  }
}

#[test]
fn golden_core_only_inspect_run() {
  let root = std::env::temp_dir().join(format!("auv-inspect-golden-core-{}", stamp()));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("root");
  let store = LocalStore::new(root.clone()).expect("store");
  let run = dummy_run("run_golden_core");
  let span = dummy_span(&run.root_span_id);
  let verification = VerificationResult {
    api_version: VERIFICATION_RESULT_API_VERSION.to_string(),
    method: VerificationMethod::TextVisible,
    executed: true,
    state_changed: true,
    semantic_matched: Some(true),
    failure_layer: None,
    evidence: Vec::new(),
    consumed_candidate_ref: None,
    consumed_node_ref: None,
    consumed_recognition_artifact_ref: None,
    consumed_recognition_id: None,
    consumed_recognized_item_id: None,
    observed_label: Some("hello".to_string()),
  };
  let operation = OperationResult {
    api_version: OPERATION_RESULT_API_VERSION.to_string(),
    run_id: run.run_id.clone(),
    status: OperationStatus::Completed,
    operation_id: "verify.golden".to_string(),
    evidence_artifacts: Vec::new(),
    output: OperationOutput::Verification {
      verification: Box::new(verification.clone()),
    },
    verifications: vec![verification],
    freshness_basis: None,
    known_limits: Vec::new(),
  };
  let observation = dummy_observation_snapshot(&run.run_id, &span.span_id);
  let scroll_scan = ScrollScanArtifact {
    scan_id: "scan_golden".to_string(),
    target: ScanTarget {
      application_id: Some("com.example.app".to_string()),
      window_title: Some("Example".to_string()),
      region: ScanRegion {
        left_ratio: 0.0,
        top_ratio: 0.0,
        right_ratio: 1.0,
        bottom_ratio: 1.0,
      },
    },
    stop_policy: StopPolicy::Bounded {
      max_pages: 1,
      max_scrolls: 0,
    },
    pages: Vec::<ScanPageRecord>::new(),
    observations: Vec::<CollectionObservation>::new(),
    nodes: Vec::new(),
    snapshots: vec![observation],
    clusters: Vec::<ObservationCluster>::new(),
    section_candidates: Vec::<SectionCandidate>::new(),
    scroll_boundary_candidates: Vec::<ScrollBoundaryCandidate>::new(),
    hook_decisions: Vec::<HookDecisionRecord>::new(),
    stop_evidence: StopEvidence {
      reason: StopReason::MaxPages,
      message: "bounded".to_string(),
      page_index: 0,
    },
    completeness_claim: CompletenessClaim::PartialMaxPages,
    warnings: Vec::new(),
  };
  let artifacts = vec![
    stage_json_artifact(&store, &root, &run.run_id, &span.span_id, 0, "operation-result", "op.json", &operation),
    stage_json_artifact(&store, &root, &run.run_id, &span.span_id, 1, "scroll-scan", "scan.json", &scroll_scan),
  ];
  assert_eq!(artifacts[0].api_version, ARTIFACT_API_VERSION);
  store
    .write_run_snapshot(&CanonicalRun {
      run,
      spans: vec![span],
      events: Vec::new(),
      artifacts,
    })
    .expect("snapshot");

  let composer = build_product_inspect_composer().expect("composer");
  let via_composer = inspect_run_with(&composer, &store, "run_golden_core").expect("composer inspect");
  let via_entry = inspect_run(&store, "run_golden_core").expect("entry inspect");
  assert_eq!(via_composer, via_entry, "CLI entry and composer path must match");

  let normalized = normalize_inspect_text(&via_composer, &[&root]);
  assert_or_update_golden("core.txt", &normalized);
  let _ = fs::remove_dir_all(root);
}

#[test]
fn golden_minecraft_training_package_inspect_run() {
  use auv_game_minecraft::{TrainingCompatibilityStatus, TrainingCompatibilityViewReport, TrainingPackageCounts, TrainingPackageManifest};

  let root = std::env::temp_dir().join(format!("auv-inspect-golden-mc-{}", stamp()));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("root");
  let store = LocalStore::new(root.clone()).expect("store");
  let run = dummy_run("run_golden_minecraft");
  let span = dummy_span(&run.root_span_id);
  let manifest = TrainingPackageManifest {
    schema_version: 1,
    generated_at_millis: 1,
    source_scene_packet_manifest_path: root.join("scene-packet/run.json").display().to_string(),
    source_bundle_manifest_paths: vec![root.join("bundle-a/run.json").display().to_string()],
    source_run_ids: vec!["run_a".to_string()],
    counts: TrainingPackageCounts {
      frames: 2,
      images: 2,
      compatibility_exported_frames: 2,
      compatibility_skipped_frames: 0,
    },
    frames: Vec::new(),
    compatibility_views: vec![TrainingCompatibilityViewReport {
      view_name: "nerfstudio".to_string(),
      status: TrainingCompatibilityStatus::Ready,
      exported_frame_count: 2,
      skipped_frame_count: 0,
      transforms_path: Some("compat/nerfstudio/transforms.json".to_string()),
      export_report_path: "compat/nerfstudio/export_report.json".to_string(),
      exported_frame_indices: vec![0, 1],
      frame_decisions: Vec::new(),
      skip_reason_counts: Vec::new(),
      warnings: Vec::new(),
      used_legacy_view_translation_fallback_frame_indices: Vec::new(),
      known_limits: Vec::new(),
    }],
    known_limits: vec!["golden fixture".to_string()],
  };
  let artifacts = vec![stage_json_artifact(
    &store,
    &root,
    &run.run_id,
    &span.span_id,
    0,
    crate::integrations::minecraft::MINECRAFT_3DGS_TRAINING_PACKAGE_ARTIFACT_ROLE,
    "minecraft-3dgs-training-package-run.json",
    &manifest,
  )];
  store
    .write_run_snapshot(&CanonicalRun {
      run,
      spans: vec![span],
      events: Vec::new(),
      artifacts,
    })
    .expect("snapshot");
  let text = inspect_run(&store, "run_golden_minecraft").expect("inspect");
  assert!(text.contains("MC-7 Training Packages:"));
  assert_or_update_golden("minecraft.txt", &normalize_inspect_text(&text, &[&root]));
  let _ = fs::remove_dir_all(root);
}

#[test]
fn golden_osu_visual_truth_inspect_run() {
  use auv_tracing_driver::recording::RunRecordingBackend;

  use crate::integrations::osu::run_osu_visual_truth_semantic_validation;

  let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-game-osu/tests/fixtures/osu_visual_truth_probe");
  let work = std::env::temp_dir().join(format!("auv-inspect-golden-osu-work-{}", stamp()));
  let store_root = std::env::temp_dir().join(format!("auv-inspect-golden-osu-store-{}", stamp()));
  let _ = fs::remove_dir_all(&work);
  let _ = fs::remove_dir_all(&store_root);
  fs::create_dir_all(&work).expect("work");
  fs::create_dir_all(&store_root).expect("store");
  for name in ["visual_truth_manifest.json", "projection.json"] {
    fs::copy(fixture_root.join(name), work.join(name)).expect("copy");
  }
  let store = LocalStore::new(store_root.clone()).expect("store");
  let recording = RunRecordingBackend::local_only(store.clone()).handle();
  let semantic = run_osu_visual_truth_semantic_validation(&recording, work.clone(), work.join("semantic-out")).expect("semantic");
  let text = inspect_run(&store, semantic.run_id.as_str()).expect("inspect");
  assert!(text.contains("Osu Visual Truth Semantic:"));
  let mut normalized = normalize_inspect_text(&text, &[&store_root, &work, &fixture_root]);
  normalized = normalized.replace(semantic.run_id.as_str(), "<RUN_ID>");
  assert_or_update_golden("osu.txt", &normalized);
  let _ = fs::remove_dir_all(work);
  let _ = fs::remove_dir_all(store_root);
}

#[test]
fn golden_balatro_card_detection_inspect_run() {
  use auv_game_balatro::ObjectZone;
  use auv_tracing_driver::recording::RunRecordingBackend;

  use crate::integrations::balatro::run_balatro_consumption_probe_chain;

  let fixture_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-game-balatro/tests/fixtures/balatro_consumption_probe");
  let store_root = std::env::temp_dir().join(format!("auv-inspect-golden-balatro-store-{}", stamp()));
  let work_dir = std::env::temp_dir().join(format!("auv-inspect-golden-balatro-work-{}", stamp()));
  let _ = fs::remove_dir_all(&store_root);
  let _ = fs::remove_dir_all(&work_dir);
  fs::create_dir_all(&store_root).expect("store");
  fs::create_dir_all(&work_dir).expect("work");
  let store = LocalStore::new(store_root.clone()).expect("store");
  let recording = RunRecordingBackend::local_only(store.clone()).handle();
  let chain = run_balatro_consumption_probe_chain(
    &recording,
    fixture_root.clone(),
    fixture_root.join("expected_slots.json"),
    auv_game_balatro::SlotId::new(ObjectZone::Hand, 0),
    work_dir.clone(),
  )
  .expect("probe");
  let text = inspect_run(&store, chain.run_id.as_str()).expect("inspect");
  assert!(text.contains("Balatro Card Detection Semantic:"));
  let mut normalized = normalize_inspect_text(&text, &[&store_root, &work_dir, &fixture_root]);
  normalized = normalized.replace(chain.run_id.as_str(), "<RUN_ID>");
  assert_or_update_golden("balatro.txt", &normalized);
  let _ = fs::remove_dir_all(store_root);
  let _ = fs::remove_dir_all(work_dir);
}

#[test]
fn mcp_and_composer_share_same_text_for_core_fixture() {
  let root = std::env::temp_dir().join(format!("auv-inspect-golden-parity-{}", stamp()));
  let _ = fs::remove_dir_all(&root);
  fs::create_dir_all(&root).expect("root");
  let store = LocalStore::new(root.clone()).expect("store");
  let run = dummy_run("run_golden_parity");
  let span = dummy_span(&run.root_span_id);
  store
    .write_run_snapshot(&CanonicalRun {
      run,
      spans: vec![span],
      events: Vec::new(),
      artifacts: Vec::new(),
    })
    .expect("snapshot");
  let composer = build_product_inspect_composer().expect("composer");
  let via_composer = inspect_run_with(&composer, &store, "run_golden_parity").expect("composer");
  let via_entry = inspect_run(&store, "run_golden_parity").expect("entry");
  assert_eq!(via_composer, via_entry);
  let mcp = auv_cli::mcp::McpServer::with_inspect_composer(root.clone(), composer.clone());
  let via_mcp = inspect_run_with(mcp.inspect_composer(), &store, "run_golden_parity").expect("mcp path");
  assert_eq!(via_composer, via_mcp);

  // The product inspect-server projection consumes the same composer for text/document.
  use auv_inspect_server::InspectReadProjection;
  let projection = crate::projection::ProductInspectReadProjection::with_composer(composer.clone());
  let via_server = projection.inspect_text(&store, "run_golden_parity").expect("server projection text").expect("product inspect text");
  assert_eq!(via_composer, via_server);
  let document = projection
    .inspect_document(&store, &store.read_run("run_golden_parity").expect("run"))
    .expect("server projection document")
    .expect("product inspect document");
  assert_eq!(document.render_text(), via_composer);
  assert!(!document.sections.is_empty(), "product document should include collected sections");

  let _ = fs::remove_dir_all(root);
}
