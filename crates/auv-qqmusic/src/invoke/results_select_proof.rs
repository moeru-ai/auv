use std::fs;
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

use auv_cli_invoke::{
  ArgSpec, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult, arg::FIXTURE_DIR,
};
use serde::{Deserialize, Serialize};

use crate::recording::{QQ_MUSIC_SEARCH_SELECT_RESULT_ROLE, persist_search_select_proof};
use crate::search::SearchAnchorMatch;

pub const RESULTS_SELECT_PROOF_COMMAND_ID: &str = "qqmusic.search.resultsSelectProof";
pub const SELECT_RESULT_FILE: &str = "select-result.json";
const EXPECTED_COMMAND: &str = "search.results.select";

pub const RESULTS_SELECT_PROOF_ARGS: &[ArgSpec] = &[
  FIXTURE_DIR,
  ArgSpec {
    flag: "--store-root",
    value_name: "PATH",
    required: true,
    help: "Local store root where the select-proof run is persisted.",
  },
];

/// Wire document aligned with [`crate::search::SearchCommandReport`] for hermetic fixtures.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchSelectProofDocument {
  pub command: String,
  pub steps: Vec<SearchSelectProofStep>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub anchor: Option<SearchAnchorMatch>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub unsupported: Option<String>,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub known_limits: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub run_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchSelectProofStep {
  pub step_id: String,
  pub summary: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub input_action_result: Option<auv_driver::InputActionResult>,
}

pub fn build_select_document_from_fixture_dir(
  fixture_dir: &Path,
) -> Result<SearchSelectProofDocument, String> {
  if !fixture_dir.is_dir() {
    return Err(format!(
      "fixture directory does not exist: {}",
      fixture_dir.display()
    ));
  }

  let fixture_path = fixture_dir.join(SELECT_RESULT_FILE);
  if !fixture_path.is_file() {
    return Err(format!(
      "fixture file missing at {}",
      fixture_path.display()
    ));
  }

  let bytes = fs::read(&fixture_path)
    .map_err(|error| format!("failed to read {}: {error}", fixture_path.display()))?;
  let mut document: SearchSelectProofDocument =
    serde_json::from_slice(&bytes).map_err(|error| {
      format!(
        "failed to parse {} as search select proof document: {error}",
        fixture_path.display()
      )
    })?;

  if document.command != EXPECTED_COMMAND {
    return Err(format!(
      "fixture command must be {EXPECTED_COMMAND}; got {}",
      document.command
    ));
  }

  if let Some(query) = read_optional_query_fixture(fixture_dir)? {
    document.known_limits.push(format!("fixture_query:{query}"));
  }

  Ok(document)
}

fn read_optional_query_fixture(fixture_dir: &Path) -> Result<Option<String>, String> {
  let query_path = fixture_dir.join("query.txt");
  if !query_path.is_file() {
    return Ok(None);
  }
  let query = fs::read_to_string(&query_path)
    .map_err(|error| format!("failed to read {}: {error}", query_path.display()))?
    .trim()
    .to_string();
  if query.is_empty() {
    return Err(format!("{} must not be empty", query_path.display()));
  }
  Ok(Some(query))
}

pub fn results_select_proof_handler(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  let fixture_dir = required_input(&input, "fixture-dir")?;
  let store_root = required_input(&input, "store-root")?;
  let fixture_path = Path::new(fixture_dir);

  let preview = build_select_document_from_fixture_dir(fixture_path)?;

  if input.dry_run {
    let mut output = InvokeCommandOutput::new(format!(
      "validated hermetic search select proof fixture at {}",
      fixture_dir
    ));
    output.verification = Some("dry-run; no store proof written".to_string());
    output
      .known_limits
      .push("hermetic_fixture_only".to_string());
    output
      .signals
      .insert("fixture_dir".to_string(), fixture_dir.to_string());
    output
      .signals
      .insert("command".to_string(), preview.command.clone());
    if let Some(anchor) = &preview.anchor {
      output
        .signals
        .insert("anchor_text".to_string(), anchor.text.clone());
    }
    return Ok(output);
  }

  let run_id = persist_search_select_proof(Path::new(store_root), |persisted_run_id| {
    let mut document = preview.clone();
    document.run_id = Some(persisted_run_id.to_string());
    serde_json::to_vec_pretty(&document)
      .map_err(|error| format!("failed to serialize search select proof document: {error}"))
  })?;

  let mut output = InvokeCommandOutput::new(format!(
    "persisted hermetic search select proof run {run_id} under {}",
    store_root
  ));
  output.verification =
    Some("hermetic fixture proof only; no live search or semantic success claim".to_string());
  output
    .known_limits
    .push("hermetic_fixture_only".to_string());
  output.signals.insert("run_id".to_string(), run_id.clone());
  output
    .signals
    .insert("store_root".to_string(), store_root.to_string());
  output.signals.insert(
    "artifact_role".to_string(),
    QQ_MUSIC_SEARCH_SELECT_RESULT_ROLE.to_string(),
  );
  Ok(output)
}

fn required_input<'a>(input: &InvokeCommandInput<'a>, key: &str) -> Result<&'a str, String> {
  input
    .inputs
    .get(key)
    .map(String::as_str)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| format!("{RESULTS_SELECT_PROOF_COMMAND_ID} requires --{key}"))
}

#[cfg(test)]
pub fn hermetic_results_select_proof_fixture_dir() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/select-proof/hermetic_v0")
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::fs;

  use auv_cli_invoke::default_registry;
  use auv_tracing_driver::LocalStore;

  use super::*;
  use crate::invoke::qqmusic_registry;

  #[test]
  fn build_select_document_from_fixture_dir_reads_hermetic_fixture() {
    let fixture_dir = hermetic_results_select_proof_fixture_dir();
    let document =
      build_select_document_from_fixture_dir(&fixture_dir).expect("fixture should parse");
    assert_eq!(document.command, EXPECTED_COMMAND);
    assert!(document.anchor.is_some());
    assert!(
      document
        .known_limits
        .iter()
        .any(|limit| limit.contains("hermetic") || limit.contains("fixture_query"))
    );
  }

  #[test]
  fn qqmusic_search_results_select_proof_is_registered_in_qqmusic_registry() {
    let registry = qqmusic_registry();
    let command = registry
      .resolve(RESULTS_SELECT_PROOF_COMMAND_ID)
      .expect("resultsSelectProof should resolve");
    assert_eq!(command.id, RESULTS_SELECT_PROOF_COMMAND_ID);
  }

  #[test]
  fn qqmusic_search_results_select_proof_not_in_default_registry() {
    assert!(
      default_registry()
        .resolve(RESULTS_SELECT_PROOF_COMMAND_ID)
        .is_none()
    );
  }

  #[test]
  fn results_select_proof_fixture_writes_run_and_artifact() {
    let root = std::env::temp_dir().join(format!("auv-acp-b-select-proof-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = fs::remove_dir_all(&root);

    let mut inputs = BTreeMap::new();
    inputs.insert(
      "fixture-dir".to_string(),
      hermetic_results_select_proof_fixture_dir()
        .display()
        .to_string(),
    );
    inputs.insert("store-root".to_string(), store_root.display().to_string());

    let registry = qqmusic_registry();
    let command = registry
      .resolve(RESULTS_SELECT_PROOF_COMMAND_ID)
      .expect("command");
    let output = command
      .invoke(InvokeCommandInput {
        command_id: command.id,
        target_application_id: None,
        inputs: &inputs,
        dry_run: false,
      })
      .expect("handler");

    let run_id = output.signals.get("run_id").expect("run_id signal");
    let store = LocalStore::new(store_root.clone()).expect("store");
    let run = store.read_run(run_id).expect("run");
    assert!(
      run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == QQ_MUSIC_SEARCH_SELECT_RESULT_ROLE)
    );

    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn results_select_proof_run_uses_qqmusic_runspec() {
    let root = std::env::temp_dir().join(format!("auv-acp-b-runspec-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = fs::remove_dir_all(&root);

    let mut inputs = BTreeMap::new();
    inputs.insert(
      "fixture-dir".to_string(),
      hermetic_results_select_proof_fixture_dir()
        .display()
        .to_string(),
    );
    inputs.insert("store-root".to_string(), store_root.display().to_string());

    let registry = qqmusic_registry();
    let command = registry
      .resolve(RESULTS_SELECT_PROOF_COMMAND_ID)
      .expect("command");
    command
      .invoke(InvokeCommandInput {
        command_id: command.id,
        target_application_id: None,
        inputs: &inputs,
        dry_run: false,
      })
      .expect("handler");

    let store = LocalStore::new(store_root.clone()).expect("store");
    let runs = store.list_runs().expect("runs");
    assert_eq!(runs.len(), 1);
    let run = store.read_run(runs[0].run_id.as_str()).expect("run");
    let root_span = run
      .spans
      .iter()
      .find(|span| span.parent_span_id.is_none())
      .expect("root span");
    assert_eq!(root_span.name, "auv.qqmusic.search.select");

    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn results_select_proof_requires_store_root() {
    let mut inputs = BTreeMap::new();
    inputs.insert(
      "fixture-dir".to_string(),
      hermetic_results_select_proof_fixture_dir()
        .display()
        .to_string(),
    );

    let registry = qqmusic_registry();
    let command = registry
      .resolve(RESULTS_SELECT_PROOF_COMMAND_ID)
      .expect("command");
    let error = command
      .invoke(InvokeCommandInput {
        command_id: command.id,
        target_application_id: None,
        inputs: &inputs,
        dry_run: false,
      })
      .expect_err("missing store-root should fail");

    assert!(error.contains("store-root"));
  }

  #[test]
  fn search_select_proof_document_round_trips_anchor_point() {
    use auv_driver::Point;

    let document = SearchSelectProofDocument {
      command: EXPECTED_COMMAND.to_string(),
      steps: vec![SearchSelectProofStep {
        step_id: "select-result".to_string(),
        summary: "fixture".to_string(),
        input_action_result: None,
      }],
      anchor: Some(SearchAnchorMatch {
        text: "Cure For Me".to_string(),
        confidence: 0.9,
        point: Point { x: 10.0, y: 20.0 },
      }),
      unsupported: None,
      known_limits: vec!["hermetic_fixture_only".to_string()],
      run_id: None,
    };
    let json = serde_json::to_vec(&document).expect("serialize");
    let decoded: SearchSelectProofDocument = serde_json::from_slice(&json).expect("deserialize");
    assert_eq!(decoded, document);
  }
}
