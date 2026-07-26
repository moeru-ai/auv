use auv_cli_invoke::{InvokeNamespace, default_registry};

#[test]
fn default_registry_contains_the_scan_commands() {
  let registry = default_registry();

  assert_eq!(registry.resolve("scan.frame").expect("scan.frame").namespace, InvokeNamespace::Scan);
  assert_eq!(registry.resolve("scan.coverage").expect("scan.coverage").namespace, InvokeNamespace::Scan);
}
