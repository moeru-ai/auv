use std::{
  cell::Cell,
  collections::hash_map::DefaultHasher,
  hash::{Hash, Hasher},
  rc::Rc,
};

use auv_run::{
  AttributeKey, AttributeValue, Attributes, BoundedString, ContentType, EncodedPayload, FiniteF64, NonEmptyVec, OperationName, PageLimit,
  PayloadSchema, Revision, RunId, SchemaVersion, Sha256Digest, Timestamp,
};
use serde::de::{
  DeserializeSeed, IntoDeserializer, MapAccess,
  value::{Error as ValueDeserializerError, MapAccessDeserializer, MapDeserializer},
};

struct CountingMapAccess {
  keys: Vec<String>,
  next: usize,
  value_deserializations: Rc<Cell<usize>>,
}

impl<'de> MapAccess<'de> for CountingMapAccess {
  type Error = ValueDeserializerError;

  fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
  where
    K: DeserializeSeed<'de>,
  {
    let Some(key) = self.keys.get(self.next) else {
      return Ok(None);
    };
    seed.deserialize(key.clone().into_deserializer()).map(Some)
  }

  fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
  where
    V: DeserializeSeed<'de>,
  {
    self.value_deserializations.set(self.value_deserializations.get() + 1);
    let value = self.next as i64;
    self.next += 1;
    seed.deserialize(value.into_deserializer())
  }

  fn size_hint(&self) -> Option<usize> {
    Some(self.keys.len() - self.next)
  }
}

fn deserialize_counted_attributes(keys: Vec<String>, value_deserializations: Rc<Cell<usize>>) -> Result<Attributes, ValueDeserializerError> {
  let access = CountingMapAccess {
    keys,
    next: 0,
    value_deserializations,
  };
  <Attributes as serde::Deserialize>::deserialize(MapAccessDeserializer::new(access))
}

fn hash(value: &impl Hash) -> u64 {
  let mut hasher = DefaultHasher::new();
  value.hash(&mut hasher);
  hasher.finish()
}

#[test]
fn names_are_lowercase_dot_separated() {
  assert!(OperationName::parse("netease.playback.pause").is_ok());
  assert!(OperationName::parse("Netease.pause").is_err());
  assert!(OperationName::parse("netease..pause").is_err());
  assert!(serde_json::from_str::<OperationName>(r#""Netease.pause""#).is_err());
}

#[test]
fn attribute_keys_are_limited_to_128_bytes() {
  assert!(AttributeKey::parse(&format!("key.{}", "a".repeat(124))).is_ok());
  assert!(AttributeKey::parse(&format!("key.{}", "a".repeat(125))).is_err());
}

#[test]
fn ids_reject_nil_uuid_and_use_canonical_wire_strings() {
  let error = serde_json::from_str::<RunId>(r#""00000000-0000-0000-0000-000000000000""#).expect_err("nil run id must be rejected");
  assert!(error.to_string().contains("nil"));

  let id = RunId::parse("01890f47-9bd8-7cc2-98c7-1b4f87b5c6a1").unwrap();
  assert_eq!(serde_json::to_string(&id).unwrap(), r#""01890f47-9bd8-7cc2-98c7-1b4f87b5c6a1""#,);
  assert!(serde_json::from_str::<RunId>(r#""01890F47-9BD8-7CC2-98C7-1B4F87B5C6A1""#).is_err());

  let generated = RunId::new();
  assert!(!generated.as_uuid().is_nil());
  assert_eq!(generated.as_uuid().get_version_num(), 7);
}

#[test]
fn non_empty_vectors_reject_empty_values() {
  assert!(NonEmptyVec::<u8>::new(Vec::new()).is_err());
  assert!(serde_json::from_str::<NonEmptyVec<u8>>("[]").is_err());

  let values = NonEmptyVec::new(vec![1_u8, 2]).unwrap();
  assert_eq!(values.as_slice(), &[1, 2]);
  assert_eq!(serde_json::to_string(&values).unwrap(), "[1,2]");
}

#[test]
fn finite_floats_reject_non_finite_values() {
  assert!(FiniteF64::new(f64::NAN).is_err());
  assert!(FiniteF64::new(f64::INFINITY).is_err());
  assert!(FiniteF64::new(f64::NEG_INFINITY).is_err());
  assert_eq!(FiniteF64::new(1.25).unwrap().get(), 1.25);
}

#[test]
fn schema_versions_begin_at_one() {
  assert!(SchemaVersion::new(0).is_err());
  assert!(serde_json::from_str::<SchemaVersion>("0").is_err());
  assert_eq!(SchemaVersion::new(1).unwrap().get(), 1);
}

#[test]
fn page_limits_are_between_one_and_one_thousand() {
  assert!(PageLimit::new(0).is_err());
  assert!(PageLimit::new(1).is_ok());
  assert!(PageLimit::new(1_000).is_ok());
  assert!(PageLimit::new(1_001).is_err());
  assert!(serde_json::from_str::<PageLimit>("0").is_err());
  assert!(serde_json::from_str::<PageLimit>("1001").is_err());
}

#[test]
fn content_types_reject_invalid_mime_values() {
  assert!(ContentType::parse("not a mime").is_err());
  assert!(serde_json::from_str::<ContentType>(r#""not a mime""#).is_err());

  let content_type = ContentType::parse("application/json").unwrap();
  assert_eq!(content_type.as_str(), "application/json");
  assert_eq!(serde_json::to_string(&content_type).unwrap(), r#""application/json""#);

  let canonicalized: ContentType = serde_json::from_str(r#""APPLICATION/JSON""#).unwrap();
  assert_eq!(serde_json::to_string(&canonicalized).unwrap(), r#""application/json""#);
}

#[test]
fn content_types_have_one_canonical_parameter_order() {
  // ROOT CAUSE:
  //
  // `mime::Mime` retains input presentation details that cannot define AUV's
  // canonical wire identity. AUV stores one normalized representation instead.
  let content_type = ContentType::parse("Text/Plain; z=last; a=first").unwrap();
  assert_eq!(content_type.as_str(), "text/plain; a=first; z=last");
  assert_eq!(serde_json::to_string(&content_type).unwrap(), r#""text/plain; a=first; z=last""#);
}

#[test]
fn equivalent_content_type_parameters_have_equal_values_and_hashes() {
  let unquoted = ContentType::parse("multipart/form-data; boundary=ABC").unwrap();
  let quoted = ContentType::parse(r#"multipart/form-data; boundary="ABC""#).unwrap();

  assert_eq!(unquoted, quoted);
  assert_eq!(hash(&unquoted), hash(&quoted));
  assert_eq!(unquoted.as_str(), "multipart/form-data; boundary=ABC");
}

#[test]
fn content_type_parameter_values_remain_case_sensitive() {
  let uppercase = ContentType::parse("multipart/form-data; boundary=ABC").unwrap();
  let lowercase = ContentType::parse("multipart/form-data; boundary=abc").unwrap();
  assert_ne!(uppercase, lowercase);
}

#[test]
fn content_types_reject_duplicate_parameter_names() {
  assert!(ContentType::parse("text/plain; charset=utf-8; charset=us-ascii").is_err());
  assert!(ContentType::parse("text/plain; charset=utf-8; CHARSET=us-ascii").is_err());
}

#[test]
fn content_type_parameter_quoting_has_one_canonical_wire_form() {
  let content_type = ContentType::parse(r#"text/plain; note="one two;three""#).unwrap();
  assert_eq!(content_type.as_str(), r#"text/plain; note="one two;three""#);

  let encoded = serde_json::to_string(&content_type).unwrap();
  assert_eq!(encoded, r#""text/plain; note=\"one two;three\"""#);
  assert_eq!(serde_json::from_str::<ContentType>(&encoded).unwrap(), content_type);
}

#[test]
fn timestamps_reject_invalid_nanoseconds_and_use_utc_rfc3339() {
  assert!(Timestamp::new(1_700_000_000, 1_000_000_000).is_err());

  let timestamp = Timestamp::new(1_700_000_000, 123_456_789).unwrap();
  assert_eq!(timestamp.unix_seconds(), 1_700_000_000);
  assert_eq!(timestamp.nanoseconds(), 123_456_789);
  assert_eq!(serde_json::to_string(&timestamp).unwrap(), r#""2023-11-14T22:13:20.123456789Z""#,);

  let parsed: Timestamp = serde_json::from_str(r#""2023-11-14T23:13:20.123456789+01:00""#).unwrap();
  assert_eq!(parsed, timestamp);
  assert_eq!(serde_json::to_string(&parsed).unwrap(), serde_json::to_string(&timestamp).unwrap());
}

#[test]
fn timestamps_only_construct_when_utc_rfc3339_serialization_is_valid() {
  assert!(Timestamp::new(-62_198_755_200, 0).is_err());

  let earliest = Timestamp::new(-62_167_219_200, 0).unwrap();
  let encoded = serde_json::to_string(&earliest).unwrap();
  assert_eq!(encoded, r#""0000-01-01T00:00:00Z""#);
  assert_eq!(serde_json::from_str::<Timestamp>(&encoded).unwrap(), earliest);

  let latest = Timestamp::new(253_402_300_799, 999_999_999).unwrap();
  let encoded = serde_json::to_string(&latest).unwrap();
  assert_eq!(encoded, r#""9999-12-31T23:59:59.999999999Z""#);
  assert_eq!(serde_json::from_str::<Timestamp>(&encoded).unwrap(), latest);
}

#[test]
fn encoded_payload_enforces_the_compact_size_limit() {
  let schema = PayloadSchema::parse("test.input", 1).unwrap();
  let maximum = serde_json::Value::String("x".repeat(1_048_574));
  assert!(EncodedPayload::new(schema.clone(), maximum).is_ok());

  let oversized = serde_json::Value::String("x".repeat(1_048_577));
  assert_eq!(EncodedPayload::new(schema.clone(), oversized.clone()).unwrap_err().code(), "auv.payload.too_large",);

  let encoded = serde_json::json!({ "schema": schema, "data": oversized });
  assert_eq!(serde_json::from_value::<EncodedPayload>(encoded).unwrap_err().to_string(), "auv.payload.too_large",);
}

#[test]
fn encoded_payload_consumption_preserves_schema_and_data() {
  let schema = PayloadSchema::parse("test.input", 1).unwrap();
  let data = serde_json::json!({ "input": true });
  let (decoded_schema, decoded_data) = EncodedPayload::new(schema.clone(), data.clone()).unwrap().into_parts();

  assert_eq!(decoded_schema, schema);
  assert_eq!(decoded_data, data);
}

#[test]
fn payload_schema_rejects_unknown_and_duplicate_fields() {
  assert!(
    serde_json::from_value::<PayloadSchema>(serde_json::json!({
      "name": "test.input",
      "version": 1,
      "unexpected": true,
    }))
    .is_err()
  );
  assert!(serde_json::from_str::<PayloadSchema>(r#"{"name":"test.input","name":"test.output","version":1}"#).is_err());
}

#[test]
fn encoded_payload_rejects_unknown_and_duplicate_fields() {
  assert!(
    serde_json::from_value::<EncodedPayload>(serde_json::json!({
      "schema": { "name": "test.input", "version": 1 },
      "data": null,
      "large_unknown": "x".repeat(1_048_577),
    }))
    .is_err()
  );
  assert!(serde_json::from_str::<EncodedPayload>(r#"{"schema":{"name":"test.input","version":1},"data":null,"data":true}"#,).is_err());
}

#[test]
fn bounded_strings_enforce_the_four_kibibyte_limit() {
  assert!(BoundedString::new("x".repeat(4_096)).is_ok());
  assert_eq!(BoundedString::new("x".repeat(4_097)).unwrap_err().code(), "auv.string.too_large",);
  assert!(serde_json::from_value::<BoundedString>(serde_json::json!("x".repeat(4_097))).is_err());
}

#[test]
fn attributes_reject_more_than_64_entries() {
  let maximum = (0..64).map(|index| (AttributeKey::parse(&format!("test.value{index}")).unwrap(), AttributeValue::I64(index)));
  assert_eq!(Attributes::try_from_iter(maximum).unwrap().len(), 64);

  let entries = (0..65).map(|index| (AttributeKey::parse(&format!("test.value{index}")).unwrap(), AttributeValue::I64(index)));
  assert_eq!(Attributes::try_from_iter(entries).unwrap_err().code(), "auv.attributes.too_many");

  let encoded = serde_json::Value::Object((0..65).map(|index| (format!("test.value{index}"), serde_json::json!(index))).collect());
  assert!(serde_json::from_value::<Attributes>(encoded).is_err());
}

#[test]
fn attribute_deserialization_stops_at_the_65th_unique_entry() {
  // ROOT CAUSE:
  //
  // Collecting map entries before validation let hostile inputs allocate
  // without bound. The visitor now rejects as soon as it reads entry 65.
  let visits = Rc::new(Cell::new(0));
  let recorded_visits = Rc::clone(&visits);
  let entries = (0..).map(move |index| {
    assert!(index < 65, "attribute deserialization read beyond the rejecting entry");
    recorded_visits.set(index + 1);
    (format!("test.value{index}"), index as i64)
  });
  let deserializer = MapDeserializer::<_, ValueDeserializerError>::new(entries);

  let error = <Attributes as serde::Deserialize>::deserialize(deserializer).unwrap_err();
  assert!(error.to_string().contains("auv.attributes.too_many"));
  assert_eq!(visits.get(), 65);
}

#[test]
fn duplicate_attribute_keys_are_rejected_before_value_deserialization() {
  // ROOT CAUSE:
  //
  // `MapAccess::next_entry` deserializes the value before duplicate-key
  // admission. Rejected keys must never execute their value deserializer.
  let value_deserializations = Rc::new(Cell::new(0));
  let error =
    deserialize_counted_attributes(vec!["test.duplicate".to_owned(), "test.duplicate".to_owned()], Rc::clone(&value_deserializations))
      .unwrap_err();

  assert!(error.to_string().contains("auv.attributes.duplicate_key"));
  assert_eq!(value_deserializations.get(), 1);
}

#[test]
fn attribute_limit_is_rejected_before_65th_value_deserialization() {
  let value_deserializations = Rc::new(Cell::new(0));
  let keys = (0..65).map(|index| format!("test.value{index}")).collect();
  let error = deserialize_counted_attributes(keys, Rc::clone(&value_deserializations)).unwrap_err();

  assert!(error.to_string().contains("auv.attributes.too_many"));
  assert_eq!(value_deserializations.get(), 64);
}

#[test]
fn attributes_reject_duplicate_keys_without_rewriting_values() {
  let key = AttributeKey::parse("test.duplicate").unwrap();
  let error = Attributes::try_from_iter([
    (key.clone(), AttributeValue::Bool(true)),
    (key, AttributeValue::Bool(false)),
  ])
  .unwrap_err();
  assert_eq!(error.code(), "auv.attributes.duplicate_key");

  let error = serde_json::from_str::<Attributes>(r#"{"test.duplicate":true,"test.duplicate":false}"#).unwrap_err();
  assert!(error.to_string().contains("duplicate"));
}

#[test]
fn attribute_integer_deserialization_preserves_the_i64_domain() {
  let minimum: AttributeValue = serde_json::from_str(&i64::MIN.to_string()).unwrap();
  assert_eq!(minimum, AttributeValue::I64(i64::MIN));

  let maximum: AttributeValue = serde_json::from_str(&i64::MAX.to_string()).unwrap();
  assert_eq!(maximum, AttributeValue::I64(i64::MAX));

  let just_above = (i64::MAX as u64) + 1;
  for raw in [
    just_above.to_string(),
    u64::MAX.to_string(),
    "-9223372036854775809".to_owned(),
    "18446744073709551616".to_owned(),
    "9".repeat(80),
    format!("-{}", "9".repeat(80)),
  ] {
    let error = serde_json::from_str::<AttributeValue>(&raw).unwrap_err();
    assert!(error.to_string().contains("auv.attribute.integer_out_of_range"));
  }
}

#[test]
fn attribute_float_tokens_remain_finite_f64_values() {
  for (raw, expected) in [("1.0", 1.0), ("1e38", 1e38)] {
    let value: AttributeValue = serde_json::from_str(raw).unwrap();
    assert_eq!(value, AttributeValue::F64(FiniteF64::new(expected).unwrap()));
  }
}

#[test]
fn attribute_values_keep_supported_scalars_and_reject_non_scalars() {
  assert_eq!(serde_json::from_str::<AttributeValue>("true").unwrap(), AttributeValue::Bool(true));
  assert_eq!(serde_json::from_str::<AttributeValue>(r#""text""#).unwrap(), AttributeValue::String(BoundedString::new("text").unwrap()),);

  for raw in ["null", "[]", "{}"] {
    assert!(serde_json::from_str::<AttributeValue>(raw).is_err());
  }
}

#[test]
fn attributes_accept_exactly_32_kibibytes_of_canonical_json() {
  let entries = (0..8).map(|index| {
    // Eight 11-byte keys add 137 JSON syntax bytes, leaving 32,631 value bytes.
    let value_bytes = if index < 7 { 4_096 } else { 3_959 };
    (
      AttributeKey::parse(&format!("test.value{index}")).unwrap(),
      AttributeValue::String(BoundedString::new("x".repeat(value_bytes)).unwrap()),
    )
  });

  let attributes = Attributes::try_from_iter(entries).unwrap();
  assert_eq!(serde_json::to_vec(&attributes).unwrap().len(), 32_768);
}

#[test]
fn attributes_enforce_the_32_kibibyte_aggregate_limit() {
  let entries = (0..9).map(|index| {
    (AttributeKey::parse(&format!("test.value{index}")).unwrap(), AttributeValue::String(BoundedString::new("x".repeat(4_096)).unwrap()))
  });
  assert_eq!(Attributes::try_from_iter(entries).unwrap_err().code(), "auv.attributes.too_large");

  let encoded =
    serde_json::Value::Object((0..9).map(|index| (format!("test.value{index}"), serde_json::json!("x".repeat(4_096)))).collect());
  assert!(serde_json::from_value::<Attributes>(encoded).is_err());
}

#[test]
fn digest_hex_is_lowercase_and_fixed_width() {
  let digest = Sha256Digest::of_bytes(b"abc");
  assert_eq!(digest.to_hex(), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",);
  assert_eq!(serde_json::to_value(digest).unwrap(), serde_json::json!(digest.to_hex()));
  assert!(serde_json::from_value::<Sha256Digest>(serde_json::json!(digest.to_hex().to_uppercase())).is_err());
  assert!(serde_json::from_str::<Sha256Digest>(r#""abcd""#).is_err());
}

#[test]
fn revisions_start_at_zero_and_increment_with_checked_addition() {
  assert_eq!(Revision::ZERO.get(), 0);
  assert_eq!(Revision::ZERO.next().unwrap().get(), 1);

  let maximum: Revision = serde_json::from_str(&u64::MAX.to_string()).unwrap();
  assert_eq!(maximum.next().unwrap_err().code(), "auv.revision.overflow");
}
