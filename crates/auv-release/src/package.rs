//! `auv-release package-linux`: the self-contained glibc Linux archive.
//!
//! Runs in the Debian 12 release container. The executable's shared-library
//! closure, the PipeWire client files and the English Tesseract model are
//! copied from the system, together with the copyright file of every Debian
//! package they came from.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use auv_bundle_layout as layout;

use crate::manifest::{self, Library, Manifest, Model};
use crate::rules;
use crate::system::{self, create_dir, install, output};

pub struct PackageOptions {
  pub binary: PathBuf,
  pub archive: PathBuf,
  pub target: String,
  pub version: String,
  pub project_license: PathBuf,
}

/// PipeWire client configuration of the build system.
const CLIENT_CONF: &str = "/usr/share/pipewire/client.conf";
/// Debian package that provides the English Tesseract model.
const ENGLISH_MODEL_PACKAGE: &str = "tesseract-ocr-eng";

pub fn run(options: &PackageOptions) -> Result<(), String> {
  system::require_commands(&["dpkg-query", "lddtree", "patchelf", "tar"])?;
  for required in [&options.binary, &options.project_license] {
    if !required.is_file() {
      return Err(format!("required file not found: {}", required.display()));
    }
  }

  let staging = tempfile::tempdir().map_err(|error| format!("failed to create a staging directory: {error}"))?;
  let mut bundle = Bundle {
    root: staging.path().to_path_buf(),
    libraries: Vec::new(),
    licensed: BTreeSet::new(),
  };
  // The archive's root entry becomes the install directory; temporary
  // directories are private by default.
  system::set_mode(&bundle.root, 0o755)?;
  for directory in [
    layout::LIBRARY_DIR,
    layout::TESSDATA_DIR,
    layout::PROJECT_LICENSE_DIR,
    layout::SYSTEM_LICENSE_DIR,
  ] {
    create_dir(&bundle.path(directory))?;
  }
  install(&options.binary, &bundle.root.join(layout::EXECUTABLE), 0o755)?;
  install(&options.project_license, &bundle.path(layout::PROJECT_LICENSE_DIR).join("LICENSE"), 0o644)?;

  bundle.add_dependencies(&options.binary)?;
  bundle.add_pipewire_client(&options.binary)?;
  let model_package = bundle.add_english_model()?;
  bundle.set_run_paths()?;

  let mut libraries = std::mem::take(&mut bundle.libraries);
  libraries.sort_by(|left, right| left.path.cmp(&right.path));
  let model_path = format!("{}/eng.traineddata", layout::relative(layout::TESSDATA_DIR));
  let manifest = Manifest {
    schema_version: manifest::SCHEMA_VERSION,
    name: layout::EXECUTABLE.to_string(),
    version: options.version.clone(),
    target: options.target.clone(),
    glibc_baseline: rules::GLIBC_BASELINE.to_string(),
    layout: manifest::Layout::current(),
    libraries,
    models: vec![Model {
      language: "eng".to_string(),
      path: model_path,
      license: manifest::system_license(&model_package),
      package: model_package,
    }],
    files: manifest::file_entries(&bundle.root)?,
  };
  manifest::write(&bundle.root, &manifest)?;

  // TODO(linux-minimal-artifact): keep the default archive self-contained; add a
  // separate auv-minimal artifact only after its distro dependency contract and
  // package-manager documentation are owner-approved.
  let archive = std::path::absolute(&options.archive).map_err(|error| format!("invalid archive path: {error}"))?;
  if let Some(parent) = archive.parent() {
    create_dir(parent)?;
  }
  let mut tar = Command::new("tar");
  tar
    .args([
      "--sort=name",
      "--mtime=@0",
      "--owner=0",
      "--group=0",
      "--numeric-owner",
      "-czf",
    ])
    .arg(&archive)
    .arg("-C")
    .arg(&bundle.root)
    .arg(".");
  system::checked(tar)?;
  Ok(())
}

struct Bundle {
  root: PathBuf,
  libraries: Vec<Library>,
  /// Packages whose copyright file is already in the bundle.
  licensed: BTreeSet<String>,
}

impl Bundle {
  fn path(&self, segments: &[&str]) -> PathBuf {
    layout::path(&self.root, segments)
  }

  /// Copies the shared-library closure of one ELF file into the library
  /// directory.
  fn add_dependencies(&mut self, elf: &Path) -> Result<(), String> {
    let elf_resolved = canonical(elf)?;
    for dependency in dependency_closure(elf)? {
      let dependency = PathBuf::from(dependency);
      if !dependency.is_file() {
        continue;
      }
      let resolved = canonical(&dependency)?;
      let name = file_name(&dependency)?;
      if resolved == elf_resolved || rules::is_runtime_provided(&name) {
        continue;
      }
      // A PipeWire module that links another module finds it beside itself.
      if is_pipewire_module(&resolved) {
        if !self.path(layout::PIPEWIRE_MODULE_DIR).join(&name).is_file() {
          return Err(format!("{} needs PipeWire module {name}, which client.conf does not load", elf.display()));
        }
        continue;
      }

      let destination = self.path(layout::LIBRARY_DIR).join(&name);
      if destination.exists() {
        if std::fs::read(&resolved).ok() != std::fs::read(&destination).ok() {
          return Err(format!("dependency basename collision for {name}"));
        }
        continue;
      }
      install(&resolved, &destination, 0o644)?;
      let package = package_for_file(&resolved).or_else(|_| package_for_file(&dependency))?;
      self.add_license(&package)?;
      self.libraries.push(Library {
        path: format!("{}/{name}", layout::relative(layout::LIBRARY_DIR)),
        license: manifest::system_license(&package),
        package,
      });
    }
    Ok(())
  }

  // NOTICE(linux-bundled-pipewire): the bundled libpipewire loads client modules,
  // SPA plugins and client.conf at runtime. Host files built for a newer
  // PipeWire need symbols it lacks (Debian 13 PipeWire 1.4 and Ubuntu 26.04
  // PipeWire 1.6 fail with `undefined symbol: pw_log_topic_register`), so ship
  // the matching client files and let `auv_driver_linux::bundle` point
  // libpipewire at them. The bundle still talks to the host daemon through its
  // socket protocol, as Flatpak runtimes do.
  fn add_pipewire_client(&mut self, binary: &Path) -> Result<(), String> {
    let pipewire = dependency_closure(binary)?
      .into_iter()
      .find(|dependency| dependency.ends_with("/libpipewire-0.3.so.0"))
      .ok_or_else(|| format!("{} does not link libpipewire-0.3.so.0", binary.display()))?;
    let pipewire = canonical(Path::new(&pipewire))?;
    let library_directory = pipewire.parent().ok_or("libpipewire has no parent directory")?;
    let client_conf = Path::new(CLIENT_CONF);
    let text =
      std::fs::read_to_string(client_conf).map_err(|error| format!("PipeWire client configuration not found: {CLIENT_CONF}: {error}"))?;
    let config_directory = self.path(layout::PIPEWIRE_CONFIG_DIR);
    install(client_conf, &config_directory.join(layout::PIPEWIRE_CLIENT_CONF), 0o644)?;
    self.add_license(&package_for_file(client_conf)?)?;

    let references = rules::client_conf_references(&text);
    if references.modules.is_empty() {
      return Err(format!("no PipeWire modules found in {CLIENT_CONF}"));
    }
    // Copy every module before resolving their dependencies, so a module that
    // links another one finds it already bundled.
    let mut sources = Vec::new();
    for module in &references.modules {
      let source = library_directory.join("pipewire-0.3").join(format!("{module}.so"));
      if !source.is_file() {
        return Err(format!("PipeWire module not found: {}", source.display()));
      }
      install(&source, &self.path(layout::PIPEWIRE_MODULE_DIR).join(format!("{module}.so")), 0o644)?;
      self.add_license(&package_for_file(&source)?)?;
      sources.push(source);
    }
    for source in &sources {
      self.add_dependencies(source)?;
    }
    for plugin in &references.plugins {
      let source = library_directory.join("spa-0.2").join(format!("{plugin}.so"));
      if !source.is_file() {
        return Err(format!("SPA plugin not found: {}", source.display()));
      }
      install(&source, &self.path(layout::SPA_PLUGIN_DIR).join(format!("{plugin}.so")), 0o644)?;
      self.add_license(&package_for_file(&source)?)?;
      self.add_dependencies(&source)?;
    }
    Ok(())
  }

  /// Copies `eng.traineddata` and returns the package that provided it.
  fn add_english_model(&mut self) -> Result<String, String> {
    let files = output("dpkg-query", ["-L", ENGLISH_MODEL_PACKAGE])?;
    let model = files
      .lines()
      .find(|line| line.ends_with("/eng.traineddata"))
      .map(PathBuf::from)
      .filter(|model| model.is_file())
      .ok_or_else(|| format!("{ENGLISH_MODEL_PACKAGE} did not provide eng.traineddata"))?;
    install(&model, &self.path(layout::TESSDATA_DIR).join("eng.traineddata"), 0o644)?;
    let package = package_for_file(&model)?;
    self.add_license(&package)?;
    Ok(package)
  }

  fn add_license(&mut self, package: &str) -> Result<(), String> {
    if !self.licensed.insert(package.to_string()) {
      return Ok(());
    }
    let copyright = Path::new("/usr/share/doc").join(package).join("copyright");
    if !copyright.is_file() {
      return Err(format!("copyright file not found for package {package}"));
    }
    install(&copyright, &self.path(layout::SYSTEM_LICENSE_DIR).join(format!("{package}.copyright")), 0o644)
  }

  fn set_run_paths(&self) -> Result<(), String> {
    for (relative, path) in manifest::bundle_files(&self.root)? {
      if let Some(run_path) = rules::expected_run_path(&relative) {
        let mut patchelf = Command::new("patchelf");
        patchelf.arg("--set-rpath").arg(run_path).arg(&path);
        system::checked(patchelf)?;
      }
    }
    Ok(())
  }
}

/// `lddtree -l`: the ELF file itself, then every library it loads.
fn dependency_closure(elf: &Path) -> Result<Vec<String>, String> {
  Ok(output("lddtree", [std::ffi::OsStr::new("-l"), elf.as_os_str()])?.lines().map(str::to_string).collect())
}

/// The Debian package that installed `path`.
fn package_for_file(path: &Path) -> Result<String, String> {
  let owners = output("dpkg-query", [std::ffi::OsStr::new("-S"), path.as_os_str()])
    .map_err(|_| format!("no Debian package owns {}", path.display()))?;
  let first = owners.lines().next().unwrap_or_default();
  let package = first.split(':').next().unwrap_or_default().trim();
  if package.is_empty() {
    return Err(format!("no Debian package owns {}", path.display()));
  }
  Ok(package.to_string())
}

fn is_pipewire_module(path: &Path) -> bool {
  let parent = path.parent().and_then(Path::file_name).and_then(|name| name.to_str());
  let name = path.file_name().and_then(|name| name.to_str()).unwrap_or_default();
  parent == Some("pipewire-0.3") && name.starts_with("libpipewire-module-") && name.ends_with(".so")
}

fn canonical(path: &Path) -> Result<PathBuf, String> {
  std::fs::canonicalize(path).map_err(|error| format!("failed to resolve {}: {error}", path.display()))
}

fn file_name(path: &Path) -> Result<String, String> {
  path.file_name().and_then(|name| name.to_str()).map(str::to_string).ok_or_else(|| format!("invalid file name: {}", path.display()))
}
