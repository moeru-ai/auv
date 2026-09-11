//! Device, Run, and Runner placement above the transport client.
//!
//! This module owns selector precedence and implicit Run lifecycle. Capability
//! clients receive canonical refs and do not need to know how placement was
//! chosen.

pub mod runner;

use std::collections::HashMap;

use auv_api_proto::auv::api::daemon::v1 as proto;

use auv_api_client::PairedConnectConfig;
use auv_api_client::protocol::grpc::Client as GrpcClient;

use crate::resource::{DeviceSelector, RunSelector, RunnerClassId};
use crate::{AuvContext, ContextError, discovery, profile};

/// Failure while resolving placement or managing an implicit Run.
#[derive(Debug, thiserror::Error)]
pub enum PlacementError {
  /// Resolving the connection context failed.
  #[error(transparent)]
  Context(#[from] ContextError),
  /// The inherited context environment is not valid Unicode.
  #[error("AUV_CONTEXT is not valid Unicode: {0}")]
  ContextEnvironment(std::env::VarError),
  /// A daemon control request failed.
  #[error(transparent)]
  Client(#[from] crate::error::ClientError),
  /// A routed Runner capability operation failed.
  #[error(transparent)]
  Capability(#[from] runner::CapabilityError),
  /// Resource placement could not be resolved.
  #[error("{0}")]
  Selection(String),
  /// The primary operation and its cleanup both failed.
  #[error("{primary}; cleanup also failed: {cleanup}")]
  Cleanup {
    /// Failure from the primary operation.
    primary: String,
    /// Failure from cleanup after the primary failure.
    cleanup: String,
  },
}

/// How a high-level operation obtains its Run.
#[derive(Clone, Debug, Default)]
pub enum RunSelection {
  /// Inherit `AUV_CONTEXT.run_id` when present, otherwise create a Run.
  #[default]
  Auto,
  /// Attach to one existing Run selected by canonical ID or prefix.
  Existing(RunSelector),
  /// Explicitly create a Run even when inherited context names another Run.
  New,
}

/// Run selection, Device placement, and labels for high-level operations.
#[derive(Clone, Debug, Default)]
pub struct RunOptions {
  /// Whether to inherit, attach, or create a Run.
  pub selection: RunSelection,
  /// Optional Device placement selector.
  pub device: DeviceSelector,
  /// Labels applied when this operation creates a Run.
  pub labels: HashMap<String, String>,
}

/// Device and RunnerClass placement for a routed Runner.
#[derive(Clone, Debug)]
pub struct RunnerOptions {
  /// Optional Device placement selector.
  pub device: DeviceSelector,
  /// RunnerClass to route or instantiate.
  pub runner_class: RunnerClassId,
}

impl Default for RunnerOptions {
  fn default() -> Self {
    Self {
      device: DeviceSelector::default(),
      runner_class: "auv.core.local".parse().expect("the built-in RunnerClass ID is valid"),
    }
  }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PlacementConstraint {
  #[default]
  Automatic,
  LocalOnly,
}

/// High-level client that selects Device, Run, and Runner resources without
/// exposing the underlying Unix-socket, TCP, or paired-bearer transport.
#[derive(Clone, Debug)]
pub struct Client {
  // TODO(multi-protocol-client): keep the concrete gRPC adapter until an
  // owner-approved non-gRPC protocol provides a second implementation; that
  // implementation is the trigger for extracting a protocol-neutral seam.
  grpc: GrpcClient,
  context: Option<AuvContext>,
  paired_remote: bool,
  constraint: PlacementConstraint,
}

impl Client {
  pub(crate) fn grpc_client(&self) -> GrpcClient {
    self.grpc.clone()
  }

  /// Returns Device inventory operations.
  pub fn devices(&self) -> crate::devices::Devices {
    crate::devices::Devices::new(self.clone())
  }

  /// Returns live pairing operations.
  pub fn pairing(&self) -> crate::pairing::Pairing {
    crate::pairing::Pairing::new(self.clone())
  }

  /// Returns Run control operations.
  pub fn runs(&self) -> crate::runs::Runs {
    crate::runs::Runs::new(self.clone())
  }

  /// Returns Runner and RunnerClass control operations.
  pub fn runners(&self) -> crate::runners::Runners {
    crate::runners::Runners::new(self.clone())
  }

  pub(crate) fn from_grpc(grpc: GrpcClient) -> Self {
    Self {
      grpc,
      context: None,
      paired_remote: false,
      constraint: PlacementConstraint::Automatic,
    }
  }

  /// Connects using one non-secret AUV context.
  pub async fn from_context(context: AuvContext) -> Result<Self, PlacementError> {
    Ok(Self::resolve_context(context).await?)
  }

  /// Connects using a context and an explicitly supplied profile store.
  pub async fn from_context_with_profiles(context: AuvContext, profiles: &profile::ProfileStore) -> Result<Self, ContextError> {
    Self::resolve_context_with_profiles(context, profiles).await
  }

  /// Loads `AUV_CONTEXT` and connects to its selected daemon.
  pub async fn from_env() -> Result<Self, PlacementError> {
    Self::from_context(AuvContext::from_env()?).await
  }

  /// Returns the resolved non-secret context, when connection used one.
  pub fn context(&self) -> Option<&AuvContext> {
    self.context.as_ref()
  }

  /// Uses inherited plugin context when present, otherwise discovers the
  /// current user's local daemon.
  pub async fn from_env_or_local() -> Result<Self, PlacementError> {
    match std::env::var("AUV_CONTEXT") {
      Ok(value) => {
        let context = serde_json::from_str(&value).map_err(|error| PlacementError::Context(ContextError::Decode(error)))?;
        Self::from_context(context).await
      }
      Err(std::env::VarError::NotPresent) => {
        let endpoint = discovery::resolve(None).map_err(ContextError::Discovery)?.ok_or(ContextError::EndpointNotDiscovered)?;
        Ok(Self::from_grpc(GrpcClient::connect(endpoint).await.map_err(|error| ContextError::Connect(error.to_string()))?))
      }
      Err(error) => Err(PlacementError::ContextEnvironment(error)),
    }
  }

  /// Connects to an explicit daemon endpoint or the locally discovered daemon.
  /// A missing discovery descriptor is reported as `Ok(None)` so inventory
  /// interfaces can preserve their daemon-optional behavior.
  pub async fn discover(explicit: Option<&str>) -> Result<Option<Self>, ContextError> {
    let Some(endpoint) = discovery::resolve(explicit)? else {
      return Ok(None);
    };
    Ok(Some(Self::from_grpc(GrpcClient::connect(endpoint).await.map_err(|error| ContextError::Connect(error.to_string()))?)))
  }

  async fn resolve_context(mut context: AuvContext) -> Result<Self, ContextError> {
    validate_context_selectors(&context)?;
    if context.config_profile.is_some() {
      return Self::resolve_context_with_profiles(context, &profile::ProfileStore::from_env()?).await;
    }
    if context.daemon_endpoint.is_none() && context.run_id.is_some() && context.device_id.is_none() && context.device_name.is_none() {
      return Self::resolve_run_context(context).await;
    }
    if context.daemon_endpoint.is_none() && (context.device_id.is_some() || context.device_name.is_some()) {
      let profiles = profile::ProfileStore::from_env()?;
      let configured = match profiles.list_devices() {
        Ok(configured) => configured,
        Err(profile::ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(error) => return Err(error.into()),
      };
      let remote_matches = configured
        .iter()
        .filter(|device| context.device_id.as_ref().is_none_or(|id| device.device_id() == id))
        .filter(|device| context.device_name.as_ref().is_none_or(|name| device.device_name() == name))
        .collect::<Vec<_>>();
      let mut local = match discovery::resolve(None)? {
        Some(endpoint) => {
          let endpoint_display = endpoint.to_string();
          Some((endpoint_display, GrpcClient::connect(endpoint).await.map_err(|error| ContextError::Connect(error.to_string()))?))
        }
        None => None,
      };
      let local_devices = match local.as_mut() {
        Some((_, client)) => client
          .devices()
          .list_devices()
          .await
          .map_err(|status| ContextError::RemoteDeviceList(crate::error::ClientError::from_status("ListDevices", status)))?,
        None => Vec::new(),
      };
      let local_matches = matching_devices(&context, &local_devices);
      let candidate_ids = local_matches
        .iter()
        .filter_map(|device| device.r#ref.as_ref().map(|reference| reference.device_id.as_str()))
        .chain(remote_matches.iter().map(|device| device.device_id()))
        .collect::<Vec<_>>();
      match (local_matches.as_slice(), remote_matches.as_slice()) {
        ([local_device], []) => {
          let (endpoint, grpc) = local.expect("local match requires a connected local daemon");
          context_matches_canonical_device(&context, local_device)?;
          context.daemon_endpoint = Some(endpoint);
          return Ok(Self {
            grpc,
            context: Some(context),
            paired_remote: false,
            constraint: PlacementConstraint::Automatic,
          });
        }
        ([], [remote]) => {
          context.config_profile = Some(remote.config_profile().to_string());
          return Self::resolve_context_with_profiles(context, &profiles).await;
        }
        ([], []) => return Err(ContextError::DeviceNotConfigured),
        _ => {
          return Err(ContextError::DeviceSelectionAmbiguous {
            selector: context.device_id.clone().or(context.device_name.clone()).unwrap_or_default(),
            candidate_ids: candidate_ids.join(", "),
          });
        }
      }
    }
    let endpoint = match context.daemon_endpoint.as_deref() {
      Some(endpoint) => endpoint.parse().map_err(|source| discovery::DiscoveryError::InvalidEndpoint {
        endpoint: endpoint.to_string(),
        source,
      })?,
      None => {
        let endpoint = discovery::resolve(None)?.ok_or(ContextError::EndpointNotDiscovered)?;
        context.daemon_endpoint = Some(endpoint.to_string());
        endpoint
      }
    };
    let grpc = GrpcClient::connect(endpoint).await.map_err(|error| ContextError::Connect(error.to_string()))?;
    Ok(Self {
      grpc,
      context: Some(context),
      paired_remote: false,
      constraint: PlacementConstraint::Automatic,
    })
  }

  async fn resolve_run_context(context: AuvContext) -> Result<Self, ContextError> {
    let run_id = context.run_id.clone().expect("run-only resolution requires run_id");
    let mut matches = Vec::<(String, String, Self)>::new();
    if let Some(endpoint) = discovery::resolve(None)? {
      let endpoint_display = endpoint.to_string();
      let grpc = GrpcClient::connect(endpoint).await.map_err(|error| ContextError::Connect(error.to_string()))?;
      let runs = grpc.runs().list_runs().await.map_err(|status| ContextError::RunLookup {
        location: "local daemon".to_string(),
        error: crate::error::ClientError::from_status("ListRuns", status),
      })?;
      for canonical_run_id in matching_run_ids(&run_id, &runs) {
        let mut local_context = context.clone();
        local_context.daemon_endpoint = Some(endpoint_display.clone());
        local_context.run_id = Some(canonical_run_id.clone());
        matches.push((
          "local".to_string(),
          canonical_run_id,
          Self {
            grpc: grpc.clone(),
            context: Some(local_context),
            paired_remote: false,
            constraint: PlacementConstraint::Automatic,
          },
        ));
      }
    }
    let profiles = profile::ProfileStore::from_env()?;
    let configured = match profiles.list_devices() {
      Ok(configured) => configured,
      Err(profile::ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
      Err(error) => return Err(error.into()),
    };
    for configured in configured {
      let mut remote_context = context.clone();
      remote_context.config_profile = Some(configured.config_profile().to_string());
      let mut remote = Self::resolve_context_with_profiles(remote_context, &profiles).await?;
      let location = format!("paired profile {:?}", configured.config_profile());
      let runs = remote.grpc.runs().list_runs().await.map_err(|status| ContextError::RunLookup {
        location: location.clone(),
        error: crate::error::ClientError::from_status("ListRuns", status),
      })?;
      for canonical_run_id in matching_run_ids(&run_id, &runs) {
        if let Some(remote_context) = remote.context.as_mut() {
          remote_context.run_id = Some(canonical_run_id.clone());
        }
        matches.push((location.clone(), canonical_run_id, remote.clone()));
      }
    }
    match matches.len() {
      0 => Err(ContextError::RunNotFound(run_id)),
      1 => Ok(matches.pop().expect("one Run match").2),
      _ => Err(ContextError::RunAmbiguous {
        run_id,
        locations: matches.into_iter().map(|(location, canonical, _)| format!("{location} ({canonical})")).collect::<Vec<_>>().join(", "),
      }),
    }
  }

  async fn resolve_context_with_profiles(mut context: AuvContext, profiles: &profile::ProfileStore) -> Result<Self, ContextError> {
    validate_context_selectors(&context)?;
    let profile = profiles.resolve(&context)?;
    if let Some(endpoint) = context.daemon_endpoint.as_deref() {
      let selected = profile::validate_remote_endpoint(endpoint)?;
      if selected.authority() != profile.endpoint().authority() {
        return Err(ContextError::ProfileEndpointMismatch {
          context: selected.to_string(),
          profile: profile.endpoint().to_string(),
        });
      }
    }
    let grpc = GrpcClient::connect_paired(PairedConnectConfig {
      endpoint: profile.endpoint().clone(),
      device_credential: profile.device_credential().to_string(),
    })
    .await
    .map_err(|error| ContextError::PairedConnect(error.to_string()))?;
    let devices = grpc
      .devices()
      .list_devices()
      .await
      .map_err(|status| ContextError::RemoteDeviceList(crate::error::ClientError::from_status("ListDevices", status)))?;
    let canonical = devices
      .into_iter()
      .find(|device| device.r#ref.as_ref().is_some_and(|reference| reference.device_id == profile.device_id()))
      .ok_or_else(|| ContextError::CanonicalDeviceMissing(profile.device_id().to_string()))?;
    context.device_id = Some(profile.device_id().to_string());
    context.device_name = (!canonical.name.is_empty()).then_some(canonical.name);
    context.daemon_endpoint = Some(profile.endpoint().to_string());
    context.config_profile = Some(profile.config_profile().to_string());
    Ok(Self {
      grpc,
      context: Some(context),
      paired_remote: true,
      constraint: PlacementConstraint::Automatic,
    })
  }

  /// Constrains placement to the current user's local daemon and its local
  /// Device. It never reinterprets a paired daemon's own `Device.local` bit as
  /// caller-local placement.
  pub fn local(mut self) -> Result<Self, PlacementError> {
    if self.paired_remote {
      return Err(PlacementError::Selection("local placement conflicts with an explicitly paired remote daemon transport".to_string()));
    }
    self.constraint = PlacementConstraint::LocalOnly;
    Ok(self)
  }

  /// Creates or attaches to a Run using shared placement policy.
  pub async fn run(&self, options: RunOptions) -> Result<RunClient, PlacementError> {
    // TODO(distributed-run-authority): one Run currently owns one daemon
    // transport; see the accepted aggregated API design before adding peers.
    let grpc = self.grpc.clone();
    let devices = grpc.devices().list_devices().await.map_err(|status| crate::error::ClientError::from_status("ListDevices", status))?;
    let context = self.context.clone().unwrap_or_default();
    let run_id = match &options.selection {
      RunSelection::Auto => {
        context.run_id.as_deref().map(RunSelector::parse).transpose().map_err(|error| PlacementError::Selection(error.to_string()))?
      }
      RunSelection::Existing(run_id) => Some(run_id.clone()),
      RunSelection::New => None,
    };
    let existing = match run_id {
      Some(run_selector) => {
        let runs = grpc.runs().list_runs().await.map_err(|status| crate::error::ClientError::from_status("ListRuns", status))?;
        let run_id =
          resolve_run_id(&run_selector, runs.iter().filter_map(|run| run.r#ref.as_ref().map(|reference| reference.run_id.as_str())))?;
        Some(grpc.runs().get_run(run_id).await.map_err(|status| crate::error::ClientError::from_status("GetRun", status))?)
      }
      None => None,
    };
    if let Some(run) = &existing
      && run.phase != proto::RunPhase::Running as i32
    {
      return Err(PlacementError::Selection("Runner placement requires a running Run".to_string()));
    }

    let explicit = (!options.device.is_empty()).then_some(options.device.clone());
    let inherited = context_device_selector(&context);
    let selector = explicit.or(inherited);
    let allowed = existing.as_ref().map(|run| run.devices.as_slice());
    let selected_device = match selector {
      Some(selector) => Some(select_device(&devices, &selector, self.constraint, allowed)?),
      None => select_default_device(&devices, self.constraint, allowed)?,
    };

    let (run, owned) = match existing {
      Some(run) => (run, false),
      None => {
        let device =
          selected_device.as_ref().ok_or_else(|| PlacementError::Selection("creating a Run requires one unambiguous Device".to_string()))?;
        let device_ref = required_device_ref(device)?;
        (
          grpc
            .runs()
            .create_run(proto::CreateRunRequest {
              devices: vec![device_ref],
              labels: options.labels,
            })
            .await
            .map_err(|status| crate::error::ClientError::from_status("CreateRun", status))?,
          true,
        )
      }
    };

    let run_devices = devices
      .into_iter()
      .filter(|device| {
        device.r#ref.as_ref().is_some_and(|reference| run.devices.iter().any(|candidate| candidate.device_id == reference.device_id))
      })
      .collect();
    let resource = crate::runs::Run::try_from(run.clone()).map_err(|error| PlacementError::Selection(error.to_string()))?;
    let device = selected_device
      .clone()
      .map(crate::devices::Device::try_from)
      .transpose()
      .map_err(|error| PlacementError::Selection(error.to_string()))?;
    Ok(RunClient {
      client: self.clone(),
      run,
      resource,
      selected_device,
      device,
      run_devices,
      owned,
    })
  }

  /// Creates or inherits a Run, then constructs a RunnerClass route using the
  /// same placement rules. It owns cleanup only when it created the Run.
  pub async fn runner(&self, options: RunnerOptions) -> Result<RunnerExecution, PlacementError> {
    self.runner_with(RunOptions::default(), options).await
  }

  /// Creates or attaches to a Run and routes a Runner with explicit options.
  pub async fn runner_with(&self, run_options: RunOptions, runner_options: RunnerOptions) -> Result<RunnerExecution, PlacementError> {
    let run = self.run(run_options).await?;
    match run.runner(runner_options).await {
      Ok(runner) => Ok(RunnerExecution { run, runner }),
      Err(primary) if run.is_owned() => match run.finish_if_owned(crate::runs::RunOutcome::Canceled).await {
        Ok(_) => Err(primary),
        Err(cleanup) => Err(PlacementError::Cleanup {
          primary: primary.to_string(),
          cleanup: cleanup.to_string(),
        }),
      },
      Err(primary) => Err(primary),
    }
  }
}

/// A high-level Run handle that tracks whether it owns cleanup.
#[derive(Debug)]
pub struct RunClient {
  client: Client,
  run: proto::Run,
  resource: crate::runs::Run,
  selected_device: Option<proto::Device>,
  device: Option<crate::devices::Device>,
  run_devices: Vec<proto::Device>,
  owned: bool,
}

// TODO(run-abrupt-cleanup): network cleanup cannot be made reliable from
// async Drop. Add a cancellation-aware scoped helper once frontend signal
// forwarding owns a bounded cleanup deadline; normal paths must call
// `finish_if_owned` or `RunnerExecution::finish` explicitly.

/// A routed Runner together with its Run lifecycle owner.
#[derive(Debug)]
pub struct RunnerExecution {
  run: RunClient,
  runner: runner::RunnerClient,
}

impl RunnerExecution {
  /// Returns the associated typed Run resource.
  pub fn run(&self) -> &crate::runs::Run {
    self.run.resource()
  }

  /// Returns display capabilities.
  pub fn displays(&self) -> runner::DisplaysClient {
    self.runner.displays()
  }

  /// Returns window capabilities.
  pub fn windows(&self) -> runner::WindowsClient {
    self.runner.windows()
  }

  /// Returns input-delivery capabilities.
  pub fn input(&self) -> runner::InputClient {
    self.runner.input()
  }

  /// Returns overlay presentation capabilities.
  pub fn overlay(&self) -> runner::OverlayClient {
    self.runner.overlay()
  }

  /// Returns macOS-specific capabilities.
  pub fn macos(&self) -> runner::MacosClient {
    self.runner.macos()
  }

  /// Runs OCR on an existing capture through this Runner.
  pub async fn recognize_text(
    &self,
    capture: auv_driver::Capture,
    region: Option<runner::NormalizedRegion>,
    custom_words: Vec<String>,
    recognition_languages: Vec<String>,
  ) -> Result<auv_driver::TextRecognition, runner::CapabilityError> {
    self.runner.recognize_text(capture, region, custom_words, recognition_languages).await
  }

  /// Completes an implicitly owned Run and returns its terminal resource.
  pub async fn finish(self, outcome: crate::runs::RunOutcome) -> Result<crate::runs::Run, PlacementError> {
    self.run.finish_if_owned(outcome).await
  }
}

impl RunClient {
  /// Returns the typed Run resource.
  pub fn resource(&self) -> &crate::runs::Run {
    &self.resource
  }

  /// Returns the selected Device, when placement resolved one.
  pub fn device(&self) -> Option<&crate::devices::Device> {
    self.device.as_ref()
  }

  /// Returns whether this handle created and therefore owns the Run.
  pub fn is_owned(&self) -> bool {
    self.owned
  }

  /// Constrains subsequent Runner placement to a caller-local Device.
  pub fn local(mut self) -> Result<Self, PlacementError> {
    self.client = self.client.local()?;
    self.selected_device = select_default_device(&self.run_devices, PlacementConstraint::LocalOnly, Some(&self.run.devices))?;
    self.device = self
      .selected_device
      .clone()
      .map(crate::devices::Device::try_from)
      .transpose()
      .map_err(|error| PlacementError::Selection(error.to_string()))?;
    Ok(self)
  }

  /// Routes a Runner within this Run.
  pub async fn runner(&self, options: RunnerOptions) -> Result<runner::RunnerClient, PlacementError> {
    let run_id = self
      .run
      .r#ref
      .as_ref()
      .map(|run| run.run_id.clone())
      .filter(|run_id| !run_id.trim().is_empty())
      .ok_or_else(|| PlacementError::Selection("Run omitted its canonical ref".to_string()))?;
    let selected_device = if options.device.is_empty() {
      self.selected_device.clone()
    } else {
      Some(select_device(&self.run_devices, &options.device, self.client.constraint, Some(&self.run.devices))?)
    };
    if self.client.constraint == PlacementConstraint::LocalOnly && selected_device.as_ref().is_none_or(|device| !device.local) {
      return Err(PlacementError::Selection("local Runner placement requires one caller-local Device in the Run".to_string()));
    }
    let device_id = selected_device.as_ref().map(required_device_ref).transpose()?.map(|device| device.device_id);
    runner::RunnerClient::new(
      self.client.grpc.clone(),
      auv_api_client::RunnerRoute {
        device_id,
        run_id: Some(run_id),
        runner_class: options.runner_class.to_string(),
      },
    )
    .map_err(Into::into)
  }

  /// Stops only Runs created implicitly by this high-level client. Attached
  /// Runs remain open for later operations.
  pub async fn finish_if_owned(mut self, outcome: crate::runs::RunOutcome) -> Result<crate::runs::Run, PlacementError> {
    if !self.owned {
      return Ok(self.resource);
    }
    let run_id = self
      .run
      .r#ref
      .as_ref()
      .map(|run| run.run_id.clone())
      .filter(|run_id| !run_id.is_empty())
      .ok_or_else(|| PlacementError::Selection("Run omitted its canonical ref".to_string()))?;
    self.run = self
      .client
      .grpc
      .runs()
      .stop_run(run_id, outcome.into())
      .await
      .map_err(|status| crate::error::ClientError::from_status("StopRun", status))?;
    self.resource = crate::runs::Run::try_from(self.run.clone()).map_err(|error| PlacementError::Selection(error.to_string()))?;
    self.owned = false;
    Ok(self.resource)
  }
}

fn validate_context_selectors(context: &AuvContext) -> Result<(), ContextError> {
  if let Some(device_id) = context.device_id.as_deref() {
    DeviceSelector::parse_id(device_id)?;
  }
  if let Some(run_id) = context.run_id.as_deref() {
    RunSelector::parse(run_id)?;
  }
  Ok(())
}

fn context_device_selector(context: &AuvContext) -> Option<DeviceSelector> {
  match (&context.device_id, &context.device_name) {
    // Root-injected IDs are canonical. The name is only a display snapshot and
    // must not turn a later rename into a selector conflict.
    (Some(id), _) => Some(DeviceSelector::by_id(id.clone())),
    (None, Some(name)) => Some(DeviceSelector::by_name(name.clone())),
    (None, None) => None,
  }
}

fn matching_devices<'a>(context: &AuvContext, devices: &'a [proto::Device]) -> Vec<&'a proto::Device> {
  let selector = match (&context.device_id, &context.device_name) {
    (Some(id), Some(name)) => Some(DeviceSelector::by_id_and_name(id, name.clone())),
    (Some(id), None) => Some(DeviceSelector::by_id(id)),
    (None, Some(name)) => Some(DeviceSelector::by_name(name.clone())),
    (None, None) => None,
  };
  devices
    .iter()
    .filter(|device| {
      selector
        .as_ref()
        .is_none_or(|selector| device.r#ref.as_ref().is_some_and(|reference| selector.matches_wire(&reference.device_id, &device.name)))
    })
    .collect()
}

fn matching_run_ids(selector: &str, runs: &[proto::Run]) -> Vec<String> {
  let Ok(selector) = RunSelector::parse(selector) else {
    return Vec::new();
  };
  runs
    .iter()
    .filter_map(|run| run.r#ref.as_ref().map(|reference| reference.run_id.as_str()))
    .filter(|run_id| selector.matches_wire(run_id))
    .map(str::to_string)
    .collect()
}

fn context_matches_canonical_device(context: &AuvContext, device: &proto::Device) -> Result<(), ContextError> {
  if let Some(id) = context.device_id.as_deref()
    && device.r#ref.as_ref().is_none_or(|reference| !DeviceSelector::by_id(id).matches_wire(&reference.device_id, &device.name))
  {
    return Err(ContextError::CanonicalDeviceMissing(id.to_string()));
  }
  Ok(())
}

fn select_default_device(
  devices: &[proto::Device],
  constraint: PlacementConstraint,
  allowed: Option<&[proto::DeviceRef]>,
) -> Result<Option<proto::Device>, PlacementError> {
  let allowed_devices = devices.iter().filter(|device| is_allowed(device, allowed)).collect::<Vec<_>>();
  if allowed.is_some() && constraint == PlacementConstraint::Automatic {
    return match allowed_devices.as_slice() {
      [device] => Ok(Some((*device).clone())),
      _ => Ok(None),
    };
  }
  let local = allowed_devices.into_iter().filter(|device| device.local).collect::<Vec<_>>();
  match local.as_slice() {
    [device] => Ok(Some((*device).clone())),
    [] => Err(PlacementError::Selection("the selected daemon exposes no eligible implicit local Device".to_string())),
    _ => Err(PlacementError::Selection("the selected daemon exposes more than one eligible implicit local Device".to_string())),
  }
}

fn select_device(
  devices: &[proto::Device],
  selector: &DeviceSelector,
  constraint: PlacementConstraint,
  allowed: Option<&[proto::DeviceRef]>,
) -> Result<proto::Device, PlacementError> {
  let candidates = devices.iter().filter(|device| is_allowed(device, allowed)).collect::<Vec<_>>();
  let by_id = match selector.id() {
    Some(id) => {
      let matches = candidates
        .iter()
        .copied()
        .filter(|device| {
          device.r#ref.as_ref().is_some_and(|reference| DeviceSelector::by_id(id).matches_wire(&reference.device_id, &device.name))
        })
        .collect::<Vec<_>>();
      match matches.as_slice() {
        [] => return Err(PlacementError::Selection(format!("unknown or Run-ineligible Device ID {id:?}"))),
        [device] => Some(*device),
        _ => return Err(PlacementError::Selection(format!("Device ID prefix {id:?} is ambiguous"))),
      }
    }
    None => None,
  };
  let by_name = match selector.name() {
    Some(name) => {
      let matches = candidates.iter().copied().filter(|device| device.name == name).collect::<Vec<_>>();
      match matches.as_slice() {
        [] => return Err(PlacementError::Selection(format!("unknown or Run-ineligible Device name {name:?}"))),
        [device] => Some(*device),
        matches => {
          let ids = matches.iter().map(|device| device_id(device)).collect::<Vec<_>>().join(", ");
          return Err(PlacementError::Selection(format!("Device name {name:?} is ambiguous; candidate IDs: {ids}")));
        }
      }
    }
    None => None,
  };
  let selected = match (by_id, by_name) {
    (Some(by_id), Some(by_name)) if !std::ptr::eq(by_id, by_name) => {
      return Err(PlacementError::Selection(format!(
        "Device name and ID select different Devices ({:?} and {:?})",
        device_id(by_name),
        device_id(by_id)
      )));
    }
    (Some(device), _) | (_, Some(device)) => device,
    (None, None) => {
      return select_default_device(devices, constraint, allowed)?
        .ok_or_else(|| PlacementError::Selection("Device selection is ambiguous".to_string()));
    }
  };
  if constraint == PlacementConstraint::LocalOnly && !selected.local {
    return Err(PlacementError::Selection(format!("local placement conflicts with remote Device {:?}", device_id(selected))));
  }
  Ok(selected.clone())
}

fn resolve_run_id<'a>(selector: &RunSelector, candidates: impl Iterator<Item = &'a str>) -> Result<String, PlacementError> {
  let matches = candidates.filter(|candidate| selector.matches_wire(candidate)).collect::<Vec<_>>();
  match matches.as_slice() {
    [] => Err(PlacementError::Selection(format!("unknown Run ID {:?}", selector.as_str()))),
    [candidate] => Ok((*candidate).to_string()),
    _ => Err(PlacementError::Selection(format!("Run ID prefix {:?} is ambiguous", selector.as_str()))),
  }
}

fn is_allowed(device: &proto::Device, allowed: Option<&[proto::DeviceRef]>) -> bool {
  allowed.is_none_or(|allowed| {
    device.r#ref.as_ref().is_some_and(|reference| allowed.iter().any(|candidate| candidate.device_id == reference.device_id))
  })
}

fn required_device_ref(device: &proto::Device) -> Result<proto::DeviceRef, PlacementError> {
  device
    .r#ref
    .clone()
    .filter(|reference| !reference.device_id.trim().is_empty())
    .ok_or_else(|| PlacementError::Selection("selected Device omitted its canonical ref".to_string()))
}

fn device_id(device: &proto::Device) -> &str {
  device.r#ref.as_ref().map(|reference| reference.device_id.as_str()).unwrap_or("<missing>")
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
