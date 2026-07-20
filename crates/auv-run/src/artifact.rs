//! Artifact metadata and authority-owned byte IO.

use serde::{Deserialize, Deserializer, Serialize};

use crate::{
  ArtifactId, ArtifactPurpose, Attributes, ByteLength, ContentType, ExecutionId, IdempotencyKey, PayloadSchema, Revision, RunId,
  Sha256Digest, Timestamp,
};

/// Committed inspection or replay material owned by a run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
  artifact_ref: ArtifactRef,
  scope: ArtifactScope,
  purpose: ArtifactPurpose,
  content: ArtifactContent,
  created_at: Timestamp,
  attributes: Attributes,
}

impl Artifact {
  pub const fn new(
    artifact_ref: ArtifactRef,
    scope: ArtifactScope,
    purpose: ArtifactPurpose,
    content: ArtifactContent,
    created_at: Timestamp,
    attributes: Attributes,
  ) -> Self {
    Self {
      artifact_ref,
      scope,
      purpose,
      content,
      created_at,
      attributes,
    }
  }

  pub const fn artifact_ref(&self) -> ArtifactRef {
    self.artifact_ref
  }

  pub const fn scope(&self) -> ArtifactScope {
    self.scope
  }

  pub fn purpose(&self) -> &ArtifactPurpose {
    &self.purpose
  }

  pub fn content(&self) -> &ArtifactContent {
    &self.content
  }

  pub const fn created_at(&self) -> Timestamp {
    self.created_at
  }

  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }
}

/// Stable public identity of one artifact within a run.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
  run_id: RunId,
  artifact_id: ArtifactId,
}

impl ArtifactRef {
  pub const fn new(run_id: RunId, artifact_id: ArtifactId) -> Self {
    Self {
      run_id,
      artifact_id,
    }
  }

  pub const fn run_id(self) -> RunId {
    self.run_id
  }

  pub const fn artifact_id(self) -> ArtifactId {
    self.artifact_id
  }
}

/// The run fact to which an artifact belongs.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum ArtifactScope {
  Run,
  Execution { execution_id: ExecutionId },
  Verification { execution_id: ExecutionId },
}

// Unit variants use struct-shaped wire variants so unknown fields are rejected.
#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ArtifactScopeWire {
  Run {},
  Execution { execution_id: ExecutionId },
  Verification { execution_id: ExecutionId },
}

impl From<&ArtifactScope> for ArtifactScopeWire {
  fn from(scope: &ArtifactScope) -> Self {
    match scope {
      ArtifactScope::Run => Self::Run {},
      ArtifactScope::Execution { execution_id } => Self::Execution {
        execution_id: *execution_id,
      },
      ArtifactScope::Verification { execution_id } => Self::Verification {
        execution_id: *execution_id,
      },
    }
  }
}

impl From<ArtifactScopeWire> for ArtifactScope {
  fn from(scope: ArtifactScopeWire) -> Self {
    match scope {
      ArtifactScopeWire::Run {} => Self::Run,
      ArtifactScopeWire::Execution { execution_id } => Self::Execution { execution_id },
      ArtifactScopeWire::Verification { execution_id } => Self::Verification { execution_id },
    }
  }
}

impl Serialize for ArtifactScope {
  fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
  where
    S: serde::Serializer,
  {
    ArtifactScopeWire::from(self).serialize(serializer)
  }
}

impl<'de> Deserialize<'de> for ArtifactScope {
  fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
  where
    D: Deserializer<'de>,
  {
    Ok(ArtifactScopeWire::deserialize(deserializer)?.into())
  }
}

/// Integrity and media metadata for committed artifact bytes.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactContent {
  content_type: ContentType,
  payload_schema: Option<PayloadSchema>,
  sha256: Sha256Digest,
  byte_length: ByteLength,
}

impl ArtifactContent {
  pub const fn new(content_type: ContentType, payload_schema: Option<PayloadSchema>, sha256: Sha256Digest, byte_length: ByteLength) -> Self {
    Self {
      content_type,
      payload_schema,
      sha256,
      byte_length,
    }
  }

  pub fn content_type(&self) -> &ContentType {
    &self.content_type
  }

  pub fn payload_schema(&self) -> Option<&PayloadSchema> {
    self.payload_schema.as_ref()
  }

  pub const fn sha256(&self) -> Sha256Digest {
    self.sha256
  }

  pub const fn byte_length(&self) -> ByteLength {
    self.byte_length
  }
}

/// Metadata supplied before an authority consumes and validates artifact bytes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactWriteRequest {
  run_id: RunId,
  expected_revision: Revision,
  idempotency_key: IdempotencyKey,
  scope: ArtifactScope,
  purpose: ArtifactPurpose,
  content_type: ContentType,
  payload_schema: Option<PayloadSchema>,
  expected_sha256: Sha256Digest,
  expected_byte_length: ByteLength,
  attributes: Attributes,
}

impl ArtifactWriteRequest {
  // NOTICE: The explicit field list is the accepted V1 metadata contract;
  // replacing it with a generic metadata bag would weaken validation.
  #[allow(clippy::too_many_arguments)]
  pub const fn new(
    run_id: RunId,
    expected_revision: Revision,
    idempotency_key: IdempotencyKey,
    scope: ArtifactScope,
    purpose: ArtifactPurpose,
    content_type: ContentType,
    payload_schema: Option<PayloadSchema>,
    expected_sha256: Sha256Digest,
    expected_byte_length: ByteLength,
    attributes: Attributes,
  ) -> Self {
    Self {
      run_id,
      expected_revision,
      idempotency_key,
      scope,
      purpose,
      content_type,
      payload_schema,
      expected_sha256,
      expected_byte_length,
      attributes,
    }
  }

  pub const fn run_id(&self) -> RunId {
    self.run_id
  }

  pub const fn expected_revision(&self) -> Revision {
    self.expected_revision
  }

  pub const fn idempotency_key(&self) -> IdempotencyKey {
    self.idempotency_key
  }

  pub const fn scope(&self) -> ArtifactScope {
    self.scope
  }

  pub fn purpose(&self) -> &ArtifactPurpose {
    &self.purpose
  }

  pub fn content_type(&self) -> &ContentType {
    &self.content_type
  }

  pub fn payload_schema(&self) -> Option<&PayloadSchema> {
    self.payload_schema.as_ref()
  }

  pub const fn expected_sha256(&self) -> Sha256Digest {
    self.expected_sha256
  }

  pub const fn expected_byte_length(&self) -> ByteLength {
    self.expected_byte_length
  }

  pub fn attributes(&self) -> &Attributes {
    &self.attributes
  }
}
