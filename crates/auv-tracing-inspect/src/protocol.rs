use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use auv_tracing::{
  ArtifactId, ArtifactPurpose, ArtifactUri, Attributes, AuthorityId, ByteLength, ContentType, ErrorCode, IdempotencyKey, NonEmptyVec,
  RunMutation, RunRevision, Sha256Digest, SpanId, Timestamp,
};
use serde::de::{self, DeserializeOwned};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Number;
use url::Url;
use uuid::Uuid;

const MAX_JSON_NESTING: usize = 128;
const MAX_JSON_OBJECT_MEMBERS: usize = 8_192;
const JAVASCRIPT_EXACT_INTEGER_MAX: u64 = 9_007_199_254_740_991;

/// Versioned media type for Inspect run JSON requests and responses.
pub const RUN_MEDIA_TYPE: &str = "application/vnd.auv.run+json; version=1";

/// Versioned media type for Inspect artifact upload metadata and errors.
pub const ARTIFACT_UPLOAD_MEDIA_TYPE: &str = "application/vnd.auv.artifact-upload+json; version=1";

/// Media type for the Inspect-specific artifact resolver.
pub const ARTIFACT_RESOLVE_MEDIA_TYPE: &str = "application/json";

/// Strict authority response header carrying the trusted public artifact base.
pub const ARTIFACT_ORIGIN_HEADER: &str = "Auv-Artifact-Origin";

/// Generation-bound control header used to grant one artifact body upload.
///
/// Inspect V1 requires exactly one canonical admission ID on every artifact
/// draft POST and content PUT. A missing value is a protocol precondition
/// failure, while an equal replay may return [`ARTIFACT_UPLOAD_ADMISSION_BUSY`].
pub const ARTIFACT_UPLOAD_ADMISSION_HEADER: &str = "Auv-Artifact-Upload-Admission";

/// Response value used when an equal draft replay does not own body admission.
pub const ARTIFACT_UPLOAD_ADMISSION_BUSY: &str = "busy";

/// Inspect V1 admission lease in seconds.
pub const ARTIFACT_UPLOAD_ADMISSION_LEASE_SECONDS: u64 = 30;

/// Stable error code returned when the required admission header is absent.
pub const ARTIFACT_UPLOAD_ADMISSION_REQUIRED_ERROR: &str = "auv.inspect.upload_admission_required";

/// Stable wire code for reuse of an idempotency key with different content.
pub const IDEMPOTENCY_MISMATCH_ERROR: &str = "auv.inspect.idempotency_mismatch";

/// Stable wire code for reuse of an artifact identity by a different write.
pub const ARTIFACT_IDENTITY_CONFLICT_ERROR: &str = "auv.inspect.artifact_identity_conflict";

/// Strict response header carrying the authority currently serving a request.
pub const AUTHORITY_ID_HEADER: &str = "Auv-Authority-Id";

const MAX_RESOLVED_ARTIFACTS: usize = 256;

/// The stable identity returned by an Inspect authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorityResponse {
  pub authority_id: AuthorityId,
}

/// Path-independent body for one ordinary run commit request.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunCommitBody {
  pub authority_id: AuthorityId,
  pub mutations: NonEmptyVec<RunMutation>,
}

/// Recoverable SSE history boundary emitted before the stream closes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunStreamGap {
  pub requested_after: RunRevision,
  pub earliest_available: RunRevision,
}

/// Metadata accepted before Inspect consumes a one-shot artifact body.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactUploadDraftRequest {
  authority_id: AuthorityId,
  artifact_id: ArtifactId,
  span_id: Option<SpanId>,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  byte_length: ByteLength,
  sha256: Sha256Digest,
  attributes: Attributes,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArtifactUploadDraftRequest {
  authority_id: AuthorityId,
  artifact_id: ArtifactId,
  span_id: Option<SpanId>,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  byte_length: u64,
  sha256: Sha256Digest,
  attributes: Attributes,
}

impl TryFrom<RawArtifactUploadDraftRequest> for ArtifactUploadDraftRequest {
  type Error = auv_tracing::ValidationError;

  fn try_from(wire: RawArtifactUploadDraftRequest) -> Result<Self, Self::Error> {
    Ok(Self::new(
      wire.authority_id,
      wire.artifact_id,
      wire.span_id,
      wire.purpose,
      wire.content_type,
      ByteLength::new(wire.byte_length)?,
      wire.sha256,
      wire.attributes,
    ))
  }
}

impl<'de> Deserialize<'de> for ArtifactUploadDraftRequest {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    RawArtifactUploadDraftRequest::deserialize(deserializer)?.try_into().map_err(de::Error::custom)
  }
}

impl ArtifactUploadDraftRequest {
  /// Creates upload metadata from validated run-contract values.
  #[allow(clippy::too_many_arguments)]
  pub fn new(
    authority_id: AuthorityId,
    artifact_id: ArtifactId,
    span_id: Option<SpanId>,
    purpose: ArtifactPurpose,
    content_type: ContentType,
    byte_length: ByteLength,
    sha256: Sha256Digest,
    attributes: Attributes,
  ) -> Self {
    Self {
      authority_id,
      artifact_id,
      span_id,
      purpose,
      content_type,
      byte_length,
      sha256,
      attributes,
    }
  }

  pub fn authority_id(&self) -> AuthorityId {
    self.authority_id
  }

  pub fn artifact_id(&self) -> ArtifactId {
    self.artifact_id
  }

  pub fn span_id(&self) -> Option<SpanId> {
    self.span_id
  }

  pub fn purpose(&self) -> &ArtifactPurpose {
    &self.purpose
  }

  pub fn content_type(&self) -> &ContentType {
    &self.content_type
  }

  pub fn byte_length(&self) -> ByteLength {
    self.byte_length
  }

  pub fn sha256(&self) -> Sha256Digest {
    self.sha256
  }

  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }
}

/// Identifies one temporary Inspect upload resource.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ArtifactUploadId(Uuid);

impl ArtifactUploadId {
  /// Generates a non-nil UUIDv7 upload identity.
  pub fn new() -> Self {
    Self(Uuid::now_v7())
  }

  /// Uses an idempotency key's UUID bytes as the reversible upload identity.
  pub fn from_idempotency_key(idempotency_key: IdempotencyKey) -> Self {
    Self(*idempotency_key.as_uuid())
  }

  /// Recovers the idempotency key represented by this upload identity.
  pub fn to_idempotency_key(self) -> IdempotencyKey {
    self.to_string().parse().expect("a non-nil upload UUID is a valid idempotency key")
  }
}

impl Default for ArtifactUploadId {
  fn default() -> Self {
    Self::new()
  }
}

impl fmt::Display for ArtifactUploadId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.0.fmt(formatter)
  }
}

impl FromStr for ArtifactUploadId {
  type Err = ArtifactUploadIdError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    let uuid = Uuid::parse_str(value).map_err(|_| ArtifactUploadIdError)?;
    if uuid.is_nil() || uuid.hyphenated().to_string() != value {
      return Err(ArtifactUploadIdError);
    }
    Ok(Self(uuid))
  }
}

impl Serialize for ArtifactUploadId {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: Serializer,
  {
    serializer.collect_str(self)
  }
}

impl<'de> Deserialize<'de> for ArtifactUploadId {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    String::deserialize(deserializer)?.parse().map_err(de::Error::custom)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("artifact upload ID must be a non-nil canonical UUID")]
pub struct ArtifactUploadIdError;

/// One client-generated capability for a single upload admission generation.
#[derive(Clone, Copy, Eq)]
pub struct ArtifactUploadAdmissionId(Uuid);

impl ArtifactUploadAdmissionId {
  /// Generates a non-nil UUIDv7 admission capability.
  pub fn new() -> Self {
    Self(Uuid::now_v7())
  }

  /// Compares capability bytes without data-dependent early return.
  pub fn matches(self, other: Self) -> bool {
    self.0.as_bytes().iter().zip(other.0.as_bytes()).fold(0_u8, |difference, (left, right)| difference | (left ^ right)) == 0
  }
}

impl Default for ArtifactUploadAdmissionId {
  fn default() -> Self {
    Self::new()
  }
}

impl PartialEq for ArtifactUploadAdmissionId {
  fn eq(&self, other: &Self) -> bool {
    self.matches(*other)
  }
}

impl fmt::Debug for ArtifactUploadAdmissionId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("ArtifactUploadAdmissionId([REDACTED])")
  }
}

impl fmt::Display for ArtifactUploadAdmissionId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    self.0.fmt(formatter)
  }
}

impl FromStr for ArtifactUploadAdmissionId {
  type Err = ArtifactUploadAdmissionIdError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    let uuid = Uuid::parse_str(value).map_err(|_| ArtifactUploadAdmissionIdError)?;
    if uuid.is_nil() || uuid.hyphenated().to_string() != value {
      return Err(ArtifactUploadAdmissionIdError);
    }
    Ok(Self(uuid))
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("artifact upload admission ID must be a non-nil canonical UUID")]
pub struct ArtifactUploadAdmissionIdError;

/// One temporary upload locator returned by Inspect.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactUploadDraft {
  upload_id: ArtifactUploadId,
  artifact_uri: ArtifactUri,
  expires_at: Timestamp,
}

impl ArtifactUploadDraft {
  pub fn new(upload_id: ArtifactUploadId, artifact_uri: ArtifactUri, expires_at: Timestamp) -> Self {
    Self {
      upload_id,
      artifact_uri,
      expires_at,
    }
  }

  pub fn upload_id(&self) -> ArtifactUploadId {
    self.upload_id
  }

  pub fn artifact_uri(&self) -> &ArtifactUri {
    &self.artifact_uri
  }

  pub fn expires_at(&self) -> Timestamp {
    self.expires_at
  }
}

/// One validated Inspect-specific artifact resolution request.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveArtifactsRequest {
  authority_id: AuthorityId,
  uris: Vec<ArtifactUri>,
}

impl ResolveArtifactsRequest {
  pub fn new(authority_id: AuthorityId, uris: Vec<ArtifactUri>) -> Result<Self, ResolveArtifactsRequestError> {
    if uris.len() > MAX_RESOLVED_ARTIFACTS {
      return Err(ResolveArtifactsRequestError);
    }
    Ok(Self { authority_id, uris })
  }

  pub fn authority_id(&self) -> AuthorityId {
    self.authority_id
  }

  pub fn uris(&self) -> &[ArtifactUri] {
    &self.uris
  }
}

impl<'de> Deserialize<'de> for ResolveArtifactsRequest {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
      authority_id: AuthorityId,
      uris: Vec<ArtifactUri>,
    }

    let wire = Wire::deserialize(deserializer)?;
    Self::new(wire.authority_id, wire.uris).map_err(de::Error::custom)
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("artifact resolution batch exceeds 256 URIs")]
pub struct ResolveArtifactsRequestError;

/// One position-preserving result from the Inspect artifact resolver.
// NOTICE(inspect-resolver-wire-v1): The accepted public DTO stores `Url`
// directly. Remove this lint allowance only with a versioned protocol change.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedArtifact {
  Available {
    uri: ArtifactUri,
    content_type: ContentType,
    byte_length: ByteLength,
    sha256: Sha256Digest,
    content_url: Url,
  },
  NotFound {
    uri: ArtifactUri,
  },
}

/// Position-preserving results for one validated resolution batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResolveArtifactsResponse {
  results: Vec<ResolvedArtifact>,
}

impl ResolveArtifactsResponse {
  pub fn new(results: Vec<ResolvedArtifact>) -> Self {
    Self { results }
  }

  pub fn results(&self) -> &[ResolvedArtifact] {
    &self.results
  }

  pub fn into_results(self) -> Vec<ResolvedArtifact> {
    self.results
  }
}

/// Exact V1 artifact endpoint error body.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactApiError {
  pub error: ErrorCode,
}

impl ArtifactApiError {
  pub fn error(&self) -> &ErrorCode {
    &self.error
  }
}

/// Typed error body shared by Inspect run protocol adapters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum RunApiError {
  NotFound,
  Forbidden,
  InvalidReference {
    code: ErrorCode,
  },
  AuthorityMismatch {
    expected: AuthorityId,
    received: AuthorityId,
  },
  IdempotencyMismatch,
  Rejected {
    code: ErrorCode,
  },
  HistoryGap {
    requested_after: RunRevision,
    earliest_available: RunRevision,
  },
  CursorAhead {
    requested_after: RunRevision,
    latest: RunRevision,
  },
  Integrity {
    code: ErrorCode,
  },
  Unavailable {
    code: ErrorCode,
  },
}

/// Reports whether strict draft decoding failed at the V1 byte-size boundary.
#[derive(Debug, thiserror::Error)]
#[error("invalid artifact upload draft request")]
pub struct ArtifactUploadDraftRequestDecodeError {
  payload_too_large: bool,
}

impl ArtifactUploadDraftRequestDecodeError {
  pub fn is_payload_too_large(&self) -> bool {
    self.payload_too_large
  }
}

/// Strictly decodes draft metadata while preserving the 413 size classification.
pub fn decode_artifact_upload_draft_request(bytes: &[u8]) -> Result<ArtifactUploadDraftRequest, ArtifactUploadDraftRequestDecodeError> {
  let wire = decode_strict::<RawArtifactUploadDraftRequest>(bytes).map_err(|_| ArtifactUploadDraftRequestDecodeError {
    payload_too_large: false,
  })?;
  ArtifactUploadDraftRequest::try_from(wire).map_err(|_| ArtifactUploadDraftRequestDecodeError {
    payload_too_large: true,
  })
}

/// Reports malformed JSON, duplicate object keys, or a typed DTO mismatch.
#[derive(Debug, thiserror::Error)]
#[error("invalid Inspect protocol JSON: {message}")]
pub struct ProtocolDecodeError {
  message: String,
}

impl ProtocolDecodeError {
  fn new(message: impl Into<String>) -> Self {
    Self {
      message: message.into(),
    }
  }
}

/// Decodes one strict protocol DTO without first coercing it through a JSON value.
pub fn decode_strict<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, ProtocolDecodeError> {
  validate_json_structure(bytes)?;
  serde_json::from_slice(bytes).map_err(protocol_json_error)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StructureStats {
  max_depth: usize,
  scanned_bytes: usize,
  peak_retained_bytes: usize,
}

fn validate_json_structure(bytes: &[u8]) -> Result<StructureStats, ProtocolDecodeError> {
  JsonStructureScanner::new(bytes).scan()
}

fn protocol_json_error(error: serde_json::Error) -> ProtocolDecodeError {
  ProtocolDecodeError::new(error.to_string())
}

struct JsonStructureScanner<'a> {
  bytes: &'a [u8],
  cursor: usize,
  retained_bytes: usize,
  stats: StructureStats,
}

impl<'a> JsonStructureScanner<'a> {
  fn new(bytes: &'a [u8]) -> Self {
    Self {
      bytes,
      cursor: 0,
      retained_bytes: 0,
      stats: StructureStats::default(),
    }
  }

  fn scan(mut self) -> Result<StructureStats, ProtocolDecodeError> {
    self.skip_whitespace();
    if self.cursor == self.bytes.len() {
      return Err(ProtocolDecodeError::new("JSON is empty"));
    }
    self.scan_value(0)?;
    self.skip_whitespace();
    if self.cursor != self.bytes.len() {
      return Err(ProtocolDecodeError::new("JSON has trailing input"));
    }
    self.stats.scanned_bytes = self.cursor;
    Ok(self.stats)
  }

  fn scan_value(&mut self, depth: usize) -> Result<(), ProtocolDecodeError> {
    self.skip_whitespace();
    match self.current_byte() {
      Some(b'{') => {
        let depth = self.container_depth(depth)?;
        self.scan_object(depth)
      }
      Some(b'[') => {
        let depth = self.container_depth(depth)?;
        self.scan_array(depth)
      }
      Some(b'"') => self.scan_string().map(|_| ()),
      Some(b't') => self.consume_exact(b"true"),
      Some(b'f') => self.consume_exact(b"false"),
      Some(b'n') => self.consume_exact(b"null"),
      Some(b'-' | b'0'..=b'9') => self.scan_number(),
      Some(_) => Err(ProtocolDecodeError::new("JSON has an invalid leading token")),
      None => Err(ProtocolDecodeError::new("JSON ended before a value")),
    }
  }

  fn scan_object(&mut self, depth: usize) -> Result<(), ProtocolDecodeError> {
    self.expect_byte(b'{')?;
    self.skip_whitespace();
    if self.consume_if(b'}') {
      return Ok(());
    }

    let mut keys = HashSet::new();
    let mut retained_key_bytes = 0;
    loop {
      if keys.len() == MAX_JSON_OBJECT_MEMBERS {
        return Err(ProtocolDecodeError::new(format!("JSON object exceeds {MAX_JSON_OBJECT_MEMBERS} members")));
      }

      self.skip_whitespace();
      let key_start = self.cursor;
      let key_end = self.scan_string()?;
      let key = serde_json::from_slice::<String>(&self.bytes[key_start..key_end]).map_err(protocol_json_error)?;
      if keys.contains(&key) {
        return Err(ProtocolDecodeError::new(format!("duplicate JSON object key `{key}`")));
      }
      retained_key_bytes += key.capacity();
      self.retained_bytes += key.capacity();
      self.stats.peak_retained_bytes = self.stats.peak_retained_bytes.max(self.retained_bytes);
      keys.insert(key);

      self.skip_whitespace();
      self.expect_byte(b':')?;
      self.scan_value(depth)?;
      self.skip_whitespace();
      if self.consume_if(b'}') {
        self.retained_bytes -= retained_key_bytes;
        return Ok(());
      }
      self.expect_byte(b',')?;
    }
  }

  fn scan_array(&mut self, depth: usize) -> Result<(), ProtocolDecodeError> {
    self.expect_byte(b'[')?;
    self.skip_whitespace();
    if self.consume_if(b']') {
      return Ok(());
    }

    loop {
      self.scan_value(depth)?;
      self.skip_whitespace();
      if self.consume_if(b']') {
        return Ok(());
      }
      self.expect_byte(b',')?;
    }
  }

  fn scan_string(&mut self) -> Result<usize, ProtocolDecodeError> {
    self.expect_byte(b'"')?;
    let mut segment_start = self.cursor;
    loop {
      match self.current_byte() {
        Some(b'"') => {
          self.validate_utf8(segment_start, self.cursor)?;
          self.cursor += 1;
          return Ok(self.cursor);
        }
        Some(b'\\') => {
          self.validate_utf8(segment_start, self.cursor)?;
          self.cursor += 1;
          match self.current_byte() {
            Some(b'"' | b'\\' | b'/' | b'b' | b'f' | b'n' | b'r' | b't') => self.cursor += 1,
            Some(b'u') => {
              self.cursor += 1;
              let code_unit = self.scan_hex_quad()?;
              if (0xd800..=0xdbff).contains(&code_unit) {
                if self.bytes.get(self.cursor..self.cursor + 2) != Some(b"\\u") {
                  return Err(ProtocolDecodeError::new("JSON string has an unpaired UTF-16 surrogate"));
                }
                self.cursor += 2;
                let low = self.scan_hex_quad()?;
                if !(0xdc00..=0xdfff).contains(&low) {
                  return Err(ProtocolDecodeError::new("JSON string has an unpaired UTF-16 surrogate"));
                }
              } else if (0xdc00..=0xdfff).contains(&code_unit) {
                return Err(ProtocolDecodeError::new("JSON string has an unpaired UTF-16 surrogate"));
              }
            }
            _ => return Err(ProtocolDecodeError::new("JSON string has an invalid escape")),
          }
          segment_start = self.cursor;
        }
        Some(0x00..=0x1f) => return Err(ProtocolDecodeError::new("JSON string contains an unescaped control character")),
        Some(_) => self.cursor += 1,
        None => return Err(ProtocolDecodeError::new("JSON string is not terminated")),
      }
    }
  }

  fn scan_number(&mut self) -> Result<(), ProtocolDecodeError> {
    let start = self.cursor;
    self.consume_if(b'-');

    match self.current_byte() {
      Some(b'0') => {
        self.cursor += 1;
        if matches!(self.current_byte(), Some(b'0'..=b'9')) {
          return Err(ProtocolDecodeError::new("JSON number has a leading zero"));
        }
      }
      Some(b'1'..=b'9') => {
        self.cursor += 1;
        self.consume_digits();
      }
      _ => return Err(ProtocolDecodeError::new("JSON number has no integer digits")),
    }

    if self.consume_if(b'.') {
      if !matches!(self.current_byte(), Some(b'0'..=b'9')) {
        return Err(ProtocolDecodeError::new("JSON number has no fractional digits"));
      }
      self.consume_digits();
    }
    if matches!(self.current_byte(), Some(b'e' | b'E')) {
      self.cursor += 1;
      if matches!(self.current_byte(), Some(b'+' | b'-')) {
        self.cursor += 1;
      }
      if !matches!(self.current_byte(), Some(b'0'..=b'9')) {
        return Err(ProtocolDecodeError::new("JSON number has no exponent digits"));
      }
      self.consume_digits();
    }

    let lexeme = std::str::from_utf8(&self.bytes[start..self.cursor]).expect("JSON number scanner only accepts ASCII bytes");
    parse_number_lexeme(lexeme).map(|_| ())
  }

  fn scan_hex_quad(&mut self) -> Result<u16, ProtocolDecodeError> {
    let mut value = 0_u16;
    for _ in 0..4 {
      let digit = match self.current_byte() {
        Some(b'0'..=b'9') => self.current_byte().unwrap() - b'0',
        Some(b'a'..=b'f') => self.current_byte().unwrap() - b'a' + 10,
        Some(b'A'..=b'F') => self.current_byte().unwrap() - b'A' + 10,
        _ => return Err(ProtocolDecodeError::new("JSON string has an invalid Unicode escape")),
      };
      value = (value << 4) | u16::from(digit);
      self.cursor += 1;
    }
    Ok(value)
  }

  fn container_depth(&mut self, depth: usize) -> Result<usize, ProtocolDecodeError> {
    let depth = depth + 1;
    if depth > MAX_JSON_NESTING {
      return Err(ProtocolDecodeError::new(format!("JSON exceeds {MAX_JSON_NESTING} nested containers")));
    }
    self.stats.max_depth = self.stats.max_depth.max(depth);
    Ok(depth)
  }

  fn validate_utf8(&self, start: usize, end: usize) -> Result<(), ProtocolDecodeError> {
    std::str::from_utf8(&self.bytes[start..end]).map(|_| ()).map_err(|_| ProtocolDecodeError::new("JSON string is not valid UTF-8"))
  }

  fn consume_digits(&mut self) {
    while matches!(self.current_byte(), Some(b'0'..=b'9')) {
      self.cursor += 1;
    }
  }

  fn consume_exact(&mut self, expected: &[u8]) -> Result<(), ProtocolDecodeError> {
    if self.bytes.get(self.cursor..self.cursor + expected.len()) != Some(expected) {
      return Err(ProtocolDecodeError::new("JSON literal is invalid"));
    }
    self.cursor += expected.len();
    Ok(())
  }

  fn expect_byte(&mut self, expected: u8) -> Result<(), ProtocolDecodeError> {
    if !self.consume_if(expected) {
      return Err(ProtocolDecodeError::new(format!("JSON expected `{}`", char::from(expected))));
    }
    Ok(())
  }

  fn consume_if(&mut self, expected: u8) -> bool {
    if self.current_byte() == Some(expected) {
      self.cursor += 1;
      true
    } else {
      false
    }
  }

  fn skip_whitespace(&mut self) {
    while matches!(self.current_byte(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
      self.cursor += 1;
    }
  }

  fn current_byte(&self) -> Option<u8> {
    self.bytes.get(self.cursor).copied()
  }
}

fn parse_number_lexeme(value: &str) -> Result<Number, ProtocolDecodeError> {
  if value.contains(['.', 'e', 'E']) {
    let value = value.parse::<f64>().map_err(|_| ProtocolDecodeError::new("JSON number is not representable as a finite float"))?;
    return Number::from_f64(value).ok_or_else(|| ProtocolDecodeError::new("JSON floating-point value must be finite"));
  }

  if value.starts_with('-') {
    let value = value.parse::<i64>().map_err(|_| ProtocolDecodeError::new("JSON integer is below the exact integer range"))?;
    if value < -(JAVASCRIPT_EXACT_INTEGER_MAX as i64) {
      return Err(ProtocolDecodeError::new("JSON integer is below the exact integer range"));
    }
    return Ok(Number::from(value));
  }

  let value = value.parse::<u64>().map_err(|_| ProtocolDecodeError::new("JSON integer exceeds the exact integer range"))?;
  if value > JAVASCRIPT_EXACT_INTEGER_MAX {
    return Err(ProtocolDecodeError::new("JSON integer exceeds the exact integer range"));
  }
  Ok(Number::from(value))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn artifact_upload_id_preserves_the_idempotency_key_uuid() {
    let key: auv_tracing::IdempotencyKey = "019f8b1e-4b2d-7a00-8f00-000000000006".parse().unwrap();

    let upload_id = ArtifactUploadId::from_idempotency_key(key);

    assert_eq!(upload_id.to_string(), key.to_string());
    assert_eq!(upload_id.to_idempotency_key(), key);
    assert!(!upload_id.0.is_nil());
    assert_eq!(upload_id.to_string().parse::<ArtifactUploadId>().unwrap(), upload_id);
  }

  #[test]
  fn strict_decoder_rejects_duplicate_keys_at_every_depth() {
    let top_level =
      br#"{"authority_id":"019f8b1e-4b2d-7a00-8f00-0000000000aa","authority_id":"019f8b1e-4b2d-7a00-8f00-0000000000aa","mutations":[]}"#;
    let nested = br#"{"outer":{"value":1,"value":2}}"#;

    assert!(decode_strict::<RunCommitBody>(top_level).is_err());
    assert!(decode_strict::<serde::de::IgnoredAny>(nested).is_err());
  }

  #[test]
  fn error_variant_payloads_reject_unknown_fields() {
    let body = br#"{"history_gap":{"requested_after":4,"earliest_available":9,"latest":10}}"#;
    assert!(decode_strict::<RunApiError>(body).is_err());
  }

  #[test]
  fn v1_error_decoders_reject_unaccepted_wire_extensions() {
    let artifact_authority_fields = br#"{
      "error":"auv.inspect.authority_mismatch",
      "expected":"019f8b1e-4b2d-7a00-8f00-0000000000aa",
      "received":"019f8b1e-4b2d-7a00-8f00-0000000000ab"
    }"#;
    let commit_unknown_variant = br#"{"commit_unknown":{"code":"auv.inspect.commit_unknown"}}"#;

    assert!(decode_strict::<ArtifactApiError>(artifact_authority_fields).is_err());
    assert!(decode_strict::<RunApiError>(commit_unknown_variant).is_err());
  }

  #[test]
  fn v1_control_headers_and_admission_precondition_are_public_protocol_constants() {
    assert_eq!(ARTIFACT_ORIGIN_HEADER, "Auv-Artifact-Origin");
    assert_eq!(ARTIFACT_UPLOAD_ADMISSION_HEADER, "Auv-Artifact-Upload-Admission");
    assert_eq!(ARTIFACT_UPLOAD_ADMISSION_BUSY, "busy");
    assert_eq!(ARTIFACT_UPLOAD_ADMISSION_LEASE_SECONDS, 30);
    assert_eq!(ARTIFACT_UPLOAD_ADMISSION_REQUIRED_ERROR, "auv.inspect.upload_admission_required");
    assert_eq!(AUTHORITY_ID_HEADER, "Auv-Authority-Id");
    assert_eq!(IDEMPOTENCY_MISMATCH_ERROR, "auv.inspect.idempotency_mismatch");
    assert_eq!(ARTIFACT_IDENTITY_CONFLICT_ERROR, "auv.inspect.artifact_identity_conflict");
  }

  fn nested_arrays(depth: usize) -> Vec<u8> {
    format!("{}0{}", "[".repeat(depth), "]".repeat(depth)).into_bytes()
  }

  #[test]
  fn strict_decoder_accepts_depth_128_and_rejects_depth_129() {
    assert!(decode_strict::<serde::de::IgnoredAny>(&nested_arrays(128)).is_ok());
    assert!(decode_strict::<serde::de::IgnoredAny>(&nested_arrays(129)).is_err());
  }

  #[test]
  fn strict_decoder_rejects_oversized_object_member_count() {
    let body = format!("{{{}}}", (0..=MAX_JSON_OBJECT_MEMBERS).map(|index| format!(r#""key_{index}":null"#)).collect::<Vec<_>>().join(","));

    assert!(decode_strict::<serde::de::IgnoredAny>(body.as_bytes()).is_err());
  }

  #[test]
  fn strict_decoder_rejects_non_exact_integers_invalid_numbers_and_trailing_input() {
    for body in [
      "9007199254740992",
      "-9007199254740992",
      "01",
      "+1",
      "NaN",
      "1 true",
    ] {
      assert!(decode_strict::<serde::de::IgnoredAny>(body.as_bytes()).is_err(), "accepted `{body}`");
    }
  }

  #[test]
  fn strict_decoder_validates_exact_number_lexemes_at_root_and_nested_positions() {
    // ROOT CAUSE:
    //
    // With arbitrary_precision enabled, deserialize_any can expose oversized
    // numeric lexemes as serde_json's private map instead of a number callback.
    for body in [
      "9007199254740992",
      "-9007199254740992",
      "18446744073709551615",
      "18446744073709551616",
      "170141183460469231731687303715884105727",
      "-170141183460469231731687303715884105728",
      "1e400",
      r#"{"number":18446744073709551616}"#,
      "[1e400]",
    ] {
      assert!(decode_strict::<serde::de::IgnoredAny>(body.as_bytes()).is_err(), "accepted `{body}`");
    }

    for body in [
      "9007199254740991",
      "-9007199254740991",
      "1.5",
      "1e3",
      r#"{"number":-2.5}"#,
      "[6.25e-2]",
    ] {
      assert!(decode_strict::<serde::de::IgnoredAny>(body.as_bytes()).is_ok(), "rejected `{body}`");
    }
  }

  #[test]
  fn strict_decoder_accepts_negative_zero_for_compatible_destinations() {
    let float = decode_strict::<f64>(b"-0").unwrap();
    assert_eq!(float, 0.0);
    assert!(float.is_sign_negative());
    assert_eq!(decode_strict::<serde_json::Number>(b"-0").unwrap().as_f64(), Some(-0.0));
    assert_eq!(decode_strict::<Vec<f64>>(b"[-0]").unwrap(), vec![-0.0]);
  }

  #[test]
  fn strict_decoder_rejects_escaped_duplicate_keys() {
    assert!(decode_strict::<serde::de::IgnoredAny>(br#"{"key":1,"\u006bey":2}"#).is_err());
  }

  #[test]
  fn strict_protocol_types_reject_unknown_fields_at_nested_and_variant_depths() {
    let nested = br#"{
      "authority_id":"019f8b1e-4b2d-7a00-8f00-0000000000aa",
      "mutations":[{"start_span":{
        "span_id":"019f8b1e-4b2d-7a00-8f00-000000000011",
        "name":"auv.test.root",
        "started_at":{"unix_seconds":1,"nanoseconds":0,"clock":"utc"},
        "attributes":{}
      }}]
    }"#;
    let variant = br#"{
      "not_found":{
        "uri":"auv://runs/019f8b1e-4b2d-7a00-8f00-000000000001/artifacts/019f8b1e-4b2d-7a00-8f00-000000000002",
        "retry":false
      }
    }"#;

    assert!(decode_strict::<RunCommitBody>(nested).is_err());
    assert!(decode_strict::<ResolvedArtifact>(variant).is_err());
  }

  #[test]
  fn structural_validation_retains_and_scans_linearly_for_a_large_deep_leaf() {
    let depth = 128;
    let leaf_bytes = 4 * 1024 * 1024;
    let mut body = Vec::with_capacity(depth * 2 + leaf_bytes + 2);
    body.extend(std::iter::repeat_n(b'[', depth));
    body.push(b'"');
    body.extend(std::iter::repeat_n(b'a', leaf_bytes));
    body.push(b'"');
    body.extend(std::iter::repeat_n(b']', depth));

    let stats = validate_json_structure(&body).expect("valid nested JSON");

    assert_eq!(stats.max_depth, depth);
    assert_eq!(stats.scanned_bytes, body.len());
    assert!(stats.peak_retained_bytes < 64 * 1024, "scanner retained {} bytes for a {} byte leaf", stats.peak_retained_bytes, leaf_bytes);
  }
}
