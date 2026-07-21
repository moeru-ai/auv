use std::collections::BTreeSet;
use std::fmt;

use auv_tracing::{AuthorityId, ErrorCode, NonEmptyVec, RunMutation, RunRevision};
use serde::de::{self, DeserializeOwned, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::value::RawValue;

/// Versioned media type for Inspect run JSON requests and responses.
pub const RUN_MEDIA_TYPE: &str = "application/vnd.auv.run+json; version=1";

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
  reject_duplicate_keys_recursively(bytes)?;
  serde_json::from_slice(bytes).map_err(|error| ProtocolDecodeError::new(error.to_string()))
}

fn reject_duplicate_keys_recursively(bytes: &[u8]) -> Result<(), ProtocolDecodeError> {
  let raw = serde_json::from_slice::<Box<RawValue>>(bytes).map_err(|error| ProtocolDecodeError::new(error.to_string()))?;
  validate_raw(&raw, 128)
}

fn validate_raw(raw: &RawValue, remaining_depth: usize) -> Result<(), ProtocolDecodeError> {
  let token = raw.get().bytes().find(|byte| !byte.is_ascii_whitespace()).ok_or_else(|| ProtocolDecodeError::new("JSON body is empty"))?;
  match token {
    b'{' => {
      let depth = remaining_depth.checked_sub(1).ok_or_else(|| ProtocolDecodeError::new("JSON exceeds 128 nested containers"))?;
      let mut deserializer = serde_json::Deserializer::from_str(raw.get());
      deserializer
        .deserialize_map(UniqueObjectVisitor {
          remaining_depth: depth,
        })
        .map_err(protocol_json_error)
    }
    b'[' => {
      let depth = remaining_depth.checked_sub(1).ok_or_else(|| ProtocolDecodeError::new("JSON exceeds 128 nested containers"))?;
      let mut deserializer = serde_json::Deserializer::from_str(raw.get());
      deserializer
        .deserialize_seq(SequenceVisitor {
          remaining_depth: depth,
        })
        .map_err(protocol_json_error)
    }
    _ => Ok(()),
  }
}

fn protocol_json_error(error: serde_json::Error) -> ProtocolDecodeError {
  ProtocolDecodeError::new(error.to_string())
}

struct UniqueObjectVisitor {
  remaining_depth: usize,
}

impl<'de> Visitor<'de> for UniqueObjectVisitor {
  type Value = ();

  fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("a JSON object with unique keys")
  }

  fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
  where
    A: MapAccess<'de>,
  {
    let mut keys = BTreeSet::new();
    while let Some(key) = map.next_key::<String>()? {
      if !keys.insert(key.clone()) {
        return Err(de::Error::custom(format!("duplicate JSON object key `{key}`")));
      }
      let raw = map.next_value::<Box<RawValue>>()?;
      validate_raw(&raw, self.remaining_depth).map_err(de::Error::custom)?;
    }
    Ok(())
  }
}

struct SequenceVisitor {
  remaining_depth: usize,
}

impl<'de> Visitor<'de> for SequenceVisitor {
  type Value = ();

  fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("a JSON array")
  }

  fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
  where
    A: SeqAccess<'de>,
  {
    while let Some(raw) = sequence.next_element::<Box<RawValue>>()? {
      validate_raw(&raw, self.remaining_depth).map_err(de::Error::custom)?;
    }
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn strict_decoder_rejects_duplicate_keys_at_every_depth() {
    let top_level =
      br#"{"authority_id":"019f8b1e-4b2d-7a00-8f00-0000000000aa","authority_id":"019f8b1e-4b2d-7a00-8f00-0000000000aa","mutations":[]}"#;
    let nested = br#"{"outer":{"value":1,"value":2}}"#;

    assert!(decode_strict::<RunCommitBody>(top_level).is_err());
    assert!(decode_strict::<Box<serde_json::value::RawValue>>(nested).is_err());
  }

  #[test]
  fn error_variant_payloads_reject_unknown_fields() {
    let body = br#"{"history_gap":{"requested_after":4,"earliest_available":9,"latest":10}}"#;
    assert!(decode_strict::<RunApiError>(body).is_err());
  }
}
