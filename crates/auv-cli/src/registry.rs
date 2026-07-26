//! Product invoke registry: core catalog plus app-owned extensions.

use auv_cli_invoke::{InvokeRegistry, default_registry};

use crate::integrations::{balatro, textedit};

/// Product invoke registry used for CLI adapters and MCP catalog metadata.
///
/// Core `auv-cli-invoke::default_registry` stays free of app crates. TextEdit
/// registration lives here so `auv-runtime` does not depend on
/// `auv-apple-textedit`. MCP execution uses its own typed adapters rather than
/// invoking commands from this registry.
pub fn product_registry() -> InvokeRegistry {
  let mut groups = default_registry().groups().to_vec();
  groups.push(balatro::group());
  groups.push(textedit::group());
  InvokeRegistry::from_groups(groups)
}
