// File: src/driver/macos/support/call.rs
use super::super::*;

pub(crate) fn app_identifier(call: &DriverCall) -> Option<String> {
  optional_string(call, "app").or_else(|| {
    call
      .target
      .application_id
      .clone()
      .filter(|value| !value.trim().is_empty())
  })
}

pub(crate) fn optional_string(call: &DriverCall, key: &str) -> Option<String> {
  call.inputs.get(key).cloned()
}

pub(crate) fn optional_non_empty_string(call: &DriverCall, key: &str) -> Option<String> {
  optional_string(call, key)
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty())
}

pub(crate) fn required_non_empty_string(call: &DriverCall, key: &str) -> AuvResult<String> {
  let value = optional_non_empty_string(call, key)
    .ok_or_else(|| format!("operation requires --{} <text>", key))?;
  Ok(value)
}

pub(crate) fn parse_window_selection(call: &DriverCall) -> AuvResult<WindowSelection> {
  if call.inputs.contains_key("window_index") {
    return Err(
      "--window_index is not supported because window candidate order is not stable; use --window_ref, --native_window_id, or --title"
        .to_string(),
    );
  }
  Ok(WindowSelection {
    window_ref: optional_non_empty_string(call, "window_ref"),
    native_window_id: optional_non_empty_string(call, "native_window_id"),
    title: optional_non_empty_string(call, "title"),
  })
}

pub(crate) fn required_f64(call: &DriverCall, key: &str) -> AuvResult<f64> {
  optional_f64(call, key)?.ok_or_else(|| format!("operation requires --{} <number>", key))
}

pub(crate) fn optional_f64(call: &DriverCall, key: &str) -> AuvResult<Option<f64>> {
  match call.inputs.get(key) {
    Some(value) => {
      let parsed = value
        .parse::<f64>()
        .map_err(|error| format!("invalid --{} value {}: {}", key, value, error))?;
      if !parsed.is_finite() {
        return Err(format!(
          "invalid --{} value {}: expected a finite number",
          key, value
        ));
      }
      Ok(Some(parsed))
    }
    None => Ok(None),
  }
}

pub(crate) fn optional_i64(call: &DriverCall, key: &str) -> AuvResult<Option<i64>> {
  match call.inputs.get(key) {
    Some(value) => value
      .parse::<i64>()
      .map(Some)
      .map_err(|error| format!("invalid --{} value {}: {}", key, value, error)),
    None => Ok(None),
  }
}

pub(crate) fn optional_bool(call: &DriverCall, key: &str) -> AuvResult<Option<bool>> {
  match optional_non_empty_string(call, key) {
    Some(value) => match value.to_ascii_lowercase().as_str() {
      "1" | "true" | "yes" | "on" => Ok(Some(true)),
      "0" | "false" | "no" | "off" => Ok(Some(false)),
      _ => Err(format!(
        "invalid --{} value {}: expected true/false or 1/0",
        key, value
      )),
    },
    None => Ok(None),
  }
}

pub(crate) fn optional_positive_u64(call: &DriverCall, key: &str) -> AuvResult<Option<u64>> {
  match optional_i64(call, key)? {
    Some(value) if value < 0 => Err(format!(
      "invalid --{} value {}: expected a non-negative integer",
      key, value
    )),
    Some(value) => Ok(Some(value as u64)),
    None => Ok(None),
  }
}

pub(crate) fn parse_mouse_button(call: &DriverCall) -> AuvResult<(&'static str, i32)> {
  match optional_string(call, "button")
    .unwrap_or_else(|| "left".to_string())
    .trim()
    .to_ascii_lowercase()
    .as_str()
  {
    "left" => Ok(("left", 0)),
    "right" => Ok(("right", 1)),
    "middle" => Ok(("middle", 2)),
    other => Err(format!(
      "invalid --button value {}; expected left, right, or middle",
      other
    )),
  }
}

pub(crate) fn resolve_scroll_deltas(call: &DriverCall) -> AuvResult<(f64, f64, String)> {
  let explicit_delta_x = optional_f64(call, "delta_x")?;
  let explicit_delta_y = optional_f64(call, "delta_y")?;
  if explicit_delta_x.is_some() || explicit_delta_y.is_some() {
    let delta_x = explicit_delta_x.unwrap_or(0.0);
    let delta_y = explicit_delta_y.unwrap_or(0.0);
    return Ok((
      delta_x,
      delta_y,
      format!("delta_x={:.0},delta_y={:.0}", delta_x, delta_y),
    ));
  }

  let direction = required_non_empty_string(call, "direction")?.to_ascii_lowercase();
  let pages = optional_f64(call, "pages")?.unwrap_or(1.0);
  if !pages.is_finite() || pages <= 0.0 {
    return Err(format!(
      "invalid --pages value {:.3}: expected a positive finite number",
      pages
    ));
  }
  let magnitude = (pages * 480.0).round();
  let (delta_x, delta_y) = match direction.as_str() {
    "up" => (0.0, magnitude),
    "down" => (0.0, -magnitude),
    "left" => (magnitude, 0.0),
    "right" => (-magnitude, 0.0),
    other => {
      return Err(format!(
        "invalid --direction value {}; expected up, down, left, or right",
        other
      ));
    }
  };

  Ok((
    delta_x,
    delta_y,
    format!("direction={direction},pages={pages:.3}"),
  ))
}
