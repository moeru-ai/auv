use std::collections::BTreeMap;
use std::fmt;
use std::num::NonZeroU32;
use std::str::FromStr;

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;
use uuid::Uuid;

const JAVASCRIPT_EXACT_INTEGER_MAX: u64 = 9_007_199_254_740_991;
const MAX_NAMESPACED_NAME_BYTES: usize = 128;
const MAX_ATTRIBUTE_COUNT: usize = 32;
const MAX_ATTRIBUTE_STRING_BYTES: usize = 1_024;
const MAX_ATTRIBUTES_JSON_BYTES: usize = 16_384;
const MAX_PAGE_LIMIT: u32 = 1_024;
const MAX_CONTENT_TYPE_BYTES: usize = 256;
const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

/// Reports a value that violates a V1 run-data invariant.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct ValidationError {
  message: &'static str,
}

impl ValidationError {
  pub(crate) const fn new(message: &'static str) -> Self {
    Self { message }
  }
}

macro_rules! uuid_id {
  ($name:ident, $description:literal) => {
    #[doc = $description]
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
    #[serde(transparent)]
    pub struct $name(Uuid);

    #[allow(clippy::new_without_default)]
    impl $name {
      /// Generates a non-nil UUIDv7 identifier.
      pub fn new() -> Self {
        Self(Uuid::now_v7())
      }

      /// Returns the underlying UUID.
      pub fn as_uuid(&self) -> &Uuid {
        &self.0
      }

      fn from_uuid(value: Uuid) -> Result<Self, ValidationError> {
        if value.is_nil() {
          return Err(ValidationError::new("identifier must not be nil"));
        }
        Ok(Self(value))
      }
    }

    impl fmt::Display for $name {
      fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
      }
    }

    impl FromStr for $name {
      type Err = ValidationError;

      fn from_str(value: &str) -> Result<Self, Self::Err> {
        let uuid = Uuid::parse_str(value).map_err(|_| ValidationError::new("identifier must be a UUID"))?;
        if uuid.hyphenated().to_string() != value {
          return Err(ValidationError::new("identifier must use canonical lowercase hyphenated UUID text"));
        }
        Self::from_uuid(uuid)
      }
    }

    impl<'de> Deserialize<'de> for $name {
      fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
      where
        D: Deserializer<'de>,
      {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(de::Error::custom)
      }
    }
  };
}

uuid_id!(RunId, "Identifies one explicit AUV run scope.");
uuid_id!(AuthorityId, "Identifies the store authoritative for a run.");
uuid_id!(SpanId, "Identifies a span within a run.");
uuid_id!(EventId, "Identifies an immutable event within a run.");
uuid_id!(ArtifactId, "Identifies an artifact within a run.");
uuid_id!(IdempotencyKey, "Identifies one idempotent store operation.");

/// A browser-exact run ordering cursor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RunRevision(u64);

impl RunRevision {
  /// Validates a run revision, including revision zero as the pre-history cursor.
  pub fn new(value: u64) -> Result<Self, ValidationError> {
    if value > JAVASCRIPT_EXACT_INTEGER_MAX {
      return Err(ValidationError::new("run revision exceeds the JavaScript exact integer limit"));
    }
    Ok(Self(value))
  }

  /// Returns the numeric revision.
  pub fn get(self) -> u64 {
    self.0
  }
}

impl<'de> Deserialize<'de> for RunRevision {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
  }
}

/// A bounded non-zero commit page size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PageLimit(NonZeroU32);

impl PageLimit {
  /// Creates a page limit in `1..=1024`.
  pub fn new(value: u32) -> Result<Self, ValidationError> {
    let value = NonZeroU32::new(value).ok_or_else(|| ValidationError::new("page limit must be non-zero"))?;
    if value.get() > MAX_PAGE_LIMIT {
      return Err(ValidationError::new("page limit exceeds 1024 commits"));
    }
    Ok(Self(value))
  }

  /// Returns the non-zero page limit.
  pub fn get(self) -> NonZeroU32 {
    self.0
  }
}

impl<'de> Deserialize<'de> for PageLimit {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
  }
}

/// A string bounded for use as an attribute scalar.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct BoundedString(String);

impl BoundedString {
  /// Creates a string no larger than 1,024 UTF-8 bytes.
  pub fn new(value: impl Into<String>) -> Result<Self, ValidationError> {
    let value = value.into();
    if value.len() > MAX_ATTRIBUTE_STRING_BYTES {
      return Err(ValidationError::new("attribute string exceeds 1024 UTF-8 bytes"));
    }
    Ok(Self(value))
  }

  /// Borrows the validated text.
  pub fn as_str(&self) -> &str {
    &self.0
  }

  /// Consumes the wrapper and returns its text.
  pub fn into_string(self) -> String {
    self.0
  }
}

impl<'de> Deserialize<'de> for BoundedString {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Self::new(String::deserialize(deserializer)?).map_err(de::Error::custom)
  }
}

/// A finite floating-point attribute value.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct FiniteF64(f64);

impl FiniteF64 {
  /// Rejects NaN and positive or negative infinity.
  pub fn new(value: f64) -> Result<Self, ValidationError> {
    if !value.is_finite() {
      return Err(ValidationError::new("floating-point value must be finite"));
    }
    Ok(Self(value))
  }

  /// Returns the finite value.
  pub fn get(self) -> f64 {
    self.0
  }
}

impl<'de> Deserialize<'de> for FiniteF64 {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Self::new(f64::deserialize(deserializer)?).map_err(de::Error::custom)
  }
}

/// A vector that always contains at least one item.
// TODO(run-history-v1): The 256-item mutation/fact bound belongs to the Task 3
// commit constructors; keep this reusable value responsible only for non-emptiness.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NonEmptyVec<T>(Vec<T>);

impl<T> NonEmptyVec<T> {
  /// Rejects an empty vector.
  pub fn new(values: Vec<T>) -> Result<Self, ValidationError> {
    if values.is_empty() {
      return Err(ValidationError::new("vector must not be empty"));
    }
    Ok(Self(values))
  }

  /// Returns the item count, which is always non-zero.
  #[allow(clippy::len_without_is_empty)]
  pub fn len(&self) -> usize {
    self.0.len()
  }

  /// Borrows the items.
  pub fn as_slice(&self) -> &[T] {
    &self.0
  }

  /// Consumes the wrapper and returns the items.
  pub fn into_vec(self) -> Vec<T> {
    self.0
  }
}

impl<'de, T> Deserialize<'de> for NonEmptyVec<T>
where
  T: Deserialize<'de>,
{
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Self::new(Vec::deserialize(deserializer)?).map_err(de::Error::custom)
  }
}

/// A validated lowercase dotted name used by typed AUV values.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NamespacedName(String);

impl NamespacedName {
  /// Parses at least two lowercase ASCII name segments.
  pub fn parse(value: impl Into<String>) -> Result<Self, ValidationError> {
    let value = value.into();
    if value.len() > MAX_NAMESPACED_NAME_BYTES {
      return Err(ValidationError::new("namespaced name exceeds 128 UTF-8 bytes"));
    }

    let mut segments = value.split('.');
    let Some(first) = segments.next() else {
      return Err(ValidationError::new("namespaced name is empty"));
    };
    let Some(second) = segments.next() else {
      return Err(ValidationError::new("namespaced name requires at least two segments"));
    };
    if !valid_name_segment(first) || !valid_name_segment(second) || segments.any(|segment| !valid_name_segment(segment)) {
      return Err(ValidationError::new("namespaced name contains an invalid segment"));
    }
    Ok(Self(value))
  }

  /// Borrows the canonical name.
  pub fn as_str(&self) -> &str {
    &self.0
  }
}

fn valid_name_segment(segment: &str) -> bool {
  let mut bytes = segment.bytes();
  matches!(bytes.next(), Some(b'a'..=b'z')) && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

impl fmt::Display for NamespacedName {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.0)
  }
}

impl FromStr for NamespacedName {
  type Err = ValidationError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    Self::parse(value)
  }
}

impl<'de> Deserialize<'de> for NamespacedName {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Self::parse(String::deserialize(deserializer)?).map_err(de::Error::custom)
  }
}

macro_rules! namespaced_type {
  ($name:ident, $description:literal) => {
    #[doc = $description]
    #[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
    #[serde(transparent)]
    pub struct $name(NamespacedName);

    impl $name {
      /// Parses and validates a namespaced name.
      pub fn parse(value: impl Into<String>) -> Result<Self, ValidationError> {
        NamespacedName::parse(value).map(Self)
      }

      /// Borrows the canonical name.
      pub fn as_str(&self) -> &str {
        self.0.as_str()
      }
    }

    impl fmt::Display for $name {
      fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
      }
    }

    impl FromStr for $name {
      type Err = ValidationError;

      fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
      }
    }

    impl<'de> Deserialize<'de> for $name {
      fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
      where
        D: Deserializer<'de>,
      {
        NamespacedName::deserialize(deserializer).map(Self)
      }
    }
  };
}

namespaced_type!(AttributeKey, "A validated key for bounded attributes.");
namespaced_type!(ErrorCode, "A stable machine-readable error code.");
namespaced_type!(SpanName, "A validated typed span name.");
namespaced_type!(EventName, "A validated typed event name.");
namespaced_type!(ArtifactPurpose, "A stable artifact relationship name.");

/// A concrete canonical MIME content type.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentType(mime::Mime);

impl ContentType {
  /// Parses a concrete canonical MIME value of at most 256 UTF-8 bytes.
  pub fn parse(value: &str) -> Result<Self, ValidationError> {
    let parsed = value.parse::<mime::Mime>().map_err(|_| ValidationError::new("content type is not valid MIME text"))?;
    let canonical = parsed.to_string();
    if canonical != value {
      return Err(ValidationError::new("content type must use canonical MIME text"));
    }
    if canonical.len() > MAX_CONTENT_TYPE_BYTES {
      return Err(ValidationError::new("content type exceeds 256 UTF-8 bytes"));
    }
    if parsed.type_() == mime::STAR || parsed.subtype() == mime::STAR {
      return Err(ValidationError::new("content type must not contain a wildcard"));
    }
    Ok(Self(parsed))
  }

  /// Borrows the parsed MIME value.
  pub fn as_mime(&self) -> &mime::Mime {
    &self.0
  }
}

impl fmt::Display for ContentType {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.0.fmt(formatter)
  }
}

impl FromStr for ContentType {
  type Err = ValidationError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    Self::parse(value)
  }
}

impl Serialize for ContentType {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.collect_str(self)
  }
}

impl<'de> Deserialize<'de> for ContentType {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Self::parse(&String::deserialize(deserializer)?).map_err(de::Error::custom)
  }
}

/// A byte count bounded by the V1 whole-artifact limit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ByteLength(u64);

impl ByteLength {
  /// Creates a byte count no larger than 512 MiB.
  pub fn new(value: u64) -> Result<Self, ValidationError> {
    if value > MAX_ARTIFACT_BYTES {
      return Err(ValidationError::new("byte length exceeds the 512 MiB artifact limit"));
    }
    Ok(Self(value))
  }

  /// Returns the byte count.
  pub fn get(self) -> u64 {
    self.0
  }
}

impl<'de> Deserialize<'de> for ByteLength {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Self::new(u64::deserialize(deserializer)?).map_err(de::Error::custom)
  }
}

/// A SHA-256 digest with canonical lowercase hexadecimal wire text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
  /// Creates a digest from its 32 raw bytes.
  pub fn new(value: [u8; 32]) -> Self {
    Self(value)
  }

  /// Returns the raw digest bytes.
  pub fn as_bytes(&self) -> &[u8; 32] {
    &self.0
  }
}

impl fmt::Display for Sha256Digest {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&hex::encode(self.0))
  }
}

impl FromStr for Sha256Digest {
  type Err = ValidationError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)) {
      return Err(ValidationError::new("SHA-256 digest must be 64 lowercase hexadecimal characters"));
    }
    let mut bytes = [0; 32];
    hex::decode_to_slice(value, &mut bytes).map_err(|_| ValidationError::new("SHA-256 digest is invalid"))?;
    Ok(Self(bytes))
  }
}

impl Serialize for Sha256Digest {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.collect_str(self)
  }
}

impl<'de> Deserialize<'de> for Sha256Digest {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    String::deserialize(deserializer)?.parse().map_err(de::Error::custom)
  }
}

/// A validated wall-clock timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Timestamp {
  unix_seconds: i64,
  nanoseconds: u32,
}

impl Timestamp {
  /// Creates a timestamp with a browser-exact seconds value and valid nanos.
  pub fn new(unix_seconds: i64, nanoseconds: u32) -> Result<Self, ValidationError> {
    if !exact_i64(unix_seconds) {
      return Err(ValidationError::new("timestamp seconds exceed the JavaScript exact integer range"));
    }
    if nanoseconds > 999_999_999 {
      return Err(ValidationError::new("timestamp nanoseconds must not exceed 999999999"));
    }
    Ok(Self {
      unix_seconds,
      nanoseconds,
    })
  }

  /// Returns whole Unix seconds.
  pub fn unix_seconds(self) -> i64 {
    self.unix_seconds
  }

  /// Returns the fractional nanoseconds.
  pub fn nanoseconds(self) -> u32 {
    self.nanoseconds
  }
}

impl<'de> Deserialize<'de> for Timestamp {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      unix_seconds: i64,
      nanoseconds: u32,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.unix_seconds, wire.nanoseconds).map_err(de::Error::custom)
  }
}

/// A bounded scalar attribute value.
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
  /// A boolean scalar.
  Bool(bool),
  /// A browser-exact signed integer.
  I64(i64),
  /// A finite floating-point number.
  F64(FiniteF64),
  /// A bounded UTF-8 string.
  String(BoundedString),
}

impl AttributeValue {
  /// Creates a boolean value.
  pub fn boolean(value: bool) -> Self {
    Self::Bool(value)
  }

  /// Creates a browser-exact integer value.
  pub fn integer(value: i64) -> Result<Self, ValidationError> {
    if !exact_i64(value) {
      return Err(ValidationError::new("attribute integer exceeds the JavaScript exact integer range"));
    }
    Ok(Self::I64(value))
  }

  /// Creates a finite floating-point value.
  pub fn float(value: f64) -> Result<Self, ValidationError> {
    FiniteF64::new(value).map(Self::F64)
  }

  /// Creates a bounded string value.
  pub fn string(value: impl Into<String>) -> Result<Self, ValidationError> {
    BoundedString::new(value).map(Self::String)
  }

  fn validate(&self) -> Result<(), ValidationError> {
    match self {
      Self::Bool(_) => Ok(()),
      Self::I64(value) if exact_i64(*value) => Ok(()),
      Self::I64(_) => Err(ValidationError::new("attribute integer exceeds the JavaScript exact integer range")),
      Self::F64(value) => FiniteF64::new(value.get()).map(|_| ()),
      Self::String(value) => BoundedString::new(value.as_str()).map(|_| ()),
    }
  }
}

impl Serialize for AttributeValue {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    self.validate().map_err(serde::ser::Error::custom)?;
    match self {
      Self::Bool(value) => serializer.serialize_bool(*value),
      Self::I64(value) => serializer.serialize_i64(*value),
      Self::F64(value) => serializer.serialize_f64(value.get()),
      Self::String(value) => serializer.serialize_str(value.as_str()),
    }
  }
}

impl<'de> Deserialize<'de> for AttributeValue {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = Box::<RawValue>::deserialize(deserializer)?;
    attribute_value_from_raw(&raw).map_err(de::Error::custom)
  }
}

fn attribute_value_from_raw(raw: &RawValue) -> Result<AttributeValue, ValidationError> {
  let value = raw.get().trim();
  match value.as_bytes().first() {
    Some(b't' | b'f') => {
      value.parse::<bool>().map(AttributeValue::boolean).map_err(|_| ValidationError::new("attribute boolean is invalid"))
    }
    Some(b'"') => {
      serde_json::from_str::<String>(value).map_err(|_| ValidationError::new("attribute string is invalid")).and_then(AttributeValue::string)
    }
    Some(b'-' | b'0'..=b'9') => attribute_number_from_lexeme(value),
    _ => Err(ValidationError::new("attribute value must be a boolean, integer, finite float, or string")),
  }
}

fn attribute_number_from_lexeme(value: &str) -> Result<AttributeValue, ValidationError> {
  if value.contains(['.', 'e', 'E']) {
    return value.parse::<f64>().map_err(|_| ValidationError::new("attribute float is invalid")).and_then(AttributeValue::float);
  }

  if value.starts_with('-') {
    return value
      .parse::<i64>()
      .map_err(|_| ValidationError::new("attribute integer exceeds the exact range"))
      .and_then(AttributeValue::integer);
  }

  let value = value.parse::<u64>().map_err(|_| ValidationError::new("attribute integer exceeds the exact range"))?;
  let value = i64::try_from(value).map_err(|_| ValidationError::new("attribute integer exceeds the exact range"))?;
  AttributeValue::integer(value)
}

/// A deterministic bounded map of searchable scalar metadata.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(transparent)]
pub struct Attributes(BTreeMap<AttributeKey, AttributeValue>);

impl Attributes {
  /// Returns an empty attributes map.
  pub fn empty() -> Self {
    Self(BTreeMap::new())
  }

  /// Builds an attributes map while rejecting duplicate keys and all bounds.
  pub fn try_from_iter<I>(entries: I) -> Result<Self, ValidationError>
  where
    I: IntoIterator<Item = (AttributeKey, AttributeValue)>,
  {
    let mut values = BTreeMap::new();
    for (key, value) in entries {
      value.validate()?;
      if values.insert(key, value).is_some() {
        return Err(ValidationError::new("attribute keys must be unique"));
      }
      if values.len() > MAX_ATTRIBUTE_COUNT {
        return Err(ValidationError::new("attributes exceed 32 entries"));
      }
    }

    let attributes = Self(values);
    attributes.validate_encoded_size()?;
    Ok(attributes)
  }

  /// Returns the number of attributes.
  pub fn len(&self) -> usize {
    self.0.len()
  }

  /// Reports whether no attributes are present.
  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  /// Returns the value associated with a validated key.
  pub fn get(&self, key: &AttributeKey) -> Option<&AttributeValue> {
    self.0.get(key)
  }

  /// Iterates in canonical key order.
  pub fn iter(&self) -> impl Iterator<Item = (&AttributeKey, &AttributeValue)> {
    self.0.iter()
  }

  fn validate_encoded_size(&self) -> Result<(), ValidationError> {
    let size = serde_json::to_vec(&self.0).map_err(|_| ValidationError::new("attributes could not be encoded"))?.len();
    if size > MAX_ATTRIBUTES_JSON_BYTES {
      return Err(ValidationError::new("attributes exceed 16384 compact JSON bytes"));
    }
    Ok(())
  }
}

impl<'de> Deserialize<'de> for Attributes {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    struct AttributesVisitor;

    impl<'de> Visitor<'de> for AttributesVisitor {
      type Value = Attributes;

      fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a bounded map of namespaced scalar attributes")
      }

      fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
      where
        M: MapAccess<'de>,
      {
        let mut entries = Vec::new();
        let mut seen = BTreeMap::<AttributeKey, ()>::new();
        while let Some(key) = map.next_key::<AttributeKey>()? {
          if seen.insert(key.clone(), ()).is_some() {
            return Err(de::Error::custom("attribute keys must be unique"));
          }
          let value = map.next_value::<AttributeValue>()?;
          entries.push((key, value));
        }
        Attributes::try_from_iter(entries).map_err(de::Error::custom)
      }
    }

    deserializer.deserialize_map(AttributesVisitor)
  }
}

fn exact_i64(value: i64) -> bool {
  value >= -(JAVASCRIPT_EXACT_INTEGER_MAX as i64) && value <= JAVASCRIPT_EXACT_INTEGER_MAX as i64
}
