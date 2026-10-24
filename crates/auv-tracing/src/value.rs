use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

use serde::de;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::value::RawValue;
use uuid::Uuid;

const JAVASCRIPT_EXACT_INTEGER_MAX: u64 = 9_007_199_254_740_991;
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

impl RunId {
  /// Projects a daemon Run id onto its tracing Run identity.
  ///
  /// Daemon Run ids are UUIDs written as 32 hex digits (older stores add a
  /// `run_` prefix); trace records use the canonical hyphenated form of the
  /// same UUID, so a Run's daemon id and trace id name one Run.
  // TODO(resource-id-migration): drop the `run_` prefix once old control
  // stores no longer expose prefixed Run ids.
  pub fn from_daemon_run_id(value: &str) -> Result<Self, ValidationError> {
    let value = value.strip_prefix("run_").unwrap_or(value);
    Self::from_uuid(Uuid::parse_str(value).map_err(|_| ValidationError::new("Run id must be a UUID"))?)
  }
}
uuid_id!(SpanId, "Identifies a span within a run.");
uuid_id!(EventId, "Identifies an immutable event within a run.");
uuid_id!(ArtifactId, "Identifies an artifact within a run.");

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

macro_rules! string_type {
  ($name:ident, $description:literal) => {
    #[doc = $description]
    #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
    #[serde(transparent)]
    pub struct $name(String);

    impl $name {
      pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
      }

      pub fn as_str(&self) -> &str {
        &self.0
      }
    }

    impl fmt::Display for $name {
      fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
      }
    }

    impl From<&str> for $name {
      fn from(value: &str) -> Self {
        Self::new(value)
      }
    }

    impl From<String> for $name {
      fn from(value: String) -> Self {
        Self::new(value)
      }
    }
  };
}

string_type!(ErrorCode, "A stable machine-readable error code.");
string_type!(SpanName, "A typed span name.");
string_type!(EventName, "A typed event name.");
string_type!(ArtifactPurpose, "An artifact relationship name.");
string_type!(ContentType, "A caller-declared artifact content type.");

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

impl fmt::Display for ByteLength {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.0.fmt(formatter)
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

  /// Reads the system clock.
  ///
  /// Records from different processes are ordered by this wall clock, so
  /// cross-process order is only as good as the hosts' clock agreement.
  pub fn now() -> Result<Self, ValidationError> {
    let duration = std::time::SystemTime::now()
      .duration_since(std::time::UNIX_EPOCH)
      .map_err(|_| ValidationError::new("system clock is before the Unix epoch"))?;
    let seconds = i64::try_from(duration.as_secs()).map_err(|_| ValidationError::new("system clock is out of range"))?;
    Self::new(seconds, duration.subsec_nanos())
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

/// A scalar attribute value.
#[derive(Clone, Debug, PartialEq)]
pub enum AttributeValue {
  /// A boolean scalar.
  Bool(bool),
  /// A signed integer.
  I64(i64),
  /// A finite floating-point number.
  F64(FiniteF64),
  /// A UTF-8 string.
  String(String),
}

impl AttributeValue {
  /// Creates a boolean value.
  pub fn boolean(value: bool) -> Self {
    Self::Bool(value)
  }

  /// Creates an integer value.
  pub fn integer(value: i64) -> Self {
    Self::I64(value)
  }

  /// Creates a finite floating-point value.
  pub fn float(value: f64) -> Result<Self, ValidationError> {
    FiniteF64::new(value).map(Self::F64)
  }

  /// Creates a string value.
  pub fn string(value: impl Into<String>) -> Self {
    Self::String(value.into())
  }
}

impl Serialize for AttributeValue {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    match self {
      Self::Bool(value) => serializer.serialize_bool(*value),
      Self::I64(value) => serializer.serialize_i64(*value),
      Self::F64(value) => serializer.serialize_f64(value.get()),
      Self::String(value) => serializer.serialize_str(value),
    }
  }
}

impl<'de> Deserialize<'de> for AttributeValue {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    // NOTICE(json-payload-tagged-read): attributes sit inside the internally
    // tagged `TraceRecord`, whose buffered fields cannot yield a `RawValue`.
    // With `arbitrary_precision`, a `Value` keeps the number lexeme, so the
    // integer/float distinction below survives the detour.
    let value = serde_json::Value::deserialize(deserializer)?;
    let text = serde_json::to_string(&value).map_err(de::Error::custom)?;
    let raw = RawValue::from_string(text).map_err(de::Error::custom)?;
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
      serde_json::from_str::<String>(value).map(AttributeValue::string).map_err(|_| ValidationError::new("attribute string is invalid"))
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
    return value.parse::<i64>().map(AttributeValue::integer).map_err(|_| ValidationError::new("attribute integer is invalid"));
  }

  let value = value.parse::<u64>().map_err(|_| ValidationError::new("attribute integer is invalid"))?;
  let value = i64::try_from(value).map_err(|_| ValidationError::new("attribute integer is invalid"))?;
  Ok(AttributeValue::integer(value))
}

/// A deterministic map of searchable scalar metadata.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Attributes(BTreeMap<String, AttributeValue>);

impl Attributes {
  /// Returns an empty attributes map.
  pub fn empty() -> Self {
    Self(BTreeMap::new())
  }

  /// Builds an attributes map using normal map replacement semantics.
  pub fn from_iter<I, K>(entries: I) -> Self
  where
    I: IntoIterator<Item = (K, AttributeValue)>,
    K: Into<String>,
  {
    Self(entries.into_iter().map(|(key, value)| (key.into(), value)).collect())
  }

  /// Returns the number of attributes.
  pub fn len(&self) -> usize {
    self.0.len()
  }

  /// Reports whether no attributes are present.
  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  /// Returns the value associated with a key.
  pub fn get(&self, key: &str) -> Option<&AttributeValue> {
    self.0.get(key)
  }

  /// Iterates in key order.
  pub fn iter(&self) -> impl Iterator<Item = (&str, &AttributeValue)> {
    self.0.iter().map(|(key, value)| (key.as_str(), value))
  }
}

fn exact_i64(value: i64) -> bool {
  value >= -(JAVASCRIPT_EXACT_INTEGER_MAX as i64) && value <= JAVASCRIPT_EXACT_INTEGER_MAX as i64
}
