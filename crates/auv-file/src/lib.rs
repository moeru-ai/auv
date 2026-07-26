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
