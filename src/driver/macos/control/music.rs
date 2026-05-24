use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};

use super::super::*;
use super::screen::click_screen_text;
use super::window_ocr::{
  capture_resolved_window_observation, click_window_row, detect_rows_for_capture,
};
use crate::contract::{
  AnchorRecheckPrecondition, ArtifactRef, Candidate, CandidateEvidence, CandidateLiveness,
  ControlRequirements, FailureLayer, FreshnessBasis, LivenessPreconditions, OperationOutput,
  OperationResult, OperationStatus, RatioRegion, TargetGrounding, TargetSpec, VerificationResult,
  WindowRefPrecondition,
};
use crate::model::ExecutionTarget;
use crate::trace::RunId;

/// Default `source_artifact_id` for callers consuming `music.search.results`
/// output. Coupled to the slot `music_search_results` uses for its
/// `OperationResult` JSON artifact; update both sides together if the producer
/// response shape changes.
const MUSIC_SEARCH_RESULTS_DEFAULT_OPERATION_RESULT_ARTIFACT_ID: &str = "artifact_0003";

struct ResolvedMusicCandidate {
  operation_result: OperationResult,
  candidate: Candidate,
}

struct CandidateLivenessCheck {
  anchor_recheck_ran: bool,
}

pub(crate) fn music_search_results(call: &DriverCall) -> AuvResult<DriverResponse> {
  let capture = capture_resolved_window_observation(call, "music-search-results")?;
  let (detection, rows) = detect_rows_for_capture(call, &capture)?;
  let region =
    parse_ocr_region_constraint(call, capture.dimensions.width, capture.dimensions.height)?;

  let app_bundle_id = app_identifier(call).unwrap_or_default();

  // The response contains four artifacts in this order. Reserve refs up front so
  // the OperationResult JSON (slot 2) can cite the recognition artifact (slot 3)
  // before either has been built. `push` below must follow the same slot order.
  let mut artifacts = DriverArtifactBuilder::new(&call.run_context);
  let screenshot_ref = artifacts.ref_at(0);
  let report_ref = artifacts.ref_at(1);
  let op_result_ref = artifacts.ref_at(2);
  let recognition_ref = artifacts.ref_at(3);

  let ocr_text_strategy = detection.strategy == "ocr-text";
  let candidates: Vec<Candidate> = rows
    .iter()
    .map(|row| {
      let anchor_text = row.text_fragments.first().cloned();
      let w = capture.dimensions.width.max(1) as f64;
      let h = capture.dimensions.height.max(1) as f64;
      let region = RatioRegion {
        left: row.bounds.x as f64 / w,
        top: row.bounds.y as f64 / h,
        right: (row.bounds.x + row.bounds.width) as f64 / w,
        bottom: (row.bounds.y + row.bounds.height) as f64 / h,
      };
      let joined_label = row.text_fragments.join(" ");
      // VisualRow grounding when OCR text detection fell back to visual-bands.
      // anchor_recheck requires window-scoped OCR; omit it for visual-bands rows
      // where window OCR returns no matches (e.g. WebView-based result lists).
      let (grounding, anchor_recheck) = if ocr_text_strategy {
        (
          TargetGrounding::OcrAnchor,
          anchor_text.as_ref().map(|text| AnchorRecheckPrecondition {
            text: text.clone(),
            region_hint: None,
            expected_min_confidence: 0.5,
            max_pixel_distance: 32.0,
          }),
        )
      } else {
        (TargetGrounding::VisualRow, None)
      };
      Candidate {
        candidate_local_id: format!("row#{}", row.row_index + 1),
        kind: "search_result_row".to_string(),
        label: if joined_label.is_empty() {
          None
        } else {
          Some(joined_label)
        },
        target_spec: TargetSpec {
          grounding,
          anchor_text: anchor_text.clone(),
          region_hint: Some(region),
          row_index: Some(row.row_index + 1),
        },
        evidence: CandidateEvidence {
          artifact_ref: screenshot_ref.clone(),
          observation: serde_json::json!({
            "provider": "vision_ocr.window_rows",
            "row_index": row.row_index,
            "source": row.source,
            "text_fragments": row.text_fragments,
            "bounds": {
              "x": row.bounds.x,
              "y": row.bounds.y,
              "width": row.bounds.width,
              "height": row.bounds.height,
            },
            "recognition_result_ref": recognition_ref.clone(),
            "recognized_item_id": format!("row#{}", row.row_index + 1),
          }),
        },
        liveness: CandidateLiveness {
          preconditions: LivenessPreconditions {
            window_ref: Some(WindowRefPrecondition {
              app_bundle_id: app_bundle_id.clone(),
              window_title_substring: None,
              window_number: None,
            }),
            anchor_recheck,
          },
          ttl_hint_ms: Some(5000),
        },
        control: ControlRequirements {
          requires_app_frontmost: true,
          requires_window_focus: true,
        },
        known_limits: Vec::new(),
      }
    })
    .collect();

  let operation_result = OperationResult {
    run_id: RunId::new(call.run_context.run_id.as_str()),
    status: OperationStatus::Completed,
    operation_id: "music.search.results".to_string(),
    evidence_artifacts: vec![
      screenshot_ref.clone(),
      report_ref.clone(),
      recognition_ref.clone(),
    ],
    output: OperationOutput::Candidates {
      candidates: candidates.clone(),
    },
    freshness_basis: Some(FreshnessBasis {
      source_artifact: Some(screenshot_ref.clone()),
      source_operation_id: Some("debug.findWindowRows".to_string()),
      notes: vec!["window-scoped OCR rows".to_string()],
    }),
    known_limits: Vec::new(),
  };

  let operation_result_json = serde_json::to_string_pretty(&operation_result)
    .map(|mut s| {
      s.push('\n');
      s
    })
    .map_err(|error| format!("failed to serialize OperationResult: {error}"))?;

  let screenshot = screenshot_artifact(&capture, "music-search-results", "music search results");
  let report = build_text_artifact(
    "window-rows-report",
    "txt",
    "music-search-results-rows",
    detection.report.clone(),
    "Row-detection report for music.search.results.",
  )?;
  let result_artifact = build_text_artifact(
    "operation-result",
    "json",
    "music-search-results-operation-result",
    operation_result_json,
    "Typed OperationResult candidate set for music.search.results.",
  )?;
  let (display_ref, native_display_id) = match &capture.capture_contract.capture_source {
    crate::driver::macos::capture::types::CaptureSource::Window {
      display_ref,
      native_display_id,
      ..
    } => (Some(display_ref.as_str()), Some(native_display_id.as_str())),
    _ => (None, None),
  };
  let recognition_artifact = row_recognition_artifact(
    "music-search-results-recognition",
    "music-search-results-recognition",
    "Structured recognition result for music.search.results row detection.",
    RowRecognitionArtifactRequest {
      recognition_id: "music_search_results".to_string(),
      source: recognition_source_for_rows(&detection.strategy, &rows),
      surface: crate::contract::RecognitionSurface::Window,
      rows: &rows,
      strategy: &detection.strategy,
      raw_match_count: detection.raw_match_count,
      filtered_match_count: detection.filtered_match_count,
      screenshot_path: capture.screenshot_path.as_path(),
      screenshot_dimensions: &capture.dimensions,
      display_ref,
      native_display_id,
      app_bundle_id: looks_like_bundle_identifier(&app_bundle_id).then_some(app_bundle_id.as_str()),
      window_title: None,
      window_number: window_number_from_ref(&capture.capture_source),
      region_hint: region
        .as_ref()
        .map(|value| observed_rect_to_ratio_region(value, &capture.dimensions)),
      capture_contract: Some(&capture.capture_contract),
      additional_detail: serde_json::json!({
        "scope": &capture.scope,
        "capture_source": &capture.capture_source,
        "consumer": "music.search.results",
      }),
      known_limits: vec![
        "driver-stage recognition evidence has no runtime artifact refs yet".to_string(),
      ],
    },
  )?;

  // Push in slot order: must match the ref_at(0..=3) reservations above.
  artifacts.push(screenshot);
  artifacts.push(report);
  artifacts.push(result_artifact);
  artifacts.push(recognition_artifact);

  let op_result_id = op_result_ref.artifact_id.as_str();
  let recognition_id = recognition_ref.artifact_id.as_str();
  let row_count = rows.len();
  let summary = if row_count > 0 {
    format!(
      "Produced {} search-result candidate(s) from window OCR rows (strategy {}); typed OperationResult at {} and structured recognition at {}.",
      row_count, detection.strategy, op_result_id, recognition_id
    )
  } else {
    format!(
      "Detected 0 rows inside resolved window after strategy {}; empty candidate set in OperationResult {} with structured recognition at {}.",
      detection.strategy, op_result_id, recognition_id
    )
  };

  Ok(DriverResponse {
    summary,
    backend: Some(format!(
      "macos.vision.music-search-results.{}",
      detection.strategy
    )),
    signals: crate::driver::macos::observe::row_detection_signals(row_count),
    notes: vec![
      "scope=window".to_string(),
      format!("windowRef={}", capture.capture_source),
      format!("rowStrategy={}", detection.strategy),
      format!("rowCount={row_count}"),
      format!("candidateCount={row_count}"),
      format!("operationResultArtifact={op_result_id}"),
      format!("recognitionResultArtifact={recognition_id}"),
    ],
    artifacts: artifacts.into_vec(),
  })
}

pub(crate) fn music_validate_candidate_liveness(call: &DriverCall) -> AuvResult<DriverResponse> {
  let source_run_id = required_non_empty_string(call, "source_run_id")?;
  let source_artifact_id = optional_non_empty_string(call, "source_artifact_id")
    .unwrap_or_else(|| MUSIC_SEARCH_RESULTS_DEFAULT_OPERATION_RESULT_ARTIFACT_ID.to_string());
  let candidate_local_id = required_non_empty_string(call, "candidate_local_id")?;

  let resolved = load_music_candidate(
    call,
    &source_run_id,
    &source_artifact_id,
    &candidate_local_id,
  )?;
  let liveness = check_music_candidate_liveness(call, &resolved.candidate, &candidate_local_id)?;

  let anchor_text = resolved
    .candidate
    .target_spec
    .anchor_text
    .clone()
    .unwrap_or_default();
  let row_index = resolved
    .candidate
    .target_spec
    .row_index
    .map(|value| value.to_string())
    .unwrap_or_default();
  let label = resolved.candidate.label.clone().unwrap_or_default();

  Ok(DriverResponse {
    summary: format!(
      "Candidate {candidate_local_id} liveness OK; anchor_text={anchor_text}; row_index={row_index}"
    ),
    backend: Some("macos.contract.music-validate-candidate-liveness".to_string()),
    signals: BTreeMap::from([
      ("candidate.resolved".to_string(), "true".to_string()),
      ("candidate.local_id".to_string(), candidate_local_id.clone()),
      ("candidate.anchor_text".to_string(), anchor_text),
      ("candidate.row_index".to_string(), row_index),
      ("candidate.label".to_string(), label),
      ("candidate.liveness_ok".to_string(), "true".to_string()),
      (
        "candidate.anchor_recheck_ran".to_string(),
        liveness.anchor_recheck_ran.to_string(),
      ),
    ]),
    notes: vec![
      format!("sourceRunId={source_run_id}"),
      format!("sourceArtifactId={source_artifact_id}"),
      format!("candidateLocalId={candidate_local_id}"),
      format!("operationId={}", resolved.operation_result.operation_id),
    ],
    artifacts: Vec::new(),
  })
}

pub(crate) fn music_result_play(call: &DriverCall) -> AuvResult<DriverResponse> {
  let source_run_id = required_non_empty_string(call, "source_run_id")?;
  let source_artifact_id = optional_non_empty_string(call, "source_artifact_id")
    .unwrap_or_else(|| MUSIC_SEARCH_RESULTS_DEFAULT_OPERATION_RESULT_ARTIFACT_ID.to_string());
  let candidate_local_id = required_non_empty_string(call, "candidate_local_id")?;
  let target_title = required_non_empty_string(call, "target_title")?;
  let target_artist = optional_non_empty_string(call, "target_artist");

  let resolved = match load_music_candidate(
    call,
    &source_run_id,
    &source_artifact_id,
    &candidate_local_id,
  ) {
    Ok(resolved) => resolved,
    Err(error) => {
      return music_result_play_failure_response(
        call,
        MusicResultPlayFailure {
          summary: format!("Could not resolve candidate {candidate_local_id}: {error}"),
          failure_layer: FailureLayer::CandidateExpired,
          resolved: false,
          executed: false,
          state_changed: false,
          observed_label: None,
          evidence: Vec::new(),
          notes: vec![
            format!("sourceRunId={source_run_id}"),
            format!("sourceArtifactId={source_artifact_id}"),
            format!("candidateLocalId={candidate_local_id}"),
            format!("error={}", report_text(&error)),
          ],
          artifacts: Vec::new(),
        },
      );
    }
  };
  let candidate = &resolved.candidate;
  let candidate_evidence = candidate_evidence_refs(candidate);

  let liveness = match check_music_candidate_liveness(call, candidate, &candidate_local_id) {
    Ok(liveness) => liveness,
    Err(error) => {
      return music_result_play_failure_response(
        call,
        MusicResultPlayFailure {
          summary: format!("Candidate {candidate_local_id} liveness failed: {error}"),
          failure_layer: FailureLayer::CandidateExpired,
          resolved: true,
          executed: false,
          state_changed: false,
          observed_label: candidate.label.clone(),
          evidence: candidate_evidence,
          notes: vec![
            format!("sourceRunId={source_run_id}"),
            format!("sourceArtifactId={source_artifact_id}"),
            format!("candidateLocalId={candidate_local_id}"),
            format!("error={}", report_text(&error)),
          ],
          artifacts: Vec::new(),
        },
      );
    }
  };

  let row_index = match candidate.target_spec.row_index {
    Some(row_index) => row_index,
    None => {
      return music_result_play_failure_response(
        call,
        MusicResultPlayFailure {
          summary: format!(
            "Candidate {candidate_local_id} cannot be played because target_spec.row_index is missing"
          ),
          failure_layer: FailureLayer::GroundingFailed,
          resolved: true,
          executed: false,
          state_changed: false,
          observed_label: candidate.label.clone(),
          evidence: candidate_evidence,
          notes: vec![
            format!("sourceRunId={source_run_id}"),
            format!("sourceArtifactId={source_artifact_id}"),
            format!("candidateLocalId={candidate_local_id}"),
            "missing=target_spec.row_index".to_string(),
          ],
          artifacts: Vec::new(),
        },
      );
    }
  };

  let app_id = resolve_music_result_play_app(call, candidate);
  let mut artifacts = Vec::new();
  let mut notes = vec![
    format!("sourceRunId={source_run_id}"),
    format!("sourceArtifactId={source_artifact_id}"),
    format!("candidateLocalId={candidate_local_id}"),
    format!("candidateRowIndex={row_index}"),
    format!(
      "candidateLabel={}",
      candidate.label.clone().unwrap_or_default()
    ),
    format!("candidateGrounding={:?}", candidate.target_spec.grounding),
    format!("candidateAnchorRecheckRan={}", liveness.anchor_recheck_ran),
  ];

  let row_response = match click_music_candidate_row(call, &app_id, row_index) {
    Ok(response) => response,
    Err(error) => {
      return music_result_play_failure_response(
        call,
        MusicResultPlayFailure {
          summary: format!("Candidate {candidate_local_id} row activation failed: {error}"),
          failure_layer: FailureLayer::ControlFailed,
          resolved: true,
          executed: true,
          state_changed: false,
          observed_label: candidate.label.clone(),
          evidence: candidate_evidence,
          notes: {
            let mut failure_notes = notes;
            failure_notes.push(format!("error={}", report_text(&error)));
            failure_notes
          },
          artifacts,
        },
      );
    }
  };
  notes.push(format!("rowClickSummary={}", row_response.summary));
  notes.extend(prefix_notes("row", &row_response.notes));
  artifacts.extend(row_response.artifacts);

  let press_response = match press_music_play_button(call, &app_id) {
    Ok(response) => response,
    Err(error) => {
      return music_result_play_failure_response(
        call,
        MusicResultPlayFailure {
          summary: format!("Candidate {candidate_local_id} play-button press failed: {error}"),
          failure_layer: FailureLayer::ControlFailed,
          resolved: true,
          executed: true,
          state_changed: false,
          observed_label: candidate.label.clone(),
          evidence: candidate_evidence,
          notes: {
            let mut failure_notes = notes;
            failure_notes.push(format!("error={}", report_text(&error)));
            failure_notes
          },
          artifacts,
        },
      );
    }
  };
  // click_screen_text uses full-display OCR; no AX/pointer fallback applies.
  let smart_press_strategy = press_response
    .backend
    .as_deref()
    .unwrap_or("screen-ocr")
    .to_string();
  let smart_press_fallback = "false".to_string();
  notes.push(format!("playPressSummary={}", press_response.summary));
  notes.extend(prefix_notes("playPress", &press_response.notes));
  artifacts.extend(press_response.artifacts);

  let verify_response = match verify_music_now_playing(call, &app_id, &target_title, &target_artist)
  {
    Ok(response) => response,
    Err(error) => {
      return music_result_play_failure_response(
        call,
        MusicResultPlayFailure {
          summary: format!(
            "Candidate {candidate_local_id} was activated, but now-playing verification failed: {error}"
          ),
          failure_layer: FailureLayer::VerificationUnreliable,
          resolved: true,
          executed: true,
          state_changed: true,
          observed_label: None,
          evidence: candidate_evidence,
          notes: {
            let mut failure_notes = notes;
            failure_notes.push(format!("error={}", report_text(&error)));
            failure_notes
          },
          artifacts,
        },
      );
    }
  };
  let observed_label = verify_response
    .signals
    .get("ax.now_playing_title")
    .cloned()
    .or_else(|| Some(target_title.clone()));
  notes.push(format!("verifySummary={}", verify_response.summary));
  notes.extend(prefix_notes("verify", &verify_response.notes));
  artifacts.extend(verify_response.artifacts);

  let verification = VerificationResult {
    executed: true,
    state_changed: true,
    semantic_matched: Some(true),
    failure_layer: None,
    evidence: candidate_evidence.clone(),
    observed_label,
  };
  let operation_result = music_result_play_operation_result(
    call,
    OperationStatus::Completed,
    verification,
    candidate_evidence,
  );
  artifacts.push(music_result_play_artifact(&operation_result)?);

  let mut signals = BTreeMap::from([
    ("music.result.play.resolved".to_string(), "true".to_string()),
    ("music.result.executed".to_string(), "true".to_string()),
    ("music.result.state_changed".to_string(), "true".to_string()),
    (
      "music.result.semantic_matched".to_string(),
      "true".to_string(),
    ),
    ("music.result.failure_layer".to_string(), "".to_string()),
    ("candidate.local_id".to_string(), candidate_local_id.clone()),
    ("candidate.row_index".to_string(), row_index.to_string()),
    ("candidate.liveness_ok".to_string(), "true".to_string()),
    (
      "candidate.anchor_recheck_ran".to_string(),
      liveness.anchor_recheck_ran.to_string(),
    ),
    ("target.title".to_string(), target_title.clone()),
    ("smartPress.strategy".to_string(), smart_press_strategy),
    ("smartPress.fallbackUsed".to_string(), smart_press_fallback),
  ]);
  if let Some(artist) = target_artist.as_deref() {
    signals.insert("target.artist".to_string(), artist.to_string());
  }

  Ok(DriverResponse {
    summary: format!(
      "Played candidate {candidate_local_id} (row {row_index}) and verified now-playing title {target_title}."
    ),
    backend: Some("macos.contract.music-result-play".to_string()),
    signals,
    notes,
    artifacts,
  })
}

fn find_artifact_path_in_jsonl(
  jsonl_path: &std::path::Path,
  artifact_id: &str,
) -> AuvResult<String> {
  let file = std::fs::File::open(jsonl_path).map_err(|error| {
    format!(
      "failed to open artifacts.jsonl at {}: {error}",
      jsonl_path.display()
    )
  })?;
  let reader = BufReader::new(file);
  for line in reader.lines() {
    let line = line.map_err(|error| format!("failed to read artifacts.jsonl: {error}"))?;
    if line.trim().is_empty() {
      continue;
    }
    let record: serde_json::Value = serde_json::from_str(&line)
      .map_err(|error| format!("failed to parse artifacts.jsonl entry: {error}"))?;
    if record.get("artifact_id").and_then(|v| v.as_str()) == Some(artifact_id) {
      return record
        .get("path")
        .and_then(|v| v.as_str())
        .map(|p| p.to_string())
        .ok_or_else(|| {
          format!("artifact {artifact_id} record has no 'path' field in artifacts.jsonl")
        });
    }
  }
  Err(format!(
    "artifact {artifact_id} not found in artifacts.jsonl at {}",
    jsonl_path.display()
  ))
}

fn load_music_candidate(
  call: &DriverCall,
  source_run_id: &str,
  source_artifact_id: &str,
  candidate_local_id: &str,
) -> AuvResult<ResolvedMusicCandidate> {
  let store_root = call.working_directory.join(".auv");
  let run_dir = store_root.join("runs").join(source_run_id);
  let artifacts_jsonl_path = run_dir.join("artifacts.jsonl");

  let artifact_relative_path =
    find_artifact_path_in_jsonl(&artifacts_jsonl_path, source_artifact_id)?;
  let artifact_abs_path = run_dir.join(&artifact_relative_path);

  let json_content = std::fs::read_to_string(&artifact_abs_path).map_err(|error| {
    format!("failed to read artifact {source_artifact_id} from run {source_run_id}: {error}")
  })?;

  let operation_result: OperationResult = serde_json::from_str(&json_content).map_err(|error| {
    format!("failed to parse OperationResult from {source_artifact_id}: {error}")
  })?;

  let candidates = match &operation_result.output {
    OperationOutput::Candidates { candidates } => candidates,
    OperationOutput::Verification { .. } => {
      return Err(format!(
        "artifact {source_artifact_id} contains a verification result, not a candidate set"
      ));
    }
    OperationOutput::Acknowledged { .. } => {
      return Err(format!(
        "artifact {source_artifact_id} contains an acknowledged result, not a candidate set"
      ));
    }
  };

  let candidate = candidates
    .iter()
    .find(|c| c.candidate_local_id == candidate_local_id)
    .cloned()
    .ok_or_else(|| {
      let available = candidates
        .iter()
        .map(|c| c.candidate_local_id.as_str())
        .collect::<Vec<_>>()
        .join(", ");
      format!(
        "candidate {candidate_local_id} not found in {source_artifact_id}; available: [{available}]"
      )
    })?;

  Ok(ResolvedMusicCandidate {
    operation_result,
    candidate,
  })
}

fn check_music_candidate_liveness(
  call: &DriverCall,
  candidate: &Candidate,
  candidate_local_id: &str,
) -> AuvResult<CandidateLivenessCheck> {
  if let Some(window_ref) = &candidate.liveness.preconditions.window_ref {
    let snapshot =
      crate::driver::macos::observe::observe_windows_snapshot(128, &window_ref.app_bundle_id)?;
    let selector = parse_app_selector(&window_ref.app_bundle_id)?;
    resolve_app_ref(&snapshot, &selector).map_err(|_| {
      format!(
        "candidate {candidate_local_id} liveness failed: app {} has no visible windows",
        window_ref.app_bundle_id
      )
    })?;
  }

  let anchor_recheck_ran = if let Some(anchor_recheck) =
    &candidate.liveness.preconditions.anchor_recheck
  {
    let app_bundle_id = candidate
      .liveness
      .preconditions
      .window_ref
      .as_ref()
      .map(|w| w.app_bundle_id.clone())
      .unwrap_or_default();
    if app_bundle_id.is_empty() {
      return Err(format!(
        "candidate {candidate_local_id} has anchor_recheck but no window_ref.app_bundle_id; cannot capture window"
      ));
    }
    let mut recheck_call = call.clone();
    recheck_call.inputs.insert("app".to_string(), app_bundle_id);
    let capture = capture_resolved_window_observation(&recheck_call, "liveness-anchor-recheck")
      .map_err(|error| {
        format!("candidate {candidate_local_id} liveness failed: window capture failed: {error}")
      })?;
    let ocr_result = crate::driver::macos::native::ocr::find_text(
      &capture.screenshot_path,
      &anchor_recheck.text,
      false,
      false,
      64,
      None,
    )?;
    let found = ocr_result
      .snapshot
      .matches
      .iter()
      .any(|m| m.confidence >= anchor_recheck.expected_min_confidence);
    if !found {
      return Err(format!(
        "candidate {candidate_local_id} liveness failed: anchor '{}' not found with confidence >= {:.2}",
        anchor_recheck.text, anchor_recheck.expected_min_confidence
      ));
    }
    true
  } else {
    false
  };

  Ok(CandidateLivenessCheck { anchor_recheck_ran })
}

fn candidate_evidence_refs(candidate: &Candidate) -> Vec<ArtifactRef> {
  let mut refs = vec![candidate.evidence.artifact_ref.clone()];
  if let Some(recognition_ref) = recognition_result_ref(&candidate.evidence)
    && recognition_ref != candidate.evidence.artifact_ref
  {
    refs.push(recognition_ref);
  }
  refs
}

fn recognition_result_ref(evidence: &CandidateEvidence) -> Option<ArtifactRef> {
  let value = evidence.observation.get("recognition_result_ref")?.clone();
  serde_json::from_value(value).ok()
}

fn resolve_music_result_play_app(call: &DriverCall, candidate: &Candidate) -> String {
  app_identifier(call).unwrap_or_else(|| {
    candidate
      .liveness
      .preconditions
      .window_ref
      .as_ref()
      .map(|window_ref| window_ref.app_bundle_id.clone())
      .unwrap_or_default()
  })
}

fn click_music_candidate_row(
  call: &DriverCall,
  app_id: &str,
  row_index: usize,
) -> AuvResult<DriverResponse> {
  let mut inputs = BTreeMap::new();
  inputs.insert("row_index".to_string(), row_index.to_string());
  inputs.insert("label".to_string(), "music-result-play-row".to_string());
  // Do NOT activate before capture by default — activating QQ音乐 when it is not
  // frontmost causes it to navigate away from search results to the home screen.
  // The caller must ensure QQ音乐 is already frontmost with search results visible.
  inputs.insert(
    "activate_target_before_capture".to_string(),
    optional_string(call, "activate_target_before_capture").unwrap_or_else(|| "false".to_string()),
  );
  copy_input_or_default(call, &mut inputs, "click_count", "click_count", "2");
  copy_input_or_default(
    call,
    &mut inputs,
    "activation_settle_ms",
    "settle_ms",
    "900",
  );
  copy_input_or_default(
    call,
    &mut inputs,
    "row_min_confidence",
    "min_confidence",
    "0.90",
  );
  copy_input_or_default(
    call,
    &mut inputs,
    "row_max_observations",
    "max_observations",
    "128",
  );
  copy_input_or_default(
    call,
    &mut inputs,
    "row_anchor_x_ratio",
    "row_anchor_x_ratio",
    "0.25",
  );
  copy_input_or_default(
    call,
    &mut inputs,
    "row_anchor_y_ratio",
    "row_anchor_y_ratio",
    "0.50",
  );
  copy_optional_input(call, &mut inputs, "row_anchor_mode", "row_anchor_mode");

  click_window_row(&DriverCall {
    operation: "click_window_row".to_string(),
    target: ExecutionTarget {
      application_id: Some(app_id.to_string()),
    },
    inputs,
    working_directory: call.working_directory.clone(),
    run_context: call.run_context.clone(),
  })
}

fn press_music_play_button(call: &DriverCall, app_id: &str) -> AuvResult<DriverResponse> {
  let query =
    optional_non_empty_string(call, "play_button_query").unwrap_or_else(|| "播放".to_string());
  let min_confidence = optional_f64(call, "play_button_min_confidence")?.unwrap_or(0.75);
  let label = optional_non_empty_string(call, "play_button_overlay_label")
    .unwrap_or_else(|| "auv · play".to_string());

  // Window-relative region ratios for the "播放" hover button search area.
  let win_left = optional_f64(call, "play_button_region_left_ratio")?.unwrap_or(0.18);
  let win_top = optional_f64(call, "play_button_region_top_ratio")?.unwrap_or(0.28);
  let win_right = optional_f64(call, "play_button_region_right_ratio")?.unwrap_or(0.60);
  let win_bottom = optional_f64(call, "play_button_region_bottom_ratio")?.unwrap_or(0.42);

  // smart_press / ax_click_window_text cannot find "播放" in QQ音乐's WebView
  // because the native window capture misses WebView-rendered pixels. Use
  // click_screen_text (full-display OCR) instead, converting the window-relative
  // search region to screen-relative ratios so the OCR search stays anchored to
  // the visible QQ音乐 window rather than scanning the whole display.
  let snapshot = super::super::observe::observe_windows_snapshot(24, app_id)?;
  let selector = parse_app_selector(app_id)?;
  let resolved_app = resolve_app_ref(&snapshot, &selector)?;
  let xcap_displays = super::super::capture::xcap_backend::list_displays()?;
  let window_candidate = resolve_window_candidate(
    &snapshot,
    &resolved_app,
    &xcap_displays,
    &WindowSelection::default(),
  )?;
  let wb = &window_candidate.window_ref.bounds;

  let display_snapshot = enumerate_displays()?;
  let main_display = display_snapshot
    .displays
    .iter()
    .find(|d| d.is_main)
    .ok_or_else(|| "press_music_play_button: no main display found".to_string())?;
  let screen_w = main_display.bounds.width as f64;
  let screen_h = main_display.bounds.height as f64;
  let screen_ox = main_display.bounds.x as f64;
  let screen_oy = main_display.bounds.y as f64;

  // Absolute logical screen coordinates of the search region corners.
  let abs_left = wb.x as f64 + win_left * wb.width as f64;
  let abs_top = wb.y as f64 + win_top * wb.height as f64;
  let abs_right = wb.x as f64 + win_right * wb.width as f64;
  let abs_bottom = wb.y as f64 + win_bottom * wb.height as f64;

  // Screen-relative ratios for click_screen_text, clamped to [0, 1].
  let screen_left = ((abs_left - screen_ox) / screen_w).clamp(0.0, 1.0);
  let screen_top = ((abs_top - screen_oy) / screen_h).clamp(0.0, 1.0);
  let screen_right = ((abs_right - screen_ox) / screen_w).clamp(0.0, 1.0);
  let screen_bottom = ((abs_bottom - screen_oy) / screen_h).clamp(0.0, 1.0);

  let mut inputs = BTreeMap::new();
  inputs.insert("query".to_string(), query);
  inputs.insert("min_confidence".to_string(), format!("{min_confidence:.3}"));
  inputs.insert("label".to_string(), label);
  inputs.insert("region_left_ratio".to_string(), format!("{screen_left:.4}"));
  inputs.insert("region_top_ratio".to_string(), format!("{screen_top:.4}"));
  inputs.insert(
    "region_right_ratio".to_string(),
    format!("{screen_right:.4}"),
  );
  inputs.insert(
    "region_bottom_ratio".to_string(),
    format!("{screen_bottom:.4}"),
  );

  click_screen_text(&DriverCall {
    operation: "click_screen_text".to_string(),
    target: ExecutionTarget {
      application_id: Some(app_id.to_string()),
    },
    inputs,
    working_directory: call.working_directory.clone(),
    run_context: call.run_context.clone(),
  })
}

fn verify_music_now_playing(
  call: &DriverCall,
  app_id: &str,
  target_title: &str,
  target_artist: &Option<String>,
) -> AuvResult<DriverResponse> {
  let mut inputs = BTreeMap::new();
  inputs.insert("target_title".to_string(), target_title.to_string());
  if let Some(artist) = target_artist.as_deref() {
    inputs.insert("target_artist".to_string(), artist.to_string());
  }
  inputs.insert(
    "scope_path_prefix".to_string(),
    optional_non_empty_string(call, "now_playing_scope_path_prefix")
      .unwrap_or_else(|| "0.4.4".to_string()),
  );
  copy_input_or_default(call, &mut inputs, "max_depth", "max_depth", "8");
  copy_input_or_default(call, &mut inputs, "max_children", "max_children", "40");

  crate::driver::macos::observe::verify_now_playing_title(&DriverCall {
    operation: "verify_now_playing_title".to_string(),
    target: ExecutionTarget {
      application_id: Some(app_id.to_string()),
    },
    inputs,
    working_directory: call.working_directory.clone(),
    run_context: call.run_context.clone(),
  })
}

struct MusicResultPlayFailure {
  summary: String,
  failure_layer: FailureLayer,
  resolved: bool,
  executed: bool,
  state_changed: bool,
  observed_label: Option<String>,
  evidence: Vec<ArtifactRef>,
  notes: Vec<String>,
  artifacts: Vec<ProducedArtifact>,
}

fn music_result_play_failure_response(
  call: &DriverCall,
  failure: MusicResultPlayFailure,
) -> AuvResult<DriverResponse> {
  let verification = VerificationResult {
    executed: failure.executed,
    state_changed: failure.state_changed,
    semantic_matched: Some(false),
    failure_layer: Some(failure.failure_layer),
    evidence: failure.evidence.clone(),
    observed_label: failure.observed_label,
  };
  let operation_result = music_result_play_operation_result(
    call,
    OperationStatus::Failed,
    verification,
    failure.evidence,
  );
  let mut artifacts = failure.artifacts;
  artifacts.push(music_result_play_artifact(&operation_result)?);
  let mut signals = BTreeMap::from([
    (
      "music.result.play.resolved".to_string(),
      failure.resolved.to_string(),
    ),
    (
      "music.result.executed".to_string(),
      failure.executed.to_string(),
    ),
    (
      "music.result.state_changed".to_string(),
      failure.state_changed.to_string(),
    ),
    (
      "music.result.semantic_matched".to_string(),
      "false".to_string(),
    ),
    (
      "music.result.failure_layer".to_string(),
      serde_json::to_value(failure.failure_layer)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_default(),
    ),
  ]);
  if let Some(app) = app_identifier(call) {
    signals.insert("target.app".to_string(), app);
  }
  Ok(DriverResponse {
    summary: failure.summary,
    backend: Some("macos.contract.music-result-play".to_string()),
    signals,
    notes: failure.notes,
    artifacts,
  })
}

fn music_result_play_operation_result(
  call: &DriverCall,
  status: OperationStatus,
  verification: VerificationResult,
  evidence_artifacts: Vec<ArtifactRef>,
) -> OperationResult {
  OperationResult {
    run_id: RunId::new(call.run_context.run_id.as_str()),
    status,
    operation_id: "music.result.play".to_string(),
    evidence_artifacts,
    output: OperationOutput::Verification { verification },
    freshness_basis: None,
    known_limits: Vec::new(),
  }
}

fn music_result_play_artifact(operation_result: &OperationResult) -> AuvResult<ProducedArtifact> {
  let operation_result_json = serde_json::to_string_pretty(operation_result)
    .map(|mut s| {
      s.push('\n');
      s
    })
    .map_err(|error| format!("failed to serialize music.result.play OperationResult: {error}"))?;
  build_text_artifact(
    "operation-result",
    "json",
    "music-result-play-operation-result",
    operation_result_json,
    "Typed OperationResult verification for music.result.play.",
  )
}

fn copy_optional_input(
  call: &DriverCall,
  inputs: &mut BTreeMap<String, String>,
  source_key: &str,
  target_key: &str,
) {
  if let Some(value) = optional_non_empty_string(call, source_key) {
    inputs.insert(target_key.to_string(), value);
  }
}

fn copy_input_or_default(
  call: &DriverCall,
  inputs: &mut BTreeMap<String, String>,
  source_key: &str,
  target_key: &str,
  default_value: &str,
) {
  inputs.insert(
    target_key.to_string(),
    optional_non_empty_string(call, source_key).unwrap_or_else(|| default_value.to_string()),
  );
}

fn prefix_notes(prefix: &str, notes: &[String]) -> Vec<String> {
  notes
    .iter()
    .map(|note| format!("{prefix}.{note}"))
    .collect()
}

fn report_text(raw: &str) -> String {
  raw.replace('\n', " ")
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::trace::{ArtifactId, SpanId};
  use serde_json::json;

  // Slot positions `music_search_results` reserves in its DriverArtifactBuilder.
  // These tests exercise the *consumer* side (candidate_evidence_refs); the
  // labels match what the producer's builder will emit so failures point at
  // the right contract if the producer shape ever drifts.
  const SCREENSHOT_ARTIFACT_ID: &str = "artifact_0001";
  const RECOGNITION_RESULT_ARTIFACT_ID: &str = "artifact_0004";

  fn artifact_ref(id: &str) -> ArtifactRef {
    ArtifactRef {
      run_id: RunId::new("run_test"),
      artifact_id: ArtifactId::new(id),
      span_id: SpanId::new("span_test"),
      captured_event_id: None,
    }
  }

  fn sample_candidate(observation: serde_json::Value) -> Candidate {
    Candidate {
      candidate_local_id: "row#1".to_string(),
      kind: "search_result_row".to_string(),
      label: Some("Song A".to_string()),
      target_spec: TargetSpec {
        grounding: TargetGrounding::OcrAnchor,
        anchor_text: Some("Song A".to_string()),
        region_hint: None,
        row_index: Some(1),
      },
      evidence: CandidateEvidence {
        artifact_ref: artifact_ref(SCREENSHOT_ARTIFACT_ID),
        observation,
      },
      liveness: CandidateLiveness {
        preconditions: LivenessPreconditions {
          window_ref: None,
          anchor_recheck: None,
        },
        ttl_hint_ms: None,
      },
      control: ControlRequirements {
        requires_app_frontmost: true,
        requires_window_focus: true,
      },
      known_limits: Vec::new(),
    }
  }

  #[test]
  fn candidate_evidence_refs_include_recognition_artifact_when_present() {
    let candidate = sample_candidate(json!({
      "recognition_result_ref": artifact_ref(RECOGNITION_RESULT_ARTIFACT_ID),
      "recognized_item_id": "row#1"
    }));

    let refs = candidate_evidence_refs(&candidate);

    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].artifact_id.as_str(), SCREENSHOT_ARTIFACT_ID);
    assert_eq!(refs[1].artifact_id.as_str(), RECOGNITION_RESULT_ARTIFACT_ID);
  }

  #[test]
  fn candidate_evidence_refs_stay_backward_compatible_without_recognition_ref() {
    let candidate = sample_candidate(json!({
      "provider": "vision_ocr.window_rows"
    }));

    let refs = candidate_evidence_refs(&candidate);

    assert_eq!(refs.len(), 1);
    assert_eq!(refs[0].artifact_id.as_str(), SCREENSHOT_ARTIFACT_ID);
  }

  #[test]
  fn recognition_result_ref_ignores_invalid_shape() {
    let candidate = sample_candidate(json!({
      "recognition_result_ref": { "artifact_id": "artifact_0004" }
    }));

    assert!(recognition_result_ref(&candidate.evidence).is_none());
  }
}
