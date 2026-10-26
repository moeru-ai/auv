//! Reads what verification needs from 64-bit ELF files without `readelf` or
//! `patchelf`.

use std::path::Path;

use object::Endianness;
use object::elf::{DT_RPATH, DT_RUNPATH, FileHeader64};
use object::read::elf::{Dyn as _, FileHeader as _};

/// The `DT_RUNPATH`, or the legacy `DT_RPATH`, of an ELF file; `patchelf
/// --set-rpath` writes the former.
pub fn run_path(path: &Path) -> Result<Option<String>, String> {
  let data = read(path)?;
  let error = |error: object::Error| format!("{}: {error}", path.display());
  let header = FileHeader64::<Endianness>::parse(&*data).map_err(error)?;
  let endian = header.endian().map_err(error)?;
  let sections = header.sections(endian, &*data).map_err(error)?;
  let Some((entries, link)) = sections.dynamic(endian, &*data).map_err(error)? else {
    return Ok(None);
  };
  let strings = sections.strings(endian, &*data, link).map_err(error)?;
  let mut rpath = None;
  for entry in entries {
    let tag = entry.tag32(endian);
    if tag == Some(DT_RUNPATH) || (tag == Some(DT_RPATH) && rpath.is_none()) {
      let value = entry.string(endian, strings).map_err(error)?;
      rpath = Some(String::from_utf8_lossy(value).into_owned());
      if tag == Some(DT_RUNPATH) {
        break;
      }
    }
  }
  Ok(rpath)
}

/// The `GLIBC_*` symbol versions an ELF file requires, such as `2.34`.
pub fn glibc_requirements(path: &Path) -> Result<Vec<String>, String> {
  let data = read(path)?;
  let error = |error: object::Error| format!("{}: {error}", path.display());
  let header = FileHeader64::<Endianness>::parse(&*data).map_err(error)?;
  let endian = header.endian().map_err(error)?;
  let sections = header.sections(endian, &*data).map_err(error)?;
  let mut versions = Vec::new();
  let Some((mut needed, link)) = sections.gnu_verneed(endian, &*data).map_err(error)? else {
    return Ok(versions);
  };
  let strings = sections.strings(endian, &*data, link).map_err(error)?;
  while let Some((_, mut auxiliaries)) = needed.next().map_err(error)? {
    while let Some(auxiliary) = auxiliaries.next().map_err(error)? {
      let name = auxiliary.name(endian, strings).map_err(error)?;
      if let Some(version) = String::from_utf8_lossy(name).strip_prefix("GLIBC_") {
        versions.push(version.to_string());
      }
    }
  }
  Ok(versions)
}

fn read(path: &Path) -> Result<Vec<u8>, String> {
  std::fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))
}

#[cfg(all(test, target_os = "linux"))]
#[path = "elf_test.rs"]
mod tests;
