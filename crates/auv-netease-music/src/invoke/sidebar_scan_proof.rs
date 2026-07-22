use std::fs;
use std::path::{Path, PathBuf};

use auv_cli_invoke::{ArgSpec, ArtifactInstrumentationFailure, InvokeCommandFuture, InvokeCommandInput, InvokeCommandOutput};
use auv_driver::vision::TextRecognitionOptions;

use crate::recording::{PLAYLIST_SIDEBAR_SCAN_PURPOSE, persist_playlist_ls_artifacts};
use crate::{DEFAULT_APP_ID, Inputs, PlaylistCategory, PlaylistSidebarScan, decode_playlist_sidebar_scan_json};

pub const SIDEBAR_SCAN_PROOF_COMMAND_ID: &str = "netease.playlist.sidebarScanProof";
pub const SCAN_FIXTURE_FILE: &str = "playlist-sidebar-scan.json";

pub const SIDEBAR_SCAN_PROOF_ARGS: &[ArgSpec] = &[ArgSpec {
  flag: "--fixture-dir",
  value_name: "PATH",
  required: true,
  help: "Directory containing playlist-sidebar-scan.json (hermetic scan proof fixture).",
}];

pub fn build_scan_from_fixture_dir(fixture_dir: &Path) -> Result<PlaylistSidebarScan, String> {
  if !fixture_dir.is_dir() {
    return Err(format!("fixture directory does not exist: {}", fixture_dir.display()));
  }

  let fixture_path = fixture_dir.join(SCAN_FIXTURE_FILE);
  if !fixture_path.is_file() {
    return Err(format!("fixture file missing at {}", fixture_path.display()));
  }

  let bytes = fs::read(&fixture_path).map_err(|error| format!("failed to read {}: {error}", fixture_path.display()))?;
  let json = std::str::from_utf8(&bytes).map_err(|error| format!("fixture {} is not valid UTF-8: {error}", fixture_path.display()))?;
  decode_playlist_sidebar_scan_json(json)
}

/// Minimal [`Inputs`] used only to derive the persisted app-local lineage.
pub fn persist_inputs_for_sidebar_scan_proof(scan: &PlaylistSidebarScan) -> Inputs {
  let app_id = scan.app().app_id.clone().filter(|value| !value.is_empty()).unwrap_or_else(|| DEFAULT_APP_ID.to_string());

  Inputs {
    app_id,
    artifact_dir: PathBuf::new(),
    max_scrolls: 0,
    scroll_amount: 0.0,
    scroll_settle_ms: 0,
    sidebar_region: None,
    ocr_options: TextRecognitionOptions::default(),
    category: PlaylistCategory::All,
    store_root: None,
  }
}

pub fn sidebar_scan_proof_handler(input: InvokeCommandInput) -> InvokeCommandFuture {
  Box::pin(sidebar_scan_proof(input))
}

async fn sidebar_scan_proof(input: InvokeCommandInput) -> Result<InvokeCommandOutput, String> {
  let fixture_dir = required_input(&input, "fixture-dir")?.to_string();
  let fixture_path = Path::new(&fixture_dir);
  let scan = build_scan_from_fixture_dir(fixture_path)?;

  if input.dry_run {
    let section_count = scan.projection().sections.len();
    let mut output = InvokeCommandOutput::new(format!(
      "validated hermetic sidebar scan proof fixture at {} ({section_count} projection sections)",
      fixture_dir
    ));
    output.verification = Some("dry-run; no run artifact written".to_string());
    output.known_limits.push("hermetic_fixture_only".to_string());
    output.signals.insert("fixture_dir".to_string(), fixture_dir);
    return Ok(output);
  }

  let inputs = persist_inputs_for_sidebar_scan_proof(&scan);
  let mut output = match persist_playlist_ls_artifacts(&scan, &inputs, false).await {
    Ok(persisted) => {
      let run_id = persisted.lineage.scan_uri.run_id().to_string();
      let mut output = InvokeCommandOutput::new(format!("persisted hermetic sidebar scan proof in run {run_id}"));
      output.signals.insert("run_id".to_string(), run_id);
      output.signals.insert("scan_uri".to_string(), persisted.lineage.scan_uri.to_string());
      output
    }
    Err(error) => {
      let mut output = InvokeCommandOutput::new("validated hermetic sidebar scan proof fixture; run artifact was not published");
      output.artifact_failures.push(ArtifactInstrumentationFailure {
        purpose: PLAYLIST_SIDEBAR_SCAN_PURPOSE.to_string(),
        message: error.to_string(),
      });
      output
    }
  };
  output.verification = Some("hermetic fixture proof only; no live scan or view-memory write".to_string());
  output.known_limits.push("hermetic_fixture_only".to_string());
  output.signals.insert("artifact_purpose".to_string(), PLAYLIST_SIDEBAR_SCAN_PURPOSE.to_string());
  Ok(output)
}

fn required_input<'a>(input: &'a InvokeCommandInput, key: &str) -> Result<&'a str, String> {
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

  use auv_cli_invoke::default_registry;

  use super::*;
  use crate::invoke::netease_registry;

  #[test]
  fn build_scan_from_fixture_dir_reads_hermetic_fixture() {
    let fixture_dir = hermetic_sidebar_scan_proof_fixture_dir();
    let scan = build_scan_from_fixture_dir(&fixture_dir).expect("fixture should decode");
    assert_eq!(scan.app().app_id.as_deref(), Some(DEFAULT_APP_ID));
    assert!(scan.known_limits().iter().any(|limit| limit.contains("hermetic")));
    assert_eq!(scan.projection().sections[0].items[0].label, "Hermetic Fixture Playlist");
  }

  #[test]
  fn sidebar_scan_proof_registered_in_netease_registry() {
    let registry = netease_registry();
    let command = registry.resolve(SIDEBAR_SCAN_PROOF_COMMAND_ID).expect("sidebarScanProof should resolve");
    assert_eq!(command.id, SIDEBAR_SCAN_PROOF_COMMAND_ID);
  }

  #[test]
  fn sidebar_scan_proof_not_in_default_registry() {
    assert!(default_registry().resolve(SIDEBAR_SCAN_PROOF_COMMAND_ID).is_none());
  }

  #[test]
  fn sidebar_scan_proof_without_context_preserves_direct_fixture_result() {
    let mut inputs = BTreeMap::new();
    inputs.insert("fixture-dir".to_string(), hermetic_sidebar_scan_proof_fixture_dir().display().to_string());
    let command = netease_registry().resolve(SIDEBAR_SCAN_PROOF_COMMAND_ID).expect("command").clone();

    let output = futures_executor::block_on(command.invoke(InvokeCommandInput {
      command_id: command.id.to_string(),
      target_application_id: None,
      inputs,
      dry_run: false,
      cancellation: auv_cli_invoke::InvokeCancellation::new(),
    }))
    .expect("fixture validation remains the direct result");

    assert!(output.summary.contains("validated hermetic sidebar scan proof fixture"));
    assert_eq!(output.artifact_failures.len(), 1);
    assert!(output.artifact_failures[0].message.contains("no caller-owned run authority"));
  }

  #[test]
  fn sidebar_scan_proof_requires_fixture_dir() {
    let command = netease_registry().resolve(SIDEBAR_SCAN_PROOF_COMMAND_ID).expect("command").clone();
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

  #[test]
  fn invoke_help_lists_both_proof_commands() {
    let registry = netease_registry();
    let help = crate::invoke::render_help_index(&registry);
    assert!(help.contains(crate::invoke::select_proof::SELECT_PROOF_COMMAND_ID));
    assert!(help.contains(SIDEBAR_SCAN_PROOF_COMMAND_ID));
  }
}
