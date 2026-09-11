//! Typed identities and unresolved selectors for daemon-owned resources.

use std::fmt;
use std::str::FromStr;

/// Failure to validate a resource identity or selector.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IdentityError {
  /// A canonical resource ID has the wrong length or character set.
  #[error("{kind} ID must be exactly {length} hexadecimal characters")]
  InvalidResourceId {
    /// Resource kind whose ID failed validation.
    kind: &'static str,
    /// Required number of hexadecimal characters.
    length: usize,
  },
  /// A RunnerClass identity is empty or contains whitespace.
  #[error("RunnerClass ID must not be empty or contain whitespace")]
  InvalidRunnerClassId,
  /// A Device selector contains neither an ID nor a name.
  #[error("Device selector must not be empty")]
  EmptyDeviceSelector,
  /// A resource ID selector is empty, non-hexadecimal, or too long.
  #[error("{0} selector must be a non-empty hexadecimal ID prefix")]
  InvalidIdSelector(&'static str),
}

macro_rules! resource_id {
  ($name:ident, $kind:literal, $length:literal) => {
    #[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
    #[doc = concat!("Canonical ", $kind, " identity.")]
    pub struct $name(String);

    impl $name {
      /// Returns the canonical identity text.
      pub fn as_str(&self) -> &str {
        &self.0
      }

      /// Returns the canonical identity without separators.
      pub fn compact(&self) -> String {
        self.0.chars().filter(|character| *character != '-').collect()
      }

      /// Returns the first twelve characters for human-facing display.
      pub fn short(&self) -> String {
        self.compact().chars().take(12).collect()
      }
    }

    impl FromStr for $name {
      type Err = IdentityError;

      fn from_str(value: &str) -> Result<Self, Self::Err> {
        if value.len() != $length || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
          return Err(IdentityError::InvalidResourceId {
            kind: $kind,
            length: $length,
          });
        }
        Ok(Self(value.to_ascii_lowercase()))
      }
    }

    impl fmt::Display for $name {
      fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
      }
    }
  };
}

resource_id!(DeviceId, "Device", 64);
resource_id!(RunId, "Run", 32);
resource_id!(RunnerId, "Runner", 64);

/// Canonical identity of a RunnerClass.
#[derive(Clone, Debug, Hash, PartialEq, Eq, PartialOrd, Ord)]
pub struct RunnerClassId(String);

impl RunnerClassId {
  /// Returns the exact RunnerClass identity.
  pub fn as_str(&self) -> &str {
    &self.0
  }
}

impl FromStr for RunnerClassId {
  type Err = IdentityError;

  fn from_str(value: &str) -> Result<Self, Self::Err> {
    if value.is_empty() || value.chars().any(char::is_whitespace) {
      return Err(IdentityError::InvalidRunnerClassId);
    }
    Ok(Self(value.to_string()))
  }
}

impl fmt::Display for RunnerClassId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(self.as_str())
  }
}

/// An unresolved Device selector combining an optional ID prefix and name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceSelector {
  id: Option<String>,
  name: Option<String>,
}

impl DeviceSelector {
  /// Parses one user-facing selector as an ID prefix when it is hexadecimal,
  /// otherwise as an exact Device name.
  pub fn parse(value: &str) -> Result<Self, IdentityError> {
    let value = value.trim();
    if value.is_empty() {
      return Err(IdentityError::EmptyDeviceSelector);
    }
    if value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
      Ok(Self::by_id(value))
    } else {
      Ok(Self::by_name(value))
    }
  }

  /// Selects a Device by a canonical ID or hexadecimal ID prefix.
  pub fn by_id(id: impl AsRef<str>) -> Self {
    Self {
      id: Some(normalize_prefix(id.as_ref())),
      name: None,
    }
  }

  /// Parses a Device ID or hexadecimal ID prefix without treating non-hex
  /// input as a Device name.
  pub fn parse_id(value: &str) -> Result<Self, IdentityError> {
    let value = normalize_prefix(value);
    if value.is_empty() || value.len() > 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
      return Err(IdentityError::InvalidIdSelector("Device"));
    }
    Ok(Self::by_id(value))
  }

  /// Selects a Device by its exact display name.
  pub fn by_name(name: impl Into<String>) -> Self {
    Self {
      id: None,
      name: Some(name.into()),
    }
  }

  /// Selects a Device only when both the ID and name identify it.
  pub fn by_id_and_name(id: impl AsRef<str>, name: impl Into<String>) -> Self {
    Self {
      id: Some(normalize_prefix(id.as_ref())),
      name: Some(name.into()),
    }
  }

  /// Adds an exact Device name constraint to this selector.
  pub fn with_name(mut self, name: impl Into<String>) -> Self {
    self.name = Some(name.into());
    self
  }

  pub(crate) fn is_empty(&self) -> bool {
    self.id.is_none() && self.name.is_none()
  }

  pub(crate) fn id(&self) -> Option<&str> {
    self.id.as_deref()
  }

  pub(crate) fn name(&self) -> Option<&str> {
    self.name.as_deref()
  }

  /// Returns whether a typed Device identity and name satisfy this selector.
  pub fn matches(&self, id: &DeviceId, name: &str) -> bool {
    self.id.as_deref().is_none_or(|prefix| id.compact().starts_with(prefix)) && self.name.as_deref().is_none_or(|expected| name == expected)
  }

  pub(crate) fn matches_wire(&self, id: &str, name: &str) -> bool {
    self.id.as_deref().is_none_or(|prefix| normalize_prefix(id).starts_with(prefix))
      && self.name.as_deref().is_none_or(|expected| name == expected)
  }
}

macro_rules! id_selector {
  ($name:ident, $id:ident, $kind:literal) => {
    #[derive(Clone, Debug, Hash, PartialEq, Eq)]
    #[doc = concat!("Validated ", $kind, " ID-prefix selector.")]
    pub struct $name(String);

    impl $name {
      /// Parses a canonical resource ID or hexadecimal ID prefix.
      pub fn parse(value: &str) -> Result<Self, IdentityError> {
        let value = normalize_prefix(value);
        if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
          return Err(IdentityError::InvalidIdSelector($kind));
        }
        Ok(Self(value))
      }

      /// Returns whether this selector matches the typed canonical identity.
      pub fn matches(&self, id: &$id) -> bool {
        id.compact().starts_with(&self.0)
      }

      /// Returns the normalized selector text.
      pub fn as_str(&self) -> &str {
        &self.0
      }
    }
  };
}

id_selector!(RunSelector, RunId, "Run");
id_selector!(RunnerSelector, RunnerId, "Runner");

impl RunSelector {
  pub(crate) fn matches_wire(&self, id: &str) -> bool {
    normalize_prefix(id).starts_with(self.as_str())
  }
}

/// Validated exact selector for a RunnerClass identity.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct RunnerClassSelector(RunnerClassId);

impl RunnerClassSelector {
  /// Parses an exact RunnerClass identity.
  pub fn parse(value: &str) -> Result<Self, IdentityError> {
    value.parse().map(Self)
  }

  /// Returns the selected RunnerClass identity.
  pub fn id(&self) -> &RunnerClassId {
    &self.0
  }
}

fn normalize_prefix(value: &str) -> String {
  value.trim().chars().filter(|character| *character != '-').collect::<String>().to_ascii_lowercase()
}
