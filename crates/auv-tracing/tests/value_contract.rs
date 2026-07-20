use auv_tracing::{
  ArtifactId, ArtifactUri, AttributeKey, AttributeValue, Attributes, ContentType, EventName, EventPayload, EventSchema, JsonPayload,
  PageLimit, RunId, RunRevision, Sha256Digest,
};
use serde::Serialize;

#[derive(Serialize)]
struct SampleEvent {
  count: u64,
}

impl EventPayload for SampleEvent {
  const NAME: &'static str = "auv.test.sample";
  const VERSION: u32 = 1;
}

#[test]
fn artifact_uri_has_one_canonical_form() {
  let run_id = RunId::new();
  let artifact_id = ArtifactId::new();
  let uri = ArtifactUri::from_ids(run_id, artifact_id);
  assert_eq!(uri.run_id(), run_id);
  assert_eq!(uri.artifact_id(), artifact_id);
  assert!("auv://runs/not-a-uuid/artifacts/nope".parse::<ArtifactUri>().is_err());
  assert!(format!("{uri}?download=1").parse::<ArtifactUri>().is_err());
  assert!(format!("{uri}#fragment").parse::<ArtifactUri>().is_err());
}

#[test]
fn attributes_enforce_v1_shape_and_size() {
  let key = AttributeKey::parse("auv.test.label").unwrap();
  let attrs = Attributes::try_from_iter([(key, AttributeValue::string("ok").unwrap())]).unwrap();
  assert_eq!(attrs.len(), 1);
  assert!(AttributeKey::parse("Label").is_err());
  assert!(AttributeValue::integer(9_007_199_254_740_992).is_err());
}

#[test]
fn public_attribute_variants_cannot_bypass_integer_validation() {
  // ROOT CAUSE:
  //
  // A public enum variant bypassed the checked constructor because both the
  // attributes builder and derived serializer trusted the variant payload.
  let invalid = AttributeValue::I64(9_007_199_254_740_992);
  assert!(serde_json::to_string(&invalid).is_err());

  let key = AttributeKey::parse("auv.test.count").unwrap();
  assert!(Attributes::try_from_iter([(key, invalid)]).is_err());
}

#[test]
fn event_schema_and_payload_are_bounded() {
  let schema = EventSchema::for_payload::<SampleEvent>().unwrap();
  let payload = JsonPayload::encode(&SampleEvent { count: 4 }).unwrap();
  assert_eq!(schema.version().get(), 1);
  assert_eq!(payload.get(), r#"{"count":4}"#);
}

#[test]
fn revisions_stop_at_javascript_exact_integer_limit() {
  assert!(RunRevision::new(9_007_199_254_740_991).is_ok());
  assert!(RunRevision::new(9_007_199_254_740_992).is_err());
}

#[test]
fn page_and_content_type_values_have_concrete_bounds() {
  assert!(PageLimit::new(1024).is_ok());
  assert!(PageLimit::new(1025).is_err());
  assert!(ContentType::parse("image/png").is_ok());
  assert!(ContentType::parse("image/*").is_err());
  assert!(ContentType::parse(&format!("text/plain; label={}", "a".repeat(256))).is_err());
}

#[test]
fn digest_requires_lowercase_sha256_hex() {
  assert!("A123".parse::<Sha256Digest>().is_err());
  assert!("00".repeat(32).parse::<Sha256Digest>().is_ok());
}

#[test]
fn namespaced_names_are_bounded_by_encoded_bytes() {
  let accepted = format!("auv.test.{}", "a".repeat(119));
  let rejected = format!("auv.test.{}", "a".repeat(120));
  assert_eq!(accepted.len(), 128);
  assert!(EventName::parse(&accepted).is_ok());
  assert!(EventName::parse(&rejected).is_err());
}

#[test]
fn json_payload_rejects_duplicate_object_keys() {
  assert!(JsonPayload::from_str(r#"{"outer":{"value":1,"value":2}}"#).is_err());
}

#[test]
fn json_payload_preserves_single_private_marker_key_as_object_data() {
  // ROOT CAUSE:
  //
  // The recursive visitor guessed that a real object was serde_json's private
  // arbitrary-precision number map by comparing its first key's text.
  let payload = JsonPayload::from_str(r#"{"$serde_json::private::Number":"1.5"}"#).unwrap();
  assert_eq!(payload.get(), r#"{"$serde_json::private::Number":"1.5"}"#);
}

#[test]
fn json_payload_preserves_private_marker_key_among_canonical_object_fields() {
  let payload = JsonPayload::from_str(r#"{"$serde_json::private::Number":"1.5","z":2,"a":1}"#).unwrap();
  assert_eq!(payload.get(), r#"{"$serde_json::private::Number":"1.5","a":1,"z":2}"#);
}
