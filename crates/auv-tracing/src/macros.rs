/// Starts a typed span in the current AUV context.
#[macro_export]
macro_rules! start_span {
  ($spec:expr) => {
    $crate::start_span($spec)
  };
}

/// Emits a typed event in the current AUV context.
#[macro_export]
macro_rules! emit_event {
  ($event:expr) => {
    $crate::emit_event($event)
  };
}

/// Emits one detached artifact under the current AUV context.
#[macro_export]
macro_rules! emit_artifact {
  ($artifact:expr) => {
    $crate::emit_artifact($artifact)
  };
}
