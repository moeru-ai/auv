use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use super::{ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, coverage_invoke_command, frame_invoke_command, scan_coverage_artifact};
use auv_tracing::{Context, MemoryTracingStore, RunId, configure, dispatcher};

fn scan_fixture(relative: &str) -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../auv-scan/tests/testdata/scan").join(relative)
}

#[test]
fn scan_coverage_artifact_enforces_four_mibibyte_bound() {
  let oversized = auv_scan::CoverageView::incomplete(
    Vec::new(),
    "oversized fixture",
    vec!["x".repeat(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT as usize)],
    Vec::new(),
  );
  let size_error = scan_coverage_artifact(&oversized).err().expect("oversized coverage must fail");
  assert!(size_error.contains("4194304-byte limit"));
}

#[test]
fn scan_frame_requires_fixture_dir() {
  let err = futures_executor::block_on(frame_invoke_command().invoke(crate::InvokeCommandInput {
    command_id: "scan.frame".to_string(),
    target: None,
    inputs: BTreeMap::new(),
    typed_args: None,
    dry_run: false,
    cancellation: crate::InvokeCancellation::new(),
  }))
  .expect_err("missing fixture-dir should fail");

  assert!(err.message.contains("fixture-dir"));
}

#[test]
fn scan_coverage_requires_fixture_dir() {
  let err = futures_executor::block_on(coverage_invoke_command().invoke(crate::InvokeCommandInput {
    command_id: "scan.coverage".to_string(),
    target: None,
    inputs: BTreeMap::new(),
    typed_args: None,
    dry_run: false,
    cancellation: crate::InvokeCancellation::new(),
  }))
  .expect_err("missing fixture-dir should fail");

  assert!(err.message.contains("fixture-dir"));
}

#[test]
fn scan_frame_dry_run_produces_no_artifacts() {
  let output = futures_executor::block_on(frame_invoke_command().invoke(crate::InvokeCommandInput {
    command_id: "scan.frame".to_string(),
    target: None,
    inputs: BTreeMap::from([("fixture-dir".to_string(), "unused".to_string())]),
    typed_args: None,
    dry_run: true,
    cancellation: crate::InvokeCancellation::new(),
  }))
  .expect("dry-run should succeed");

  assert!(output.report.is_none());
}

#[test]
fn scan_coverage_dry_run_produces_no_artifacts() {
  let output = futures_executor::block_on(coverage_invoke_command().invoke(crate::InvokeCommandInput {
    command_id: "scan.coverage".to_string(),
    target: None,
    inputs: BTreeMap::from([("fixture-dir".to_string(), "unused".to_string())]),
    typed_args: None,
    dry_run: true,
    cancellation: crate::InvokeCancellation::new(),
  }))
  .expect("dry-run should succeed");

  assert!(output.report.is_none());
}

#[tokio::test]
async fn scan_frame_returns_both_primary_artifact_receipts() {
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let input = crate::InvokeCommandInput {
    command_id: "scan.frame".to_string(),
    target: None,
    inputs: BTreeMap::from([("fixture-dir".to_string(), scan_fixture("temporal/single_frame_v0").display().to_string())]),
    typed_args: None,
    dry_run: false,
    cancellation: crate::InvokeCancellation::new(),
  };

  let output = root.instrument(root.in_scope(|| frame_invoke_command().invoke(input))).await.expect("scan frame");

  assert_eq!(output.artifacts().len(), 2);
  assert_eq!(output.artifacts()[0].purpose().as_str(), "auv.scan.frame_image");
  assert_eq!(output.artifacts()[1].purpose().as_str(), "auv.scan.frame");
}

#[tokio::test]
async fn scan_coverage_returns_its_primary_artifact_receipt() {
  let store = Arc::new(MemoryTracingStore::new());
  let dispatch = configure().tracing_store(store).build().expect("dispatch");
  let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
  let input = crate::InvokeCommandInput {
    command_id: "scan.coverage".to_string(),
    target: None,
    inputs: BTreeMap::from([("fixture-dir".to_string(), scan_fixture("coverage/coverage_stable_v0").display().to_string())]),
    typed_args: None,
    dry_run: false,
    cancellation: crate::InvokeCancellation::new(),
  };

  let output = root.instrument(root.in_scope(|| coverage_invoke_command().invoke(input))).await.expect("scan coverage");

  assert_eq!(output.artifacts().len(), 1);
  assert_eq!(output.artifacts()[0].purpose().as_str(), "auv.runtime.scan_coverage");
}
