//! Task22 legacy recorded invoke adapter.

use auv_cli_invoke::{InvokeCliOutcome, InvokeOutputOptions, InvokeRegistry, InvokeRequest, InvokeResult};
use auv_tracing_driver::RunRecordingBackend;

use crate::integrations::textedit;

/// Runs the legacy recording backend with the TextEdit finalizer.
///
/// New CLI and MCP execution paths call typed domain functions and own their
/// recording composition independently.
pub fn invoke_recorded(recording: &RunRecordingBackend, registry: &InvokeRegistry, request: InvokeRequest) -> Result<InvokeResult, String> {
  auv_cli_invoke::invoke_recorded_with_finalize(recording, registry, request, &textedit::finalize_recorded_invoke)
}

pub fn invoke_recorded_and_render(
  recording: &RunRecordingBackend,
  registry: &InvokeRegistry,
  request: InvokeRequest,
  output: InvokeOutputOptions,
) -> Result<InvokeCliOutcome, String> {
  auv_cli_invoke::render_recorded_invoke(recording, registry, request, output, Some(&textedit::finalize_recorded_invoke))
}
