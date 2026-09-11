//! Daemon-side selection of Runner providers.
//!
//! A provider manifest selects how the daemon reaches a RunnerClass. Runtime
//! business services belong to the running endpoint and are deliberately not
//! duplicated or pinned in this configuration.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const LOCAL_RUNNER_CLASS: &str = "auv.core.local";

/// Experimental daemon-side selection of one RunnerClass provider.
///
/// `runner_class` is the stable selector known before connecting. Display
/// Registration supplies identity and process configuration. Standard gRPC
/// Health reports endpoint readiness; Reflection remains available to callers
/// through the opaque route.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RunnerProviderConfig {
  /// RunnerClass served by this provider.
  pub runner_class: String,
  /// Runtime used to reach the provider.
  pub runtime: RunnerRuntime,
}

/// Daemon-side transport used to realize one Runner provider.
///
/// This is a Rust/configuration enum. Compatible runtimes expose the same
/// the same standard gRPC service contract; the transport choice is
/// intentionally absent from the protobuf resource model.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", content = "config", rename_all = "kebab-case")]
pub enum RunnerRuntime {
  /// Spawn a child process that inherits the daemon Runner transport.
  Executable(ExecutableRunnerRuntime),
  /// Connect to an already-running gRPC provider.
  RemoteGrpc(RemoteGrpcRunnerRuntime),
}

/// Executable child-process runtime configuration.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct ExecutableRunnerRuntime {
  /// Program passed to the operating system at spawn time.
  ///
  /// A bare name uses normal `PATH` lookup. A relative path containing a
  /// directory is resolved relative to the provider manifest by `load_json`.
  /// Absolute paths remain unchanged.
  pub executable: PathBuf,
  #[serde(default)]
  /// Arguments passed to the child process.
  pub arguments: Vec<String>,
  /// Optional child working directory. A relative directory is resolved from
  /// the provider manifest directory by `load_json`.
  pub working_directory: Option<PathBuf>,
  /// Environment entries overlaid on the daemon's inherited environment.
  ///
  /// This is intentionally not an allowlist and does not request that the
  /// daemon clear unrelated variables before spawning the child.
  #[serde(default)]
  pub environment: BTreeMap<String, String>,
}

/// Remote gRPC Runner runtime configuration.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct RemoteGrpcRunnerRuntime {
  /// Runner endpoint URI.
  pub endpoint: String,
  // TODO(remote-runner-credentials): add a typed daemon-owned credential
  // reference when authenticated outbound Runner connections are approved;
  // caller Authorization must never be forwarded to a provider endpoint.
}

/// First-party runtimes supplied by the process that embeds the API server.
#[derive(Clone, Debug, Default)]
pub struct FirstPartyRunnerRuntimes {
  /// Runtime for the built-in local Driver Runner.
  pub local_driver: Option<RunnerRuntime>,
}

impl RunnerProviderConfig {
  /// Loads one JSON provider configuration.
  ///
  /// The manifest selects a program or endpoint; it is not a filesystem trust
  /// boundary. Executable existence, permissions, symlinks, ownership, and
  /// `PATH` resolution are therefore left to the operating system at spawn.
  pub fn load_json(path: impl AsRef<Path>) -> Result<Self, RunnerProviderConfigError> {
    let path = path.as_ref();
    let bytes = std::fs::read(path).map_err(|source| RunnerProviderConfigError::FileRead {
      path: path.to_path_buf(),
      source,
    })?;
    let mut config: Self = serde_json::from_slice(&bytes).map_err(|source| RunnerProviderConfigError::Json {
      path: path.to_path_buf(),
      source,
    })?;
    config.resolve_manifest_relative_executable(path);
    Ok(config)
  }

  fn resolve_manifest_relative_executable(&mut self, manifest_path: &Path) {
    let RunnerRuntime::Executable(runtime) = &mut self.runtime else {
      return;
    };
    let directory = manifest_path.parent().unwrap_or_else(|| Path::new(""));
    if runtime.executable.is_absolute() || is_bare_executable_name(&runtime.executable) {
      // Bare executable names retain normal operating-system PATH lookup.
    } else {
      runtime.executable = directory.join(&runtime.executable);
    }
    if let Some(working_directory) = &mut runtime.working_directory
      && !working_directory.is_absolute()
    {
      *working_directory = directory.join(&*working_directory);
    }
  }
}

fn is_bare_executable_name(path: &Path) -> bool {
  path.parent().is_none_or(|parent| parent.as_os_str().is_empty())
}

/// Failure while loading a Runner provider manifest.
#[derive(Debug, thiserror::Error)]
pub enum RunnerProviderConfigError {
  /// Reading the manifest file failed.
  #[error("failed to read Runner provider config {path}: {source}")]
  FileRead {
    /// Manifest path.
    path: PathBuf,
    /// Filesystem error returned while reading.
    #[source]
    source: std::io::Error,
  },
  /// Decoding the manifest JSON failed.
  #[error("invalid Runner provider JSON {path}: {source}")]
  Json {
    /// Manifest path.
    path: PathBuf,
    /// JSON decoding error.
    #[source]
    source: serde_json::Error,
  },
  /// A manifest contains an invalid RunnerClass identity.
  #[error("invalid RunnerClass identity: {0}")]
  InvalidRunnerClass(String),
  /// A manifest attempts to replace a daemon-owned RunnerClass.
  #[error("RunnerClass {0} is reserved by the daemon")]
  ReservedRunnerClass(String),
  /// More than one manifest entry owns the same RunnerClass.
  #[error("RunnerClass is configured more than once: {0}")]
  DuplicateRunnerClass(String),
}

#[derive(Clone, Debug)]
pub(crate) struct RegisteredRunnerProvider {
  pub(crate) runner_class: String,
  pub(crate) runtime: RunnerRuntime,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RunnerProviderRegistry {
  providers: BTreeMap<String, RegisteredRunnerProvider>,
}

impl RunnerProviderRegistry {
  pub(crate) fn build_with_first_party(
    local_runtime: Option<RunnerRuntime>,
    custom: Vec<RunnerProviderConfig>,
  ) -> Result<Self, RunnerProviderConfigError> {
    let mut registry = Self::default();
    if let Some(runtime) = local_runtime {
      registry.insert(first_party_provider(LOCAL_RUNNER_CLASS, runtime))?;
    }
    for config in custom {
      registry.insert(register_custom(config)?)?;
    }
    Ok(registry)
  }

  fn insert(&mut self, provider: RegisteredRunnerProvider) -> Result<(), RunnerProviderConfigError> {
    if self.providers.contains_key(&provider.runner_class) {
      return Err(RunnerProviderConfigError::DuplicateRunnerClass(provider.runner_class));
    }
    self.providers.insert(provider.runner_class.clone(), provider);
    Ok(())
  }

  pub(crate) fn get(&self, runner_class: &str) -> Option<&RegisteredRunnerProvider> {
    self.providers.get(runner_class)
  }

  pub(crate) fn values(&self) -> impl Iterator<Item = &RegisteredRunnerProvider> {
    self.providers.values()
  }
}

fn first_party_provider(runner_class: &str, runtime: RunnerRuntime) -> RegisteredRunnerProvider {
  RegisteredRunnerProvider {
    runner_class: runner_class.to_string(),
    runtime,
  }
}

fn register_custom(config: RunnerProviderConfig) -> Result<RegisteredRunnerProvider, RunnerProviderConfigError> {
  validate_class_identity(&config.runner_class)?;
  if is_reserved_runner_class(&config.runner_class) {
    return Err(RunnerProviderConfigError::ReservedRunnerClass(config.runner_class));
  }
  Ok(RegisteredRunnerProvider {
    runner_class: config.runner_class,
    runtime: config.runtime,
  })
}

fn validate_class_identity(value: &str) -> Result<(), RunnerProviderConfigError> {
  if value.is_empty() {
    return Err(RunnerProviderConfigError::InvalidRunnerClass(value.to_string()));
  }
  Ok(())
}

fn is_reserved_runner_class(value: &str) -> bool {
  value == LOCAL_RUNNER_CLASS
}

#[cfg(test)]
#[path = "runner_provider_test.rs"]
mod tests;
