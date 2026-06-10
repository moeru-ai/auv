use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use thiserror::Error;

use crate::model::BalatroState;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputMode {
  Human,
  Json,
  JsonFile(PathBuf),
}

#[derive(Debug, Error)]
pub enum OutputError {
  #[error("failed to write JSON output: {0}")]
  Io(#[from] io::Error),
  #[error("failed to serialize JSON output: {0}")]
  Json(#[from] serde_json::Error),
}

pub fn write_json_file(path: &Path, value: &impl Serialize) -> Result<(), OutputError> {
  let mut bytes = serde_json::to_vec_pretty(value)?;
  bytes.push(b'\n');

  let temp_path = temporary_path(path);
  let mut file = OpenOptions::new()
    .create_new(true)
    .write(true)
    .open(&temp_path)?;
  file.write_all(&bytes)?;
  file.sync_all()?;
  drop(file);

  fs::rename(&temp_path, path)?;
  Ok(())
}

fn temporary_path(path: &Path) -> PathBuf {
  let nonce = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map(|duration| duration.as_nanos())
    .unwrap_or_default();
  let file_name = path
    .file_name()
    .and_then(|name| name.to_str())
    .unwrap_or("auv-game-balatro-output");
  path.with_file_name(format!(".{file_name}.{}.{}.tmp", process::id(), nonce))
}

pub struct HumanStateSummary<'a>(pub &'a BalatroState);

impl fmt::Display for HumanStateSummary<'_> {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    let state = self.0;
    write!(
      formatter,
      "Balatro state: phase={:?}, hand={}, jokers={}, consumables={}, store_items={}, buttons={}",
      state.phase,
      state.hand.len(),
      state.jokers.len(),
      state.consumables.len(),
      state.store.item_count,
      state.buttons.len()
    )
  }
}
