use std::path::PathBuf;

use crate::{
  CommandGroup, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult,
  arg::{SCAN_COVERAGE_ARGS, SCAN_FRAME_ARGS},
  artifact::{ArtifactInstrumentationReceipt, ArtifactPublication},
  invoke_command,
};
use auv_scan::{produce_coverage_from_fixture_dir, produce_frame_from_fixture_dir};
use auv_tracing::{ArtifactMetadata, ArtifactPurpose, Attributes, ByteLength, ContentType, Context, NewArtifact, Sha256Digest};
use futures_util::io::Cursor as AsyncCursor;
use sha2::{Digest, Sha256};
use tempfile::TempDir;

const SCAN_COVERAGE_PURPOSE: &str = "auv.runtime.scan_coverage";
const ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

pub fn group() -> CommandGroup {
  CommandGroup::new("scan", "SCAN").command(frame_invoke_command()).command(coverage_invoke_command())
}

#[invoke_command(
  id = "scan.frame",
  group = "scan",
  summary = "Produce a single scan-frame-v0 artifact bundle from a hermetic fixture directory and stage it into the run.",
  args = SCAN_FRAME_ARGS,
)]
async fn frame(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    let mut output = InvokeCommandOutput::new("scan.frame dry-run");
    output.verification = Some("dry-run; no artifacts produced".to_string());
    output.known_limits.push("scan.frame dry-run does not write scan artifacts.".to_string());
    return Ok(output);
  }

  let fixture_dir = input.required_input("fixture-dir")?.to_string();
  let (_, instrumentation) = produce_scan_frame(PathBuf::from(&fixture_dir)).await?.into_parts();

  let mut output = InvokeCommandOutput::new(format!("scan frame produced from fixture {}", fixture_dir));
  output.backend = Some("auv-scan.produce_frame_from_fixture_dir".to_string());
  output.verification = Some("capture-only; no semantic success claim".to_string());
  output.known_limits.push("scan.frame records a single scan-frame-v0 bundle only; multi-frame invoke is deferred.".to_string());
  output.apply_artifact_instrumentation(instrumentation);
  Ok(output)
}

pub async fn produce_scan_frame(fixture_dir: PathBuf) -> Result<ArtifactPublication<auv_scan::ScanFrame>, String> {
  if !fixture_dir.is_dir() {
    return Err(format!("scan.frame fixture directory does not exist: {}", fixture_dir.display()));
  }
  // The producer directory remains alive until both owned artifact readers are admitted.
  let producer_out = TempDir::new().map_err(|error| format!("scan.frame failed to create producer output directory: {error}"))?;
  let produced =
    produce_frame_from_fixture_dir(&fixture_dir, producer_out.path()).map_err(|error| format!("scan.frame producer failed: {error}"))?;
  let mut instrumentation = ArtifactInstrumentationReceipt::default();
  instrumentation.publish_file("auv.scan.frame", "application/json", &produced.json_path).await;
  instrumentation.publish_file("auv.scan.frame_image", "image/png", &produced.image_path).await;
  Ok(ArtifactPublication::new(produced.frame, instrumentation))
}

#[invoke_command(
  id = "scan.coverage",
  group = "scan",
  summary = "Produce a scan-coverage-v0 artifact from a coverage scenario fixture and stage it into the run.",
  args = SCAN_COVERAGE_ARGS,
)]
async fn coverage(input: InvokeCommandInput) -> InvokeCommandResult {
  if input.dry_run {
    let mut output = InvokeCommandOutput::new("scan.coverage dry-run");
    output.verification = Some("dry-run; no artifacts produced".to_string());
    output.known_limits.push("scan.coverage dry-run does not write scan artifacts.".to_string());
    return Ok(output);
  }

  let fixture_dir = input.required_input("fixture-dir")?.to_string();
  let (_, recording) = produce_scan_coverage(PathBuf::from(&fixture_dir)).await?;

  let mut output = InvokeCommandOutput::new(format!("scan coverage produced from fixture {fixture_dir}"));
  output.backend = Some("auv-scan.produce_coverage_from_fixture_dir".to_string());
  output.verification = Some("evaluator + projection; no semantic success claim".to_string());
  output.known_limits.push(
    "scan.coverage resolves frame PNGs via manifest frame_fixture cross-reference under .../scan/coverage/<scenario>/ layout only."
      .to_string(),
  );
  output.artifacts.extend(recording);
  Ok(output)
}

pub async fn produce_scan_coverage(fixture_dir: PathBuf) -> Result<(auv_scan::ScanCoverageWire, Option<ArtifactMetadata>), String> {
  if !fixture_dir.is_dir() {
    return Err(format!("scan.coverage fixture directory does not exist: {}", fixture_dir.display()));
  }
  // The producer directory remains alive until the owned artifact reader is admitted.
  let producer_out = TempDir::new().map_err(|error| format!("scan.coverage failed to create producer output directory: {error}"))?;
  let produced = produce_coverage_from_fixture_dir(&fixture_dir, producer_out.path())
    .map_err(|error| format!("scan.coverage producer failed: {error}"))?;
  let recording = publish_scan_coverage(&produced.wire).await?;
  Ok((produced.wire, recording))
}

async fn publish_scan_coverage(value: &auv_scan::ScanCoverageWire) -> Result<Option<ArtifactMetadata>, String> {
  let context = Context::current();
  if !context.can_publish_artifacts() {
    return Ok(None);
  }
  let artifact = scan_coverage_artifact(value)?;
  context
    .in_scope(|| auv_tracing::emit_artifact!(artifact))
    .await
    .map_err(|error| format!("failed to publish {SCAN_COVERAGE_PURPOSE} artifact: {error}"))
}

fn scan_coverage_artifact(value: &auv_scan::ScanCoverageWire) -> Result<NewArtifact<AsyncCursor<Vec<u8>>>, String> {
  if value.schema_version != auv_scan::SCAN_COVERAGE_SCHEMA_VERSION {
    return Err(format!("{SCAN_COVERAGE_PURPOSE} failed domain validation: schema version mismatch: found {}", value.schema_version));
  }
  let body = serde_json::to_vec(value).map_err(|error| format!("failed to serialize {SCAN_COVERAGE_PURPOSE} artifact: {error}"))?;
  let byte_length = u64::try_from(body.len()).map_err(|_| format!("{SCAN_COVERAGE_PURPOSE} JSON length does not fit u64"))?;
  if byte_length > ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT {
    return Err(format!(
      "{SCAN_COVERAGE_PURPOSE} is {byte_length} bytes, exceeding the {ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT}-byte limit"
    ));
  }
  Ok(NewArtifact::new(
    ArtifactPurpose::parse(SCAN_COVERAGE_PURPOSE).map_err(|error| format!("invalid {SCAN_COVERAGE_PURPOSE} purpose: {error}"))?,
    ContentType::parse("application/json").map_err(|error| format!("invalid {SCAN_COVERAGE_PURPOSE} content type: {error}"))?,
    ByteLength::new(byte_length).map_err(|error| format!("invalid {SCAN_COVERAGE_PURPOSE} byte length: {error}"))?,
    Sha256Digest::new(Sha256::digest(&body).into()),
    Attributes::empty(),
    AsyncCursor::new(body),
  ))
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::env;
  use std::path::PathBuf;
  use std::sync::Arc;
  use std::sync::atomic::{AtomicUsize, Ordering};

  use auv_tracing::{
    ArtifactBody, ArtifactReader, ArtifactUri, ArtifactWriteError, AuthorityId, BoxFuture, CommitError, CommitResult, Context, ErrorCode,
    IdempotencyKey, MemoryRunStore, PageLimit, ReadError, RunCommit, RunCommitPage, RunCommitRequest, RunId, RunRevision, RunStore,
    RunSubscription, StoreArtifactRequest, configure, dispatcher,
  };
  use futures_util::StreamExt;

  use crate::{
    InvokeCommand, InvokeCommandInput, InvokeCommandOutput, InvokeNamespace, arg::SCAN_COVERAGE_ARGS, default_registry, render_command_help,
  };

  use super::{
    ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, coverage, coverage_invoke_command, frame, frame_invoke_command, produce_scan_coverage,
    produce_scan_frame, publish_scan_coverage, scan_coverage_artifact,
  };

  fn single_frame_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-scan/tests/fixtures/scan/temporal/single_frame_v0")
  }

  fn coverage_stable_fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-scan/tests/fixtures/scan/coverage/coverage_stable_v0")
  }

  struct RejectArtifactStore {
    inner: MemoryRunStore,
    writes: AtomicUsize,
  }

  impl RejectArtifactStore {
    fn new() -> Self {
      Self {
        inner: MemoryRunStore::new(AuthorityId::new()),
        writes: AtomicUsize::new(0),
      }
    }
  }

  impl RunStore for RejectArtifactStore {
    fn authority_id(&self) -> AuthorityId {
      self.inner.authority_id()
    }

    fn commit(&self, request: RunCommitRequest) -> BoxFuture<'_, Result<CommitResult, CommitError>> {
      self.inner.commit(request)
    }

    fn write_artifact(
      &self,
      _request: StoreArtifactRequest,
      _body: ArtifactBody,
    ) -> BoxFuture<'_, Result<CommitResult, ArtifactWriteError>> {
      self.writes.fetch_add(1, Ordering::SeqCst);
      Box::pin(async { Err(ArtifactWriteError::Rejected(ErrorCode::parse("auv.test.scan_coverage_rejected").expect("test error code"))) })
    }

    fn lookup_commit(&self, run_id: RunId, key: IdempotencyKey) -> BoxFuture<'_, Result<Option<RunCommit>, ReadError>> {
      self.inner.lookup_commit(run_id, key)
    }

    fn load_snapshot(&self, run_id: RunId) -> BoxFuture<'_, Result<Option<auv_tracing::RunSnapshot>, ReadError>> {
      self.inner.load_snapshot(run_id)
    }

    fn commits_after(&self, run_id: RunId, after: RunRevision, limit: PageLimit) -> BoxFuture<'_, Result<RunCommitPage, ReadError>> {
      self.inner.commits_after(run_id, after, limit)
    }

    fn subscribe(&self, run_id: RunId, after: RunRevision) -> BoxFuture<'_, Result<RunSubscription, ReadError>> {
      self.inner.subscribe(run_id, after)
    }

    fn open_artifact(&self, uri: ArtifactUri) -> BoxFuture<'_, Result<ArtifactReader, ReadError>> {
      self.inner.open_artifact(uri)
    }
  }

  async fn invoke_traced(command: InvokeCommand, input: InvokeCommandInput) -> (InvokeCommandOutput, Arc<MemoryRunStore>, RunId) {
    let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
    let dispatch = configure().run_store(store.clone()).build().expect("dispatch should build");
    let run_id = RunId::new();
    let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
    let future = root.in_scope(|| command.invoke(input));
    let output = root.instrument(future).await.expect("invoke should succeed");
    dispatch.flush().await.expect("tracing should flush");
    (output, store, run_id)
  }

  #[test]
  fn scan_frame_command_uses_scan_namespace() {
    let command = frame_invoke_command();
    assert_eq!(command.id, "scan.frame");
    assert_eq!(command.namespace, InvokeNamespace::Scan);
  }

  #[test]
  fn scan_coverage_command_uses_scan_namespace() {
    let command = coverage_invoke_command();
    assert_eq!(command.id, "scan.coverage");
    assert_eq!(command.namespace, InvokeNamespace::Scan);
  }

  #[test]
  fn scan_frame_is_registered_in_default_registry() {
    let registry = default_registry();
    let command = registry.resolve("scan.frame").expect("scan.frame should be registered");
    assert_eq!(command.namespace, InvokeNamespace::Scan);
  }

  #[test]
  fn scan_coverage_is_registered_in_default_registry() {
    let registry = default_registry();
    let command = registry.resolve("scan.coverage").expect("scan.coverage should be registered");
    assert_eq!(command.namespace, InvokeNamespace::Scan);
  }

  #[test]
  fn scan_coverage_args_use_coverage_fixture_help() {
    assert_eq!(SCAN_COVERAGE_ARGS.len(), 1);
    assert!(SCAN_COVERAGE_ARGS[0].help.contains("coverage scenario manifest"));
    assert!(SCAN_COVERAGE_ARGS[0].help.contains("frame_fixture cross-reference"));
  }

  #[test]
  fn typed_scan_calls_return_domain_values_without_cli_context() {
    let frame = futures_executor::block_on(produce_scan_frame(single_frame_fixture_dir())).expect("typed frame");
    let coverage = futures_executor::block_on(produce_scan_coverage(coverage_stable_fixture_dir())).expect("typed coverage");

    assert_eq!(frame.value().schema_version, auv_scan::SCAN_FRAME_SCHEMA_VERSION);
    assert_eq!(coverage.0.schema_version, auv_scan::SCAN_COVERAGE_SCHEMA_VERSION);
    assert!(coverage.1.is_none());
  }

  #[tokio::test]
  async fn scan_coverage_typed_call_propagates_enabled_publication_failure() {
    let store = Arc::new(RejectArtifactStore::new());
    let dispatch = configure().run_store(store.clone()).build().expect("rejecting dispatch");
    let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
    let future = root.in_scope(|| produce_scan_coverage(coverage_stable_fixture_dir()));

    let error = root.instrument(future).await.expect_err("enabled publication failure must propagate");

    assert!(error.contains("failed to publish"), "{error}");
    assert_eq!(store.writes.load(Ordering::SeqCst), 1);
  }

  #[tokio::test]
  async fn scan_coverage_publication_short_circuits_without_run_context() {
    let (mut coverage, recording) = produce_scan_coverage(coverage_stable_fixture_dir()).await.expect("typed coverage");
    assert!(recording.is_none());
    coverage.schema_version = "scan-coverage-v999".to_string();

    assert!(publish_scan_coverage(&coverage).await.expect("disabled telemetry skips domain validation").is_none());
  }

  #[test]
  fn scan_coverage_artifact_enforces_schema_and_four_mibibyte_bounds() {
    let (mut invalid, _) = futures_executor::block_on(produce_scan_coverage(coverage_stable_fixture_dir())).expect("typed coverage");
    invalid.schema_version = "scan-coverage-v999".to_string();
    let schema_error = scan_coverage_artifact(&invalid).err().expect("wrong schema must fail");
    assert!(schema_error.contains("schema version mismatch"));

    let (mut oversized, _) = futures_executor::block_on(produce_scan_coverage(coverage_stable_fixture_dir())).expect("typed coverage");
    oversized.open_uncertainty_codes = vec!["x".repeat(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT as usize)];
    let size_error = scan_coverage_artifact(&oversized).err().expect("oversized coverage must fail");
    assert!(size_error.contains("4194304-byte limit"));
  }

  #[test]
  fn scan_frame_requires_fixture_dir() {
    let err = futures_executor::block_on(frame(crate::InvokeCommandInput {
      command_id: "scan.frame".to_string(),
      target_application_id: None,
      inputs: BTreeMap::new(),
      dry_run: false,
      cancellation: crate::InvokeCancellation::new(),
    }))
    .expect_err("missing fixture-dir should fail");

    assert!(err.contains("fixture-dir"));
  }

  #[test]
  fn scan_coverage_requires_fixture_dir() {
    let err = futures_executor::block_on(coverage(crate::InvokeCommandInput {
      command_id: "scan.coverage".to_string(),
      target_application_id: None,
      inputs: BTreeMap::new(),
      dry_run: false,
      cancellation: crate::InvokeCancellation::new(),
    }))
    .expect_err("missing fixture-dir should fail");

    assert!(err.contains("fixture-dir"));
  }

  #[test]
  fn scan_frame_dry_run_produces_no_artifacts() {
    let output = futures_executor::block_on(frame(crate::InvokeCommandInput {
      command_id: "scan.frame".to_string(),
      target_application_id: None,
      inputs: BTreeMap::from([("fixture-dir".to_string(), "/tmp/unused".to_string())]),
      dry_run: true,
      cancellation: crate::InvokeCancellation::new(),
    }))
    .expect("dry-run should succeed");

    assert!(output.artifact_failures.is_empty());
    assert!(output.verification.as_deref().is_some_and(|claim| claim.contains("dry-run")));
  }

  #[test]
  fn scan_coverage_dry_run_produces_no_artifacts() {
    let output = futures_executor::block_on(coverage(crate::InvokeCommandInput {
      command_id: "scan.coverage".to_string(),
      target_application_id: None,
      inputs: BTreeMap::from([("fixture-dir".to_string(), "/tmp/unused".to_string())]),
      dry_run: true,
      cancellation: crate::InvokeCancellation::new(),
    }))
    .expect("dry-run should succeed");

    assert!(output.artifact_failures.is_empty());
    assert!(output.verification.as_deref().is_some_and(|claim| claim.contains("dry-run")));
  }

  #[test]
  fn scan_frame_from_fixture_dir_emits_owned_artifacts() {
    let fixture_dir = single_frame_fixture_dir();
    let (output, store, run_id) = futures_executor::block_on(invoke_traced(
      frame_invoke_command(),
      InvokeCommandInput {
        command_id: "scan.frame".to_string(),
        target_application_id: None,
        inputs: BTreeMap::from([("fixture-dir".to_string(), fixture_dir.to_string_lossy().into_owned())]),
        dry_run: false,
        cancellation: crate::InvokeCancellation::new(),
      },
    ));

    assert!(output.artifact_failures.is_empty(), "successful artifact publication records no instrumentation failure");
    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).expect("snapshot read").expect("recorded run");
    let purposes = snapshot.artifacts().values().map(|publication| publication.metadata().purpose().as_str()).collect::<Vec<_>>();
    assert_eq!(purposes.len(), 2);
    assert!(purposes.contains(&"auv.scan.frame"));
    assert!(purposes.contains(&"auv.scan.frame_image"));
  }

  #[test]
  fn scan_coverage_from_fixture_dir_emits_owned_artifact() {
    let fixture_dir = coverage_stable_fixture_dir();
    let (expected, recording) = futures_executor::block_on(produce_scan_coverage(fixture_dir.clone())).expect("direct typed coverage");
    assert!(recording.is_none());
    let (output, store, run_id) = futures_executor::block_on(invoke_traced(
      coverage_invoke_command(),
      InvokeCommandInput {
        command_id: "scan.coverage".to_string(),
        target_application_id: None,
        inputs: BTreeMap::from([("fixture-dir".to_string(), fixture_dir.to_string_lossy().into_owned())]),
        dry_run: false,
        cancellation: crate::InvokeCancellation::new(),
      },
    ));

    assert!(output.artifact_failures.is_empty(), "successful artifact publication records no instrumentation failure");
    let snapshot = futures_executor::block_on(store.load_snapshot(run_id)).expect("snapshot read").expect("recorded run");
    let publication = snapshot.artifacts().values().next().expect("coverage artifact");
    assert_eq!(snapshot.artifacts().len(), 1);
    assert_eq!(publication.metadata().purpose().as_str(), "auv.runtime.scan_coverage");
    assert_eq!(publication.metadata().content_type().to_string(), "application/json");
    let actual = futures_executor::block_on(async {
      let mut reader = store.open_artifact(publication.metadata().uri().clone()).await.expect("open coverage artifact");
      let mut bytes = Vec::new();
      while let Some(chunk) = reader.next().await {
        bytes.extend_from_slice(&chunk.expect("coverage artifact chunk"));
      }
      serde_json::from_slice::<auv_scan::ScanCoverageWire>(&bytes).expect("typed coverage payload")
    });
    assert_eq!(actual, expected);
  }

  #[test]
  fn help_lists_scan_coverage_with_coverage_fixture_help() {
    let command = coverage_invoke_command();
    let help = render_command_help(&command);
    assert!(help.contains("scan.coverage"));
    assert!(help.contains("coverage scenario manifest"));
    assert!(help.contains("frame_fixture cross-reference"));
  }
}
