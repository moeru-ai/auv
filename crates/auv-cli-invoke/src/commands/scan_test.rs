use std::collections::BTreeMap;

use super::{ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT, coverage, emit_scan_coverage, frame, scan_coverage_artifact};

#[tokio::test]
async fn scan_coverage_publication_short_circuits_without_run_context() {
  let coverage = auv_scan::CoverageView::incomplete(
    Vec::new(),
    "oversized detached fixture",
    vec!["x".repeat(ROOT_STRUCTURED_ARTIFACT_JSON_BYTE_LIMIT as usize)],
    Vec::new(),
  );

  emit_scan_coverage(&coverage);
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
    inputs: BTreeMap::from([("fixture-dir".to_string(), "unused".to_string())]),
    dry_run: true,
    cancellation: crate::InvokeCancellation::new(),
  }))
  .expect("dry-run should succeed");

  assert!(output.report.is_none());
}

#[test]
fn scan_coverage_dry_run_produces_no_artifacts() {
  let output = futures_executor::block_on(coverage(crate::InvokeCommandInput {
    command_id: "scan.coverage".to_string(),
    target_application_id: None,
    inputs: BTreeMap::from([("fixture-dir".to_string(), "unused".to_string())]),
    dry_run: true,
    cancellation: crate::InvokeCancellation::new(),
  }))
  .expect("dry-run should succeed");

  assert!(output.report.is_none());
}
