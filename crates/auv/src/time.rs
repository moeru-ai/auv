//! Protocol-neutral timestamp values returned by control operations.

/// Protocol-neutral timestamp carried by daemon resources.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timestamp {
  /// Whole seconds since the Unix epoch.
  pub seconds: i64,
  /// Nanosecond adjustment within the second.
  pub nanos: i32,
}

impl From<prost_types::Timestamp> for Timestamp {
  fn from(value: prost_types::Timestamp) -> Self {
    Self {
      seconds: value.seconds,
      nanos: value.nanos,
    }
  }
}
