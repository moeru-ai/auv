use std::collections::BTreeMap;

use auv_tracing::{
  ArtifactId, ArtifactUri, AttributeKey, AttributeValue, Attributes, ContentType, EventName, EventPayload, EventSchema, JsonPayload,
  PageLimit, RunId, RunRevision, Sha256Digest,
};
use serde::Serialize;

#[derive(Serialize)]
struct SampleEvent {
  count: u64,
}

#[derive(Serialize)]
struct NestedStruct {
  value: f64,
}

#[derive(Serialize)]
enum NestedVariant {
  Value { value: u64 },
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
fn attribute_float_round_trips_standalone_and_in_attributes() {
  // ROOT CAUSE:
  //
  // With arbitrary_precision enabled, deserialize_any exposes a JSON float as
  // serde_json's private map instead of calling the float visitor.
  let value = AttributeValue::float(1.5).unwrap();
  let json = serde_json::to_string(&value).unwrap();
  assert_eq!(json, "1.5");
  assert_eq!(serde_json::from_str::<AttributeValue>(&json).unwrap(), value);

  let key = AttributeKey::parse("auv.test.ratio").unwrap();
  let attributes = Attributes::try_from_iter([(key, value)]).unwrap();
  let json = serde_json::to_string(&attributes).unwrap();
  assert_eq!(json, r#"{"auv.test.ratio":1.5}"#);
  assert_eq!(serde_json::from_str::<Attributes>(&json).unwrap(), attributes);
}

#[test]
fn attribute_wire_decode_accepts_scalars_and_rejects_non_scalars() {
  assert_eq!(serde_json::from_str::<AttributeValue>("true").unwrap(), AttributeValue::boolean(true));
  assert_eq!(serde_json::from_str::<AttributeValue>("42").unwrap(), AttributeValue::integer(42).unwrap());
  assert_eq!(serde_json::from_str::<AttributeValue>(r#""value""#).unwrap(), AttributeValue::string("value").unwrap(),);

  for invalid in [
    "null",
    "[]",
    "{}",
    r#"{"$serde_json::private::Number":"1.5"}"#,
  ] {
    assert!(serde_json::from_str::<AttributeValue>(invalid).is_err());
  }
}

#[test]
fn attributes_enforce_count_string_and_exact_encoded_size_boundaries() {
  let entries = (0..32).map(|index| (AttributeKey::parse(format!("auv.test.k{index}")).unwrap(), AttributeValue::boolean(true)));
  assert_eq!(Attributes::try_from_iter(entries).unwrap().len(), 32);

  let entries = (0..33).map(|index| (AttributeKey::parse(format!("auv.test.k{index}")).unwrap(), AttributeValue::boolean(true)));
  assert!(Attributes::try_from_iter(entries).is_err());

  assert!(AttributeValue::string("a".repeat(1024)).is_ok());
  assert!(AttributeValue::string("a".repeat(1025)).is_err());

  let prefix = (0..15)
    .map(|index| (AttributeKey::parse(format!("auv.test.size{index}")).unwrap(), AttributeValue::string("a".repeat(1024)).unwrap()))
    .collect::<Vec<_>>();
  let final_key = AttributeKey::parse("auv.test.size15").unwrap();
  let mut probe_entries = prefix.clone();
  probe_entries.push((final_key.clone(), AttributeValue::string("").unwrap()));
  let probe = Attributes::try_from_iter(probe_entries).unwrap();
  let final_string_len = 16_384 - serde_json::to_vec(&probe).unwrap().len();
  assert!(final_string_len > 0);
  assert!(final_string_len <= 1024);

  let mut exact_entries = prefix.clone();
  exact_entries.push((final_key.clone(), AttributeValue::string("a".repeat(final_string_len)).unwrap()));
  let exact = Attributes::try_from_iter(exact_entries).unwrap();
  assert_eq!(serde_json::to_vec(&exact).unwrap().len(), 16_384);

  let mut oversized_entries = prefix;
  oversized_entries.push((final_key, AttributeValue::string("a".repeat(final_string_len + 1)).unwrap()));
  let wire_map = oversized_entries.iter().cloned().collect::<BTreeMap<_, _>>();
  assert_eq!(serde_json::to_vec(&wire_map).unwrap().len(), 16_385);
  assert!(Attributes::try_from_iter(oversized_entries).is_err());
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

#[test]
fn json_payload_enforces_explicit_v1_nesting_limit() {
  // ROOT CAUSE:
  //
  // Recursing through child RawValues creates a fresh serde_json deserializer
  // at every level, so its implicit recursion guard cannot bound the full tree.
  fn nested_array(depth: usize) -> String {
    format!("{}0{}", "[".repeat(depth), "]".repeat(depth))
  }

  assert!(JsonPayload::from_str(&nested_array(128)).is_ok());
  assert!(JsonPayload::from_str(&nested_array(129)).is_err());
}

#[test]
fn typed_payload_checks_nested_sequence_map_struct_and_enum_values() {
  assert!(JsonPayload::encode(&vec![f64::NAN]).is_err());

  let map = BTreeMap::from([("value", 9_007_199_254_740_992_u64)]);
  assert!(JsonPayload::encode(&map).is_err());

  assert!(JsonPayload::encode(&NestedStruct { value: f64::NAN }).is_err());
  assert!(
    JsonPayload::encode(&NestedVariant::Value {
      value: 9_007_199_254_740_992,
    })
    .is_err()
  );
}
