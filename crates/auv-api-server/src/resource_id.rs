//! Opaque daemon resource identity generation.

/// Generates a Docker-style full random identifier.
pub(crate) fn generate() -> Result<String, String> {
  let mut bytes = [0_u8; 32];
  getrandom::fill(&mut bytes).map_err(|error| format!("failed to generate resource ID: {error}"))?;
  Ok(hex::encode(bytes))
}

/// Generates the compact 128-bit identity shared with tracing Run records.
pub(crate) fn generate_run() -> Result<String, String> {
  let mut bytes = [0_u8; 16];
  getrandom::fill(&mut bytes).map_err(|error| format!("failed to generate resource ID: {error}"))?;
  Ok(hex::encode(bytes))
}

pub(crate) fn validate(value: &str) -> bool {
  value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}
