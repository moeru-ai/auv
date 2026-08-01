//! Trusted schema admission for daemon-supervised custom Runner providers.
//!
//! Reflection describes what a child claims to serve. This module admits that
//! claim only when it is an exact match for daemon-owned policy and a bounded,
//! self-contained protobuf descriptor closure.

use prost::Message;
use prost_reflect::{DescriptorPool, Value};
use prost_types::{DescriptorProto, FileDescriptorProto, FileDescriptorSet, ServiceDescriptorProto};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use auv_api_proto::auv::api::core::v1 as core_proto;

const MAX_PROVIDER_CONFIG_BYTES: u64 = 1024 * 1024;
const LOCAL_RUNNER_CLASS: &str = "auv.core.local";
const INFERENCE_RUNNER_CLASS: &str = "auv.inference.ultralytics";
const INFERENCE_SERVICE: &str = "auv.api.inference.v1.ObjectDetectionService";
#[cfg(target_os = "macos")]
const LOCAL_DRIVER_SERVICES: &[&str] = &[
  "auv.api.driver.macos.v1.AccessibilityService",
  "auv.api.driver.macos.v1.ApplicationService",
  "auv.api.driver.v1.CaptureService",
  "auv.api.driver.v1.DisplayService",
  "auv.api.driver.v1.InputService",
  "auv.api.driver.v1.OverlayService",
  "auv.api.driver.v1.TextRecognitionService",
  "auv.api.driver.v1.WindowService",
  "auv.api.driver.macos.v1.MediaControlService",
  "auv.api.driver.macos.v1.PermissionService",
];
#[cfg(not(target_os = "macos"))]
const LOCAL_DRIVER_SERVICES: &[&str] = &[
  "auv.api.driver.v1.CaptureService",
  "auv.api.driver.v1.DisplayService",
  "auv.api.driver.v1.InputService",
  "auv.api.driver.v1.TextRecognitionService",
  "auv.api.driver.v1.WindowService",
];
const DISCOVERABLE_EXTENSION: &str = "auv.api.annotations.v1.discoverable";
const EFFECT_EXTENSION: &str = "auv.api.annotations.v1.effect";

/// Experimental daemon-side configuration for one operator-trusted RunnerClass.
///
/// This is local daemon configuration, not a child registration message.
/// Reflection remains untrusted and must exactly match the descriptor snapshot
/// named here before a spawned Runner becomes ready.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerProviderConfig {
  pub runner_class: String,
  pub display_name: String,
  pub runtime: RunnerRuntime,
  pub descriptor_set: PathBuf,
  /// Lowercase or uppercase hexadecimal SHA-256 of the canonical descriptor set.
  pub descriptor_set_sha256: String,
  pub services: Vec<RunnerProviderServiceConfig>,
  pub supported_lifecycles: Vec<RunnerProviderLifecycle>,
  pub operation_capacity: u32,
}

/// Daemon-side transport used to realize one Runner provider.
///
/// This is a Rust/configuration enum. Compatible runtimes all expose the same
/// protobuf services; the transport choice is intentionally absent from the
/// protobuf resource model.
#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(tag = "type", content = "config", rename_all = "kebab-case")]
pub enum RunnerRuntime {
  Executable(ExecutableRunnerRuntime),
  RemoteGrpc(RemoteGrpcRunnerRuntime),
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutableRunnerRuntime {
  pub executable: PathBuf,
  #[serde(default)]
  pub arguments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteGrpcRunnerRuntime {
  pub endpoint: String,
  // TODO(remote-runner-credentials): add a typed daemon-owned credential
  // reference when authenticated outbound Runner connections are approved;
  // caller Authorization must never be forwarded to a provider endpoint.
}

/// First-party runtimes supplied by the process that embeds the API server.
#[derive(Clone, Debug, Default)]
pub struct FirstPartyRunnerRuntimes {
  pub local_driver: Option<RunnerRuntime>,
  pub inference_ultralytics: Option<RunnerRuntime>,
}

impl RunnerProviderConfig {
  /// Loads one bounded, operator-owned JSON provider configuration.
  pub fn load_json(path: impl AsRef<Path>) -> Result<Self, RunnerProviderConfigError> {
    let path = path.as_ref();
    validate_trusted_file(path, TrustedFileKind::Configuration)?;
    let metadata = std::fs::metadata(path).map_err(|source| RunnerProviderConfigError::FileMetadata {
      path: path.to_path_buf(),
      source,
    })?;
    if metadata.len() > MAX_PROVIDER_CONFIG_BYTES {
      return Err(RunnerProviderConfigError::ConfigTooLarge {
        path: path.to_path_buf(),
        bytes: metadata.len(),
        maximum: MAX_PROVIDER_CONFIG_BYTES,
      });
    }
    let bytes = std::fs::read(path).map_err(|source| RunnerProviderConfigError::FileRead {
      path: path.to_path_buf(),
      source,
    })?;
    serde_json::from_slice(&bytes).map_err(|source| RunnerProviderConfigError::Json {
      path: path.to_path_buf(),
      source,
    })
  }

  /// Computes the canonical descriptor pin used by provider admission.
  ///
  /// Runner projects can call this while producing an operator manifest. The
  /// daemon repeats the same bounded closure and service-policy validation
  /// when it loads that manifest; this helper does not register authority.
  pub fn canonical_descriptor_sha256(
    descriptor_set: impl AsRef<Path>,
    services: &[RunnerProviderServiceConfig],
  ) -> Result<String, RunnerProviderConfigError> {
    if services.is_empty() {
      return Err(RunnerProviderConfigError::MissingServices);
    }
    let descriptor_set = load_trusted_descriptor_set(descriptor_set.as_ref())?;
    let services = services.iter().map(|service| (service.name.as_str(), service.externally_exposed)).collect::<Vec<_>>();
    let manifest = manifest_from_trusted_descriptors(&descriptor_set, &services, None)
      .map_err(|error| RunnerProviderConfigError::Schema(error.to_string()))?;
    let validated = validate_encoded(&descriptor_set, &manifest, SchemaLimits::default())
      .map_err(|error| RunnerProviderConfigError::Schema(error.to_string()))?;
    Ok(hex::encode(validated.descriptor_set_sha256))
  }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunnerProviderServiceConfig {
  pub name: String,
  #[serde(default = "default_true")]
  pub externally_exposed: bool,
}

fn default_true() -> bool {
  true
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RunnerProviderLifecycle {
  Ephemeral,
  UnlessIdle,
  UnlessShutdown,
}

impl RunnerProviderLifecycle {
  fn proto(self) -> i32 {
    match self {
      Self::Ephemeral => core_proto::RunnerLifecycle::Ephemeral as i32,
      Self::UnlessIdle => core_proto::RunnerLifecycle::UnlessIdle as i32,
      Self::UnlessShutdown => core_proto::RunnerLifecycle::UnlessShutdown as i32,
    }
  }
}

#[derive(Debug, thiserror::Error)]
pub enum RunnerProviderConfigError {
  #[error("Runner provider path must be absolute: {0}")]
  PathNotAbsolute(PathBuf),
  #[error("Runner provider trusted path must not be a symbolic link: {0}")]
  SymbolicLink(PathBuf),
  #[error("Runner provider trusted path is not a regular file: {0}")]
  NotAFile(PathBuf),
  #[error("Runner provider executable is not executable: {0}")]
  NotExecutable(PathBuf),
  #[error("Runner provider trusted path is group/world writable: {0}")]
  InsecurePermissions(PathBuf),
  #[error("Runner provider trusted path is not owned by the daemon uid or root: {0}")]
  UntrustedOwner(PathBuf),
  #[error("failed to inspect Runner provider file {path}: {source}")]
  FileMetadata {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("failed to read Runner provider file {path}: {source}")]
  FileRead {
    path: PathBuf,
    #[source]
    source: std::io::Error,
  },
  #[error("Runner provider config {path} exceeds {maximum} bytes (observed {bytes})")]
  ConfigTooLarge {
    path: PathBuf,
    bytes: u64,
    maximum: u64,
  },
  #[error("invalid Runner provider JSON {path}: {source}")]
  Json {
    path: PathBuf,
    #[source]
    source: serde_json::Error,
  },
  #[error("invalid RunnerClass identity: {0}")]
  InvalidRunnerClass(String),
  #[error("RunnerClass {0} is reserved by the daemon")]
  ReservedRunnerClass(String),
  #[error("Runner provider display_name is required")]
  MissingDisplayName,
  #[error("Runner provider must declare at least one service")]
  MissingServices,
  #[error("Runner provider must declare at least one lifecycle")]
  MissingLifecycles,
  #[error("Runner provider operation_capacity must be positive")]
  InvalidOperationCapacity,
  #[error("invalid remote gRPC Runner endpoint: {0}")]
  InvalidRemoteGrpcEndpoint(String),
  #[error("Runner provider descriptor SHA-256 must be exactly 64 hexadecimal characters")]
  InvalidDescriptorDigest,
  #[error("Runner provider descriptor set is invalid: {0}")]
  DescriptorDecode(String),
  #[error("Runner provider schema is not trusted: {0}")]
  Schema(String),
  #[error("RunnerClass is configured more than once: {0}")]
  DuplicateRunnerClass(String),
  #[error("public gRPC path {0} has conflicting schemas across RunnerClasses")]
  ConflictingExternalRoute(String),
}

#[derive(Clone, Debug)]
pub(crate) struct RegisteredRunnerProvider {
  pub(crate) runner_class: String,
  pub(crate) display_name: String,
  pub(crate) runtime: RunnerRuntime,
  pub(crate) manifest: TrustedManifest,
  pub(crate) validated_schema: ValidatedSchema,
  pub(crate) supported_lifecycles: Vec<i32>,
  pub(crate) operation_capacity: u32,
  pub(crate) capabilities: Vec<core_proto::RunnerCapability>,
  pub(crate) external_routes: BTreeSet<(String, String)>,
}

impl RegisteredRunnerProvider {
  pub(crate) fn runner_class_record(&self, device: core_proto::DeviceRef) -> core_proto::RunnerClass {
    core_proto::RunnerClass {
      r#ref: Some(core_proto::RunnerClassRef {
        runner_class: self.runner_class.clone(),
      }),
      device: Some(device),
      display_name: self.display_name.clone(),
      supported_lifecycles: self.supported_lifecycles.clone(),
      capabilities: self.capabilities.clone(),
      available: true,
    }
  }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct RunnerProviderRegistry {
  providers: BTreeMap<String, RegisteredRunnerProvider>,
}

impl RunnerProviderRegistry {
  #[cfg(test)]
  pub(crate) fn build(local_executable: Option<PathBuf>, custom: Vec<RunnerProviderConfig>) -> Result<Self, RunnerProviderConfigError> {
    Self::build_with_first_party(local_executable.map(executable_runtime), None, custom)
  }

  pub(crate) fn build_with_first_party(
    local_runtime: Option<RunnerRuntime>,
    inference_runtime: Option<RunnerRuntime>,
    custom: Vec<RunnerProviderConfig>,
  ) -> Result<Self, RunnerProviderConfigError> {
    let mut registry = Self::default();
    if let Some(runtime) = local_runtime {
      registry.insert(local_provider(runtime)?)?;
    }
    if let Some(runtime) = inference_runtime {
      registry.insert(inference_provider(runtime)?)?;
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
    for existing in self.providers.values() {
      for route in provider.external_routes.intersection(&existing.external_routes) {
        // Identical paths are safe to share only when the full canonical
        // descriptor closure is identical. Comparing message names alone
        // would allow two providers to assign different fields to the same
        // externally visible protobuf type.
        if provider.validated_schema.canonical_descriptor_set != existing.validated_schema.canonical_descriptor_set {
          return Err(RunnerProviderConfigError::ConflictingExternalRoute(format!("{}/{}", route.0, route.1)));
        }
      }
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

  pub(crate) fn service_catalog(&self) -> Vec<core_proto::ApiService> {
    self
      .providers
      .values()
      .flat_map(|provider| {
        provider.manifest.services.iter().filter_map(|service| {
          let methods = service
            .methods
            .iter()
            .filter(|method| method.discoverable)
            .map(|method| core_proto::ApiMethod {
              full_name: format!("{}.{}", service.name, method.name),
              discoverable: true,
              effect: method.effect,
            })
            .collect::<Vec<_>>();
          (!methods.is_empty()).then(|| core_proto::ApiService {
            full_name: service.name.clone(),
            runner_class: provider.runner_class.clone(),
            methods,
          })
        })
      })
      .collect()
  }
}

#[cfg(test)]
pub(crate) fn executable_runtime(executable: PathBuf) -> RunnerRuntime {
  RunnerRuntime::Executable(ExecutableRunnerRuntime {
    executable,
    arguments: Vec::new(),
  })
}

fn validate_runtime(runtime: RunnerRuntime) -> Result<RunnerRuntime, RunnerProviderConfigError> {
  match runtime {
    RunnerRuntime::Executable(mut executable) => {
      validate_trusted_file(&executable.executable, TrustedFileKind::Executable)?;
      executable.executable = canonical_trusted_path(&executable.executable)?;
      Ok(RunnerRuntime::Executable(executable))
    }
    RunnerRuntime::RemoteGrpc(remote) => {
      tonic::transport::Endpoint::from_shared(remote.endpoint.clone())
        .map_err(|error| RunnerProviderConfigError::InvalidRemoteGrpcEndpoint(error.to_string()))?;
      Ok(RunnerRuntime::RemoteGrpc(remote))
    }
  }
}

fn register_custom(config: RunnerProviderConfig) -> Result<RegisteredRunnerProvider, RunnerProviderConfigError> {
  validate_class_identity(&config.runner_class)?;
  if config.runner_class == LOCAL_RUNNER_CLASS {
    return Err(RunnerProviderConfigError::ReservedRunnerClass(config.runner_class));
  }
  if config.display_name.trim().is_empty() {
    return Err(RunnerProviderConfigError::MissingDisplayName);
  }
  if config.services.is_empty() {
    return Err(RunnerProviderConfigError::MissingServices);
  }
  if config.supported_lifecycles.is_empty() {
    return Err(RunnerProviderConfigError::MissingLifecycles);
  }
  if config.operation_capacity == 0 {
    return Err(RunnerProviderConfigError::InvalidOperationCapacity);
  }
  let runtime = validate_runtime(config.runtime)?;
  let descriptor_set = load_trusted_descriptor_set(&config.descriptor_set)?;
  let expected_digest = parse_digest(&config.descriptor_set_sha256)?;
  let services = config.services.iter().map(|service| (service.name.as_str(), service.externally_exposed)).collect::<Vec<_>>();
  let manifest = manifest_from_trusted_descriptors(&descriptor_set, &services, Some(expected_digest))
    .map_err(|error| RunnerProviderConfigError::Schema(error.to_string()))?;
  let validated_schema = validate_encoded(&descriptor_set, &manifest, SchemaLimits::default())
    .map_err(|error| RunnerProviderConfigError::Schema(error.to_string()))?;
  let (capabilities, external_routes) = project_manifest(&manifest);
  let mut supported_lifecycles = config.supported_lifecycles.into_iter().map(RunnerProviderLifecycle::proto).collect::<Vec<_>>();
  supported_lifecycles.sort_unstable();
  supported_lifecycles.dedup();
  Ok(RegisteredRunnerProvider {
    runner_class: config.runner_class,
    display_name: config.display_name,
    runtime,
    manifest,
    validated_schema,
    supported_lifecycles,
    operation_capacity: config.operation_capacity,
    capabilities,
    external_routes,
  })
}

fn load_trusted_descriptor_set(path: &Path) -> Result<Vec<u8>, RunnerProviderConfigError> {
  validate_trusted_file(path, TrustedFileKind::DescriptorSet)?;
  let metadata = std::fs::metadata(path).map_err(|source| RunnerProviderConfigError::FileMetadata {
    path: path.to_path_buf(),
    source,
  })?;
  let maximum = u64::try_from(SchemaLimits::default().max_total_bytes).expect("schema byte limit fits u64");
  if metadata.len() > maximum {
    return Err(RunnerProviderConfigError::ConfigTooLarge {
      path: path.to_path_buf(),
      bytes: metadata.len(),
      maximum,
    });
  }
  let bytes = std::fs::read(path).map_err(|source| RunnerProviderConfigError::FileRead {
    path: path.to_path_buf(),
    source,
  })?;
  DescriptorPool::decode(bytes.as_slice()).map_err(|error| RunnerProviderConfigError::DescriptorDecode(error.to_string()))?;
  Ok(bytes)
}

fn local_provider(runtime: RunnerRuntime) -> Result<RegisteredRunnerProvider, RunnerProviderConfigError> {
  let runtime = validate_runtime(runtime)?;
  let bytes = auv_api_proto::descriptor_set_for_services(LOCAL_DRIVER_SERVICES).map_err(RunnerProviderConfigError::DescriptorDecode)?;
  let services = LOCAL_DRIVER_SERVICES.iter().map(|service| (*service, true)).collect::<Vec<_>>();
  let mut manifest =
    manifest_from_trusted_descriptors(&bytes, &services, None).map_err(|error| RunnerProviderConfigError::Schema(error.to_string()))?;
  let validated_schema =
    validate_encoded(&bytes, &manifest, SchemaLimits::default()).map_err(|error| RunnerProviderConfigError::Schema(error.to_string()))?;
  manifest.expected_descriptor_set_sha256 = Some(validated_schema.descriptor_set_sha256);
  let (capabilities, external_routes) = project_manifest(&manifest);
  Ok(RegisteredRunnerProvider {
    runner_class: LOCAL_RUNNER_CLASS.to_string(),
    display_name: "AUV local driver".to_string(),
    runtime,
    manifest,
    validated_schema,
    supported_lifecycles: vec![
      core_proto::RunnerLifecycle::Ephemeral as i32,
      core_proto::RunnerLifecycle::UnlessIdle as i32,
      core_proto::RunnerLifecycle::UnlessShutdown as i32,
    ],
    operation_capacity: 16,
    capabilities,
    external_routes,
  })
}

fn inference_provider(runtime: RunnerRuntime) -> Result<RegisteredRunnerProvider, RunnerProviderConfigError> {
  let runtime = validate_runtime(runtime)?;
  let bytes = auv_api_proto::descriptor_set_for_service(INFERENCE_SERVICE).map_err(RunnerProviderConfigError::DescriptorDecode)?;
  // ObjectDetection is a daemon-owned typed route. The child advertises its
  // capability, but the generic aggregated fallback must not claim this path.
  let services = [(INFERENCE_SERVICE, false)];
  let mut manifest =
    manifest_from_trusted_descriptors(&bytes, &services, None).map_err(|error| RunnerProviderConfigError::Schema(error.to_string()))?;
  let validated_schema =
    validate_encoded(&bytes, &manifest, SchemaLimits::default()).map_err(|error| RunnerProviderConfigError::Schema(error.to_string()))?;
  manifest.expected_descriptor_set_sha256 = Some(validated_schema.descriptor_set_sha256);
  let (capabilities, external_routes) = project_manifest(&manifest);
  Ok(RegisteredRunnerProvider {
    runner_class: INFERENCE_RUNNER_CLASS.to_string(),
    display_name: "AUV Ultralytics inference".to_string(),
    runtime,
    manifest,
    validated_schema,
    supported_lifecycles: vec![
      core_proto::RunnerLifecycle::Ephemeral as i32,
      core_proto::RunnerLifecycle::UnlessIdle as i32,
      core_proto::RunnerLifecycle::UnlessShutdown as i32,
    ],
    operation_capacity: 1,
    capabilities,
    external_routes,
  })
}

fn project_manifest(manifest: &TrustedManifest) -> (Vec<core_proto::RunnerCapability>, BTreeSet<(String, String)>) {
  let capabilities = manifest
    .services
    .iter()
    .map(|service| core_proto::RunnerCapability {
      service: service.name.clone(),
      methods: service.methods.iter().map(|method| method.name.clone()).collect(),
    })
    .collect();
  let external_routes = manifest
    .services
    .iter()
    .flat_map(|service| {
      service.methods.iter().filter(|method| method.externally_exposed).map(|method| (service.name.clone(), method.name.clone()))
    })
    .collect();
  (capabilities, external_routes)
}

fn validate_class_identity(value: &str) -> Result<(), RunnerProviderConfigError> {
  if value.is_empty()
    || value.len() > 256
    || value.starts_with('.')
    || value.ends_with('.')
    || !value.contains('.')
    || value.split('.').any(str::is_empty)
    || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
  {
    return Err(RunnerProviderConfigError::InvalidRunnerClass(value.to_string()));
  }
  Ok(())
}

fn parse_digest(value: &str) -> Result<[u8; 32], RunnerProviderConfigError> {
  let bytes = hex::decode(value).map_err(|_| RunnerProviderConfigError::InvalidDescriptorDigest)?;
  bytes.try_into().map_err(|_| RunnerProviderConfigError::InvalidDescriptorDigest)
}

#[derive(Clone, Copy)]
enum TrustedFileKind {
  Configuration,
  DescriptorSet,
  Executable,
}

fn canonical_trusted_path(path: &Path) -> Result<PathBuf, RunnerProviderConfigError> {
  path.canonicalize().map_err(|source| RunnerProviderConfigError::FileMetadata {
    path: path.to_path_buf(),
    source,
  })
}

fn validate_trusted_file(path: &Path, kind: TrustedFileKind) -> Result<(), RunnerProviderConfigError> {
  if !path.is_absolute() {
    return Err(RunnerProviderConfigError::PathNotAbsolute(path.to_path_buf()));
  }
  let metadata = std::fs::symlink_metadata(path).map_err(|source| RunnerProviderConfigError::FileMetadata {
    path: path.to_path_buf(),
    source,
  })?;
  if metadata.file_type().is_symlink() {
    return Err(RunnerProviderConfigError::SymbolicLink(path.to_path_buf()));
  }
  if !metadata.is_file() {
    return Err(RunnerProviderConfigError::NotAFile(path.to_path_buf()));
  }
  // NOTICE(custom-runner-trust-v1): provider executables are explicitly
  // operator-trusted in this phase. Metadata validation does not prevent the
  // operator from replacing a file between admission and spawn. Pin an open
  // executable handle (or add a platform sandbox/package verifier) before
  // treating community code as an untrusted security boundary.
  #[cfg(unix)]
  {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let mode = metadata.permissions().mode();
    if mode & 0o022 != 0 {
      return Err(RunnerProviderConfigError::InsecurePermissions(path.to_path_buf()));
    }
    if matches!(kind, TrustedFileKind::Executable) && mode & 0o111 == 0 {
      return Err(RunnerProviderConfigError::NotExecutable(path.to_path_buf()));
    }
    // SAFETY: geteuid has no preconditions and does not dereference memory.
    let daemon_uid = unsafe { libc::geteuid() };
    if metadata.uid() != daemon_uid && metadata.uid() != 0 {
      return Err(RunnerProviderConfigError::UntrustedOwner(path.to_path_buf()));
    }
  }
  Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrustedService {
  pub(crate) name: String,
  pub(crate) methods: Vec<TrustedMethod>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrustedMethod {
  pub(crate) name: String,
  pub(crate) input_type: String,
  pub(crate) output_type: String,
  pub(crate) client_streaming: bool,
  pub(crate) server_streaming: bool,
  pub(crate) externally_exposed: bool,
  pub(crate) discoverable: bool,
  pub(crate) effect: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TrustedManifest {
  /// `None` is reserved for first-party descriptors compiled into the daemon.
  /// A custom RunnerClass must pin the canonical digest it was approved with.
  pub(crate) expected_descriptor_set_sha256: Option<[u8; 32]>,
  pub(crate) services: Vec<TrustedService>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SchemaLimits {
  pub(crate) max_files: usize,
  pub(crate) max_file_bytes: usize,
  pub(crate) max_total_bytes: usize,
  pub(crate) max_services: usize,
  pub(crate) max_methods_per_service: usize,
  pub(crate) max_dependencies_per_file: usize,
}

impl Default for SchemaLimits {
  fn default() -> Self {
    Self {
      max_files: 128,
      max_file_bytes: 1024 * 1024,
      max_total_bytes: 4 * 1024 * 1024,
      max_services: 64,
      max_methods_per_service: 128,
      max_dependencies_per_file: 64,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ValidatedSchema {
  pub(crate) canonical_descriptor_set: Vec<u8>,
  pub(crate) descriptor_set_sha256: [u8; 32],
}

#[derive(Clone, Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum SchemaError {
  #[error("schema exceeds limit {limit}: observed {observed}, maximum {maximum}")]
  LimitExceeded {
    limit: &'static str,
    observed: usize,
    maximum: usize,
  },
  #[error("descriptor file name is required")]
  MissingFileName,
  #[error("descriptor file {0} appears with conflicting contents")]
  ConflictingFile(String),
  #[error("descriptor file {file} imports missing dependency {dependency}")]
  MissingDependency { file: String, dependency: String },
  #[error("descriptor dependency graph contains a cycle through {0}")]
  DependencyCycle(String),
  #[error("descriptor file {0} is outside the admitted service dependency closure")]
  UnexpectedFile(String),
  #[error("custom Runner cannot claim reserved service {0}")]
  ReservedService(String),
  #[error("service {0} is defined more than once")]
  DuplicateService(String),
  #[error("trusted manifest defines service {0} more than once")]
  DuplicateManifestService(String),
  #[error("service set differs from trusted manifest: expected {expected:?}, observed {observed:?}")]
  ServiceSetMismatch {
    expected: Vec<String>,
    observed: Vec<String>,
  },
  #[error("method {service}/{method} is defined more than once")]
  DuplicateMethod { service: String, method: String },
  #[error("method set for {service} differs from trusted manifest: expected {expected:?}, observed {observed:?}")]
  MethodSetMismatch {
    service: String,
    expected: Vec<String>,
    observed: Vec<String>,
  },
  #[error("method signature differs for {service}/{method} {field}: expected {expected}, observed {observed}")]
  MethodSignatureMismatch {
    service: String,
    method: String,
    field: &'static str,
    expected: String,
    observed: String,
  },
  #[error("method {service}/{method} references unknown {direction} message {message}")]
  UnknownMessageType {
    service: String,
    method: String,
    direction: &'static str,
    message: String,
  },
  #[error("externally exposed method {service}/{method} cannot use streaming")]
  ExternalStreamingUnsupported { service: String, method: String },
  #[error("descriptor digest differs from trusted manifest")]
  DigestMismatch {
    expected: [u8; 32],
    observed: [u8; 32],
  },
  #[error("trusted descriptor set does not define service {0}")]
  TrustedServiceMissing(String),
  #[error("trusted descriptor set does not define annotation {0}")]
  AnnotationDefinitionMissing(&'static str),
  #[error("discoverable method {service}/{method} must declare a non-unspecified effect")]
  DiscoverableEffectUnspecified { service: String, method: String },
  #[error("method discovery annotation differs for {service}/{method} {field}: expected {expected}, observed {observed}")]
  MethodAnnotationMismatch {
    service: String,
    method: String,
    field: &'static str,
    expected: String,
    observed: String,
  },
  #[error("descriptor set cannot be reflected: {0}")]
  ReflectionDecode(String),
}

/// Derives exact method signatures from daemon-owned trusted descriptors.
///
/// `services` pairs a full protobuf service name with the daemon's explicit
/// external-exposure decision. Callers must never pass child reflection output
/// here: reflected descriptors are untrusted input for [`validate`].
pub(crate) fn manifest_from_trusted_descriptors(
  descriptor_set: &[u8],
  services: &[(&str, bool)],
  expected_descriptor_set_sha256: Option<[u8; 32]>,
) -> Result<TrustedManifest, SchemaError> {
  let pool = DescriptorPool::decode(descriptor_set).map_err(|error| SchemaError::ReflectionDecode(error.to_string()))?;
  let discoverable_extension =
    pool.get_extension_by_name(DISCOVERABLE_EXTENSION).ok_or(SchemaError::AnnotationDefinitionMissing(DISCOVERABLE_EXTENSION))?;
  let effect_extension = pool.get_extension_by_name(EFFECT_EXTENSION).ok_or(SchemaError::AnnotationDefinitionMissing(EFFECT_EXTENSION))?;

  let mut selected = BTreeMap::new();
  for (name, externally_exposed) in services {
    if is_reserved_service(name) {
      return Err(SchemaError::ReservedService((*name).to_string()));
    }
    let descriptor = pool.get_service_by_name(name).ok_or_else(|| SchemaError::TrustedServiceMissing((*name).to_string()))?;
    let mut methods = Vec::with_capacity(descriptor.methods().len());
    let mut method_names = HashSet::new();
    for method in descriptor.methods() {
      let method_name = method.name().to_string();
      if !method_names.insert(method_name.clone()) {
        return Err(SchemaError::DuplicateMethod {
          service: (*name).to_string(),
          method: method_name,
        });
      }
      let client_streaming = method.is_client_streaming();
      let server_streaming = method.is_server_streaming();
      if *externally_exposed && (client_streaming || server_streaming) {
        return Err(SchemaError::ExternalStreamingUnsupported {
          service: (*name).to_string(),
          method: method_name,
        });
      }
      let options = method.options();
      let discoverable = options.get_extension(&discoverable_extension).as_ref() == &Value::Bool(true);
      let effect = match options.get_extension(&effect_extension).as_ref() {
        Value::EnumNumber(value) => *value,
        _ => 0,
      };
      if discoverable && effect == 0 {
        return Err(SchemaError::DiscoverableEffectUnspecified {
          service: (*name).to_string(),
          method: method_name,
        });
      }
      methods.push(TrustedMethod {
        name: method_name,
        input_type: format!(".{}", method.input().full_name()),
        output_type: format!(".{}", method.output().full_name()),
        client_streaming,
        server_streaming,
        externally_exposed: *externally_exposed,
        discoverable,
        effect,
      });
    }
    if selected
      .insert(
        (*name).to_string(),
        TrustedService {
          name: (*name).to_string(),
          methods,
        },
      )
      .is_some()
    {
      return Err(SchemaError::DuplicateManifestService((*name).to_string()));
    }
  }
  Ok(TrustedManifest {
    expected_descriptor_set_sha256,
    services: selected.into_values().collect(),
  })
}

#[cfg(test)]
pub(crate) fn validate(
  descriptor_set: FileDescriptorSet,
  manifest: &TrustedManifest,
  limits: SchemaLimits,
) -> Result<ValidatedSchema, SchemaError> {
  validate_encoded_inner(&descriptor_set.encode_to_vec(), manifest, limits, false)
}

pub(crate) fn validate_encoded(
  descriptor_set: &[u8],
  manifest: &TrustedManifest,
  limits: SchemaLimits,
) -> Result<ValidatedSchema, SchemaError> {
  validate_encoded_inner(descriptor_set, manifest, limits, true)
}

fn validate_encoded_inner(
  encoded_descriptor_set: &[u8],
  manifest: &TrustedManifest,
  limits: SchemaLimits,
  validate_annotations: bool,
) -> Result<ValidatedSchema, SchemaError> {
  let descriptor_set =
    FileDescriptorSet::decode(encoded_descriptor_set).map_err(|error| SchemaError::ReflectionDecode(error.to_string()))?;
  limit("files", descriptor_set.file.len(), limits.max_files)?;
  let files = canonical_files(descriptor_set.file, limits)?;
  validate_dependencies(&files, limits)?;

  let policy = manifest_services(manifest, limits)?;
  let observed = observed_services(&files, limits)?;
  let expected_names = policy.keys().cloned().collect::<Vec<_>>();
  let observed_names = observed.keys().cloned().collect::<Vec<_>>();
  if expected_names != observed_names {
    return Err(SchemaError::ServiceSetMismatch {
      expected: expected_names,
      observed: observed_names,
    });
  }

  validate_dependency_closure(&files, observed.values().map(|service| service.file))?;
  let messages = message_names(&files);
  for (service_name, trusted) in policy {
    validate_service(service_name.as_str(), trusted, observed.get(&service_name).expect("equal service sets"), &messages, limits)?;
  }

  let pool_bytes = if validate_annotations {
    encoded_descriptor_set.to_vec()
  } else {
    FileDescriptorSet {
      file: files.values().cloned().collect(),
    }
    .encode_to_vec()
  };
  let pool = DescriptorPool::decode(pool_bytes.as_slice()).map_err(|error| SchemaError::ReflectionDecode(error.to_string()))?;
  if validate_annotations {
    for service in &manifest.services {
      validate_service_annotations(service, &pool)?;
    }
  }

  let canonical_descriptor_set = encode_canonical_descriptor_set(&pool, files.keys())?;
  limit("total descriptor bytes", canonical_descriptor_set.len(), limits.max_total_bytes)?;
  let descriptor_set_sha256: [u8; 32] = Sha256::digest(&canonical_descriptor_set).into();
  if let Some(expected) = manifest.expected_descriptor_set_sha256
    && expected != descriptor_set_sha256
  {
    return Err(SchemaError::DigestMismatch {
      expected,
      observed: descriptor_set_sha256,
    });
  }
  Ok(ValidatedSchema {
    canonical_descriptor_set,
    descriptor_set_sha256,
  })
}

fn encode_canonical_descriptor_set<'a>(pool: &DescriptorPool, file_names: impl Iterator<Item = &'a String>) -> Result<Vec<u8>, SchemaError> {
  let mut encoded = Vec::new();
  for name in file_names {
    let file = pool.get_file_by_name(name).ok_or_else(|| SchemaError::MissingFileName)?;
    let bytes = file.encode_to_vec();
    encoded.push(0x0a); // FileDescriptorSet.file, wire type length-delimited.
    encode_varint(bytes.len() as u64, &mut encoded);
    encoded.extend_from_slice(&bytes);
  }
  Ok(encoded)
}

fn encode_varint(mut value: u64, output: &mut Vec<u8>) {
  while value >= 0x80 {
    output.push((value as u8 & 0x7f) | 0x80);
    value >>= 7;
  }
  output.push(value as u8);
}

pub(crate) fn encode_descriptor_set_files(files: &[Vec<u8>]) -> Vec<u8> {
  let mut encoded = Vec::new();
  for file in files {
    encoded.push(0x0a);
    encode_varint(file.len() as u64, &mut encoded);
    encoded.extend_from_slice(file);
  }
  encoded
}

struct ObservedService<'a> {
  file: &'a str,
  descriptor: &'a ServiceDescriptorProto,
}

fn limit(limit: &'static str, observed: usize, maximum: usize) -> Result<(), SchemaError> {
  if observed > maximum {
    Err(SchemaError::LimitExceeded {
      limit,
      observed,
      maximum,
    })
  } else {
    Ok(())
  }
}

fn canonical_files(files: Vec<FileDescriptorProto>, limits: SchemaLimits) -> Result<BTreeMap<String, FileDescriptorProto>, SchemaError> {
  let mut canonical = BTreeMap::<String, (FileDescriptorProto, Vec<u8>)>::new();
  let mut total_bytes = 0usize;
  for file in files {
    let name = file.name.clone().filter(|name| !name.is_empty()).ok_or(SchemaError::MissingFileName)?;
    let bytes = file.encode_to_vec();
    limit("descriptor file bytes", bytes.len(), limits.max_file_bytes)?;
    total_bytes = total_bytes.saturating_add(bytes.len());
    limit("total descriptor bytes", total_bytes, limits.max_total_bytes)?;
    match canonical.get(&name) {
      Some((_, existing)) if existing != &bytes => return Err(SchemaError::ConflictingFile(name)),
      Some(_) => {}
      None => {
        canonical.insert(name, (file, bytes));
      }
    }
  }
  Ok(canonical.into_iter().map(|(name, (file, _))| (name, file)).collect())
}

fn validate_dependencies(files: &BTreeMap<String, FileDescriptorProto>, limits: SchemaLimits) -> Result<(), SchemaError> {
  for (name, file) in files {
    limit("dependencies per file", file.dependency.len(), limits.max_dependencies_per_file)?;
    for dependency in &file.dependency {
      if !files.contains_key(dependency) {
        return Err(SchemaError::MissingDependency {
          file: name.clone(),
          dependency: dependency.clone(),
        });
      }
    }
  }
  let mut completed = HashSet::new();
  let mut visiting = HashSet::new();
  for name in files.keys() {
    visit_dependencies(name, files, &mut visiting, &mut completed)?;
  }
  Ok(())
}

fn visit_dependencies(
  name: &str,
  files: &BTreeMap<String, FileDescriptorProto>,
  visiting: &mut HashSet<String>,
  completed: &mut HashSet<String>,
) -> Result<(), SchemaError> {
  if completed.contains(name) {
    return Ok(());
  }
  if !visiting.insert(name.to_string()) {
    return Err(SchemaError::DependencyCycle(name.to_string()));
  }
  for dependency in &files.get(name).expect("dependency names were validated").dependency {
    visit_dependencies(dependency, files, visiting, completed)?;
  }
  visiting.remove(name);
  completed.insert(name.to_string());
  Ok(())
}

fn manifest_services<'a>(manifest: &'a TrustedManifest, limits: SchemaLimits) -> Result<BTreeMap<String, &'a TrustedService>, SchemaError> {
  limit("services", manifest.services.len(), limits.max_services)?;
  let mut services = BTreeMap::new();
  for service in &manifest.services {
    if is_reserved_service(&service.name) {
      return Err(SchemaError::ReservedService(service.name.clone()));
    }
    if services.insert(service.name.clone(), service).is_some() {
      return Err(SchemaError::DuplicateManifestService(service.name.clone()));
    }
  }
  Ok(services)
}

fn observed_services<'a>(
  files: &'a BTreeMap<String, FileDescriptorProto>,
  limits: SchemaLimits,
) -> Result<BTreeMap<String, ObservedService<'a>>, SchemaError> {
  let mut services = BTreeMap::new();
  for (file_name, file) in files {
    let package = file.package.as_deref().unwrap_or_default();
    for service in &file.service {
      let local_name = service.name.as_deref().unwrap_or_default();
      let name = if package.is_empty() {
        local_name.to_string()
      } else {
        format!("{package}.{local_name}")
      };
      if is_reserved_service(&name) {
        return Err(SchemaError::ReservedService(name));
      }
      if services
        .insert(
          name.clone(),
          ObservedService {
            file: file_name,
            descriptor: service,
          },
        )
        .is_some()
      {
        return Err(SchemaError::DuplicateService(name));
      }
      limit("services", services.len(), limits.max_services)?;
    }
  }
  Ok(services)
}

fn is_reserved_service(name: &str) -> bool {
  name.starts_with("auv.api.core.")
    || name == auv_runner_protocol::RUNTIME_SERVICE_NAME
    || matches!(name, "grpc.health.v1.Health" | "grpc.reflection.v1.ServerReflection")
}

fn validate_dependency_closure<'a>(
  files: &BTreeMap<String, FileDescriptorProto>,
  roots: impl Iterator<Item = &'a str>,
) -> Result<(), SchemaError> {
  let mut required = BTreeSet::new();
  let mut pending = roots.map(str::to_string).collect::<Vec<_>>();
  while let Some(name) = pending.pop() {
    if !required.insert(name.clone()) {
      continue;
    }
    pending.extend(files.get(&name).expect("service owner is in descriptor map").dependency.iter().cloned());
  }
  if let Some(unexpected) = files.keys().find(|name| !required.contains(*name)) {
    return Err(SchemaError::UnexpectedFile(unexpected.clone()));
  }
  Ok(())
}

fn message_names(files: &BTreeMap<String, FileDescriptorProto>) -> BTreeSet<String> {
  let mut names = BTreeSet::new();
  for file in files.values() {
    let package = file.package.as_deref().unwrap_or_default();
    for message in &file.message_type {
      collect_message_names(package, message, &mut names);
    }
  }
  names
}

fn collect_message_names(parent: &str, message: &DescriptorProto, names: &mut BTreeSet<String>) {
  let local = message.name.as_deref().unwrap_or_default();
  let name = if parent.is_empty() {
    local.to_string()
  } else {
    format!("{parent}.{local}")
  };
  names.insert(format!(".{name}"));
  for nested in &message.nested_type {
    collect_message_names(&name, nested, names);
  }
}

fn validate_service(
  service_name: &str,
  trusted: &TrustedService,
  observed: &ObservedService<'_>,
  messages: &BTreeSet<String>,
  limits: SchemaLimits,
) -> Result<(), SchemaError> {
  limit("methods per service", trusted.methods.len(), limits.max_methods_per_service)?;
  limit("methods per service", observed.descriptor.method.len(), limits.max_methods_per_service)?;
  let mut policy = BTreeMap::new();
  for method in &trusted.methods {
    if policy.insert(method.name.clone(), method).is_some() {
      return Err(SchemaError::DuplicateMethod {
        service: service_name.to_string(),
        method: method.name.clone(),
      });
    }
  }
  let mut actual = BTreeMap::new();
  for method in &observed.descriptor.method {
    let name = method.name.clone().unwrap_or_default();
    if actual.insert(name.clone(), method).is_some() {
      return Err(SchemaError::DuplicateMethod {
        service: service_name.to_string(),
        method: name,
      });
    }
  }
  let expected_names = policy.keys().cloned().collect::<Vec<_>>();
  let observed_names = actual.keys().cloned().collect::<Vec<_>>();
  if expected_names != observed_names {
    return Err(SchemaError::MethodSetMismatch {
      service: service_name.to_string(),
      expected: expected_names,
      observed: observed_names,
    });
  }
  for (method_name, trusted) in policy {
    let method = actual.get(&method_name).expect("equal method sets");
    compare_signature(service_name, &method_name, "input type", &trusted.input_type, method.input_type.as_deref().unwrap_or_default())?;
    compare_signature(service_name, &method_name, "output type", &trusted.output_type, method.output_type.as_deref().unwrap_or_default())?;
    let client_streaming = method.client_streaming.unwrap_or(false);
    let server_streaming = method.server_streaming.unwrap_or(false);
    compare_signature(service_name, &method_name, "client streaming", &trusted.client_streaming.to_string(), &client_streaming.to_string())?;
    compare_signature(service_name, &method_name, "server streaming", &trusted.server_streaming.to_string(), &server_streaming.to_string())?;
    if trusted.externally_exposed && (client_streaming || server_streaming) {
      return Err(SchemaError::ExternalStreamingUnsupported {
        service: service_name.to_string(),
        method: method_name,
      });
    }
    for (direction, message) in [
      ("input", &trusted.input_type),
      ("output", &trusted.output_type),
    ] {
      if !messages.contains(message) {
        return Err(SchemaError::UnknownMessageType {
          service: service_name.to_string(),
          method: method_name.clone(),
          direction,
          message: message.clone(),
        });
      }
    }
  }
  Ok(())
}

fn validate_service_annotations(trusted: &TrustedService, pool: &DescriptorPool) -> Result<(), SchemaError> {
  let service = pool.get_service_by_name(&trusted.name).expect("structural validation found the service");
  for method in &trusted.methods {
    let reflected = service.methods().find(|candidate| candidate.name() == method.name).expect("structural validation found the method");
    let (discoverable, effect) = discovery_annotations(&reflected)?;
    compare_annotation(&trusted.name, &method.name, "discoverable", method.discoverable.to_string(), discoverable.to_string())?;
    compare_annotation(&trusted.name, &method.name, "effect", method.effect.to_string(), effect.to_string())?;
  }
  Ok(())
}

fn discovery_annotations(method: &prost_reflect::MethodDescriptor) -> Result<(bool, i32), SchemaError> {
  let pool = method.parent_pool();
  let discoverable_extension =
    pool.get_extension_by_name(DISCOVERABLE_EXTENSION).ok_or(SchemaError::AnnotationDefinitionMissing(DISCOVERABLE_EXTENSION))?;
  let effect_extension = pool.get_extension_by_name(EFFECT_EXTENSION).ok_or(SchemaError::AnnotationDefinitionMissing(EFFECT_EXTENSION))?;
  let options = method.options();
  let discoverable = options.get_extension(&discoverable_extension).as_ref() == &Value::Bool(true);
  let effect = match options.get_extension(&effect_extension).as_ref() {
    Value::EnumNumber(value) => *value,
    _ => 0,
  };
  if discoverable && effect == 0 {
    return Err(SchemaError::DiscoverableEffectUnspecified {
      service: method.parent_service().full_name().to_string(),
      method: method.name().to_string(),
    });
  }
  Ok((discoverable, effect))
}

fn compare_annotation(service: &str, method: &str, field: &'static str, expected: String, observed: String) -> Result<(), SchemaError> {
  if expected == observed {
    Ok(())
  } else {
    Err(SchemaError::MethodAnnotationMismatch {
      service: service.to_string(),
      method: method.to_string(),
      field,
      expected,
      observed,
    })
  }
}

fn compare_signature(service: &str, method: &str, field: &'static str, expected: &str, observed: &str) -> Result<(), SchemaError> {
  if expected == observed {
    Ok(())
  } else {
    Err(SchemaError::MethodSignatureMismatch {
      service: service.to_string(),
      method: method.to_string(),
      field,
      expected: expected.to_string(),
      observed: observed.to_string(),
    })
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use prost_types::{MethodDescriptorProto, ServiceDescriptorProto};

  fn portable_driver_schema() -> FileDescriptorSet {
    FileDescriptorSet::decode(portable_driver_schema_bytes().as_slice()).expect("valid embedded descriptor set")
  }

  fn portable_driver_schema_bytes() -> Vec<u8> {
    auv_api_proto::descriptor_set_for_services(&[
      "auv.api.driver.v1.DisplayService",
      "auv.api.driver.v1.WindowService",
    ])
    .expect("portable Driver descriptor closure")
  }

  fn portable_driver_manifest() -> TrustedManifest {
    TrustedManifest {
      expected_descriptor_set_sha256: None,
      services: vec![
        TrustedService {
          name: "auv.api.driver.v1.DisplayService".to_string(),
          methods: vec![TrustedMethod {
            name: "ListDisplays".to_string(),
            input_type: ".auv.api.driver.v1.ListDisplaysRequest".to_string(),
            output_type: ".auv.api.driver.v1.ListDisplaysResponse".to_string(),
            client_streaming: false,
            server_streaming: false,
            externally_exposed: true,
            discoverable: true,
            effect: 1,
          }],
        },
        TrustedService {
          name: "auv.api.driver.v1.WindowService".to_string(),
          methods: [
            ("ListWindows", "ListWindowsRequest", "ListWindowsResponse"),
            ("ResolveWindow", "ResolveWindowRequest", "ResolveWindowResponse"),
          ]
          .into_iter()
          .map(|(name, input, output)| TrustedMethod {
            name: name.to_string(),
            input_type: format!(".auv.api.driver.v1.{input}"),
            output_type: format!(".auv.api.driver.v1.{output}"),
            client_streaming: false,
            server_streaming: false,
            externally_exposed: true,
            discoverable: true,
            effect: 1,
          })
          .collect(),
        },
      ],
    }
  }

  fn method_mut<'a>(schema: &'a mut FileDescriptorSet, service_name: &str, method_name: &str) -> &'a mut MethodDescriptorProto {
    for file in &mut schema.file {
      let package = file.package.as_deref().unwrap_or_default();
      for service in &mut file.service {
        if format!("{package}.{}", service.name.as_deref().unwrap_or_default()) == service_name {
          return service.method.iter_mut().find(|method| method.name.as_deref() == Some(method_name)).expect("fixture method");
        }
      }
    }
    panic!("fixture service {service_name}");
  }

  #[test]
  fn admits_exact_portable_driver_descriptor_closure_with_stable_digest() {
    let schema = portable_driver_schema();
    let mut reversed = schema.clone();
    reversed.file.reverse();

    let first = validate(schema, &portable_driver_manifest(), SchemaLimits::default()).expect("trusted schema");
    let second = validate(reversed, &portable_driver_manifest(), SchemaLimits::default()).expect("order-independent trusted schema");

    assert_eq!(first, second);
    assert_eq!(first.descriptor_set_sha256.len(), 32);
    assert!(!first.canonical_descriptor_set.is_empty());

    let mut duplicated = portable_driver_schema();
    duplicated.file.push(duplicated.file[0].clone());
    assert_eq!(
      validate(duplicated, &portable_driver_manifest(), SchemaLimits::default()).expect("identical duplicate is canonicalized"),
      first
    );
  }

  #[test]
  fn derives_exact_manifest_only_from_selected_trusted_services() {
    let schema = portable_driver_schema_bytes();
    let derived = manifest_from_trusted_descriptors(
      &schema,
      &[
        ("auv.api.driver.v1.DisplayService", true),
        ("auv.api.driver.v1.WindowService", true),
      ],
      None,
    )
    .expect("selected services exist in daemon-owned descriptors");

    assert_eq!(derived, portable_driver_manifest());
    assert_eq!(
      manifest_from_trusted_descriptors(&schema, &[("missing.v1.Service", true)], None),
      Err(SchemaError::TrustedServiceMissing("missing.v1.Service".to_string()))
    );
  }

  #[test]
  fn rejects_discoverable_method_with_unspecified_effect() {
    let pool = DescriptorPool::decode(portable_driver_schema_bytes().as_slice()).expect("portable descriptor pool");
    let mut files = pool.files().map(|file| file.encode_to_vec()).collect::<Vec<_>>();
    let mut effect_key = Vec::new();
    encode_varint((51002_u64 << 3) | 0, &mut effect_key);
    let mut changed = false;
    for file in &mut files {
      if let Some(index) =
        file.windows(effect_key.len() + 1).position(|window| window.starts_with(&effect_key) && window[effect_key.len()] != 0)
      {
        file[index + effect_key.len()] = 0;
        changed = true;
        break;
      }
    }
    assert!(changed, "fixture contains an effect extension");
    let schema = encode_descriptor_set_files(&files);
    assert!(matches!(
      manifest_from_trusted_descriptors(
        &schema,
        &[
          ("auv.api.driver.v1.DisplayService", true),
          ("auv.api.driver.v1.WindowService", true),
        ],
        None,
      ),
      Err(SchemaError::DiscoverableEffectUnspecified { .. })
    ));
  }

  #[test]
  fn rejects_reserved_core_health_and_reflection_service_claims() {
    for (package, service_name) in [
      ("auv.api.core.v1", "DeviceService"),
      ("grpc.health.v1", "Health"),
      ("grpc.reflection.v1", "ServerReflection"),
    ] {
      let full_name = format!("{package}.{service_name}");
      let schema = FileDescriptorSet {
        file: vec![FileDescriptorProto {
          name: Some(format!("{}.proto", service_name.to_ascii_lowercase())),
          package: Some(package.to_string()),
          service: vec![ServiceDescriptorProto {
            name: Some(service_name.to_string()),
            ..Default::default()
          }],
          ..Default::default()
        }],
      };
      let manifest = TrustedManifest {
        expected_descriptor_set_sha256: None,
        services: vec![TrustedService {
          name: full_name.clone(),
          methods: Vec::new(),
        }],
      };

      assert_eq!(
        validate(
          schema.clone(),
          &TrustedManifest {
            expected_descriptor_set_sha256: None,
            services: Vec::new(),
          },
          SchemaLimits::default(),
        ),
        Err(SchemaError::ReservedService(full_name.clone()))
      );
      assert_eq!(validate(schema, &manifest, SchemaLimits::default()), Err(SchemaError::ReservedService(full_name)));
    }
  }

  #[test]
  fn rejects_conflicting_duplicate_filenames() {
    let mut schema = portable_driver_schema();
    let mut conflicting = schema.file[0].clone();
    conflicting.syntax = Some("proto2".to_string());
    let name = conflicting.name.clone().expect("fixture filename");
    schema.file.push(conflicting);

    assert_eq!(validate(schema, &portable_driver_manifest(), SchemaLimits::default()), Err(SchemaError::ConflictingFile(name)));
  }

  #[test]
  fn rejects_missing_and_out_of_closure_descriptor_files() {
    let mut missing = portable_driver_schema();
    let removed = missing.file.iter().position(|file| file.name.as_deref() == Some("auv/api/driver/v1/geometry.proto")).expect("geometry");
    missing.file.remove(removed);
    assert!(matches!(
      validate(missing, &portable_driver_manifest(), SchemaLimits::default()),
      Err(SchemaError::MissingDependency { dependency, .. }) if dependency == "auv/api/driver/v1/geometry.proto"
    ));

    let mut extra = portable_driver_schema();
    extra.file.push(FileDescriptorProto {
      name: Some("hidden/orphan.proto".to_string()),
      package: Some("hidden".to_string()),
      ..Default::default()
    });
    assert_eq!(
      validate(extra, &portable_driver_manifest(), SchemaLimits::default()),
      Err(SchemaError::UnexpectedFile("hidden/orphan.proto".to_string()))
    );
  }

  #[test]
  fn rejects_service_and_method_sets_outside_trusted_policy() {
    let mut extra_service = portable_driver_schema();
    extra_service.file.push(FileDescriptorProto {
      name: Some("hidden/service.proto".to_string()),
      package: Some("hidden.v1".to_string()),
      service: vec![ServiceDescriptorProto {
        name: Some("HiddenService".to_string()),
        ..Default::default()
      }],
      ..Default::default()
    });
    assert!(matches!(
      validate(extra_service, &portable_driver_manifest(), SchemaLimits::default()),
      Err(SchemaError::ServiceSetMismatch { .. })
    ));

    let mut extra_method = portable_driver_schema();
    let method = method_mut(&mut extra_method, "auv.api.driver.v1.DisplayService", "ListDisplays").clone();
    let display_file = extra_method
      .file
      .iter_mut()
      .find(|file| file.service.iter().any(|service| service.name.as_deref() == Some("DisplayService")))
      .expect("display file");
    let display_service = display_file.service.iter_mut().find(|service| service.name.as_deref() == Some("DisplayService")).unwrap();
    display_service.method.push(MethodDescriptorProto {
      name: Some("Hidden".to_string()),
      ..method
    });
    assert!(matches!(
      validate(extra_method, &portable_driver_manifest(), SchemaLimits::default()),
      Err(SchemaError::MethodSetMismatch { service, .. }) if service == "auv.api.driver.v1.DisplayService"
    ));
  }

  #[test]
  fn rejects_input_output_and_streaming_signature_mismatches() {
    for field in ["input", "output"] {
      let mut schema = portable_driver_schema();
      let method = method_mut(&mut schema, "auv.api.driver.v1.DisplayService", "ListDisplays");
      if field == "input" {
        method.input_type = Some(".hidden.WrongRequest".to_string());
      } else {
        method.output_type = Some(".hidden.WrongResponse".to_string());
      }
      assert!(matches!(
        validate(schema, &portable_driver_manifest(), SchemaLimits::default()),
        Err(SchemaError::MethodSignatureMismatch { field: mismatch, .. }) if mismatch == format!("{field} type")
      ));
    }

    let mut schema = portable_driver_schema();
    method_mut(&mut schema, "auv.api.driver.v1.DisplayService", "ListDisplays").server_streaming = Some(true);
    assert!(matches!(
      validate(schema, &portable_driver_manifest(), SchemaLimits::default()),
      Err(SchemaError::MethodSignatureMismatch {
        field: "server streaming",
        ..
      })
    ));
  }

  #[test]
  fn rejects_externally_exposed_streaming_even_when_manifest_matches() {
    let mut schema = portable_driver_schema();
    method_mut(&mut schema, "auv.api.driver.v1.DisplayService", "ListDisplays").server_streaming = Some(true);
    let mut manifest = portable_driver_manifest();
    manifest.services[0].methods[0].server_streaming = true;

    assert_eq!(
      validate(schema, &manifest, SchemaLimits::default()),
      Err(SchemaError::ExternalStreamingUnsupported {
        service: "auv.api.driver.v1.DisplayService".to_string(),
        method: "ListDisplays".to_string(),
      })
    );
  }

  #[test]
  fn custom_manifest_must_match_its_pinned_canonical_digest() {
    let schema = portable_driver_schema_bytes();
    let bootstrap = validate_encoded(&schema, &portable_driver_manifest(), SchemaLimits::default()).expect("built-in bootstrap");
    let mut pinned = portable_driver_manifest();
    pinned.expected_descriptor_set_sha256 = Some(bootstrap.descriptor_set_sha256);
    assert_eq!(validate_encoded(&schema, &pinned, SchemaLimits::default()).expect("matching custom pin"), bootstrap);

    let expected = [0x5a; 32];
    pinned.expected_descriptor_set_sha256 = Some(expected);
    assert_eq!(
      validate_encoded(&schema, &pinned, SchemaLimits::default()),
      Err(SchemaError::DigestMismatch {
        expected,
        observed: bootstrap.descriptor_set_sha256,
      })
    );
  }

  #[test]
  fn enforces_descriptor_resource_limits_before_admission() {
    let limits = SchemaLimits {
      max_files: 1,
      ..SchemaLimits::default()
    };
    assert!(matches!(
      validate(portable_driver_schema(), &portable_driver_manifest(), limits),
      Err(SchemaError::LimitExceeded { limit: "files", .. })
    ));
  }

  fn custom_config(directory: &Path, externally_exposed: bool) -> RunnerProviderConfig {
    let schema = portable_driver_schema_bytes();
    let admitted = validate_encoded(&schema, &portable_driver_manifest(), SchemaLimits::default()).expect("canonical fixture schema");
    let descriptor_set = directory.join("custom.binpb");
    std::fs::write(&descriptor_set, &admitted.canonical_descriptor_set).expect("write descriptor fixture");
    let executable = directory.join("custom-runner");
    std::fs::write(&executable, b"#!/bin/sh\nexit 0\n").expect("write executable fixture");
    #[cfg(unix)]
    {
      use std::os::unix::fs::PermissionsExt;
      std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).expect("secure executable mode");
    }
    RunnerProviderConfig {
      runner_class: "auv.example.custom".to_string(),
      display_name: "Custom example".to_string(),
      runtime: executable_runtime(executable),
      descriptor_set,
      descriptor_set_sha256: hex::encode(admitted.descriptor_set_sha256),
      services: vec![
        RunnerProviderServiceConfig {
          name: "auv.api.driver.v1.DisplayService".to_string(),
          externally_exposed,
        },
        RunnerProviderServiceConfig {
          name: "auv.api.driver.v1.WindowService".to_string(),
          externally_exposed,
        },
      ],
      supported_lifecycles: vec![
        RunnerProviderLifecycle::Ephemeral,
        RunnerProviderLifecycle::UnlessShutdown,
      ],
      operation_capacity: 3,
    }
  }

  #[test]
  fn first_party_inference_provider_is_typed_but_not_an_aggregated_route() {
    let directory = tempfile::tempdir().expect("provider fixture directory");
    let runtime = custom_config(directory.path(), false).runtime;
    let registry = RunnerProviderRegistry::build_with_first_party(None, Some(runtime), Vec::new()).expect("built-in inference provider");
    let provider = registry.get(INFERENCE_RUNNER_CLASS).expect("inference RunnerClass");
    assert!(provider.external_routes.is_empty());
    assert_eq!(provider.capabilities.len(), 1);
    assert_eq!(provider.capabilities[0].service, INFERENCE_SERVICE);
    assert_eq!(provider.capabilities[0].methods, ["DetectObjects"]);
  }

  #[test]
  fn json_config_registers_a_pinned_provider_and_projects_policy() {
    let directory = tempfile::tempdir().expect("provider fixture directory");
    let config = custom_config(directory.path(), true);
    let config_path = directory.path().join("provider.json");
    std::fs::write(&config_path, serde_json::to_vec_pretty(&config).expect("encode provider JSON")).expect("write provider JSON");

    let loaded = RunnerProviderConfig::load_json(&config_path).expect("load bounded trusted JSON");
    assert_eq!(loaded, config);
    assert_eq!(
      RunnerProviderConfig::canonical_descriptor_sha256(&loaded.descriptor_set, &loaded.services).expect("compute public pin"),
      loaded.descriptor_set_sha256
    );
    let registry = RunnerProviderRegistry::build(None, vec![loaded]).expect("register provider");
    let provider = registry.get("auv.example.custom").expect("custom provider");
    assert_eq!(provider.operation_capacity, 3);
    assert_eq!(provider.validated_schema.descriptor_set_sha256, parse_digest(&config.descriptor_set_sha256).unwrap());
    assert!(provider.external_routes.contains(&("auv.api.driver.v1.DisplayService".to_string(), "ListDisplays".to_string())));
    assert_eq!(provider.capabilities.len(), 2);
    assert_eq!(
      provider
        .runner_class_record(core_proto::DeviceRef {
          device_id: "device_test".to_string(),
        })
        .supported_lifecycles,
      [
        core_proto::RunnerLifecycle::Ephemeral as i32,
        core_proto::RunnerLifecycle::UnlessShutdown as i32
      ]
    );
  }

  #[test]
  fn remote_grpc_provider_config_is_typed_and_rejects_invalid_endpoints() {
    let directory = tempfile::tempdir().expect("provider fixture directory");
    let mut config = custom_config(directory.path(), false);
    config.runtime = RunnerRuntime::RemoteGrpc(RemoteGrpcRunnerRuntime {
      endpoint: "http://127.0.0.1:50051".to_string(),
    });
    let encoded = serde_json::to_value(&config).expect("encode remote provider");
    assert_eq!(encoded["runtime"]["type"], "remote-grpc");
    assert!(RunnerProviderRegistry::build(None, vec![config.clone()]).is_ok());

    config.runtime = RunnerRuntime::RemoteGrpc(RemoteGrpcRunnerRuntime {
      endpoint: "not a gRPC endpoint".to_string(),
    });
    assert!(matches!(RunnerProviderRegistry::build(None, vec![config]), Err(RunnerProviderConfigError::InvalidRemoteGrpcEndpoint(_))));
  }

  #[test]
  fn service_catalog_is_derived_from_trusted_discoverable_methods() {
    let directory = tempfile::tempdir().expect("provider fixture directory");
    let registry = RunnerProviderRegistry::build(None, vec![custom_config(directory.path(), true)]).expect("register provider");
    let catalog = registry.service_catalog();
    assert_eq!(catalog.len(), 2);
    assert!(catalog.iter().all(|service| service.runner_class == "auv.example.custom"));
    assert_eq!(catalog[0].full_name, "auv.api.driver.v1.DisplayService");
    assert_eq!(catalog[0].methods[0].full_name, "auv.api.driver.v1.DisplayService.ListDisplays");
    assert!(
      catalog.iter().flat_map(|service| &service.methods).all(|method| {
        method.discoverable && method.effect != auv_api_proto::auv::api::annotations::v1::MethodEffect::Unspecified as i32
      })
    );
  }

  #[test]
  fn non_external_services_remain_capabilities_but_cannot_be_aggregated() {
    let directory = tempfile::tempdir().expect("provider fixture directory");
    let registry = RunnerProviderRegistry::build(None, vec![custom_config(directory.path(), false)]).expect("register internal provider");
    let provider = registry.get("auv.example.custom").expect("custom provider");
    assert_eq!(provider.capabilities.len(), 2);
    assert!(provider.external_routes.is_empty());
  }

  #[test]
  fn custom_provider_rejects_an_unpinned_or_duplicate_policy() {
    let directory = tempfile::tempdir().expect("provider fixture directory");
    let mut wrong_digest = custom_config(directory.path(), true);
    wrong_digest.descriptor_set_sha256 = hex::encode([0x5a; 32]);
    assert!(matches!(
      RunnerProviderRegistry::build(None, vec![wrong_digest]),
      Err(RunnerProviderConfigError::Schema(message)) if message.contains("digest differs")
    ));

    let first = custom_config(directory.path(), true);
    let second = first.clone();
    assert!(matches!(
      RunnerProviderRegistry::build(None, vec![first, second]),
      Err(RunnerProviderConfigError::DuplicateRunnerClass(class)) if class == "auv.example.custom"
    ));
  }

  #[test]
  fn duplicate_public_routes_require_an_identical_descriptor_closure() {
    let first_directory = tempfile::tempdir().expect("first provider fixture directory");
    let second_directory = tempfile::tempdir().expect("second provider fixture directory");
    let first = custom_config(first_directory.path(), true);
    let mut identical = custom_config(second_directory.path(), true);
    identical.runner_class = "auv.example.identical".to_string();
    RunnerProviderRegistry::build(None, vec![first.clone(), identical]).expect("identical schema may have multiple providers");

    let mut conflicting = custom_config(second_directory.path(), true);
    conflicting.runner_class = "auv.example.conflicting".to_string();
    let bytes = std::fs::read(&conflicting.descriptor_set).expect("read fixture");
    let pool = DescriptorPool::decode(bytes.as_slice()).expect("decode fixture descriptor");
    let mut files = pool.files().map(|file| file.encode_to_vec()).collect::<Vec<_>>();
    // Source metadata is part of the pinned schema closure even though it does
    // not alter typed method signatures.
    files[0].extend_from_slice(&[
      0x4a, 0x0d, // source_code_info
      0x0a, 0x0b, // location
      0x1a, 0x09, b'd', b'i', b'f', b'f', b'e', b'r', b'e', b'n', b't', // leading_comments
    ]);
    let encoded = encode_descriptor_set_files(&files);
    let manifest = manifest_from_trusted_descriptors(
      &encoded,
      &[
        ("auv.api.driver.v1.DisplayService", true),
        ("auv.api.driver.v1.WindowService", true),
      ],
      None,
    )
    .expect("derive changed manifest");
    let changed = validate_encoded(&encoded, &manifest, SchemaLimits::default()).expect("changed schema remains structurally valid");
    std::fs::write(&conflicting.descriptor_set, &changed.canonical_descriptor_set).expect("write changed schema");
    conflicting.descriptor_set_sha256 = hex::encode(changed.descriptor_set_sha256);

    assert!(matches!(
      RunnerProviderRegistry::build(None, vec![first, conflicting]),
      Err(RunnerProviderConfigError::ConflictingExternalRoute(path)) if path.contains("DisplayService/ListDisplays")
    ));
  }

  #[cfg(unix)]
  #[test]
  fn custom_provider_rejects_relative_symlink_and_writable_trusted_paths() {
    use std::os::unix::fs::{PermissionsExt, symlink};

    let directory = tempfile::tempdir().expect("provider fixture directory");
    let mut relative = custom_config(directory.path(), true);
    let RunnerRuntime::Executable(runtime) = &mut relative.runtime else {
      panic!("fixture uses executable runtime")
    };
    runtime.executable = PathBuf::from("relative-runner");
    assert!(matches!(RunnerProviderRegistry::build(None, vec![relative]), Err(RunnerProviderConfigError::PathNotAbsolute(_))));

    let writable = custom_config(directory.path(), true);
    std::fs::set_permissions(&writable.descriptor_set, std::fs::Permissions::from_mode(0o666)).expect("make descriptor writable");
    assert!(matches!(RunnerProviderRegistry::build(None, vec![writable]), Err(RunnerProviderConfigError::InsecurePermissions(_))));

    let target = directory.path().join("target-runner");
    std::fs::write(&target, b"#!/bin/sh\n").expect("write target");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o700)).expect("target executable mode");
    let link = directory.path().join("linked-runner");
    symlink(&target, &link).expect("create executable symlink");
    let mut linked = custom_config(directory.path(), true);
    let RunnerRuntime::Executable(runtime) = &mut linked.runtime else {
      panic!("fixture uses executable runtime")
    };
    runtime.executable = link;
    assert!(matches!(RunnerProviderRegistry::build(None, vec![linked]), Err(RunnerProviderConfigError::SymbolicLink(_))));
  }
}
