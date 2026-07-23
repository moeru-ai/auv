//! Lightweight in-memory session registry (API-P4 responsibility A).
//!
//! Creates and looks up session handles and lets the handler reject unknown
//! sessions on `Invoke` / `StreamSessionEvents`. Deliberately metadata-only: it
//! does NOT materialize a `SessionRuntime` (API-P4: lazy provider/observation
//! state is a later RPC's concern, not the invoke-only first slice).

use std::collections::HashMap;

use crate::model::now_millis;
use crate::session::SessionId;

/// Lightweight per-session metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SessionEntry {
  pub session_id: SessionId,
  pub created_at_millis: u64,
}

/// In-memory session registry keyed by the `session_id` string.
#[derive(Debug, Default)]
pub struct SessionRegistry {
  entries: HashMap<String, SessionEntry>,
  counter: u64,
}

impl SessionRegistry {
  pub fn new() -> Self {
    Self::default()
  }

  /// Allocate a fresh session id, register lightweight metadata, and return it.
  ///
  /// API-P4 `CreateSession`: allocate/normalize then register. This skeleton
  /// always allocates a new id (the proto `CreateSessionRequest` carries only a
  /// `client_label` hint, no caller-supplied id).
  pub fn create(&mut self) -> SessionId {
    self.counter += 1;
    let session_id = SessionId::new(format!("session_{}_{}", now_millis(), self.counter));
    self.entries.insert(
      session_id.as_str().to_string(),
      SessionEntry {
        session_id: session_id.clone(),
        created_at_millis: now_millis(),
      },
    );
    session_id
  }

  /// Whether a session with this id was created.
  pub fn contains(&self, session_id: &str) -> bool {
    self.entries.contains_key(session_id)
  }

  /// Look up a registered session entry.
  pub fn get(&self, session_id: &str) -> Option<&SessionEntry> {
    self.entries.get(session_id)
  }

  pub fn len(&self) -> usize {
    self.entries.len()
  }

  pub fn is_empty(&self) -> bool {
    self.entries.is_empty()
  }
}

#[cfg(test)]
mod tests {
  use super::SessionRegistry;

  #[test]
  fn create_registers_unique_sessions() {
    let mut registry = SessionRegistry::new();
    let first = registry.create();
    let second = registry.create();

    assert_ne!(first.as_str(), second.as_str());
    assert!(registry.contains(first.as_str()));
    assert!(registry.contains(second.as_str()));
    assert_eq!(registry.len(), 2);
  }

  #[test]
  fn contains_rejects_unknown_session() {
    let registry = SessionRegistry::new();
    assert!(!registry.contains("never-created"));
  }
}
