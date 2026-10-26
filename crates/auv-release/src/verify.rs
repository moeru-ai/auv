//! Checks of release artifacts: the Linux archive, the glibc baseline of ELF
//! files, and the staged Linux npm platform package.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use auv_bundle_layout as layout;

use crate::manifest::{self, Layout};
use crate::rules;
use crate::system::{self, is_executable, is_non_empty_file, output};
use crate::{elf, ldd};

/// Unpacks a Linux release archive and checks it against its manifest, the
/// bundle layout and the glibc baseline, then starts it.
pub fn linux_archive(archive: &Path, target: &str, version: &str) -> Result<(), String> {
  system::require_commands(&["ldd", "tar"])?;
  let unpacked = tempfile::tempdir().map_err(|error| format!("failed to create a directory: {error}"))?;
  let root = unpacked.path();
  let mut tar = Command::new("tar");
  tar.arg("-xzf").arg(archive).arg("-C").arg(root);
  system::checked(tar)?;

  let executable = root.join(layout::EXECUTABLE);
  let libraries = layout::path(root, layout::LIBRARY_DIR);
  ensure(is_executable(&executable), "the archive has no executable auv")?;
  ensure(is_non_empty_file(&layout::path(root, layout::TESSDATA_DIR).join("eng.traineddata")), "the archive has no eng.traineddata")?;
  ensure(is_non_empty_file(&layout::path(root, layout::PROJECT_LICENSE_DIR).join("LICENSE")), "the archive has no AUV license")?;
  let files = manifest::bundle_files(root)?;
  let library_prefix = format!("{}/", layout::relative(layout::LIBRARY_DIR));
  let license_prefix = format!("{}/", layout::relative(layout::SYSTEM_LICENSE_DIR));
  ensure(files.keys().any(|path| path.starts_with(&library_prefix)), "the archive bundles no libraries")?;
  ensure(
    files.keys().any(|path| path.starts_with(&license_prefix) && path.ends_with(".copyright")),
    "the archive has no system package copyright files",
  )?;

  check_manifest(root, target, version)?;
  check_run_paths(root)?;
  check_pipewire_client(root)?;

  let elves = std::iter::once(executable.clone())
    .chain(files.iter().filter(|(path, _)| path.starts_with(&library_prefix)).map(|(_, path)| path.clone()))
    .collect::<Vec<_>>();
  glibc_baseline(&elves)?;
  for elf in &elves {
    ldd::check_private_closure(elf, &libraries)?;
  }
  ensure(ldd::links_bundled_ocr(&executable, &libraries)?, "auv does not load Tesseract or Leptonica from the bundle")?;
  output(&executable.to_string_lossy(), ["--help"])?;
  Ok(())
}

fn check_manifest(root: &Path, target: &str, version: &str) -> Result<(), String> {
  let manifest = manifest::read(root)?;
  ensure(manifest.schema_version == manifest::SCHEMA_VERSION, "unexpected manifest schemaVersion")?;
  ensure(manifest.target == target, &format!("manifest target is {}, expected {target}", manifest.target))?;
  ensure(manifest.version == version, &format!("manifest version is {}, expected {version}", manifest.version))?;
  ensure(manifest.glibc_baseline == rules::GLIBC_BASELINE, "manifest glibcBaseline differs from the release baseline")?;
  ensure(manifest.layout == Layout::current(), "manifest layout differs from auv-bundle-layout")?;

  // PipeWire modules and SPA plugins live in subdirectories and are listed in
  // `files` only.
  let library_directory = layout::path(root, layout::LIBRARY_DIR);
  let mut actual = BTreeSet::new();
  for entry in std::fs::read_dir(&library_directory).map_err(|error| format!("failed to list {}: {error}", library_directory.display()))? {
    let path = entry.map_err(|error| error.to_string())?.path();
    if path.is_file() {
      actual.insert(manifest::relative_path(root, &path)?);
    }
  }
  let recorded = manifest.libraries.iter().map(|library| library.path.clone()).collect::<BTreeSet<_>>();
  ensure(
    recorded == actual,
    &format!("manifest libraries differ from the archive: {:?}", recorded.symmetric_difference(&actual).collect::<Vec<_>>()),
  )?;
  for library in &manifest.libraries {
    ensure(!library.package.is_empty(), &format!("{} has no package", library.path))?;
    ensure(root.join(&library.license).is_file(), &format!("{} has no license file {}", library.path, library.license))?;
  }

  let [model] = manifest.models.as_slice() else {
    return Err("the manifest must list exactly one model".to_string());
  };
  ensure(
    model.language == "eng" && model.path == format!("{}/eng.traineddata", layout::relative(layout::TESSDATA_DIR)),
    "the manifest model must be the English Tesseract model",
  )?;
  ensure(!model.package.is_empty(), "the model has no package")?;
  ensure(root.join(&model.license).is_file(), &format!("the model has no license file {}", model.license))?;

  let actual = manifest::file_entries(root)?;
  let recorded = manifest.files.iter().map(|file| file.path.as_str()).collect::<BTreeSet<_>>();
  let present = actual.iter().map(|file| file.path.as_str()).collect::<BTreeSet<_>>();
  ensure(
    recorded == present,
    &format!("manifest files differ from the archive: {:?}", recorded.symmetric_difference(&present).collect::<Vec<_>>()),
  )?;
  for file in &actual {
    let entry = manifest.files.iter().find(|entry| entry.path == file.path).expect("same paths");
    ensure(entry == file, &format!("{} does not match its manifest hash or size", file.path))?;
  }
  Ok(())
}

fn check_run_paths(root: &Path) -> Result<(), String> {
  for (relative, path) in manifest::bundle_files(root)? {
    if let Some(expected) = rules::expected_run_path(&relative) {
      let actual = elf::run_path(&path)?;
      ensure(actual.as_deref() == Some(expected.as_str()), &format!("{relative} has run path {actual:?}, expected {expected}"))?;
    }
  }
  Ok(())
}

/// The bundled client configuration must only name modules and plugins the
/// bundle ships; libpipewire cannot start a context otherwise.
fn check_pipewire_client(root: &Path) -> Result<(), String> {
  let client_conf = layout::path(root, layout::PIPEWIRE_CONFIG_DIR).join(layout::PIPEWIRE_CLIENT_CONF);
  ensure(is_non_empty_file(&client_conf), "the archive has no PipeWire client.conf")?;
  let text = std::fs::read_to_string(&client_conf).map_err(|error| format!("failed to read {}: {error}", client_conf.display()))?;
  let references = rules::client_conf_references(&text);
  let modules = layout::path(root, layout::PIPEWIRE_MODULE_DIR);
  let plugins = layout::path(root, layout::SPA_PLUGIN_DIR);
  for module in &references.modules {
    ensure(modules.join(format!("{module}.so")).is_file(), &format!("client.conf names missing module {module}"))?;
  }
  for plugin in &references.plugins {
    ensure(plugins.join(format!("{plugin}.so")).is_file(), &format!("client.conf names missing SPA plugin {plugin}"))?;
  }
  ensure(modules.join(layout::PIPEWIRE_NATIVE_PROTOCOL_MODULE).is_file(), "the archive has no PipeWire native protocol module")?;
  let libraries = layout::path(root, layout::LIBRARY_DIR);
  for entry in std::fs::read_dir(&libraries).map_err(|error| format!("failed to list {}: {error}", libraries.display()))? {
    let name = entry.map_err(|error| error.to_string())?.file_name();
    ensure(
      !name.to_string_lossy().starts_with("libpipewire-module-"),
      &format!("PipeWire modules must only live in {}", layout::relative(layout::PIPEWIRE_MODULE_DIR)),
    )?;
  }
  Ok(())
}

/// Fails when any ELF file requires a glibc symbol version newer than
/// [`rules::GLIBC_BASELINE`]; prints the newest one required.
pub fn glibc_baseline(elves: &[PathBuf]) -> Result<(), String> {
  let mut newest: Option<(String, PathBuf)> = None;
  for elf in elves {
    for version in elf::glibc_requirements(elf)? {
      if newest.as_ref().is_none_or(|(current, _)| rules::compare_glibc_versions(&version, current).is_gt()) {
        newest = Some((version, elf.clone()));
      }
    }
  }
  let Some((newest, elf)) = newest else {
    println!("no glibc symbol versions required");
    return Ok(());
  };
  if rules::compare_glibc_versions(&newest, rules::GLIBC_BASELINE).is_gt() {
    return Err(format!("{} requires GLIBC_{newest}, newer than the declared GLIBC_{} baseline", elf.display(), rules::GLIBC_BASELINE));
  }
  println!("maximum required glibc symbol: GLIBC_{newest}");
  Ok(())
}

/// Checks a Linux npm platform package staged from the release archive, and
/// that `npm pack` keeps the bundle.
pub fn npm_package(package_root: &Path) -> Result<(), String> {
  system::require_commands(&["npm", "tar"])?;
  let bin = package_root.join("bin");
  ensure(is_executable(&bin.join(layout::EXECUTABLE)), "the package has no executable bin/auv")?;
  ensure(is_non_empty_file(&bin.join(layout::MANIFEST)), "the package has no bin/manifest.json")?;
  ensure(is_non_empty_file(&layout::path(&bin, layout::TESSDATA_DIR).join("eng.traineddata")), "the package has no eng.traineddata")?;
  let files = manifest::bundle_files(&bin)?;
  let library_prefix = format!("{}/", layout::relative(layout::LIBRARY_DIR));
  let license_prefix = format!("{}/", layout::relative(layout::SYSTEM_LICENSE_DIR));
  ensure(files.keys().any(|path| path.starts_with(&library_prefix)), "the package bundles no libraries")?;
  ensure(
    files.keys().any(|path| path.starts_with(&license_prefix) && path.ends_with(".copyright")),
    "the package has no system package copyright files",
  )?;

  let package_json = package_root.join("package.json");
  let package: serde_json::Value = serde_json::from_str(
    &std::fs::read_to_string(&package_json).map_err(|error| format!("failed to read {}: {error}", package_json.display()))?,
  )
  .map_err(|error| format!("invalid {}: {error}", package_json.display()))?;
  let published = package["files"].as_array().ok_or("package.json has no files list")?;
  ensure(published.iter().any(|file| file == "bin"), "package.json files must publish bin")?;
  ensure(!published.iter().any(|file| file == "bin/auv"), "package.json files must publish the whole bin directory, not only bin/auv")?;
  ensure(manifest::read(&bin)?.layout == Layout::current(), "bin/manifest.json layout differs from auv-bundle-layout")?;

  let packed = tempfile::tempdir().map_err(|error| format!("failed to create a directory: {error}"))?;
  let mut npm = Command::new("npm");
  npm.current_dir(package_root).arg("pack").arg("--pack-destination").arg(packed.path()).arg("--silent");
  let tarball = system::checked(npm)?;
  let tarball = packed.path().join(tarball.lines().last().unwrap_or_default().trim());
  let entries = output("tar", [std::ffi::OsStr::new("-tzf"), tarball.as_os_str()])?;
  for expected in [
    format!("package/bin/{}", layout::EXECUTABLE),
    format!("package/bin/{library_prefix}"),
    format!("package/bin/{}/eng.traineddata", layout::relative(layout::TESSDATA_DIR)),
    format!("package/bin/{license_prefix}"),
  ] {
    ensure(entries.lines().any(|entry| entry.starts_with(&expected)), &format!("npm pack dropped {expected}"))?;
  }
  Ok(())
}

fn ensure(condition: bool, message: &str) -> Result<(), String> {
  if condition {
    Ok(())
  } else {
    Err(message.to_string())
  }
}
