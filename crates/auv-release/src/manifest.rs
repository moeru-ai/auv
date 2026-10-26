//! `manifest.json` at the root of a Linux release bundle: what each file is,
//! which Debian package it came from and under which license.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use auv_bundle_layout as layout;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
  pub schema_version: u32,
  pub name: String,
  pub version: String,
  pub target: String,
  pub glibc_baseline: String,
  pub layout: Layout,
  pub libraries: Vec<Library>,
  pub models: Vec<Model>,
  pub files: Vec<File>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
  pub executable: String,
  pub library_directory: String,
  pub tessdata_directory: String,
}

impl Layout {
  pub fn current() -> Self {
    Self {
      executable: layout::EXECUTABLE.to_string(),
      library_directory: layout::relative(layout::LIBRARY_DIR),
      tessdata_directory: layout::relative(layout::TESSDATA_DIR),
    }
  }
}

/// A shared library copied into the library directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Library {
  pub path: String,
  pub package: String,
  pub license: String,
}

/// A Tesseract model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
  pub language: String,
  pub path: String,
  pub package: String,
  pub license: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct File {
  pub path: String,
  pub sha256: String,
  pub size: u64,
}

/// Bundle-relative path of the copyright file for a Debian package.
pub fn system_license(package: &str) -> String {
  format!("{}/{package}.copyright", layout::relative(layout::SYSTEM_LICENSE_DIR))
}

/// Every regular file below `root` except the manifest itself, keyed by its
/// `/`-separated bundle path.
pub fn bundle_files(root: &Path) -> Result<BTreeMap<String, PathBuf>, String> {
  let mut files = BTreeMap::new();
  let mut pending = vec![root.to_path_buf()];
  while let Some(directory) = pending.pop() {
    let entries = std::fs::read_dir(&directory).map_err(|error| format!("failed to list {}: {error}", directory.display()))?;
    for entry in entries {
      let path = entry.map_err(|error| format!("failed to list {}: {error}", directory.display()))?.path();
      let metadata = std::fs::symlink_metadata(&path).map_err(|error| format!("failed to inspect {}: {error}", path.display()))?;
      if metadata.is_dir() {
        pending.push(path);
      } else if metadata.is_file() {
        let relative = relative_path(root, &path)?;
        if relative != layout::MANIFEST {
          files.insert(relative, path);
        }
      }
    }
  }
  Ok(files)
}

/// Hash and size of every file in the bundle, sorted by path.
pub fn file_entries(root: &Path) -> Result<Vec<File>, String> {
  bundle_files(root)?
    .into_iter()
    .map(|(relative, path)| {
      let (sha256, size) = digest(&path)?;
      Ok(File {
        path: relative,
        sha256,
        size,
      })
    })
    .collect()
}

pub fn digest(path: &Path) -> Result<(String, u64), String> {
  let mut file = std::fs::File::open(path).map_err(|error| format!("failed to open {}: {error}", path.display()))?;
  let mut hasher = Sha256::new();
  let mut buffer = vec![0; 1 << 16];
  let mut size = 0;
  loop {
    let read = file.read(&mut buffer).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if read == 0 {
      break;
    }
    hasher.update(&buffer[..read]);
    size += read as u64;
  }
  Ok((format!("{:x}", hasher.finalize()), size))
}

pub fn relative_path(root: &Path, path: &Path) -> Result<String, String> {
  let relative = path.strip_prefix(root).map_err(|_| format!("{} is outside {}", path.display(), root.display()))?;
  Ok(relative.components().map(|component| component.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
}

pub fn read(root: &Path) -> Result<Manifest, String> {
  let path = root.join(layout::MANIFEST);
  let text = std::fs::read_to_string(&path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  serde_json::from_str(&text).map_err(|error| format!("invalid {}: {error}", path.display()))
}

pub fn write(root: &Path, manifest: &Manifest) -> Result<(), String> {
  let path = root.join(layout::MANIFEST);
  let text = serde_json::to_string_pretty(manifest).map_err(|error| error.to_string())?;
  std::fs::write(&path, format!("{text}\n")).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

#[cfg(test)]
#[path = "manifest_test.rs"]
mod tests;
