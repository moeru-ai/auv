use super::ViewMemoryScopeSnapshot;
use crate::ViewNodeRecord;

/// Compatibility placeholder when product commands write under `--artifact-dir`
/// without `--store-root`. Not a real persisted run id.
pub const ARTIFACT_DIR_BRIDGE_RUN_ID: &str = "artifact-dir-bridge-a3";

pub fn build_memory_id(app_bundle_id: &str, scope_id: &str) -> String {
  format!("{app_bundle_id}:{scope_id}")
}

pub struct MemoryWriteInput<'a> {
  pub app_bundle_id: &'a str,
  pub scope_id: &'a str,
  pub root: &'a ViewNodeRecord,
  pub scope_snapshot: ViewMemoryScopeSnapshot,
  pub source_reconstruction_ref: String,
  pub source_run_id: String,
  pub last_reconstructed_at_millis: u64,
  pub clean: bool,
}

pub fn try_build_memory(input: MemoryWriteInput<'_>, reconstruction: &crate::ViewReconstructionRecord) -> Option<super::ViewMemory> {
  if !input.clean {
    return None;
  }
  super::memory_from_reconstruction_parts(input, reconstruction)
}
