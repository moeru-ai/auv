use crate::{OverlayError, OverlayResult, OverlayTheme};

/// Reads optional JSON host overrides from `AUV_OVERLAY_THEME` in this process.
///
/// Set it when launching the rendering Runner (or its parent daemon), not only
/// in a CLI attached to an existing daemon. Missing means unchanged appearance;
/// empty, malformed and invalid values are errors. No global mutable cache is used.
///
/// TODO: Remote live updates need a session-owned RPC contract; see
/// `docs/ai/references/driver/2026-09-07-overlay-host-theme.md`. Parent environment
/// changes cannot update an already-running Runner.
pub fn theme_from_env() -> OverlayResult<Option<OverlayTheme>> {
  // Environment belongs to the rendering facade, not common layer/style types.
  decode_theme(std::env::var("AUV_OVERLAY_THEME"))
}

fn decode_theme(value: Result<String, std::env::VarError>) -> OverlayResult<Option<OverlayTheme>> {
  let raw = match value {
    Ok(raw) => raw,
    Err(std::env::VarError::NotPresent) => return Ok(None),
    Err(error) => {
      return Err(OverlayError::InvalidTheme {
        message: format!("AUV_OVERLAY_THEME: {error}"),
      });
    }
  };
  let theme: OverlayTheme = serde_json::from_str(&raw).map_err(|error| OverlayError::InvalidTheme {
    message: format!("AUV_OVERLAY_THEME: {error}"),
  })?;
  theme.validate().map_err(|error| OverlayError::InvalidTheme {
    message: format!("AUV_OVERLAY_THEME: {error}"),
  })?;
  Ok(Some(theme))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn missing_environment_preserves_existing_appearance() {
    assert_eq!(decode_theme(Err(std::env::VarError::NotPresent)).unwrap(), None);
    assert_eq!(decode_theme(Ok("{}".into())).unwrap(), Some(OverlayTheme::default()));
  }

  #[test]
  fn environment_rejects_bad_host_configuration() {
    for raw in [
      "",
      "null",
      "{",
      r##"{"outline_color":"#xyz"}"##,
      r#"{"typo":true}"#,
      r#"{"outline_color":{"red":2,"green":0,"blue":0,"alpha":1}}"#,
    ] {
      assert!(matches!(decode_theme(Ok(raw.into())), Err(OverlayError::InvalidTheme { .. })), "{raw}");
    }
  }

  #[test]
  fn environment_decodes_theme_without_process_global_mutation() {
    let theme = decode_theme(Ok(r##"{"outline_color":"#33669980"}"##.into())).unwrap().unwrap();
    assert_eq!(theme.outline_color, Some(crate::style::Color::rgba(0.2, 0.4, 0.6, 128.0 / 255.0)));
  }
}
