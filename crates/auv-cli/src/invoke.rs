//! Product recorded invoke: shared by CLI and MCP.

use auv_cli_invoke::{InvokeRegistry, InvokeRequest, InvokeResult};
use auv_tracing_driver::RunRecordingBackend;

use crate::integrations::textedit;

/// Product invoke path: core recording, then TextEdit canonical operation finalize.
pub fn invoke_recorded(recording: &RunRecordingBackend, registry: &InvokeRegistry, request: InvokeRequest) -> Result<InvokeResult, String> {
  let result = auv_cli_invoke::invoke_recorded(recording, registry, request)?;
  textedit::persist_canonical_operation_result(recording.store(), &result)?;
  Ok(result)
}
