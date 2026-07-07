//! NOTICE(core-b1): this crate currently owns only narrow JSON artifact file IO helpers.
//! Broader file abstraction is deferred until more cross-vertical evidence exists.

use std::fs;
use std::io::BufReader;
use std::path::Path;

use serde::Serialize;
use serde::de::DeserializeOwned;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JsonWriteOptions {
  pub create_parent_dirs: bool,
  pub trailing_newline: bool,
}

#[derive(Debug)]
pub enum JsonFileReadError {
  Open(std::io::Error),
  Parse(serde_json::Error),
}

#[derive(Debug)]
pub enum JsonFileWriteError {
  CreateParent(std::io::Error),
  Serialize(serde_json::Error),
  Write(std::io::Error),
}

pub fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, JsonFileReadError> {
  let file = fs::File::open(path).map_err(JsonFileReadError::Open)?;
  serde_json::from_reader(BufReader::new(file)).map_err(JsonFileReadError::Parse)
}

pub fn write_json_file<T: Serialize>(path: &Path, value: &T, options: JsonWriteOptions) -> Result<(), JsonFileWriteError> {
  if options.create_parent_dirs {
    if let Some(parent) = path.parent() {
      fs::create_dir_all(parent).map_err(JsonFileWriteError::CreateParent)?;
    }
  }

  let mut bytes = serde_json::to_vec_pretty(value).map_err(JsonFileWriteError::Serialize)?;
  if options.trailing_newline {
    bytes.push(b'\n');
  }

  fs::write(path, bytes).map_err(JsonFileWriteError::Write)
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use serde::{Deserialize, Serialize};

  use super::*;

  #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
  struct Fixture {
    value: String,
  }

  #[test]
  fn read_json_file_round_trips_pretty_json() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("fixture.json");
    fs::write(&path, "{\n  \"value\": \"ok\"\n}").expect("write fixture");

    let parsed = read_json_file::<Fixture>(&path).expect("parse fixture");

    assert_eq!(
      parsed,
      Fixture {
        value: "ok".to_string()
      }
    );
  }

  #[test]
  fn write_json_file_creates_parent_and_appends_newline_when_requested() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join(PathBuf::from("nested/output.json"));

    write_json_file(
      &path,
      &Fixture {
        value: "ok".to_string(),
      },
      JsonWriteOptions {
        create_parent_dirs: true,
        trailing_newline: true,
      },
    )
    .expect("write json");

    let rendered = fs::read(&path).expect("read output");
    assert_eq!(rendered.last().copied(), Some(b'\n'));
  }

  #[test]
  fn write_json_file_without_newline_preserves_non_newline_shape() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("output.json");

    write_json_file(
      &path,
      &Fixture {
        value: "ok".to_string(),
      },
      JsonWriteOptions::default(),
    )
    .expect("write json");

    let rendered = fs::read(&path).expect("read output");
    assert_ne!(rendered.last().copied(), Some(b'\n'));
  }

  #[test]
  fn read_json_file_reports_parse_error() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("broken.json");
    fs::write(&path, "{broken").expect("write broken json");

    let error = read_json_file::<Fixture>(&path).expect_err("should fail");

    assert!(matches!(error, JsonFileReadError::Parse(_)));
  }

  #[test]
  fn read_json_file_reports_open_error_for_missing_path() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join("missing.json");

    let error = read_json_file::<Fixture>(&path).expect_err("should fail");

    assert!(matches!(error, JsonFileReadError::Open(_)));
  }

  #[test]
  fn write_json_file_reports_missing_parent_when_parent_creation_disabled() {
    let temp = tempfile::tempdir().expect("tempdir");
    let path = temp.path().join(PathBuf::from("missing/output.json"));

    let error = write_json_file(
      &path,
      &Fixture {
        value: "ok".to_string(),
      },
      JsonWriteOptions::default(),
    )
    .expect_err("should fail");

    assert!(matches!(error, JsonFileWriteError::Write(_)));
  }
}
