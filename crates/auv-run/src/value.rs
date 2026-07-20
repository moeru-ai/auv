//! Validated identifiers, timestamps, payloads, and attributes.

use std::{collections::BTreeMap, fmt, num::NonZeroU32, str::FromStr};

use serde::{
  Deserialize, Deserializer, Serialize, Serializer,
  de::{self, MapAccess, Visitor},
};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, UtcOffset, format_description::well_known::Rfc3339};

const MAX_STRING_BYTES: usize = 4_096;
const MAX_ATTRIBUTE_KEY_BYTES: usize = 128;
const MAX_PAYLOAD_BYTES: usize = 1_048_576;
const MAX_ATTRIBUTE_ENTRIES: usize = 64;
const MAX_ATTRIBUTES_BYTES: usize = 32_768;
const MAX_PAGE_LIMIT: u32 = 1_000;

/// Encodes JSON with recursively sorted object keys and exact JSON numbers.
pub(crate) fn stable_json_bytes<T>(value: &T) -> Result<Vec<u8>, serde_json::Error>
where
  T: Serialize + ?Sized,
{
  let mut value = serde_json::to_value(value)?;
  sort_json_object_keys(&mut value);
  serde_json::to_vec(&value)
}

fn sort_json_object_keys(value: &mut serde_json::Value) {
  match value {
    serde_json::Value::Array(values) => {
      for value in values {
        sort_json_object_keys(value);
      }
    }
    serde_json::Value::Object(values) => {
      for value in values.values_mut() {
        sort_json_object_keys(value);
      }
      values.sort_keys();
    }
    _ => {}
  }
}

/// A stable validation failure for a value in the run contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ValueError {
  #[error("auv.string.too_large")]
  StringTooLarge,
  #[error("auv.float.non_finite")]
  NonFiniteFloat,
  #[error("auv.collection.empty")]
  EmptyCollection,
  #[error("auv.uuid.invalid")]
  InvalidUuid,
  #[error("auv.uuid.nil")]
  NilUuid,
  #[error("auv.name.invalid")]
  InvalidName,
  #[error("auv.name.too_large")]
  NameTooLarge,
  #[error("auv.schema.version_zero")]
  ZeroSchemaVersion,
  #[error("auv.revision.overflow")]
  RevisionOverflow,
  #[error("auv.page_limit.out_of_range")]
  InvalidPageLimit,
  #[error("auv.content_type.invalid")]
  InvalidContentType,
  #[error("auv.digest.invalid")]
  InvalidDigest,
  #[error("auv.timestamp.invalid")]
  InvalidTimestamp,
  #[error("auv.payload.encoding_failed")]
  PayloadEncodingFailed,
  #[error("auv.payload.too_large")]
  PayloadTooLarge,
  #[error("auv.attributes.duplicate_key")]
  DuplicateAttributeKey,
  #[error("auv.attributes.too_many")]
  TooManyAttributes,
  #[error("auv.attributes.encoding_failed")]
  AttributesEncodingFailed,
  #[error("auv.attributes.too_large")]
  AttributesTooLarge,
  #[error("auv.attribute.integer_out_of_range")]
  AttributeIntegerOutOfRange,
}

impl ValueError {
  /// Returns the stable machine-readable code for this failure.
  pub const fn code(self) -> &'static str {
    match self {
      Self::StringTooLarge => "auv.string.too_large",
      Self::NonFiniteFloat => "auv.float.non_finite",
      Self::EmptyCollection => "auv.collection.empty",
      Self::InvalidUuid => "auv.uuid.invalid",
      Self::NilUuid => "auv.uuid.nil",
      Self::InvalidName => "auv.name.invalid",
      Self::NameTooLarge => "auv.name.too_large",
      Self::ZeroSchemaVersion => "auv.schema.version_zero",
      Self::RevisionOverflow => "auv.revision.overflow",
      Self::InvalidPageLimit => "auv.page_limit.out_of_range",
      Self::InvalidContentType => "auv.content_type.invalid",
      Self::InvalidDigest => "auv.digest.invalid",
      Self::InvalidTimestamp => "auv.timestamp.invalid",
      Self::PayloadEncodingFailed => "auv.payload.encoding_failed",
      Self::PayloadTooLarge => "auv.payload.too_large",
      Self::DuplicateAttributeKey => "auv.attributes.duplicate_key",
      Self::TooManyAttributes => "auv.attributes.too_many",
      Self::AttributesEncodingFailed => "auv.attributes.encoding_failed",
      Self::AttributesTooLarge => "auv.attributes.too_large",
      Self::AttributeIntegerOutOfRange => "auv.attribute.integer_out_of_range",
    }
  }
}

/// A UTF-8 string no larger than 4 KiB.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct BoundedString(String);

impl BoundedString {
  /// Validates and constructs a bounded string.
  pub fn new(value: impl Into<String>) -> Result<Self, ValueError> {
    let value = value.into();
    if value.len() > MAX_STRING_BYTES {
      return Err(ValueError::StringTooLarge);
    }
    Ok(Self(value))
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }

  pub fn into_string(self) -> String {
    self.0
  }
}

impl AsRef<str> for BoundedString {
  fn as_ref(&self) -> &str {
    self.as_str()
  }
}

impl fmt::Display for BoundedString {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl Serialize for BoundedString {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(self.as_str())
  }
}

impl<'de> Deserialize<'de> for BoundedString {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::new(raw).map_err(de::Error::custom)
  }
}

/// A finite IEEE-754 double-precision value.
#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
pub struct FiniteF64(f64);

impl FiniteF64 {
  pub fn new(value: f64) -> Result<Self, ValueError> {
    if !value.is_finite() {
      return Err(ValueError::NonFiniteFloat);
    }
    Ok(Self(value))
  }

  pub const fn get(self) -> f64 {
    self.0
  }
}

impl Serialize for FiniteF64 {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_f64(self.get())
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

/// A lowercase ASCII dot-separated protocol name.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct NamespacedName(BoundedString);

impl NamespacedName {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    let valid = !raw.is_empty()
      && raw.split('.').all(|segment| {
        !segment.is_empty() && segment.bytes().all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-'))
      });
    if !valid {
      return Err(ValueError::InvalidName);
    }
    BoundedString::new(raw.to_owned()).map(Self).map_err(|_| ValueError::NameTooLarge)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl FromStr for NamespacedName {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl fmt::Display for NamespacedName {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl Serialize for NamespacedName {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(self.as_str())
  }
}

impl<'de> Deserialize<'de> for NamespacedName {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// A vector that contains at least one value.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct NonEmptyVec<T>(Vec<T>);

impl<T> NonEmptyVec<T> {
  pub fn new(values: Vec<T>) -> Result<Self, ValueError> {
    if values.is_empty() {
      return Err(ValueError::EmptyCollection);
    }
    Ok(Self(values))
  }

  pub fn as_slice(&self) -> &[T] {
    &self.0
  }

  pub fn into_vec(self) -> Vec<T> {
    self.0
  }
}

impl<T> Serialize for NonEmptyVec<T>
where
  T: Serialize,
{
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    self.0.serialize(serializer)
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
    Self::new(Vec::<T>::deserialize(deserializer)?).map_err(de::Error::custom)
  }
}

macro_rules! uuid_id {
  ($name:ident) => {
    #[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
    pub struct $name(uuid::Uuid);

    impl $name {
      // NOTICE: Identity generation is explicit; `Default` would create IDs implicitly.
      #[allow(clippy::new_without_default)]
      pub fn new() -> Self {
        Self(uuid::Uuid::now_v7())
      }

      pub fn parse(raw: &str) -> Result<Self, ValueError> {
        let value = uuid::Uuid::parse_str(raw).map_err(|_| ValueError::InvalidUuid)?;
        if value.is_nil() {
          return Err(ValueError::NilUuid);
        }
        Ok(Self(value))
      }

      pub fn as_uuid(&self) -> &uuid::Uuid {
        &self.0
      }
    }

    impl FromStr for $name {
      type Err = ValueError;

      fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Self::parse(raw)
      }
    }

    impl fmt::Display for $name {
      fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0.hyphenated())
      }
    }

    impl Serialize for $name {
      fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
      where
        S: Serializer,
      {
        serializer.serialize_str(&self.to_string())
      }
    }

    impl<'de> Deserialize<'de> for $name {
      fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
      where
        D: Deserializer<'de>,
      {
        let raw = String::deserialize(deserializer)?;
        let value = Self::parse(&raw).map_err(de::Error::custom)?;
        if raw != value.to_string() {
          return Err(de::Error::custom(ValueError::InvalidUuid));
        }
        Ok(value)
      }
    }
  };
}

uuid_id!(RunId);
uuid_id!(ExecutionId);
uuid_id!(SpanId);
uuid_id!(EventId);
uuid_id!(ArtifactId);
uuid_id!(IdempotencyKey);

fn serialize_name<S>(name: &NamespacedName, serializer: S) -> Result<S::Ok, S::Error>
where
  S: Serializer,
{
  serializer.serialize_str(name.as_str())
}

/// The stable identity of a reusable operation.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct OperationName(NamespacedName);

impl OperationName {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    NamespacedName::parse(raw).map(Self)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl fmt::Display for OperationName {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for OperationName {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for OperationName {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serialize_name(&self.0, serializer)
  }
}

impl<'de> Deserialize<'de> for OperationName {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// A stable machine-readable failure code.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct FailureCode(NamespacedName);

impl FailureCode {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    NamespacedName::parse(raw).map(Self)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl fmt::Display for FailureCode {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for FailureCode {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for FailureCode {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serialize_name(&self.0, serializer)
  }
}

impl<'de> Deserialize<'de> for FailureCode {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// A stable machine-readable reason code.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ReasonCode(NamespacedName);

impl ReasonCode {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    NamespacedName::parse(raw).map(Self)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl fmt::Display for ReasonCode {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for ReasonCode {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for ReasonCode {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serialize_name(&self.0, serializer)
  }
}

impl<'de> Deserialize<'de> for ReasonCode {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// The reason an operation execution was cancelled.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CancellationReason(ReasonCode);

impl CancellationReason {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    ReasonCode::parse(raw).map(Self)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl fmt::Display for CancellationReason {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for CancellationReason {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for CancellationReason {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(self.as_str())
  }
}

impl<'de> Deserialize<'de> for CancellationReason {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// The reason an authority sealed a run.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RunSealReason(ReasonCode);

impl RunSealReason {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    ReasonCode::parse(raw).map(Self)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl fmt::Display for RunSealReason {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for RunSealReason {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for RunSealReason {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(self.as_str())
  }
}

impl<'de> Deserialize<'de> for RunSealReason {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// The declared purpose of an artifact.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ArtifactPurpose(NamespacedName);

impl ArtifactPurpose {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    NamespacedName::parse(raw).map(Self)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl fmt::Display for ArtifactPurpose {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for ArtifactPurpose {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for ArtifactPurpose {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serialize_name(&self.0, serializer)
  }
}

impl<'de> Deserialize<'de> for ArtifactPurpose {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// The stable name of a committed AUV span.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SpanName(NamespacedName);

impl SpanName {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    NamespacedName::parse(raw).map(Self)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl fmt::Display for SpanName {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for SpanName {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for SpanName {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serialize_name(&self.0, serializer)
  }
}

impl<'de> Deserialize<'de> for SpanName {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// The stable name of a committed AUV event.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct EventName(NamespacedName);

impl EventName {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    NamespacedName::parse(raw).map(Self)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl fmt::Display for EventName {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for EventName {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for EventName {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serialize_name(&self.0, serializer)
  }
}

impl<'de> Deserialize<'de> for EventName {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// The stable name of an encoded payload schema.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SchemaName(NamespacedName);

impl SchemaName {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    NamespacedName::parse(raw).map(Self)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl fmt::Display for SchemaName {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for SchemaName {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for SchemaName {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serialize_name(&self.0, serializer)
  }
}

impl<'de> Deserialize<'de> for SchemaName {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// A schema version, beginning at one.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SchemaVersion(u32);

impl SchemaVersion {
  pub fn new(value: u32) -> Result<Self, ValueError> {
    if value == 0 {
      return Err(ValueError::ZeroSchemaVersion);
    }
    Ok(Self(value))
  }

  pub const fn get(self) -> u32 {
    self.0
  }
}

impl Serialize for SchemaVersion {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_u32(self.get())
  }
}

impl<'de> Deserialize<'de> for SchemaVersion {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Self::new(u32::deserialize(deserializer)?).map_err(de::Error::custom)
  }
}

/// A bounded searchable attribute key.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AttributeKey(NamespacedName);

impl AttributeKey {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    if raw.len() > MAX_ATTRIBUTE_KEY_BYTES {
      return Err(ValueError::NameTooLarge);
    }
    NamespacedName::parse(raw).map(Self)
  }

  pub fn as_str(&self) -> &str {
    self.0.as_str()
  }
}

impl fmt::Display for AttributeKey {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for AttributeKey {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for AttributeKey {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serialize_name(&self.0, serializer)
  }
}

impl<'de> Deserialize<'de> for AttributeKey {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// An ordered run commit revision. Zero means no commit exists.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Revision(u64);

impl Revision {
  pub const ZERO: Self = Self(0);

  pub const fn new(value: u64) -> Self {
    Self(value)
  }

  pub const fn get(self) -> u64 {
    self.0
  }

  pub fn next(self) -> Result<Self, ValueError> {
    self.0.checked_add(1).map(Self).ok_or(ValueError::RevisionOverflow)
  }
}

/// A bounded page size for authority reads.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PageLimit(NonZeroU32);

impl PageLimit {
  pub fn new(value: u32) -> Result<Self, ValueError> {
    if !(1..=MAX_PAGE_LIMIT).contains(&value) {
      return Err(ValueError::InvalidPageLimit);
    }
    NonZeroU32::new(value).map(Self).ok_or(ValueError::InvalidPageLimit)
  }

  pub const fn get(self) -> u32 {
    self.0.get()
  }
}

impl Serialize for PageLimit {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_u32(self.get())
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

/// A validated MIME content type with one canonical AUV wire form.
// NOTICE: `mime::Mime` validates syntax but is not retained because its source
// presentation must not define AUV equality, hashing, or serialization.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ContentType(String);

impl ContentType {
  pub fn parse(raw: &str) -> Result<Self, ValueError> {
    let parsed = raw.parse::<mime::Mime>().map_err(|_| ValueError::InvalidContentType)?;
    let mut parameters = BTreeMap::new();

    for (name, value) in parsed.params() {
      let name = name.as_str().to_ascii_lowercase();
      if parameters.insert(name, value.as_str()).is_some() {
        return Err(ValueError::InvalidContentType);
      }
    }

    let mut canonical = parsed.essence_str().to_ascii_lowercase();
    for (name, value) in parameters {
      canonical.push_str("; ");
      canonical.push_str(&name);
      canonical.push('=');
      write_canonical_mime_parameter(&mut canonical, value);
    }

    Ok(Self(canonical))
  }

  pub fn as_str(&self) -> &str {
    &self.0
  }
}

fn write_canonical_mime_parameter(output: &mut String, value: &str) {
  if !value.is_empty() && value.bytes().all(is_mime_token_byte) {
    output.push_str(value);
  } else {
    output.push('"');
    output.push_str(value);
    output.push('"');
  }
}

fn is_mime_token_byte(byte: u8) -> bool {
  byte.is_ascii_alphanumeric()
    || matches!(byte, b'!' | b'#' | b'$' | b'%' | b'&' | b'\'' | b'*' | b'+' | b'-' | b'.' | b'^' | b'_' | b'`' | b'|' | b'~')
}

impl fmt::Display for ContentType {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

impl FromStr for ContentType {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse(raw)
  }
}

impl Serialize for ContentType {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(self.as_str())
  }
}

impl<'de> Deserialize<'de> for ContentType {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse(&raw).map_err(de::Error::custom)
  }
}

/// The length of an artifact in bytes.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ByteLength(u64);

impl ByteLength {
  pub const fn new(value: u64) -> Self {
    Self(value)
  }

  pub const fn get(self) -> u64 {
    self.0
  }
}

/// A SHA-256 digest with a canonical lowercase hexadecimal wire form.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Sha256Digest([u8; 32]);

impl Sha256Digest {
  pub fn of_bytes(bytes: &[u8]) -> Self {
    Self(Sha256::digest(bytes).into())
  }

  pub fn parse_hex(raw: &str) -> Result<Self, ValueError> {
    let canonical = raw.len() == 64 && raw.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !canonical {
      return Err(ValueError::InvalidDigest);
    }

    let mut bytes = [0_u8; 32];
    hex::decode_to_slice(raw, &mut bytes).map_err(|_| ValueError::InvalidDigest)?;
    Ok(Self(bytes))
  }

  pub const fn as_bytes(&self) -> &[u8; 32] {
    &self.0
  }

  pub fn to_hex(self) -> String {
    hex::encode(self.0)
  }
}

impl fmt::Display for Sha256Digest {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.to_hex())
  }
}

impl FromStr for Sha256Digest {
  type Err = ValueError;

  fn from_str(raw: &str) -> Result<Self, Self::Err> {
    Self::parse_hex(raw)
  }
}

impl Serialize for Sha256Digest {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.serialize_str(&self.to_hex())
  }
}

impl<'de> Deserialize<'de> for Sha256Digest {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    Self::parse_hex(&raw).map_err(de::Error::custom)
  }
}

/// A UTC instant stored as Unix seconds and nanoseconds.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Timestamp {
  unix_seconds: i64,
  nanoseconds: u32,
}

impl Timestamp {
  pub fn new(unix_seconds: i64, nanoseconds: u32) -> Result<Self, ValueError> {
    if nanoseconds >= 1_000_000_000 {
      return Err(ValueError::InvalidTimestamp);
    }
    let value = OffsetDateTime::from_unix_timestamp(unix_seconds)
      .and_then(|value| value.replace_nanosecond(nanoseconds))
      .map_err(|_| ValueError::InvalidTimestamp)?;
    value.format(&Rfc3339).map_err(|_| ValueError::InvalidTimestamp)?;
    Ok(Self {
      unix_seconds,
      nanoseconds,
    })
  }

  pub const fn unix_seconds(self) -> i64 {
    self.unix_seconds
  }

  pub const fn nanoseconds(self) -> u32 {
    self.nanoseconds
  }

  fn as_offset_date_time(self) -> Result<OffsetDateTime, ValueError> {
    OffsetDateTime::from_unix_timestamp(self.unix_seconds)
      .and_then(|value| value.replace_nanosecond(self.nanoseconds))
      .map_err(|_| ValueError::InvalidTimestamp)
  }
}

impl Serialize for Timestamp {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    let encoded = self
      .as_offset_date_time()
      .and_then(|value| value.format(&Rfc3339).map_err(|_| ValueError::InvalidTimestamp))
      .map_err(serde::ser::Error::custom)?;
    serializer.serialize_str(&encoded)
  }
}

impl<'de> Deserialize<'de> for Timestamp {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    let raw = String::deserialize(deserializer)?;
    let value = OffsetDateTime::parse(&raw, &Rfc3339).map_err(de::Error::custom)?.to_offset(UtcOffset::UTC);
    Self::new(value.unix_timestamp(), value.nanosecond()).map_err(de::Error::custom)
  }
}

/// The identity of an encoded payload's schema.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PayloadSchema {
  name: SchemaName,
  version: SchemaVersion,
}

impl PayloadSchema {
  pub fn new(name: SchemaName, version: SchemaVersion) -> Self {
    Self { name, version }
  }

  pub fn parse(name: &str, version: u32) -> Result<Self, ValueError> {
    Ok(Self::new(SchemaName::parse(name)?, SchemaVersion::new(version)?))
  }

  pub fn name(&self) -> &SchemaName {
    &self.name
  }

  pub const fn version(&self) -> SchemaVersion {
    self.version
  }
}

/// A structurally bounded JSON payload paired with its declared schema.
///
/// Construction validates the stable wire size, not semantic conformance to
/// the schema. `PayloadCodec` and operation boundaries own semantic checks.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct EncodedPayload {
  schema: PayloadSchema,
  data: serde_json::Value,
}

impl EncodedPayload {
  /// Constructs a payload whose stable JSON data is at most one MiB.
  pub fn new(schema: PayloadSchema, data: serde_json::Value) -> Result<Self, ValueError> {
    let encoded = stable_json_bytes(&data).map_err(|_| ValueError::PayloadEncodingFailed)?;
    if encoded.len() > MAX_PAYLOAD_BYTES {
      return Err(ValueError::PayloadTooLarge);
    }
    Ok(Self { schema, data })
  }

  pub fn schema(&self) -> &PayloadSchema {
    &self.schema
  }

  pub fn data(&self) -> &serde_json::Value {
    &self.data
  }

  pub fn into_parts(self) -> (PayloadSchema, serde_json::Value) {
    (self.schema, self.data)
  }
}

impl<'de> Deserialize<'de> for EncodedPayload {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      schema: PayloadSchema,
      data: serde_json::Value,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.schema, wire.data).map_err(de::Error::custom)
  }
}

/// A scalar value suitable for searchable run metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum AttributeValue {
  Bool(bool),
  I64(i64),
  F64(FiniteF64),
  String(BoundedString),
}

impl<'de> Deserialize<'de> for AttributeValue {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    match serde_json::Value::deserialize(deserializer)? {
      serde_json::Value::Bool(value) => Ok(Self::Bool(value)),
      serde_json::Value::String(value) => BoundedString::new(value).map(Self::String).map_err(de::Error::custom),
      serde_json::Value::Number(number) if number.is_i64() => {
        number.as_i64().map(Self::I64).ok_or_else(|| de::Error::custom(ValueError::AttributeIntegerOutOfRange))
      }
      serde_json::Value::Number(number) if number.is_f64() => {
        number.as_f64().ok_or(ValueError::NonFiniteFloat).and_then(FiniteF64::new).map(Self::F64).map_err(de::Error::custom)
      }
      serde_json::Value::Number(number) => {
        let uses_float_syntax = number.as_str().bytes().any(|byte| matches!(byte, b'.' | b'e' | b'E'));
        let error = if uses_float_syntax {
          ValueError::NonFiniteFloat
        } else {
          ValueError::AttributeIntegerOutOfRange
        };
        Err(de::Error::custom(error))
      }
      _ => Err(de::Error::custom("attribute values must be booleans, signed 64-bit integers, finite floats, or bounded strings")),
    }
  }
}

/// A bounded map of searchable run metadata.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Attributes(BTreeMap<AttributeKey, AttributeValue>);

impl Attributes {
  pub fn try_from_iter<I>(entries: I) -> Result<Self, ValueError>
  where
    I: IntoIterator<Item = (AttributeKey, AttributeValue)>,
  {
    let mut values = BTreeMap::new();
    for (key, value) in entries {
      Self::insert_entry(&mut values, key, value)?;
    }

    Self::from_map(values)
  }

  fn insert_entry(values: &mut BTreeMap<AttributeKey, AttributeValue>, key: AttributeKey, value: AttributeValue) -> Result<(), ValueError> {
    Self::validate_entry_key(values, &key)?;
    values.insert(key, value);
    Ok(())
  }

  fn validate_entry_key(values: &BTreeMap<AttributeKey, AttributeValue>, key: &AttributeKey) -> Result<(), ValueError> {
    if values.contains_key(key) {
      return Err(ValueError::DuplicateAttributeKey);
    }
    if values.len() == MAX_ATTRIBUTE_ENTRIES {
      return Err(ValueError::TooManyAttributes);
    }
    Ok(())
  }

  fn from_map(values: BTreeMap<AttributeKey, AttributeValue>) -> Result<Self, ValueError> {
    let encoded = stable_json_bytes(&values).map_err(|_| ValueError::AttributesEncodingFailed)?;
    if encoded.len() > MAX_ATTRIBUTES_BYTES {
      return Err(ValueError::AttributesTooLarge);
    }
    Ok(Self(values))
  }

  pub fn len(&self) -> usize {
    self.0.len()
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  pub fn get(&self, key: &AttributeKey) -> Option<&AttributeValue> {
    self.0.get(key)
  }

  pub fn iter(&self) -> impl Iterator<Item = (&AttributeKey, &AttributeValue)> {
    self.0.iter()
  }
}

impl Serialize for Attributes {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    self.0.serialize(serializer)
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
        formatter.write_str("a bounded map of run attributes")
      }

      fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
      where
        A: MapAccess<'de>,
      {
        let mut values = BTreeMap::new();
        while let Some(key) = map.next_key::<AttributeKey>()? {
          Attributes::validate_entry_key(&values, &key).map_err(de::Error::custom)?;
          let value = map.next_value::<AttributeValue>()?;
          values.insert(key, value);
        }
        Attributes::from_map(values).map_err(de::Error::custom)
      }
    }

    deserializer.deserialize_map(AttributesVisitor)
  }
}

#[cfg(test)]
mod tests {
  use super::stable_json_bytes;

  #[test]
  fn stable_json_recursively_sorts_object_keys() {
    let value: serde_json::Value = serde_json::from_str(r#"{"z":{"b":1,"a":2},"a":[{"d":4,"c":3},{"b":2,"a":1}]}"#).unwrap();

    assert_eq!(stable_json_bytes(&value).unwrap(), br#"{"a":[{"c":3,"d":4},{"a":1,"b":2}],"z":{"a":2,"b":1}}"#,);
  }

  #[test]
  fn stable_json_preserves_exact_number_representations() {
    let raw_numbers = [
      "9007199254740992",
      "9007199254740993",
      "9223372036854775807",
      "1e400",
    ];
    let (number_representations, stable_numbers): (Vec<_>, Vec<_>) = raw_numbers
      .into_iter()
      .map(|raw| {
        let value: serde_json::Value = serde_json::from_str(raw).unwrap();
        let representation = value.as_number().unwrap().as_str().to_owned();
        let stable = String::from_utf8(stable_json_bytes(&value).unwrap()).unwrap();
        (representation, stable)
      })
      .unzip();

    assert_eq!(stable_numbers, number_representations);
    assert_eq!(&stable_numbers[..3], &raw_numbers[..3]);
    assert_eq!(stable_numbers[3], "1e+400");
  }
}
