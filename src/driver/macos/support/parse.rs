use super::super::*;

pub(crate) fn report_value<'a>(report: &'a str, prefix: &str) -> Option<&'a str> {
  report
    .lines()
    .find_map(|line| line.strip_prefix(prefix))
    .map(str::trim)
}

pub(crate) fn parse_bool_flag(raw: &str, label: &str) -> AuvResult<bool> {
  match raw {
    "1" | "true" => Ok(true),
    "0" | "false" => Ok(false),
    other => Err(format!("invalid {} value {}: expected 0/1", label, other)),
  }
}

pub(crate) fn parse_i64(raw: &str, label: &str) -> AuvResult<i64> {
  raw
    .parse::<i64>()
    .map_err(|error| format!("invalid {} value {}: {}", label, raw, error))
}

pub(crate) fn parse_u32(raw: &str, label: &str) -> AuvResult<u32> {
  raw
    .parse::<u32>()
    .map_err(|error| format!("invalid {} value {}: {}", label, raw, error))
}

pub(crate) fn parse_f64(raw: &str, label: &str) -> AuvResult<f64> {
  let value = raw
    .parse::<f64>()
    .map_err(|error| format!("invalid {} value {}: {}", label, raw, error))?;
  if !value.is_finite() {
    return Err(format!(
      "invalid {} value {}: expected a finite number",
      label, raw
    ));
  }
  Ok(value)
}
