//! Product invoke registry: core catalog plus app-owned extensions.

use auv_cli_invoke::{InvokeRegistry, default_registry};

use crate::integrations::textedit;

/// Product invoke registry shared by product CLI and product MCP.
///
/// Core `auv-cli-invoke::default_registry` stays free of app crates. TextEdit
/// registration lives here so `auv-runtime` does not depend on `auv-apple-textedit`.
pub fn product_registry() -> InvokeRegistry {
  let mut groups = default_registry().groups().to_vec();
  groups.push(textedit::group());
  InvokeRegistry::from_groups(groups)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::integrations::textedit::DOCUMENT_WRITE_COMMAND_ID;

  #[test]
  fn product_registry_includes_textedit_once() {
    let registry = product_registry();
    assert!(registry.resolve(DOCUMENT_WRITE_COMMAND_ID).is_some());
    assert_eq!(registry.all().iter().filter(|command| command.id == DOCUMENT_WRITE_COMMAND_ID).count(), 1);
  }

  #[test]
  fn core_registry_excludes_textedit() {
    assert!(default_registry().resolve(DOCUMENT_WRITE_COMMAND_ID).is_none());
  }
}
