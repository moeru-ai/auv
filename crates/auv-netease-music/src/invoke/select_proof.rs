use std::fs;
use std::path::{Path, PathBuf};

use auv_cli_invoke::{
  ArgSpec, ArtifactInstrumentationFailure, InvokeCommandFuture, InvokeCommandInput, InvokeCommandOutput, arg::FIXTURE_DIR,
};

use crate::commands::playlist::PlaylistSelectResult;
use crate::recording::{PLAYLIST_SELECT_RESULT_PURPOSE, PlaylistSelectInstrumentation, persist_playlist_select_proof};

#[cfg(feature = "tracing")]
mod tracing {
  use auv_tracing::{Attributes, SpanSpec};

  struct SelectProofSpan;

  impl SpanSpec for SelectProofSpan {
    const NAME: &'static str = "auv.netease.playlist.select_proof";

    fn attributes(&self) -> Attributes {
      Attributes::empty()
    }
  }

  pub(super) fn select_proof<T>(operation: impl FnOnce() -> T) -> T {
    auv_tracing::start_span(SelectProofSpan).in_scope(operation)
  }
}

#[cfg(not(feature = "tracing"))]
mod tracing {
  pub(super) fn select_proof<T>(operation: impl FnOnce() -> T) -> T {
    operation()
  }
}

pub const SELECT_PROOF_COMMAND_ID: &str = "netease.playlist.selectProof";
pub const SELECT_RESULT_FILE: &str = "select-result.json";

pub const SELECT_PROOF_ARGS: &[ArgSpec] = &[FIXTURE_DIR];

pub fn build_select_result_from_fixture_dir(fixture_dir: &Path) -> Result<PlaylistSelectResult, String> {
  tracing::select_proof(|| {
    if !fixture_dir.is_dir() {
      return Err(format!("fixture directory does not exist: {}", fixture_dir.display()));
    }

    let fixture_path = fixture_dir.join(SELECT_RESULT_FILE);
    if !fixture_path.is_file() {
      return Err(format!("fixture file missing at {}", fixture_path.display()));
    }

    let bytes = fs::read(&fixture_path).map_err(|error| format!("failed to read {}: {error}", fixture_path.display()))?;
    let mut result: PlaylistSelectResult = serde_json::from_slice(&bytes)
      .map_err(|error| format!("failed to parse {} as PlaylistSelectResult: {error}", fixture_path.display()))?;

    if let Some(query) = read_optional_query_fixture(fixture_dir)? {
      result.query = query;
    }

    Ok(result)
  })
}

fn read_optional_query_fixture(fixture_dir: &Path) -> Result<Option<String>, String> {
  let query_path = fixture_dir.join("query.txt");
  if !query_path.is_file() {
    return Ok(None);
  }
  let query =
    fs::read_to_string(&query_path).map_err(|error| format!("failed to read {}: {error}", query_path.display()))?.trim().to_string();
  if query.is_empty() {
    return Err(format!("{} must not be empty", query_path.display()));
  }
  Ok(Some(query))
}

pub fn select_proof_handler(input: InvokeCommandInput) -> InvokeCommandFuture {
  Box::pin(select_proof(input))
}

async fn select_proof(input: InvokeCommandInput) -> Result<InvokeCommandOutput, String> {
  let fixture_dir = required_input(&input, "fixture-dir")?.to_string();
  let fixture_path = Path::new(&fixture_dir);
  let preview = build_select_result_from_fixture_dir(fixture_path)?;

  if input.dry_run {
    let mut output = InvokeCommandOutput::new(format!("validated hermetic select proof fixture at {fixture_dir}"));
    output.verification = Some("dry-run; no run artifact written".to_string());
    output.known_limits.push("hermetic_fixture_only".to_string());
    output.signals.insert("fixture_dir".to_string(), fixture_dir);
    output.signals.insert("query".to_string(), preview.query.clone());
    return Ok(output);
  }

  let instrumentation = persist_playlist_select_proof(&preview).await;
  let mut output = match instrumentation {
    PlaylistSelectInstrumentation::Published(metadata) => {
      let run_id = metadata.uri().run_id().to_string();
      let mut output = InvokeCommandOutput::new(format!("persisted hermetic select proof in run {run_id}"));
      output.signals.insert("run_id".to_string(), run_id.clone());
      output.signals.insert("select_result_uri".to_string(), metadata.uri().to_string());
      output
    }
    PlaylistSelectInstrumentation::Disabled => {
      InvokeCommandOutput::new("validated hermetic select proof fixture; run artifact publication was disabled")
    }
    PlaylistSelectInstrumentation::Failed(error) => {
      let mut output = InvokeCommandOutput::new("validated hermetic select proof fixture; run artifact was not published");
      output.artifact_failures.push(ArtifactInstrumentationFailure {
        purpose: PLAYLIST_SELECT_RESULT_PURPOSE.to_string(),
        message: error.to_string(),
      });
      output
    }
  };
  output.signals.insert("query".to_string(), preview.query);
  output.verification = Some("hermetic fixture proof only; no live scan or semantic success claim".to_string());
  output.known_limits.push("hermetic_fixture_only".to_string());
  output.signals.insert("artifact_purpose".to_string(), PLAYLIST_SELECT_RESULT_PURPOSE.to_string());
  Ok(output)
}

fn required_input<'a>(input: &'a InvokeCommandInput, key: &str) -> Result<&'a str, String> {
  input
    .inputs
    .get(key)
    .map(String::as_str)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| format!("{SELECT_PROOF_COMMAND_ID} requires --{key}"))
}

pub fn hermetic_select_proof_fixture_dir() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/select-proof/hermetic_v0")
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;

  use auv_cli_invoke::default_registry;

  use super::*;
  use crate::invoke::netease_registry;

  #[test]
  fn build_select_result_from_fixture_dir_reads_hermetic_fixture() {
    let fixture_dir = hermetic_select_proof_fixture_dir();
    let result = build_select_result_from_fixture_dir(&fixture_dir).expect("fixture should parse");
    assert_eq!(result.command, "playlist.select");
    assert_eq!(result.query, "hermetic-fixture");
    assert!(result.known_limits.iter().any(|limit| limit.contains("hermetic")));
  }

  #[cfg(feature = "tracing")]
  #[test]
  fn select_proof_tracing_propagates_panic_and_closes_span() {
    use std::panic::{AssertUnwindSafe, catch_unwind};
    use std::sync::Arc;

    use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch should build");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));

    let panic = root.in_scope(|| {
      catch_unwind(AssertUnwindSafe(|| {
        tracing::select_proof(|| panic!("select proof panic"));
      }))
    });

    let payload = panic.expect_err("SelectProof panic should propagate");
    assert_eq!(payload.downcast_ref::<&str>(), Some(&"select proof panic"));
    futures_executor::block_on(dispatch.flush()).expect("span writes should flush");
    let snapshot =
      futures_executor::block_on(store.load_snapshot(run_id)).expect("snapshot read should succeed").expect("instrumented run should exist");
    let spans = snapshot.spans().values().collect::<Vec<_>>();
    assert_eq!(spans.len(), 1);
    assert_eq!(spans[0].started().name().as_str(), "auv.netease.playlist.select_proof");
    assert!(spans[0].ended().is_some());
    assert!(snapshot.events().is_empty());
  }

  #[test]
  fn netease_playlist_select_proof_is_registered_in_netease_registry() {
    let registry = netease_registry();
    let command = registry.resolve(SELECT_PROOF_COMMAND_ID).expect("selectProof should resolve");
    assert_eq!(command.id, SELECT_PROOF_COMMAND_ID);
  }

  #[test]
  fn netease_playlist_select_proof_not_in_default_registry() {
    assert!(default_registry().resolve(SELECT_PROOF_COMMAND_ID).is_none());
  }

  #[test]
  fn select_proof_without_context_returns_the_existing_result() {
    let mut inputs = BTreeMap::new();
    inputs.insert("fixture-dir".to_string(), hermetic_select_proof_fixture_dir().display().to_string());
    let registry = netease_registry();
    let command = registry.resolve(SELECT_PROOF_COMMAND_ID).expect("command");

    let output = futures_executor::block_on(command.invoke(InvokeCommandInput {
      command_id: command.id.to_string(),
      target_application_id: None,
      inputs,
      dry_run: false,
      cancellation: auv_cli_invoke::InvokeCancellation::new(),
    }))
    .expect("direct fixture result");

    assert!(output.summary.contains("publication was disabled"));
    assert!(output.artifact_failures.is_empty());
    assert!(!output.signals.contains_key("run_id"));
  }

  #[test]
  fn select_proof_requires_fixture_dir() {
    let registry = netease_registry();
    let command = registry.resolve(SELECT_PROOF_COMMAND_ID).expect("command");
    let error = futures_executor::block_on(command.invoke(InvokeCommandInput {
      command_id: command.id.to_string(),
      target_application_id: None,
      inputs: BTreeMap::new(),
      dry_run: false,
      cancellation: auv_cli_invoke::InvokeCancellation::new(),
    }))
    .expect_err("missing fixture-dir should fail");

    assert!(error.contains("fixture-dir"));
  }
}
