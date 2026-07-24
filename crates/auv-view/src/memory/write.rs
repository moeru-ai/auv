use super::ViewMemoryScopeSnapshot;
use crate::ViewNodeRecord;
use auv_tracing::ArtifactUri;

pub fn build_memory_id(app_bundle_id: &str, scope_id: &str) -> String {
  format!("{app_bundle_id}:{scope_id}")
}

pub struct MemoryWriteInput<'a> {
  pub source_scan_uri: ArtifactUri,
  pub app_bundle_id: &'a str,
  pub scope_id: &'a str,
  pub root: &'a ViewNodeRecord,
  pub scope_snapshot: ViewMemoryScopeSnapshot,
  pub last_reconstructed_at_millis: u64,
  pub clean: bool,
}

pub fn try_build_memory(input: MemoryWriteInput<'_>, reconstruction: &crate::ViewReconstructionRecord) -> Option<super::ViewMemory> {
  if !input.clean {
    return None;
  }
  super::memory_from_reconstruction_parts(input, reconstruction)
}
