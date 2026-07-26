use super::*;

#[test]
fn projection_basis_serializes_generic_provenance() {
  let basis = ProjectionBasis::new(
    "basis-frame-1",
    1_000,
    ProjectionSourceSpace::World,
    CoordinateSpace::Window("window-1".to_string()),
    ProjectionDerivationFamily::CameraMatrix,
  )
  .with_confidence(0.75)
  .with_match_radius_px(12.0);

  let value = serde_json::to_value(&basis).expect("serialize projection basis");

  assert_eq!(value["basis_id"], serde_json::json!("basis-frame-1"));
  assert_eq!(value["source_space"]["kind"], serde_json::json!("world"));
  assert_eq!(value["derivation_family"], serde_json::json!("camera_matrix"));
  assert_eq!(value["match_radius_px"], serde_json::json!(12.0));
}
