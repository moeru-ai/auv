// File: src/driver/macos/control/recognition_read.rs
//! Typed read consumer for recognition evidence.
//!
//! Consumer half of the game-recognition recipe seam
//! (`docs/ai/references/2026-06-10-game-recognition-recipe-consumer-seam.md`):
//! a producer step exports a string handle pointing at its
//! `RecognitionResult` artifact; this command reloads that artifact from
//! run/artifact lineage, asserts exactly one `current/max` numeric reading in
//! the best recognized item, and refuses on missing or ambiguous evidence.
//!
//! NOTICE: Generic invoke dispatch no longer exposes this recipe-era consumer;
//! the module remains compiled for historical tests until an app-local
//! migration either moves or deletes it.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use serde::Deserialize;

use super::super::support::call::required_non_empty_string;
use super::super::{DriverCall, DriverResponse};
use crate::contract::RecognitionResult;
use crate::model::AuvResult;

#[derive(Debug, Deserialize)]
struct RecognitionReadRef {
  source_run_id: String,
  #[serde(default)]
  source_span_id: String,
  recognition_id: String,
  artifact_role: String,
}

#[derive(Debug, PartialEq, Eq)]
struct RatioReading {
  raw: String,
  current: u64,
  max: u64,
}

pub(crate) fn recognition_read_ratio(call: &DriverCall) -> AuvResult<DriverResponse> {
  let raw_ref = required_non_empty_string(call, "recognition_ref")?;
  let read_ref = parse_recognition_read_ref(&raw_ref)?;

  let store_root = call.working_directory.join(".auv");
  let recognition = resolve_recognition_result(&store_root, &read_ref)?;

  let best = recognition.best.as_ref().ok_or_else(|| {
    format!(
      "recognition {} in run {} has no best recognized item; refusing to read a value from empty evidence",
      read_ref.recognition_id, read_ref.source_run_id
    )
  })?;

  let row_text = best.text.as_deref().ok_or_else(|| {
    format!(
      "best recognized item of recognition {} carries no text; refusing to read a value (run {})",
      read_ref.recognition_id, read_ref.source_run_id
    )
  })?;

  let readings = extract_ratio_readings(row_text);
  let reading = match readings.as_slice() {
    [single] => single,
    [] => {
      return Err(format!(
        "no current/max reading found in recognized row text {row_text:?} (recognition {}, run {})",
        read_ref.recognition_id, read_ref.source_run_id
      ));
    }
    multiple => {
      return Err(format!(
        "ambiguous current/max readings {:?} in recognized row text {row_text:?}; refusing to guess (recognition {}, run {})",
        multiple.iter().map(|r| r.raw.as_str()).collect::<Vec<_>>(),
        read_ref.recognition_id,
        read_ref.source_run_id
      ));
    }
  };

  let signals = BTreeMap::from([
    ("recognition.read.matched".to_string(), "true".to_string()),
    ("recognition.read.value".to_string(), reading.raw.clone()),
    (
      "recognition.read.current".to_string(),
      reading.current.to_string(),
    ),
    ("recognition.read.max".to_string(), reading.max.to_string()),
    (
      "recognition.read.row_text".to_string(),
      row_text.to_string(),
    ),
    (
      "recognition.read.source_run_id".to_string(),
      read_ref.source_run_id.clone(),
    ),
    (
      "recognition.read.recognition_id".to_string(),
      read_ref.recognition_id.clone(),
    ),
  ]);

  Ok(DriverResponse {
    summary: format!(
      "Read ratio {} from recognition evidence {}.",
      reading.raw, read_ref.recognition_id
    ),
    backend: Some("macos.contract.recognition-read-ratio".to_string()),
    signals,
    notes: vec![
      format!("sourceRunId={}", read_ref.source_run_id),
      format!("sourceSpanId={}", read_ref.source_span_id),
      format!("recognitionId={}", read_ref.recognition_id),
      format!("artifactRole={}", read_ref.artifact_role),
      format!("rowText={row_text}"),
    ],
    artifacts: Vec::new(),
  })
}

fn parse_recognition_read_ref(raw: &str) -> AuvResult<RecognitionReadRef> {
  let read_ref: RecognitionReadRef = serde_json::from_str(raw)
    .map_err(|error| format!("invalid --recognition_ref JSON: {error}"))?;
  if read_ref.source_run_id.trim().is_empty() {
    return Err("recognition_ref.source_run_id must not be empty".to_string());
  }
  if read_ref.recognition_id.trim().is_empty() {
    return Err("recognition_ref.recognition_id must not be empty".to_string());
  }
  if read_ref.artifact_role.trim().is_empty() {
    return Err("recognition_ref.artifact_role must not be empty".to_string());
  }
  Ok(read_ref)
}

/// Resolve the referenced `RecognitionResult` by scanning the source run's
/// `artifacts.jsonl` for role-matching records and matching the embedded
/// `recognition_id`. Artifact ids are deliberately not part of the handle:
/// driver-side `ref_at` slot numbering is only valid for the first
/// artifact-producing step of a run, so the handle carries stable identity
/// instead of a positional id.
fn resolve_recognition_result(
  store_root: &Path,
  read_ref: &RecognitionReadRef,
) -> AuvResult<RecognitionResult> {
  let run_dir = store_root.join("runs").join(&read_ref.source_run_id);
  let jsonl_path = run_dir.join("artifacts.jsonl");
  if !jsonl_path.exists() {
    // The run store writes artifacts.jsonl atomically at run finish, so a
    // same-run consumer step only sees the staged artifact files. Fall back
    // to scanning the artifacts directory by recognition identity.
    return resolve_recognition_result_from_artifact_files(&run_dir, read_ref);
  }
  let file = std::fs::File::open(&jsonl_path).map_err(|error| {
    format!(
      "failed to open artifacts.jsonl for run {} at {}: {error}",
      read_ref.source_run_id,
      jsonl_path.display()
    )
  })?;

  let mut matches: Vec<(String, RecognitionResult)> = Vec::new();
  for line in BufReader::new(file).lines() {
    let line = line.map_err(|error| format!("failed to read artifacts.jsonl: {error}"))?;
    if line.trim().is_empty() {
      continue;
    }
    let record: serde_json::Value = serde_json::from_str(&line)
      .map_err(|error| format!("failed to parse artifacts.jsonl entry: {error}"))?;
    if record.get("role").and_then(|v| v.as_str()) != Some(read_ref.artifact_role.as_str()) {
      continue;
    }
    let Some(relative_path) = record.get("path").and_then(|v| v.as_str()) else {
      continue;
    };
    let artifact_path = run_dir.join(relative_path);
    let content = std::fs::read_to_string(&artifact_path).map_err(|error| {
      format!(
        "failed to read recognition artifact {}: {error}",
        artifact_path.display()
      )
    })?;
    let recognition: RecognitionResult = serde_json::from_str(&content).map_err(|error| {
      format!(
        "failed to parse RecognitionResult from {}: {error}",
        artifact_path.display()
      )
    })?;
    if recognition.recognition_id == read_ref.recognition_id {
      matches.push((relative_path.to_string(), recognition));
    }
  }

  match matches.len() {
    1 => Ok(matches.remove(0).1),
    0 => Err(format!(
      "recognition {} not found among role={} artifacts of run {}",
      read_ref.recognition_id, read_ref.artifact_role, read_ref.source_run_id
    )),
    _ => Err(format!(
      "recognition id {} matches {} artifacts in run {} ({}); refusing ambiguous lineage",
      read_ref.recognition_id,
      matches.len(),
      read_ref.source_run_id,
      matches
        .iter()
        .map(|(path, _)| path.as_str())
        .collect::<Vec<_>>()
        .join(", ")
    )),
  }
}

/// In-flight-run fallback: scan `artifacts/*.json` files, keep the ones that
/// parse as `RecognitionResult`, and match by `recognition_id`. Files with
/// other shapes (legacy rows JSON, overlay annotations) fail the typed parse
/// and are skipped.
fn resolve_recognition_result_from_artifact_files(
  run_dir: &Path,
  read_ref: &RecognitionReadRef,
) -> AuvResult<RecognitionResult> {
  let artifacts_dir = run_dir.join("artifacts");
  let entries = std::fs::read_dir(&artifacts_dir).map_err(|error| {
    format!(
      "run {} has neither artifacts.jsonl nor a readable artifacts directory at {}: {error}",
      read_ref.source_run_id,
      artifacts_dir.display()
    )
  })?;

  let mut matches: Vec<(String, RecognitionResult)> = Vec::new();
  for entry in entries {
    let entry = entry.map_err(|error| format!("failed to enumerate artifacts: {error}"))?;
    let path = entry.path();
    if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
      continue;
    }
    let Ok(content) = std::fs::read_to_string(&path) else {
      continue;
    };
    let Ok(recognition) = serde_json::from_str::<RecognitionResult>(&content) else {
      continue;
    };
    if recognition.recognition_id == read_ref.recognition_id {
      matches.push((path.display().to_string(), recognition));
    }
  }

  match matches.len() {
    1 => Ok(matches.remove(0).1),
    0 => Err(format!(
      "recognition {} not found among staged artifact files of run {}",
      read_ref.recognition_id, read_ref.source_run_id
    )),
    _ => Err(format!(
      "recognition id {} matches {} staged artifact files in run {} ({}); refusing ambiguous lineage",
      read_ref.recognition_id,
      matches.len(),
      read_ref.source_run_id,
      matches
        .iter()
        .map(|(path, _)| path.as_str())
        .collect::<Vec<_>>()
        .join(", ")
    )),
  }
}

/// Scan text for standalone `current/max` readings (ASCII digit runs joined
/// by a single slash, with non-digit, non-slash boundaries). Chains like
/// `1/2/3` produce no reading on purpose.
fn extract_ratio_readings(text: &str) -> Vec<RatioReading> {
  let bytes = text.as_bytes();
  let mut readings = Vec::new();
  let mut index = 0;
  while index < bytes.len() {
    if !bytes[index].is_ascii_digit() {
      index += 1;
      continue;
    }
    let current_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
      index += 1;
    }
    if index >= bytes.len() || bytes[index] != b'/' {
      continue;
    }
    let slash = index;
    index += 1;
    let max_start = index;
    while index < bytes.len() && bytes[index].is_ascii_digit() {
      index += 1;
    }
    if max_start == index {
      continue;
    }
    let preceded_ok = current_start == 0
      || (!bytes[current_start - 1].is_ascii_digit() && bytes[current_start - 1] != b'/');
    let followed_ok =
      index >= bytes.len() || (!bytes[index].is_ascii_digit() && bytes[index] != b'/');
    if !preceded_ok || !followed_ok {
      continue;
    }
    let raw = &text[current_start..index];
    let (Ok(current), Ok(max)) = (
      text[current_start..slash].parse::<u64>(),
      text[max_start..index].parse::<u64>(),
    ) else {
      continue;
    };
    readings.push(RatioReading {
      raw: raw.to_string(),
      current,
      max,
    });
  }
  readings
}

#[cfg(test)]
mod tests {
  use super::*;

  fn raws(text: &str) -> Vec<String> {
    extract_ratio_readings(text)
      .into_iter()
      .map(|reading| reading.raw)
      .collect()
  }

  #[test]
  fn extract_ratio_readings_finds_single_hud_reading() {
    assert_eq!(raws("33铁甲战士 | 2 88/88 | 99"), vec!["88/88".to_string()]);
    let readings = extract_ratio_readings("3/3");
    assert_eq!(readings.len(), 1);
    assert_eq!(readings[0].current, 3);
    assert_eq!(readings[0].max, 3);
  }

  #[test]
  fn extract_ratio_readings_reports_every_standalone_reading() {
    assert_eq!(
      raws("12/12 | 15/15"),
      vec!["12/12".to_string(), "15/15".to_string()]
    );
  }

  #[test]
  fn extract_ratio_readings_rejects_chains_and_plain_numbers() {
    assert!(raws("1/2/3").is_empty());
    assert!(raws("99 gold and 2026 dates").is_empty());
    assert!(raws("v2.3.4 (12-18-2022)").is_empty());
  }

  #[test]
  fn parse_recognition_read_ref_requires_identity_fields() {
    let error = parse_recognition_read_ref("{}").expect_err("empty ref must be rejected");
    assert!(error.contains("source_run_id"), "error was: {error}");
    let error = parse_recognition_read_ref(
      r#"{"source_run_id":"run_1","recognition_id":"","artifact_role":"window-region-recognition"}"#,
    )
    .expect_err("empty recognition_id must be rejected");
    assert!(error.contains("recognition_id"), "error was: {error}");
  }

  #[test]
  fn resolve_recognition_result_matches_identity_and_refuses_ambiguity() {
    let temp_root =
      std::env::temp_dir().join(format!("auv-recognition-read-test-{}", std::process::id()));
    let run_dir = temp_root.join("runs").join("run_test");
    let artifacts_dir = run_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create temp run dir");

    let recognition = serde_json::json!({
      "recognition_id": "window_region_demo",
      "source": "visual_row",
      "scope": {"surface": "region"},
      "best": {
        "item_id": "row#1",
        "kind": "row",
        "box": {"x": 0, "y": 0, "width": 1, "height": 1},
        "text": "88/88",
        "provider_score": null,
        "detail": {}
      },
      "filtered": [],
      "all": [],
      "detail": {},
      "evidence": [],
      "known_limits": []
    });
    std::fs::write(
      artifacts_dir.join("artifact_0003_demo-recognition.json"),
      recognition.to_string(),
    )
    .expect("write recognition artifact");
    std::fs::write(
      run_dir.join("artifacts.jsonl"),
      concat!(
        "{\"artifact_id\":\"artifact_0001\",\"role\":\"screenshot\",\"path\":\"artifacts/missing.png\"}\n",
        "{\"artifact_id\":\"artifact_0003\",\"role\":\"window-region-recognition\",\"path\":\"artifacts/artifact_0003_demo-recognition.json\"}\n",
      ),
    )
    .expect("write artifacts.jsonl");

    let read_ref = RecognitionReadRef {
      source_run_id: "run_test".to_string(),
      source_span_id: String::new(),
      recognition_id: "window_region_demo".to_string(),
      artifact_role: "window-region-recognition".to_string(),
    };
    let resolved =
      resolve_recognition_result(&temp_root, &read_ref).expect("recognition should resolve");
    assert_eq!(
      resolved.best.expect("best item").text.as_deref(),
      Some("88/88")
    );

    let missing_ref = RecognitionReadRef {
      recognition_id: "window_region_other".to_string(),
      source_run_id: "run_test".to_string(),
      source_span_id: String::new(),
      artifact_role: "window-region-recognition".to_string(),
    };
    resolve_recognition_result(&temp_root, &missing_ref)
      .expect_err("unknown recognition id must refuse");

    std::fs::remove_dir_all(&temp_root).ok();
  }

  #[test]
  fn resolve_recognition_result_falls_back_to_staged_files_for_in_flight_runs() {
    let temp_root = std::env::temp_dir().join(format!(
      "auv-recognition-read-inflight-test-{}",
      std::process::id()
    ));
    let run_dir = temp_root.join("runs").join("run_live");
    let artifacts_dir = run_dir.join("artifacts");
    std::fs::create_dir_all(&artifacts_dir).expect("create temp run dir");
    // No artifacts.jsonl on purpose: the store only writes it at run finish.

    let recognition = serde_json::json!({
      "recognition_id": "window_region_live",
      "source": "visual_row",
      "scope": {"surface": "region"},
      "best": {
        "item_id": "row#1",
        "kind": "row",
        "box": {"x": 0, "y": 0, "width": 1, "height": 1},
        "text": "3/3",
        "provider_score": null,
        "detail": {}
      },
      "filtered": [],
      "all": [],
      "detail": {},
      "evidence": [],
      "known_limits": []
    });
    std::fs::write(
      artifacts_dir.join("artifact_0003_live-recognition.json"),
      recognition.to_string(),
    )
    .expect("write recognition artifact");
    std::fs::write(
      artifacts_dir.join("artifact_0002_live-rows.json"),
      r#"{"rows": []}"#,
    )
    .expect("write non-recognition artifact");

    let read_ref = RecognitionReadRef {
      source_run_id: "run_live".to_string(),
      source_span_id: String::new(),
      recognition_id: "window_region_live".to_string(),
      artifact_role: "window-region-recognition".to_string(),
    };
    let resolved = resolve_recognition_result(&temp_root, &read_ref)
      .expect("in-flight fallback should resolve by identity");
    assert_eq!(
      resolved.best.expect("best item").text.as_deref(),
      Some("3/3")
    );

    std::fs::remove_dir_all(&temp_root).ok();
  }
}
