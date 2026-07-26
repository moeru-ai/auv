use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use auv_cli_invoke::{InvokeCancellation, InvokeCommandInput, InvokeNamespace, default_registry, render_command_help};
use auv_tracing::{Context, MemoryTracingStore, RunId, TraceRecord, configure, dispatcher};

fn fixture(kind: &str, name: &str) -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("auv-scan").join("tests").join("testdata").join("scan").join(kind).join(name)
}

#[test]
fn scan_commands_expose_public_metadata_and_help() {
  let registry = default_registry();
  let frame = registry.resolve("scan.frame").expect("scan.frame");
  let coverage = registry.resolve("scan.coverage").expect("scan.coverage");

  assert_eq!(frame.namespace, InvokeNamespace::Scan);
  assert_eq!(coverage.namespace, InvokeNamespace::Scan);
  let help = render_command_help(coverage);
  assert!(help.contains("coverage scenario manifest"));
  assert!(help.contains("frame_fixture cross-reference"));
}

#[test]
fn scan_commands_return_direct_values_and_record_owned_artifacts() {
  futures_executor::block_on(async {
    let registry = default_registry();
    for (command_id, fixture_dir, expected_purposes) in [
      ("scan.frame", fixture("temporal", "single_frame_v0"), vec!["auv.scan.frame", "auv.scan.frame_image"]),
      ("scan.coverage", fixture("coverage", "coverage_stable_v0"), vec!["auv.runtime.scan_coverage"]),
    ] {
      let store = Arc::new(MemoryTracingStore::new());
      let dispatch = configure().tracing_store(store.clone()).build().expect("memory tracing dispatch");
      let root = dispatcher::with_default(&dispatch, || Context::root(RunId::new()));
      let command = registry.resolve(command_id).expect("registered scan command").clone();
      let future = root.in_scope(|| {
        command.invoke(InvokeCommandInput {
          command_id: command_id.to_string(),
          target_application_id: None,
          inputs: BTreeMap::from([("fixture-dir".to_string(), fixture_dir.to_string_lossy().into_owned())]),
          dry_run: false,
          cancellation: InvokeCancellation::new(),
        })
      });
      let output = root.instrument(future).await.expect("scan command");
      dispatch.flush().await.expect("flush tracing");

      assert!(output.report.is_some());
      let mut purposes = store
        .records()
        .into_iter()
        .filter_map(|record| match record {
          TraceRecord::Artifact { metadata, .. } => Some(metadata.purpose().as_str().to_string()),
          _ => None,
        })
        .collect::<Vec<_>>();
      purposes.sort();
      let mut expected_purposes = expected_purposes;
      expected_purposes.sort();
      assert_eq!(purposes, expected_purposes);
    }
  });
}
