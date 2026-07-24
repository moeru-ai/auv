/// Starts a typed span in the current AUV context.
#[macro_export]
macro_rules! start_span {
  ($spec:expr) => {
    $crate::start_span($spec)
  };
}

/// Runs a synchronous operation inside an empty-attribute span with a stable
/// literal name.
///
/// Use [`start_span!`](crate::start_span) with an explicit [`SpanSpec`](crate::SpanSpec)
/// when the span carries typed attributes.
#[macro_export]
macro_rules! in_span {
  ($name:literal, $operation:expr $(,)?) => {{
    struct AuvLiteralSpan;

    impl $crate::SpanSpec for AuvLiteralSpan {
      const NAME: &'static str = $name;

      fn attributes(&self) -> $crate::Attributes {
        $crate::Attributes::empty()
      }
    }

    $crate::start_span(AuvLiteralSpan).in_scope($operation)
  }};
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
