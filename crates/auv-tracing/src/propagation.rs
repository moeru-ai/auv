use crate::{AuthorityId, ErrorCode, RunId, SpanId};

const CONTEXT_VERSION: &str = "auv-context-version";
const RUN_ID: &str = "auv-run-id";
const AUTHORITY_ID: &str = "auv-authority-id";
const SPAN_ID: &str = "auv-span-id";
const FIELD_NAMES: [&str; 4] = [CONTEXT_VERSION, RUN_ID, AUTHORITY_ID, SPAN_ID];

/// Extracted cross-process run correlation without any local routing state.
pub struct RemoteContext {
  pub(crate) authority_id: Option<AuthorityId>,
  pub(crate) run_id: RunId,
  pub(crate) remote_span_id: Option<SpanId>,
}

/// Writes named text values into a cross-process carrier.
pub trait TextMapWriter {
  /// Replaces or adds one value for a canonical field name.
  fn set(&mut self, name: &'static str, value: &str);

  /// Removes every value for a canonical field name.
  fn remove(&mut self, name: &'static str);
}

/// Reads all text values associated with one carrier field.
pub trait TextMapReader {
  /// Returns every value associated with `name`, preserving duplicates.
  fn values<'a>(&'a self, name: &str) -> Box<dyn Iterator<Item = &'a str> + 'a>;
}

/// Reports invalid or incompatible AUV context propagation data.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("AUV context propagation failed: {code}")]
pub struct PropagationError {
  code: ErrorCode,
}

impl PropagationError {
  /// Returns the stable machine-readable propagation failure code.
  pub fn code(&self) -> &ErrorCode {
    &self.code
  }

  fn new(code: &'static str) -> Self {
    Self {
      code: ErrorCode::parse(code).expect("static propagation error code is valid"),
    }
  }
}

/// Extracts the complete AUV context field set from a text carrier.
///
/// All fields absent means no propagated context. Duplicate fields, missing
/// required fields, invalid canonical IDs, and unsupported versions fail.
pub fn extract(carrier: &dyn TextMapReader) -> Result<Option<RemoteContext>, PropagationError> {
  let version = single_value(carrier, CONTEXT_VERSION)?;
  let run_id = single_value(carrier, RUN_ID)?;
  let authority_id = single_value(carrier, AUTHORITY_ID)?;
  let remote_span_id = single_value(carrier, SPAN_ID)?;

  if version.is_none() && run_id.is_none() && authority_id.is_none() && remote_span_id.is_none() {
    return Ok(None);
  }

  let version = version.ok_or_else(partial_context)?;
  let run_id = run_id.ok_or_else(partial_context)?;
  if version != "1" {
    return Err(PropagationError::new("auv.propagation.unsupported_version"));
  }

  let run_id = run_id.parse().map_err(|_| invalid_id())?;
  let authority_id = authority_id.map(str::parse).transpose().map_err(|_| invalid_id())?;
  let remote_span_id = remote_span_id.map(str::parse).transpose().map_err(|_| invalid_id())?;
  Ok(Some(RemoteContext {
    authority_id,
    run_id,
    remote_span_id,
  }))
}

pub(crate) fn inject(carrier: &mut dyn TextMapWriter, authority_id: Option<AuthorityId>, run_id: Option<RunId>, span_id: Option<SpanId>) {
  for name in FIELD_NAMES {
    carrier.remove(name);
  }

  let Some(run_id) = run_id else {
    return;
  };
  carrier.set(CONTEXT_VERSION, "1");
  carrier.set(RUN_ID, &run_id.to_string());
  if let Some(authority_id) = authority_id {
    carrier.set(AUTHORITY_ID, &authority_id.to_string());
  }
  if let Some(span_id) = span_id {
    carrier.set(SPAN_ID, &span_id.to_string());
  }
}

pub(crate) fn authority_mismatch() -> PropagationError {
  PropagationError::new("auv.propagation.authority_mismatch")
}

fn single_value<'a>(carrier: &'a dyn TextMapReader, name: &str) -> Result<Option<&'a str>, PropagationError> {
  let mut values = carrier.values(name);
  let value = values.next();
  if values.next().is_some() {
    return Err(PropagationError::new("auv.propagation.duplicate_field"));
  }
  Ok(value)
}

fn partial_context() -> PropagationError {
  PropagationError::new("auv.propagation.partial_context")
}

fn invalid_id() -> PropagationError {
  PropagationError::new("auv.propagation.invalid_id")
}
