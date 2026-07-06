use serde::Serialize;

pub fn print_json<T: Serialize>(value: &T) -> Result<(), String> {
  let json = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
  println!("{json}");
  Ok(())
}
