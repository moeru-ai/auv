use std::fs;
use std::path::{Path, PathBuf};

use auv_cli_invoke::{ArgSpec, InvokeCommandInput, InvokeCommandOutput, InvokeCommandResult};
use auv_driver::vision::TextRecognitionOptions;

use crate::recording::{NETEASE_PLAYLIST_SIDEBAR_SCAN_ROLE, persist_playlist_ls_artifacts};
use crate::{
  DEFAULT_APP_ID, Inputs, PlaylistCategory, PlaylistSidebarScan, decode_playlist_sidebar_scan_json,
};

pub const SIDEBAR_SCAN_PROOF_COMMAND_ID: &str = "netease.playlist.sidebarScanProof";
pub const SCAN_FIXTURE_FILE: &str = "playlist-sidebar-scan.json";

pub const SIDEBAR_SCAN_PROOF_ARGS: &[ArgSpec] = &[
  ArgSpec {
    flag: "--fixture-dir",
    value_name: "PATH",
    required: true,
    help: "Directory containing playlist-sidebar-scan.json (hermetic scan proof fixture).",
  },
  ArgSpec {
    flag: "--store-root",
    value_name: "PATH",
    required: true,
    help: "Local store root where the sidebar-scan proof run is persisted.",
  },
];

pub fn build_scan_from_fixture_dir(fixture_dir: &Path) -> Result<PlaylistSidebarScan, String> {
  if !fixture_dir.is_dir() {
    return Err(format!(
      "fixture directory does not exist: {}",
      fixture_dir.display()
    ));
  }

  let fixture_path = fixture_dir.join(SCAN_FIXTURE_FILE);
  if !fixture_path.is_file() {
    return Err(format!(
      "fixture file missing at {}",
      fixture_path.display()
    ));
  }

  let bytes = fs::read(&fixture_path)
    .map_err(|error| format!("failed to read {}: {error}", fixture_path.display()))?;
  let json = std::str::from_utf8(&bytes).map_err(|error| {
    format!(
      "fixture {} is not valid UTF-8: {error}",
      fixture_path.display()
    )
  })?;
  decode_playlist_sidebar_scan_json(json)
}

/// Minimal [`Inputs`] for `persist_playlist_ls_artifacts(..., memory_enabled=false)` only.
pub fn persist_inputs_for_sidebar_scan_proof(
  store_root: &Path,
  scan: &PlaylistSidebarScan,
) -> Inputs {
  let app_id = scan
    .app()
    .app_id
    .clone()
    .filter(|value| !value.is_empty())
    .unwrap_or_else(|| DEFAULT_APP_ID.to_string());

  // NOTICE(acp-2): persist-only inputs; not product CLI runtime config.
  // When memory_enabled=false, persist_in_recorded_context only reads inputs.app_id.
  Inputs {
    app_id,
    artifact_dir: PathBuf::new(),
    max_scrolls: 0,
    scroll_amount: 0.0,
    scroll_settle_ms: 0,
    sidebar_region: None,
    ocr_options: TextRecognitionOptions::default(),
    category: PlaylistCategory::All,
    store_root: Some(store_root.to_path_buf()),
  }
}

pub fn sidebar_scan_proof_handler(input: InvokeCommandInput<'_>) -> InvokeCommandResult {
  let fixture_dir = required_input(&input, "fixture-dir")?;
  let store_root = required_input(&input, "store-root")?;
  let fixture_path = Path::new(fixture_dir);
  let store_path = Path::new(store_root);

  let scan = build_scan_from_fixture_dir(fixture_path)?;

  if input.dry_run {
    let section_count = scan.projection().sections.len();
    let mut output = InvokeCommandOutput::new(format!(
      "validated hermetic sidebar scan proof fixture at {} ({section_count} projection sections)",
      fixture_dir
    ));
    output.verification = Some("dry-run; no store proof written".to_string());
    output
      .known_limits
      .push("hermetic_fixture_only".to_string());
    output
      .signals
      .insert("fixture_dir".to_string(), fixture_dir.to_string());
    return Ok(output);
  }

  let inputs = persist_inputs_for_sidebar_scan_proof(store_path, &scan);
  let persisted =
    persist_playlist_ls_artifacts(store_path, &scan, &inputs, false).map_err(|error| error)?;
  let run_id = persisted.lineage.run_id;

  let mut output = InvokeCommandOutput::new(format!(
    "persisted hermetic sidebar scan proof run {run_id} under {}",
    store_root
  ));
  output.verification =
    Some("hermetic fixture proof only; no live scan or view-memory write".to_string());
  output
    .known_limits
    .push("hermetic_fixture_only".to_string());
  output.signals.insert("run_id".to_string(), run_id.clone());
  output
    .signals
    .insert("store_root".to_string(), store_root.to_string());
  output.signals.insert(
    "artifact_role".to_string(),
    NETEASE_PLAYLIST_SIDEBAR_SCAN_ROLE.to_string(),
  );
  Ok(output)
}

fn required_input<'a>(input: &InvokeCommandInput<'a>, key: &str) -> Result<&'a str, String> {
  input
    .inputs
    .get(key)
    .map(String::as_str)
    .filter(|value| !value.is_empty())
    .ok_or_else(|| format!("{SIDEBAR_SCAN_PROOF_COMMAND_ID} requires --{key}"))
}

pub fn hermetic_sidebar_scan_proof_fixture_dir() -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sidebar-scan-proof/hermetic_v0")
}

#[cfg(test)]
mod tests {
  use std::collections::BTreeMap;
  use std::fs;

  use auv_cli_invoke::default_registry;
  use auv_tracing_driver::LocalStore;
  use auv_view::memory::VIEW_MEMORY_ARTIFACT_ROLE;

  use super::*;
  use crate::invoke::netease_registry;

  #[test]
  fn build_scan_from_fixture_dir_reads_hermetic_fixture() {
    let fixture_dir = hermetic_sidebar_scan_proof_fixture_dir();
    let scan = build_scan_from_fixture_dir(&fixture_dir).expect("fixture should decode");
    assert_eq!(scan.app().app_id.as_deref(), Some(DEFAULT_APP_ID));
    assert!(
      scan
        .known_limits()
        .iter()
        .any(|limit| limit.contains("hermetic"))
    );
    assert_eq!(
      scan.projection().sections[0].items[0].label,
      "Hermetic Fixture Playlist"
    );
  }

  #[test]
  fn sidebar_scan_proof_registered_in_netease_registry() {
    let registry = netease_registry();
    let command = registry
      .resolve(SIDEBAR_SCAN_PROOF_COMMAND_ID)
      .expect("sidebarScanProof should resolve");
    assert_eq!(command.id, SIDEBAR_SCAN_PROOF_COMMAND_ID);
  }

  #[test]
  fn sidebar_scan_proof_not_in_default_registry() {
    assert!(
      default_registry()
        .resolve(SIDEBAR_SCAN_PROOF_COMMAND_ID)
        .is_none()
    );
  }

  #[test]
  fn sidebar_scan_proof_writes_scan_artifact() {
    let root = std::env::temp_dir().join(format!(
      "auv-acp2-sidebar-scan-proof-{}",
      std::process::id()
    ));
    let store_root = root.join("store");
    let _ = fs::remove_dir_all(&root);

    let mut inputs = BTreeMap::new();
    inputs.insert(
      "fixture-dir".to_string(),
      hermetic_sidebar_scan_proof_fixture_dir()
        .display()
        .to_string(),
    );
    inputs.insert("store-root".to_string(), store_root.display().to_string());

    let registry = netease_registry();
    let command = registry
      .resolve(SIDEBAR_SCAN_PROOF_COMMAND_ID)
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
        .any(|artifact| artifact.role == NETEASE_PLAYLIST_SIDEBAR_SCAN_ROLE)
    );

    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn sidebar_scan_proof_run_uses_ls_runspec() {
    let root = std::env::temp_dir().join(format!("auv-acp2-runspec-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = fs::remove_dir_all(&root);

    let mut inputs = BTreeMap::new();
    inputs.insert(
      "fixture-dir".to_string(),
      hermetic_sidebar_scan_proof_fixture_dir()
        .display()
        .to_string(),
    );
    inputs.insert("store-root".to_string(), store_root.display().to_string());

    let registry = netease_registry();
    let command = registry
      .resolve(SIDEBAR_SCAN_PROOF_COMMAND_ID)
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
    assert_eq!(root_span.name, "auv.netease.playlist.ls");

    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn sidebar_scan_proof_no_view_memory_artifact() {
    let root = std::env::temp_dir().join(format!("auv-acp2-no-memory-{}", std::process::id()));
    let store_root = root.join("store");
    let _ = fs::remove_dir_all(&root);

    let mut inputs = BTreeMap::new();
    inputs.insert(
      "fixture-dir".to_string(),
      hermetic_sidebar_scan_proof_fixture_dir()
        .display()
        .to_string(),
    );
    inputs.insert("store-root".to_string(), store_root.display().to_string());

    let registry = netease_registry();
    let command = registry
      .resolve(SIDEBAR_SCAN_PROOF_COMMAND_ID)
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
    let run = store.read_run(runs[0].run_id.as_str()).expect("run");
    assert!(
      !run
        .artifacts
        .iter()
        .any(|artifact| artifact.role == VIEW_MEMORY_ARTIFACT_ROLE)
    );

    let _ = fs::remove_dir_all(&root);
  }

  #[test]
  fn sidebar_scan_proof_requires_store_root() {
    let mut inputs = BTreeMap::new();
    inputs.insert(
      "fixture-dir".to_string(),
      hermetic_sidebar_scan_proof_fixture_dir()
        .display()
        .to_string(),
    );

    let registry = netease_registry();
    let command = registry
      .resolve(SIDEBAR_SCAN_PROOF_COMMAND_ID)
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
  fn invoke_help_lists_both_proof_commands() {
    let registry = netease_registry();
    let help = crate::invoke::render_help_index(&registry);
    assert!(help.contains(crate::invoke::select_proof::SELECT_PROOF_COMMAND_ID));
    assert!(help.contains(SIDEBAR_SCAN_PROOF_COMMAND_ID));
  }
}
