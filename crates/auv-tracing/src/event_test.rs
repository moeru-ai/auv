use super::JsonPayload;

#[test]
fn typed_payload_rejects_non_finite_float() {
  assert!(JsonPayload::encode(&f64::NAN).is_err());
}
