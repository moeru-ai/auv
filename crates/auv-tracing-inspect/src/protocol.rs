use std::collections::HashSet;
use std::fmt;

use auv_tracing::{AuthorityId, ErrorCode, NonEmptyVec, RunMutation, RunRevision};
use serde::de::{self, DeserializeOwned, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

const MAX_JSON_NESTING: usize = 128;
const MAX_JSON_OBJECT_MEMBERS: usize = 8_192;

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
  validate_json_structure(bytes)?;
  let mut deserializer = serde_json::Deserializer::from_slice(bytes);
  deserializer.disable_recursion_limit();
  let value = T::deserialize(&mut deserializer).map_err(protocol_json_error)?;
  deserializer.end().map_err(protocol_json_error)?;
  Ok(value)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct StructureStats {
  max_depth: usize,
  value_count: usize,
}

fn validate_json_structure(bytes: &[u8]) -> Result<StructureStats, ProtocolDecodeError> {
  let mut stats = StructureStats::default();
  let mut deserializer = serde_json::Deserializer::from_slice(bytes);
  deserializer.disable_recursion_limit();
  StructureSeed {
    depth: 0,
    stats: &mut stats,
  }
  .deserialize(&mut deserializer)
  .map_err(protocol_json_error)?;
  deserializer.end().map_err(protocol_json_error)?;
  Ok(stats)
}

fn protocol_json_error(error: serde_json::Error) -> ProtocolDecodeError {
  ProtocolDecodeError::new(error.to_string())
}

struct StructureSeed<'a> {
  depth: usize,
  stats: &'a mut StructureStats,
}

impl<'de> DeserializeSeed<'de> for StructureSeed<'_> {
  type Value = ();

  fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
  where
    D: Deserializer<'de>,
  {
    self.stats.value_count += 1;
    deserializer.deserialize_any(StructureVisitor {
      depth: self.depth,
      stats: self.stats,
    })
  }
}

struct StructureVisitor<'a> {
  depth: usize,
  stats: &'a mut StructureStats,
}

impl StructureVisitor<'_> {
  fn container_depth<E: de::Error>(&mut self) -> Result<usize, E> {
    let depth = self.depth + 1;
    if depth > MAX_JSON_NESTING {
      return Err(E::custom(format!("JSON exceeds {MAX_JSON_NESTING} nested containers")));
    }
    self.stats.max_depth = self.stats.max_depth.max(depth);
    Ok(depth)
  }
}

impl<'de> Visitor<'de> for StructureVisitor<'_> {
  type Value = ();

  fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str("a JSON value within the Inspect protocol limits")
  }

  fn visit_bool<E>(self, _value: bool) -> Result<Self::Value, E> {
    Ok(())
  }

  fn visit_i64<E>(self, _value: i64) -> Result<Self::Value, E> {
    Ok(())
  }

  fn visit_u64<E>(self, _value: u64) -> Result<Self::Value, E> {
    Ok(())
  }

  fn visit_f64<E>(self, _value: f64) -> Result<Self::Value, E> {
    Ok(())
  }

  fn visit_str<E>(self, _value: &str) -> Result<Self::Value, E>
  where
    E: de::Error,
  {
    Ok(())
  }

  fn visit_string<E>(self, _value: String) -> Result<Self::Value, E>
  where
    E: de::Error,
  {
    Ok(())
  }

  fn visit_none<E>(self) -> Result<Self::Value, E> {
    Ok(())
  }

  fn visit_unit<E>(self) -> Result<Self::Value, E> {
    Ok(())
  }

  fn visit_seq<A>(mut self, mut sequence: A) -> Result<Self::Value, A::Error>
  where
    A: SeqAccess<'de>,
  {
    let depth = self.container_depth()?;
    while sequence
      .next_element_seed(StructureSeed {
        depth,
        stats: self.stats,
      })?
      .is_some()
    {}
    Ok(())
  }

  fn visit_map<A>(mut self, mut map: A) -> Result<Self::Value, A::Error>
  where
    A: MapAccess<'de>,
  {
    let depth = self.container_depth()?;
    let mut keys = HashSet::new();
    while let Some(key) = map.next_key::<String>()? {
      if keys.len() == MAX_JSON_OBJECT_MEMBERS {
        return Err(de::Error::custom(format!("JSON object exceeds {MAX_JSON_OBJECT_MEMBERS} members")));
      }
      if !keys.insert(key.to_owned()) {
        return Err(de::Error::custom(format!("duplicate JSON object key `{key}`")));
      }
      map.next_value_seed(StructureSeed {
        depth,
        stats: self.stats,
      })?;
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
    assert!(decode_strict::<serde::de::IgnoredAny>(nested).is_err());
  }

  #[test]
  fn error_variant_payloads_reject_unknown_fields() {
    let body = br#"{"history_gap":{"requested_after":4,"earliest_available":9,"latest":10}}"#;
    assert!(decode_strict::<RunApiError>(body).is_err());
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
  fn structural_validation_visits_large_nested_input_once() {
    let depth = 128;
    let mut body = "0".to_string();
    for _ in 0..depth {
      body = format!("[{body},0]");
    }

    let stats = validate_json_structure(body.as_bytes()).expect("valid nested JSON");

    assert_eq!(stats.max_depth, depth);
    assert_eq!(stats.value_count, depth * 2 + 1);
  }
}
