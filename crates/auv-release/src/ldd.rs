//! Dependency resolution as the system loader performs it, through `ldd`.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::rules;

#[derive(Debug, PartialEq, Eq)]
pub enum Dependency {
  Missing(String),
  Resolved { name: String, path: PathBuf },
}

/// Parses `ldd` output. Virtual objects without a path, such as the vDSO, are
/// skipped.
pub fn parse(output: &str) -> Vec<Dependency> {
  let mut dependencies = Vec::new();
  for line in output.lines().map(str::trim) {
    if let Some((name, target)) = line.split_once("=>") {
      let target = target.trim();
      if target.starts_with("not found") {
        dependencies.push(Dependency::Missing(name.trim().to_string()));
      } else if let Some(path) = target.split_whitespace().next().filter(|path| path.starts_with('/')) {
        dependencies.push(Dependency::Resolved {
          name: name.trim().to_string(),
          path: PathBuf::from(path),
        });
      }
    } else if let Some(path) = line.split_whitespace().next().filter(|path| path.starts_with('/')) {
      let path = PathBuf::from(path);
      let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
      dependencies.push(Dependency::Resolved { name, path });
    }
  }
  dependencies
}

/// Fails when `elf` needs a library that is neither in `libraries` nor part
/// of the C runtime.
pub fn check_private_closure(elf: &Path, libraries: &Path) -> Result<(), String> {
  for dependency in parse(&ldd(elf, Some(libraries))?) {
    match dependency {
      Dependency::Missing(name) => return Err(format!("{} has an unresolved dependency: {name}", elf.display())),
      Dependency::Resolved { path, .. } if path.starts_with(libraries) => {}
      Dependency::Resolved { path, .. } => {
        let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        if !rules::is_runtime_provided(&name) {
          return Err(format!("{} resolves non-runtime dependency {name} outside the private bundle: {}", elf.display(), path.display()));
        }
      }
    }
  }
  Ok(())
}

/// Whether the executable, through its own run path, loads Tesseract or
/// Leptonica from `libraries` rather than from the host.
pub fn links_bundled_ocr(executable: &Path, libraries: &Path) -> Result<bool, String> {
  Ok(parse(&ldd(executable, None)?).into_iter().any(|dependency| match dependency {
    Dependency::Resolved { name, path } => (name.starts_with("libtesseract") || name.starts_with("liblept")) && path.starts_with(libraries),
    Dependency::Missing(_) => false,
  }))
}

/// `ldd` output. Its exit status is ignored: it fails for files that are not
/// dynamic executables, which then have no dependencies to check.
fn ldd(elf: &Path, library_path: Option<&Path>) -> Result<String, String> {
  let mut command = Command::new("ldd");
  command.arg(elf);
  if let Some(library_path) = library_path {
    command.env("LD_LIBRARY_PATH", library_path);
  }
  let output = command.output().map_err(|error| format!("failed to run ldd on {}: {error}", elf.display()))?;
  Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
#[path = "ldd_test.rs"]
mod tests;
