//! View-parser inspect read extractors (SceneBridge A8b/A8c).
//!
//! Kept separate from the large `run_read` module; `run_read` re-exports these helpers.

use std::fs::File;
use std::io::BufReader;

use serde::de::DeserializeOwned;

use auv_tracing_driver::store::{CanonicalRun, LocalStore};
use auv_tracing_driver::trace::ArtifactRecordV1Alpha1;
use auv_view::memory::{
  ATTR_MEMORY_LOAD_MEMORY_ID, ATTR_MEMORY_LOAD_SOURCE_RUN_ID, ATTR_REACQUIRE_FATAL_DIAGNOSTIC_KIND,
  ATTR_REACQUIRE_OBSERVATION_COUNT, ATTR_REACQUIRE_OUTCOME, ATTR_REACQUIRE_SCOPE_ID,
  ATTR_REACQUIRE_SKIPPED_RESCAN_REPLAY, ATTR_REACQUIRE_STAGE_USED, ATTR_REACQUIRE_TARGET_KIND,
  GeometryProofSummary, IdentityProofSummary, MemoryProofSummary,
  PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE, ReacquisitionRecord, ReplayProofSummary,
  ResolutionProofSummary, SPAN_REACQUIRE_MEMORY_LOAD, SPAN_REACQUIRE_ROOT_PREFIX,
  SPAN_REACQUIRE_STAGE_PREFIX, VIEW_MEMORY_ARTIFACT_ROLE, VerificationProofSummary, ViewMemory,
  ViewParserInspect, ViewParserSelectResultWire, ViewResolutionSummary,
};

use crate::model::AuvResult;

fn is_json_mime(mime_type: &str) -> bool {
  mime_type == "application/json" || mime_type.ends_with("+json")
}

fn read_artifact_json<T: DeserializeOwned>(
  store: &LocalStore,
  run_id: &str,
  artifact: &ArtifactRecordV1Alpha1,
  artifact_role: &str,
) -> AuvResult<T> {
  let (_record, path) = store
    .artifact_file(run_id, artifact.artifact_id.as_str())
    .map_err(|error| format!("failed to open {artifact_role} artifact: {error}"))?;
  let file = File::open(&path).map_err(|error| {
    format!(
      "failed to open {artifact_role} artifact {} for run {run_id} from {}: {error}",
      artifact.artifact_id,
      path.display()
    )
  })?;
  serde_json::from_reader(BufReader::new(file)).map_err(|error| {
    format!(
      "failed to parse {artifact_role} artifact {} for run {run_id} from {}: {error}",
      artifact.artifact_id,
      path.display()
    )
  })
}

pub fn list_view_memory_writes(store: &LocalStore, run_id: &str) -> AuvResult<Vec<ViewMemory>> {
  let run = store.read_run(run_id)?;
  extract_view_memory_writes(store, &run)
}

pub fn extract_view_memory_writes(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<ViewMemory>> {
  let mut memories = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != VIEW_MEMORY_ARTIFACT_ROLE || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let memory: ViewMemory = read_artifact_json(
      store,
      run.run.run_id.as_str(),
      artifact,
      VIEW_MEMORY_ARTIFACT_ROLE,
    )?;
    memories.push(memory);
  }
  Ok(memories)
}

fn normalize_reacquire_outcome(outcome: &str) -> String {
  outcome.replace('-', "_")
}

fn span_attr_str(
  span: &auv_tracing_driver::trace::SpanRecordV1Alpha1,
  key: &str,
) -> Option<String> {
  span
    .attributes
    .get(key)
    .and_then(|value| value.as_str())
    .map(str::to_string)
}

fn span_attr_usize(span: &auv_tracing_driver::trace::SpanRecordV1Alpha1, key: &str) -> usize {
  span
    .attributes
    .get(key)
    .and_then(|value| value.as_u64())
    .map(|value| value as usize)
    .unwrap_or(0)
}

fn span_attr_bool(span: &auv_tracing_driver::trace::SpanRecordV1Alpha1, key: &str) -> Option<bool> {
  span.attributes.get(key).and_then(|value| match value {
    serde_json::Value::Bool(flag) => Some(*flag),
    serde_json::Value::String(text) => match text.as_str() {
      "true" => Some(true),
      "false" => Some(false),
      _ => None,
    },
    _ => None,
  })
}

pub fn extract_reacquisition_records(run: &CanonicalRun) -> Vec<ReacquisitionRecord> {
  run
    .spans
    .iter()
    .filter_map(|span| {
      if span.name == SPAN_REACQUIRE_MEMORY_LOAD
        || span.name.starts_with(SPAN_REACQUIRE_STAGE_PREFIX)
      {
        return None;
      }
      let scope_id = span.name.strip_prefix(SPAN_REACQUIRE_ROOT_PREFIX)?;
      let outcome = span_attr_str(span, ATTR_REACQUIRE_OUTCOME)
        .map(|value| normalize_reacquire_outcome(&value))
        .unwrap_or_else(|| "unknown".to_string());
      let stage_used =
        span_attr_str(span, ATTR_REACQUIRE_STAGE_USED).unwrap_or_else(|| "none".into());
      let strategy_used = if stage_used == "none" {
        None
      } else {
        Some(stage_used.clone())
      };
      let stale_reason = span_attr_str(span, ATTR_REACQUIRE_FATAL_DIAGNOSTIC_KIND);
      Some(ReacquisitionRecord {
        span_name: span.name.clone(),
        scope_id: span_attr_str(span, ATTR_REACQUIRE_SCOPE_ID)
          .unwrap_or_else(|| scope_id.to_string()),
        target_kind: span_attr_str(span, ATTR_REACQUIRE_TARGET_KIND)
          .unwrap_or_else(|| "unknown".into()),
        outcome,
        stage_used,
        observation_count: span_attr_usize(span, ATTR_REACQUIRE_OBSERVATION_COUNT),
        skipped_rescan_replay: span_attr_bool(span, ATTR_REACQUIRE_SKIPPED_RESCAN_REPLAY),
        stale_reason,
        strategy_used,
      })
    })
    .collect()
}

pub fn extract_playlist_select_result_wires(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<Vec<ViewParserSelectResultWire>> {
  let mut wires = Vec::new();
  for artifact in &run.artifacts {
    if artifact.role != PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE || !is_json_mime(&artifact.mime_type) {
      continue;
    }
    let wire: ViewParserSelectResultWire = read_artifact_json(
      store,
      run.run.run_id.as_str(),
      artifact,
      PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE,
    )?;
    wires.push(wire);
  }
  Ok(wires)
}

pub fn build_view_resolution_summary(
  run: &CanonicalRun,
  memory_writes: &[ViewMemory],
  reacquisitions: &[ReacquisitionRecord],
  select_wire: &ViewParserSelectResultWire,
) -> ViewResolutionSummary {
  let scope_id = reacquisitions
    .first()
    .map(|record| record.scope_id.as_str())
    .unwrap_or("playlist_sidebar");
  let memory = memory_proof_for_scope(run, memory_writes, scope_id);
  let reacquire = select_wire.reacquire.as_ref();
  let span_record = reacquisitions
    .iter()
    .find(|record| record.scope_id == scope_id);
  let resolution = ResolutionProofSummary {
    outcome: reacquire
      .map(|value| normalize_reacquire_outcome(&value.outcome))
      .or_else(|| span_record.map(|record| record.outcome.clone()))
      .unwrap_or_else(|| "unknown".to_string()),
    strategy_used: reacquire
      .and_then(|value| value.strategy_used.clone())
      .or_else(|| span_record.and_then(|record| record.strategy_used.clone())),
    stale_reason: reacquire
      .and_then(|value| value.stale_reason.clone())
      .or_else(|| span_record.and_then(|record| record.stale_reason.clone())),
    observation_count: reacquire
      .map(|value| value.observation_count)
      .or_else(|| span_record.map(|record| record.observation_count))
      .unwrap_or(0),
    span_scope_id: span_record.map(|record| record.scope_id.clone()),
  };
  let replay = ReplayProofSummary {
    step_names: select_wire
      .steps
      .iter()
      .map(|step| step.name.clone())
      .collect(),
    skipped_rescan_replay: reacquire
      .map(|value| value.skipped_rescan_replay)
      .or_else(|| span_record.and_then(|record| record.skipped_rescan_replay))
      .unwrap_or(false),
  };
  let has_bounds = select_wire
    .steps
    .iter()
    .any(|step| step.target_bounds.is_some());
  ViewResolutionSummary {
    query: select_wire.query.clone(),
    identity: IdentityProofSummary {
      label: select_wire.target.label.clone(),
      section_kind: select_wire.target.section_kind.clone(),
      anchor_id: select_wire.target.anchor_id.clone(),
    },
    memory,
    resolution,
    replay,
    verification: VerificationProofSummary {
      status: select_wire.verification.status.clone(),
      method: select_wire.verification.method.clone(),
    },
    geometry_note: GeometryProofSummary {
      has_ephemeral_target_bounds: has_bounds,
      note: "target_bounds and observation_index are tier IV ephemeral geometry only".into(),
    },
  }
}

fn memory_proof_for_scope(
  run: &CanonicalRun,
  memory_writes: &[ViewMemory],
  scope_id: &str,
) -> MemoryProofSummary {
  if let Some(memory) = memory_writes
    .iter()
    .find(|memory| memory.scope_id == scope_id)
  {
    return MemoryProofSummary {
      present: true,
      memory_id: Some(memory.memory_id.clone()),
      source_run_id: Some(memory.source_run_id.clone()),
      last_reconstructed_at_millis: Some(memory.last_reconstructed_at_millis),
      anchor_count: Some(memory.anchors.len()),
    };
  }
  if let Some(span) = run
    .spans
    .iter()
    .find(|span| span.name == SPAN_REACQUIRE_MEMORY_LOAD)
  {
    return MemoryProofSummary {
      present: true,
      memory_id: span_attr_str(span, ATTR_MEMORY_LOAD_MEMORY_ID),
      source_run_id: span_attr_str(span, ATTR_MEMORY_LOAD_SOURCE_RUN_ID),
      last_reconstructed_at_millis: None,
      anchor_count: None,
    };
  }
  MemoryProofSummary {
    present: false,
    memory_id: None,
    source_run_id: None,
    last_reconstructed_at_millis: None,
    anchor_count: None,
  }
}

pub fn build_view_parser_inspect(
  store: &LocalStore,
  run: &CanonicalRun,
) -> AuvResult<ViewParserInspect> {
  let memory_writes = extract_view_memory_writes(store, run)?;
  let reacquisitions = extract_reacquisition_records(run);
  let select_results = extract_playlist_select_result_wires(store, run)?;
  let resolution_summaries = select_results
    .iter()
    .map(|wire| build_view_resolution_summary(run, &memory_writes, &reacquisitions, wire))
    .collect();
  Ok(ViewParserInspect {
    memory_writes,
    reacquisitions,
    select_results,
    resolution_summaries,
  })
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::fs;
  use std::path::Path;

  use serde::Serialize;

  use auv_tracing_driver::store::{CanonicalRun, LocalStore};
  use auv_tracing_driver::trace::{
    ArtifactRecordV1Alpha1, RUN_API_VERSION, RunId, RunRecordV1Alpha1, RunType, SPAN_API_VERSION,
    SpanId, SpanRecordV1Alpha1, TraceId, TraceState, TraceStatusCode,
  };
  use auv_view::ViewBounds;
  use auv_view::memory::{
    ATTR_MEMORY_LOAD_MEMORY_ID, ATTR_MEMORY_LOAD_SOURCE_RUN_ID, ATTR_REACQUIRE_OBSERVATION_COUNT,
    ATTR_REACQUIRE_OUTCOME, ATTR_REACQUIRE_SCOPE_ID, ATTR_REACQUIRE_SKIPPED_RESCAN_REPLAY,
    ATTR_REACQUIRE_STAGE_USED, ATTR_REACQUIRE_TARGET_KIND, PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE,
    SPAN_REACQUIRE_MEMORY_LOAD, VIEW_MEMORY_ARTIFACT_ROLE, VIEW_MEMORY_SCHEMA_VERSION, ViewMemory,
    ViewMemoryScopeSnapshot, build_memory_id, reacquire_root_span_name,
    view_memory_lineage_ref_wire,
  };

  use super::{
    build_view_parser_inspect, build_view_resolution_summary, extract_playlist_select_result_wires,
    extract_reacquisition_records, extract_view_memory_writes, list_view_memory_writes,
  };

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
    let rendered =
      serde_json::to_string_pretty(value).expect("artifact json should serialize") + "\n";
    fs::write(&source_path, rendered).expect("artifact source should write");
    store
      .stage_artifact_file(
        run_id,
        index,
        span_id,
        None,
        auv_tracing_driver::ArtifactFileSource {
          role: role.to_string(),
          source_path,
          preferred_name: preferred_name.to_string(),
          summary: None,
        },
      )
      .expect("artifact should stage")
  }

  fn sample_memory(run_id: &str) -> ViewMemory {
    ViewMemory {
      schema_version: VIEW_MEMORY_SCHEMA_VERSION.to_string(),
      memory_id: build_memory_id("com.netease.163music", "playlist_sidebar"),
      app_bundle_id: "com.netease.163music".into(),
      scope_id: "playlist_sidebar".into(),
      last_reconstructed_at_millis: 1_719_744_000_000,
      source_run_id: run_id.into(),
      source_reconstruction_ref: view_memory_lineage_ref_wire(run_id, "artifact_0001"),
      anchors: Vec::new(),
      landmarks: Vec::new(),
      node_snapshots: Default::default(),
      scope_snapshot: ViewMemoryScopeSnapshot {
        region_id: "playlist_sidebar".into(),
        region_bounds_window_local: ViewBounds::new(0.0, 0.0, 240.0, 400.0),
        baseline_width: 240,
        schema_version_view_ir: "view-ir-v0".into(),
      },
      diagnostics: Vec::new(),
    }
  }

  fn write_select_run_fixture(store: &LocalStore, root: &Path) -> String {
    let run_id = RunId::new("run_view_parser_select_proof");
    let span_id = SpanId::new("span_select_proof");
    let run_id_str = run_id.as_str().to_string();
    let memory = sample_memory(&run_id_str);
    let select_json = serde_json::json!({
      "command": "playlist.select",
      "query": "Test",
      "target": {
        "label": "Test Playlist",
        "section_kind": "my_playlists",
        "anchor_id": "anchor.test"
      },
      "steps": [
        {"name": "reacquire-target", "target_bounds": {"x": 1.0, "y": 2.0, "width": 3.0, "height": 4.0}},
        {"name": "deliver-click", "target_bounds": null}
      ],
      "verification": {"status": "passed", "method": "main_title_ocr_full_window_v1"},
      "reacquire": {
        "outcome": "reacquired",
        "strategy_used": "label_current_viewport",
        "observation_count": 2,
        "skipped_rescan_replay": true
      },
      "run_id": run_id_str
    });

    let root_span_name = reacquire_root_span_name("playlist_sidebar");
    let mut reacquire_attrs = BTreeMap::new();
    reacquire_attrs.insert(
      ATTR_REACQUIRE_SCOPE_ID.into(),
      serde_json::Value::String("playlist_sidebar".into()),
    );
    reacquire_attrs.insert(
      ATTR_REACQUIRE_TARGET_KIND.into(),
      serde_json::Value::String("label".into()),
    );
    reacquire_attrs.insert(
      ATTR_REACQUIRE_OUTCOME.into(),
      serde_json::Value::String("reacquired".into()),
    );
    reacquire_attrs.insert(
      ATTR_REACQUIRE_STAGE_USED.into(),
      serde_json::Value::String("label_current_viewport".into()),
    );
    reacquire_attrs.insert(
      ATTR_REACQUIRE_OBSERVATION_COUNT.into(),
      serde_json::json!(2),
    );
    reacquire_attrs.insert(
      ATTR_REACQUIRE_SKIPPED_RESCAN_REPLAY.into(),
      serde_json::Value::Bool(true),
    );

    let artifacts = vec![
      stage_json_artifact(
        store,
        root,
        &run_id,
        &span_id,
        0,
        VIEW_MEMORY_ARTIFACT_ROLE,
        "view-memory.json",
        &memory,
      ),
      stage_json_artifact(
        store,
        root,
        &run_id,
        &span_id,
        1,
        PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE,
        "netease-playlist-select-result.json",
        &select_json,
      ),
    ];

    store
      .write_run_snapshot(&CanonicalRun {
        run: RunRecordV1Alpha1 {
          api_version: RUN_API_VERSION.to_string(),
          run_id: run_id.clone(),
          trace_id: TraceId::new("trace_view_parser"),
          run_type: RunType::Command,
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 1,
          finished_at_millis: Some(2),
          root_span_id: span_id.clone(),
          attributes: BTreeMap::new(),
          summary: Some("view parser proof fixture".to_string()),
          failure: None,
        },
        spans: vec![
          SpanRecordV1Alpha1 {
            api_version: SPAN_API_VERSION.to_string(),
            span_id: span_id.clone(),
            parent_span_id: None,
            name: root_span_name,
            state: TraceState::Ended,
            status_code: TraceStatusCode::Ok,
            started_at_millis: 1,
            finished_at_millis: Some(2),
            attributes: reacquire_attrs,
            summary: None,
            failure: None,
          },
          SpanRecordV1Alpha1 {
            api_version: SPAN_API_VERSION.to_string(),
            span_id: SpanId::new("span_memory_load"),
            parent_span_id: Some(span_id.clone()),
            name: SPAN_REACQUIRE_MEMORY_LOAD.to_string(),
            state: TraceState::Ended,
            status_code: TraceStatusCode::Ok,
            started_at_millis: 1,
            finished_at_millis: Some(2),
            attributes: BTreeMap::from([
              (
                ATTR_MEMORY_LOAD_MEMORY_ID.into(),
                serde_json::Value::String(memory.memory_id.clone()),
              ),
              (
                ATTR_MEMORY_LOAD_SOURCE_RUN_ID.into(),
                serde_json::Value::String(run_id_str.clone()),
              ),
            ]),
            summary: None,
            failure: None,
          },
        ],
        events: Vec::new(),
        artifacts,
      })
      .expect("fixture run should persist");

    run_id_str
  }

  #[test]
  fn select_run_fixture_answers_six_owner_inspect_questions() {
    let root = std::env::temp_dir().join(format!(
      "auv-view-parser-read-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = write_select_run_fixture(&store, &root);

    let memories = list_view_memory_writes(&store, &run_id).expect("memory writes should list");
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].scope_id, "playlist_sidebar");

    let canonical = store.read_run(&run_id).expect("run should read");
    let wires = extract_playlist_select_result_wires(&store, &canonical).expect("select wires");
    assert_eq!(wires.len(), 1);
    assert_eq!(wires[0].query, "Test");

    let inspect = build_view_parser_inspect(&store, &canonical).expect("inspect should build");
    assert_eq!(inspect.resolution_summaries.len(), 1);
    let summary = &inspect.resolution_summaries[0];

    // I — identity
    assert_eq!(summary.identity.label, "Test Playlist");
    assert_eq!(summary.identity.section_kind, "my_playlists");
    assert_eq!(summary.identity.anchor_id.as_deref(), Some("anchor.test"));

    // II — memory
    assert!(summary.memory.present);
    assert_eq!(
      summary.memory.memory_id.as_deref(),
      Some(memories[0].memory_id.as_str())
    );
    assert_eq!(
      summary.memory.source_run_id.as_deref(),
      Some(run_id.as_str())
    );
    assert_eq!(summary.memory.anchor_count, Some(0));

    // III — resolution
    assert_eq!(summary.resolution.outcome, "reacquired");
    assert_eq!(
      summary.resolution.strategy_used.as_deref(),
      Some("label_current_viewport")
    );
    assert_eq!(summary.resolution.observation_count, 2);
    assert_eq!(
      summary.resolution.span_scope_id.as_deref(),
      Some("playlist_sidebar")
    );

    // III — replay
    assert_eq!(
      summary.replay.step_names,
      vec!["reacquire-target".to_string(), "deliver-click".to_string()]
    );
    assert!(summary.replay.skipped_rescan_replay);

    // verification (A5 separate from tiers I–III)
    assert_eq!(summary.verification.status, "passed");
    assert_eq!(summary.verification.method, "main_title_ocr_full_window_v1");

    // IV — geometry
    assert!(summary.geometry_note.has_ephemeral_target_bounds);
    assert!(summary.geometry_note.note.contains("tier IV"));

    let _ = fs::remove_dir_all(root);
  }

  /// JSON shape duplicated from `recording.rs` `sample_select_result_json` (producer contract).
  fn recording_producer_select_json(run_id: &str) -> serde_json::Value {
    serde_json::json!({
      "command": "playlist.select",
      "query": "Test",
      "app": {},
      "window": {},
      "target": {
        "label": "Test Playlist",
        "section_id": "section.created",
        "section_kind": "my_playlists",
        "item_id": "item.test",
        "anchor_id": null,
        "candidate_id": "item.test",
        "observation_index": 0,
        "bounds": {"x": 32.0, "y": 74.0, "width": 120.0, "height": 20.0}
      },
      "steps": [{
        "name": "reacquire-target",
        "target_bounds": null,
        "delivery_path": null,
        "fallback_reason": null
      }],
      "verification": {
        "status": "passed",
        "method": "main_title_ocr_full_window_v1",
        "observed_title": "Test Playlist",
        "artifact": null,
        "note": null
      },
      "diagnostics": [],
      "known_limits": [],
      "reacquire": {
        "outcome": "reacquired",
        "strategy_used": "label_current_viewport",
        "observation_count": 1,
        "skipped_rescan_replay": true
      },
      "run_id": run_id
    })
  }

  fn write_producer_select_run_fixture(store: &LocalStore, root: &Path) -> String {
    let run_id = RunId::new("run_producer_select_proof");
    let span_id = SpanId::new("span_producer_select");
    let run_id_str = run_id.as_str().to_string();
    let memory = sample_memory(&run_id_str);
    let select_json = recording_producer_select_json(&run_id_str);

    let root_span_name = reacquire_root_span_name("playlist_sidebar");
    let mut reacquire_attrs = BTreeMap::new();
    reacquire_attrs.insert(
      ATTR_REACQUIRE_SCOPE_ID.into(),
      serde_json::Value::String("playlist_sidebar".into()),
    );
    reacquire_attrs.insert(
      ATTR_REACQUIRE_TARGET_KIND.into(),
      serde_json::Value::String("label".into()),
    );
    reacquire_attrs.insert(
      ATTR_REACQUIRE_OUTCOME.into(),
      serde_json::Value::String("reacquired".into()),
    );
    reacquire_attrs.insert(
      ATTR_REACQUIRE_STAGE_USED.into(),
      serde_json::Value::String("label_current_viewport".into()),
    );
    reacquire_attrs.insert(
      ATTR_REACQUIRE_OBSERVATION_COUNT.into(),
      serde_json::json!(1),
    );
    reacquire_attrs.insert(
      ATTR_REACQUIRE_SKIPPED_RESCAN_REPLAY.into(),
      serde_json::Value::Bool(true),
    );

    let artifacts = vec![
      stage_json_artifact(
        store,
        root,
        &run_id,
        &span_id,
        0,
        VIEW_MEMORY_ARTIFACT_ROLE,
        "view-memory.json",
        &memory,
      ),
      stage_json_artifact(
        store,
        root,
        &run_id,
        &span_id,
        1,
        PLAYLIST_SELECT_RESULT_ARTIFACT_ROLE,
        "netease-playlist-select-result.json",
        &select_json,
      ),
    ];

    store
      .write_run_snapshot(&CanonicalRun {
        run: RunRecordV1Alpha1 {
          api_version: RUN_API_VERSION.to_string(),
          run_id: run_id.clone(),
          trace_id: TraceId::new("trace_producer_select"),
          run_type: RunType::Command,
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 1,
          finished_at_millis: Some(2),
          root_span_id: span_id.clone(),
          attributes: BTreeMap::new(),
          summary: Some("producer select proof fixture".to_string()),
          failure: None,
        },
        spans: vec![SpanRecordV1Alpha1 {
          api_version: SPAN_API_VERSION.to_string(),
          span_id: span_id.clone(),
          parent_span_id: None,
          name: root_span_name,
          state: TraceState::Ended,
          status_code: TraceStatusCode::Ok,
          started_at_millis: 1,
          finished_at_millis: Some(2),
          attributes: reacquire_attrs,
          summary: None,
          failure: None,
        }],
        events: Vec::new(),
        artifacts,
      })
      .expect("producer fixture run should persist");

    run_id_str
  }

  #[test]
  fn producer_select_result_json_roundtrips_through_inspect_read() {
    let root = std::env::temp_dir().join(format!(
      "auv-view-parser-producer-{}",
      crate::model::now_millis()
    ));
    let _ = fs::remove_dir_all(&root);
    let store = LocalStore::new(root.clone()).expect("store should initialize");
    let run_id = write_producer_select_run_fixture(&store, &root);
    let canonical = store.read_run(&run_id).expect("run should read");

    let wires = extract_playlist_select_result_wires(&store, &canonical).expect("select wires");
    assert_eq!(wires.len(), 1);
    assert_eq!(wires[0].query, "Test");
    assert_eq!(wires[0].target.label, "Test Playlist");
    assert_eq!(
      wires[0].verification.method,
      "main_title_ocr_full_window_v1"
    );
    assert_eq!(
      wires[0]
        .reacquire
        .as_ref()
        .map(|value| value.outcome.as_str()),
      Some("reacquired")
    );
    assert_eq!(wires[0].run_id.as_deref(), Some(run_id.as_str()));

    let memory_writes = extract_view_memory_writes(&store, &canonical).expect("memory writes");
    let reacquisitions = extract_reacquisition_records(&canonical);
    let summary =
      build_view_resolution_summary(&canonical, &memory_writes, &reacquisitions, &wires[0]);
    assert_eq!(summary.identity.label, "Test Playlist");
    assert_eq!(summary.resolution.outcome, "reacquired");
    assert_eq!(summary.verification.method, "main_title_ocr_full_window_v1");

    let inspect = build_view_parser_inspect(&store, &canonical).expect("inspect");
    assert_eq!(inspect.resolution_summaries.len(), 1);
    assert_eq!(inspect.resolution_summaries[0].query, "Test");
    assert_eq!(
      inspect.resolution_summaries[0].identity.label,
      "Test Playlist"
    );
    assert_eq!(
      inspect.resolution_summaries[0].resolution.outcome,
      "reacquired"
    );
    assert_eq!(
      inspect.resolution_summaries[0].verification.method,
      "main_title_ocr_full_window_v1"
    );

    let _ = fs::remove_dir_all(root);
  }
}
