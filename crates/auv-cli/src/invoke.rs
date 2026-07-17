//! Product recorded invoke: shared by CLI and MCP.

use auv_cli_invoke::{InvokeCliOutcome, InvokeOutputOptions, InvokeRegistry, InvokeRequest, InvokeResult};
use auv_tracing_driver::RunRecordingBackend;

use crate::integrations::textedit;

/// Product invoke path: core recording plus TextEdit finalize inside the shared
/// recorded lifecycle.
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
