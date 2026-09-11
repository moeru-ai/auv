use std::str::FromStr;

use crate::resource::{DeviceId, DeviceSelector, RunId, RunnerClassId, RunnerId};

#[test]
fn canonical_ids_validate_their_resource_prefix() {
  let resource_id = "7d938f104ccc44c6a97850dd89d56c977d938f104ccc44c6a97850dd89d56c97";
  assert!(DeviceId::from_str(resource_id).is_ok());
  assert!(RunnerId::from_str(resource_id).is_ok());
  assert!(RunId::from_str("7d938f104ccc44c6a97850dd89d56c97").is_ok());
  assert!(RunnerClassId::from_str("auv.core.local").is_ok());

  assert!(DeviceId::from_str("7d938f104ccc44c6a97850dd89d56c97").is_err());
  assert!(RunId::from_str("7d938f10").is_err());
  assert!(RunnerClassId::from_str("  ").is_err());
}

#[test]
fn compact_display_does_not_discard_the_canonical_value() {
  let canonical = "7d938f104ccc44c6a97850dd89d56c977d938f104ccc44c6a97850dd89d56c97";
  let id = DeviceId::from_str(canonical).unwrap();

  assert_eq!(id.as_str(), canonical);
  assert_eq!(id.compact(), canonical);
  assert_eq!(id.short(), "7d938f104ccc");
}

#[test]
fn user_input_is_a_selector_instead_of_a_partial_id() {
  assert_eq!(DeviceSelector::parse("Studio Mac").unwrap(), DeviceSelector::by_name("Studio Mac"));
  assert_eq!(DeviceSelector::parse("7d938f10").unwrap(), DeviceSelector::by_id("7d938f10"));
  assert!(DeviceSelector::parse("  ").is_err());
}
