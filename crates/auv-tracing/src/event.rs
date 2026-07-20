use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{
  SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple, SerializeTupleStruct, SerializeTupleVariant,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;
use serde_json::{Map, Number, Value};

use crate::{EventName, ValidationError};

const MAX_EVENT_JSON_BYTES: usize = 64 * 1024;
const JAVASCRIPT_EXACT_INTEGER_MAX: u64 = 9_007_199_254_740_991;

/// Identifies the name and positive version of a typed event payload.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct EventSchema {
  name: EventName,
  version: NonZeroU32,
}

impl EventSchema {
  /// Creates a validated event schema.
  pub fn new(name: EventName, version: u32) -> Result<Self, ValidationError> {
    let version = NonZeroU32::new(version).ok_or_else(|| ValidationError::new("event schema version must be non-zero"))?;
    Ok(Self { name, version })
  }

  /// Creates the schema declared by a typed event payload.
  pub fn for_payload<T: EventPayload>() -> Result<Self, ValidationError> {
    Self::new(EventName::parse(T::NAME)?, T::VERSION)
  }

  /// Returns the event name.
  pub fn name(&self) -> &EventName {
    &self.name
  }

  /// Returns the positive schema version.
  pub fn version(&self) -> NonZeroU32 {
    self.version
  }
}

impl<'de> Deserialize<'de> for EventSchema {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      name: EventName,
      version: u32,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.name, wire.version).map_err(de::Error::custom)
  }
}

/// Canonical, duplicate-free JSON bounded for one event.
#[derive(Clone, Debug)]
pub struct JsonPayload(Box<RawValue>);

impl JsonPayload {
  /// Encodes one typed payload and validates the resulting event JSON.
  pub fn encode<T: Serialize + ?Sized>(payload: &T) -> Result<Self, JsonPayloadError> {
    let mut encoded = Vec::new();
    let mut serializer = serde_json::Serializer::new(&mut encoded);
    payload.serialize(ExactJsonSerializer(&mut serializer)).map_err(JsonPayloadError::serialization)?;
    Self::from_slice(&encoded)
  }

  /// Parses strict JSON, rejecting duplicate keys at every nesting level.
  #[allow(clippy::should_implement_trait)]
  pub fn from_str(value: &str) -> Result<Self, JsonPayloadError> {
    Self::from_slice(value.as_bytes())
  }

  /// Borrows the canonical compact JSON text.
  pub fn get(&self) -> &str {
    self.0.get()
  }

  fn from_slice(value: &[u8]) -> Result<Self, JsonPayloadError> {
    if value.len() > MAX_EVENT_JSON_BYTES {
      return Err(JsonPayloadError::new("event JSON exceeds 65536 bytes"));
    }

    let raw = serde_json::from_slice::<Box<RawValue>>(value).map_err(JsonPayloadError::json)?;
    let parsed = StrictValue::from_raw(&raw)?;
    let canonical = serde_json::to_string(&parsed.into_json()).map_err(JsonPayloadError::json)?;
    if canonical.len() > MAX_EVENT_JSON_BYTES {
      return Err(JsonPayloadError::new("event JSON exceeds 65536 bytes"));
    }
    let raw = RawValue::from_string(canonical).map_err(JsonPayloadError::json)?;
    Ok(Self(raw))
  }
}

impl PartialEq for JsonPayload {
  fn eq(&self, other: &Self) -> bool {
    self.get() == other.get()
  }
}

impl Eq for JsonPayload {}

impl Serialize for JsonPayload {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    self.0.serialize(serializer)
  }
}

impl<'de> Deserialize<'de> for JsonPayload {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = Box::<RawValue>::deserialize(deserializer)?;
    Self::from_str(raw.get()).map_err(de::Error::custom)
  }
}

impl FromStr for JsonPayload {
  type Err = JsonPayloadError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    Self::from_str(value)
  }
}

/// A typed point-event payload declaration.
pub trait EventPayload: Serialize {
  /// Stable namespaced event name.
  const NAME: &'static str;
  /// Positive payload schema version.
  const VERSION: u32;
}

/// Reports invalid, oversized, or non-canonical event JSON.
#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct JsonPayloadError {
  message: String,
}

impl JsonPayloadError {
  fn new(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
    }
  }

  fn json(error: serde_json::Error) -> Self {
    Self::new(error.to_string())
  }

  fn serialization(error: serde_json::Error) -> Self {
    Self::new(format!("event payload serialization failed: {error}"))
  }
}

#[derive(Debug)]
enum StrictValue {
  Null,
  Bool(bool),
  Number(Number),
  String(String),
  Array(Vec<Self>),
  Object(BTreeMap<String, Self>),
}

impl StrictValue {
  fn from_raw(raw: &RawValue) -> Result<Self, JsonPayloadError> {
    let token = raw.get().bytes().find(|byte| !byte.is_ascii_whitespace()).ok_or_else(|| JsonPayloadError::new("event JSON is empty"))?;
    match token {
      b'{' => deserialize_raw_map(raw),
      b'[' => deserialize_raw_sequence(raw),
      b'"' => serde_json::from_str::<String>(raw.get()).map(Self::String).map_err(JsonPayloadError::json),
      b't' => Ok(Self::Bool(true)),
      b'f' => Ok(Self::Bool(false)),
      b'n' => Ok(Self::Null),
      b'-' | b'0'..=b'9' => parse_number_lexeme(raw.get()).map(Self::Number),
      _ => Err(JsonPayloadError::new("event JSON has an invalid leading token")),
    }
  }

  fn into_json(self) -> Value {
    match self {
      Self::Null => Value::Null,
      Self::Bool(value) => Value::Bool(value),
      Self::Number(value) => Value::Number(value),
      Self::String(value) => Value::String(value),
      Self::Array(values) => Value::Array(Self::values_into_json(values)),
      Self::Object(values) => {
        let values = values.into_iter().map(|(key, value)| (key, value.into_json())).collect::<Map<_, _>>();
        Value::Object(values)
      }
    }
  }

  fn values_into_json(values: Vec<Self>) -> Vec<Value> {
    values.into_iter().map(Self::into_json).collect()
  }
}

struct StrictObjectVisitor;

impl<'de> Visitor<'de> for StrictObjectVisitor {
  type Value = StrictValue;

  fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("a JSON object with unique keys")
  }

  fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
  where
    A: MapAccess<'de>,
  {
    let mut values = BTreeMap::new();
    while let Some(key) = map.next_key::<String>()? {
      if values.contains_key(&key) {
        return Err(de::Error::custom(format!("duplicate JSON object key `{key}`")));
      }
      let raw = map.next_value::<Box<RawValue>>()?;
      let value = StrictValue::from_raw(&raw).map_err(de::Error::custom)?;
      values.insert(key, value);
    }
    Ok(StrictValue::Object(values))
  }
}

struct StrictSequenceVisitor;

impl<'de> Visitor<'de> for StrictSequenceVisitor {
  type Value = StrictValue;

  fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("a JSON array")
  }

  fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
  where
    A: SeqAccess<'de>,
  {
    let mut values = Vec::new();
    while let Some(raw) = sequence.next_element::<Box<RawValue>>()? {
      values.push(StrictValue::from_raw(&raw).map_err(de::Error::custom)?);
    }
    Ok(StrictValue::Array(values))
  }
}

fn deserialize_raw_map(raw: &RawValue) -> Result<StrictValue, JsonPayloadError> {
  let mut deserializer = serde_json::Deserializer::from_str(raw.get());
  let value = deserializer.deserialize_map(StrictObjectVisitor).map_err(JsonPayloadError::json)?;
  deserializer.end().map_err(JsonPayloadError::json)?;
  Ok(value)
}

fn deserialize_raw_sequence(raw: &RawValue) -> Result<StrictValue, JsonPayloadError> {
  let mut deserializer = serde_json::Deserializer::from_str(raw.get());
  let value = deserializer.deserialize_seq(StrictSequenceVisitor).map_err(JsonPayloadError::json)?;
  deserializer.end().map_err(JsonPayloadError::json)?;
  Ok(value)
}

fn parse_number_lexeme(value: &str) -> Result<Number, JsonPayloadError> {
  if value.contains(['.', 'e', 'E']) {
    let value = value.parse::<f64>().map_err(|_| JsonPayloadError::new("JSON number is not representable as a finite float"))?;
    return Number::from_f64(value).ok_or_else(|| JsonPayloadError::new("JSON floating-point value must be finite"));
  }

  if value.starts_with('-') {
    let value = value.parse::<i64>().map_err(|_| JsonPayloadError::new("JSON integer is below the exact integer range"))?;
    if value < -(JAVASCRIPT_EXACT_INTEGER_MAX as i64) {
      return Err(JsonPayloadError::new("JSON integer is below the exact integer range"));
    }
    return Ok(Number::from(value));
  }

  let value = value.parse::<u64>().map_err(|_| JsonPayloadError::new("JSON integer exceeds the exact integer range"))?;
  if value > JAVASCRIPT_EXACT_INTEGER_MAX {
    return Err(JsonPayloadError::new("JSON integer exceeds the exact integer range"));
  }
  Ok(Number::from(value))
}

struct ExactJsonSerializer<S>(S);

impl<S> Serializer for ExactJsonSerializer<S>
where
  S: Serializer,
{
  type Ok = S::Ok;
  type Error = S::Error;
  type SerializeSeq = CheckedSequence<S::SerializeSeq>;
  type SerializeTuple = CheckedSequence<S::SerializeTuple>;
  type SerializeTupleStruct = CheckedSequence<S::SerializeTupleStruct>;
  type SerializeTupleVariant = CheckedTupleVariant<S::SerializeTupleVariant>;
  type SerializeMap = CheckedMap<S::SerializeMap>;
  type SerializeStruct = CheckedStruct<S::SerializeStruct>;
  type SerializeStructVariant = CheckedStructVariant<S::SerializeStructVariant>;

  fn serialize_bool(self, value: bool) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_bool(value)
  }

  fn serialize_i8(self, value: i8) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_i8(value)
  }

  fn serialize_i16(self, value: i16) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_i16(value)
  }

  fn serialize_i32(self, value: i32) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_i32(value)
  }

  fn serialize_i64(self, value: i64) -> Result<Self::Ok, Self::Error> {
    if value < -(JAVASCRIPT_EXACT_INTEGER_MAX as i64) || value > JAVASCRIPT_EXACT_INTEGER_MAX as i64 {
      return Err(serde::ser::Error::custom("JSON integer exceeds the exact integer range"));
    }
    self.0.serialize_i64(value)
  }

  fn serialize_i128(self, value: i128) -> Result<Self::Ok, Self::Error> {
    if value < -(JAVASCRIPT_EXACT_INTEGER_MAX as i128) || value > JAVASCRIPT_EXACT_INTEGER_MAX as i128 {
      return Err(serde::ser::Error::custom("JSON integer exceeds the exact integer range"));
    }
    self.0.serialize_i128(value)
  }

  fn serialize_u8(self, value: u8) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_u8(value)
  }

  fn serialize_u16(self, value: u16) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_u16(value)
  }

  fn serialize_u32(self, value: u32) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_u32(value)
  }

  fn serialize_u64(self, value: u64) -> Result<Self::Ok, Self::Error> {
    if value > JAVASCRIPT_EXACT_INTEGER_MAX {
      return Err(serde::ser::Error::custom("JSON integer exceeds the exact integer range"));
    }
    self.0.serialize_u64(value)
  }

  fn serialize_u128(self, value: u128) -> Result<Self::Ok, Self::Error> {
    if value > JAVASCRIPT_EXACT_INTEGER_MAX as u128 {
      return Err(serde::ser::Error::custom("JSON integer exceeds the exact integer range"));
    }
    self.0.serialize_u128(value)
  }

  fn serialize_f32(self, value: f32) -> Result<Self::Ok, Self::Error> {
    if !value.is_finite() {
      return Err(serde::ser::Error::custom("JSON floating-point value must be finite"));
    }
    self.0.serialize_f32(value)
  }

  fn serialize_f64(self, value: f64) -> Result<Self::Ok, Self::Error> {
    if !value.is_finite() {
      return Err(serde::ser::Error::custom("JSON floating-point value must be finite"));
    }
    self.0.serialize_f64(value)
  }

  fn serialize_char(self, value: char) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_char(value)
  }

  fn serialize_str(self, value: &str) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_str(value)
  }

  fn serialize_bytes(self, value: &[u8]) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_bytes(value)
  }

  fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_none()
  }

  fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_some(&Checked(value))
  }

  fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_unit()
  }

  fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_unit_struct(name)
  }

  fn serialize_unit_variant(self, name: &'static str, variant_index: u32, variant: &'static str) -> Result<Self::Ok, Self::Error> {
    self.0.serialize_unit_variant(name, variant_index, variant)
  }

  fn serialize_newtype_struct<T>(self, name: &'static str, value: &T) -> Result<Self::Ok, Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_newtype_struct(name, &Checked(value))
  }

  fn serialize_newtype_variant<T>(
    self,
    name: &'static str,
    variant_index: u32,
    variant: &'static str,
    value: &T,
  ) -> Result<Self::Ok, Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_newtype_variant(name, variant_index, variant, &Checked(value))
  }

  fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
    self.0.serialize_seq(len).map(CheckedSequence)
  }

  fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
    self.0.serialize_tuple(len).map(CheckedSequence)
  }

  fn serialize_tuple_struct(self, name: &'static str, len: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
    self.0.serialize_tuple_struct(name, len).map(CheckedSequence)
  }

  fn serialize_tuple_variant(
    self,
    name: &'static str,
    variant_index: u32,
    variant: &'static str,
    len: usize,
  ) -> Result<Self::SerializeTupleVariant, Self::Error> {
    self.0.serialize_tuple_variant(name, variant_index, variant, len).map(CheckedTupleVariant)
  }

  fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
    self.0.serialize_map(len).map(CheckedMap)
  }

  fn serialize_struct(self, name: &'static str, len: usize) -> Result<Self::SerializeStruct, Self::Error> {
    self.0.serialize_struct(name, len).map(CheckedStruct)
  }

  fn serialize_struct_variant(
    self,
    name: &'static str,
    variant_index: u32,
    variant: &'static str,
    len: usize,
  ) -> Result<Self::SerializeStructVariant, Self::Error> {
    self.0.serialize_struct_variant(name, variant_index, variant, len).map(CheckedStructVariant)
  }

  fn collect_str<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
  where
    T: fmt::Display + ?Sized,
  {
    self.0.collect_str(value)
  }

  fn is_human_readable(&self) -> bool {
    self.0.is_human_readable()
  }
}

struct Checked<'a, T: ?Sized>(&'a T);

impl<T> Serialize for Checked<'_, T>
where
  T: Serialize + ?Sized,
{
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    self.0.serialize(ExactJsonSerializer(serializer))
  }
}

struct CheckedSequence<S>(S);

impl<S> SerializeSeq for CheckedSequence<S>
where
  S: SerializeSeq,
{
  type Ok = S::Ok;
  type Error = S::Error;

  fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_element(&Checked(value))
  }

  fn end(self) -> Result<Self::Ok, Self::Error> {
    self.0.end()
  }
}

impl<S> SerializeTuple for CheckedSequence<S>
where
  S: SerializeTuple,
{
  type Ok = S::Ok;
  type Error = S::Error;

  fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_element(&Checked(value))
  }

  fn end(self) -> Result<Self::Ok, Self::Error> {
    self.0.end()
  }
}

impl<S> SerializeTupleStruct for CheckedSequence<S>
where
  S: SerializeTupleStruct,
{
  type Ok = S::Ok;
  type Error = S::Error;

  fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_field(&Checked(value))
  }

  fn end(self) -> Result<Self::Ok, Self::Error> {
    self.0.end()
  }
}

struct CheckedTupleVariant<S>(S);

impl<S> SerializeTupleVariant for CheckedTupleVariant<S>
where
  S: SerializeTupleVariant,
{
  type Ok = S::Ok;
  type Error = S::Error;

  fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_field(&Checked(value))
  }

  fn end(self) -> Result<Self::Ok, Self::Error> {
    self.0.end()
  }
}

struct CheckedMap<S>(S);

impl<S> SerializeMap for CheckedMap<S>
where
  S: SerializeMap,
{
  type Ok = S::Ok;
  type Error = S::Error;

  fn serialize_key<T>(&mut self, key: &T) -> Result<(), Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_key(&Checked(key))
  }

  fn serialize_value<T>(&mut self, value: &T) -> Result<(), Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_value(&Checked(value))
  }

  fn serialize_entry<K, V>(&mut self, key: &K, value: &V) -> Result<(), Self::Error>
  where
    K: Serialize + ?Sized,
    V: Serialize + ?Sized,
  {
    self.0.serialize_entry(&Checked(key), &Checked(value))
  }

  fn end(self) -> Result<Self::Ok, Self::Error> {
    self.0.end()
  }
}

struct CheckedStruct<S>(S);

impl<S> SerializeStruct for CheckedStruct<S>
where
  S: SerializeStruct,
{
  type Ok = S::Ok;
  type Error = S::Error;

  fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_field(key, &Checked(value))
  }

  fn end(self) -> Result<Self::Ok, Self::Error> {
    self.0.end()
  }
}

struct CheckedStructVariant<S>(S);

impl<S> SerializeStructVariant for CheckedStructVariant<S>
where
  S: SerializeStructVariant,
{
  type Ok = S::Ok;
  type Error = S::Error;

  fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
  where
    T: Serialize + ?Sized,
  {
    self.0.serialize_field(key, &Checked(value))
  }

  fn end(self) -> Result<Self::Ok, Self::Error> {
    self.0.end()
  }
}

#[cfg(test)]
mod tests {
  use super::JsonPayload;

  #[test]
  fn typed_payload_rejects_non_finite_float() {
    assert!(JsonPayload::encode(&f64::NAN).is_err());
  }
}
