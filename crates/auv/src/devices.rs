//! Device inventory, selector resolution, and configured-profile observation.

use std::collections::HashMap;
use std::str::FromStr;

use auv_api_proto::auv::api::daemon::v1 as proto;
use futures_util::future::join_all;

use crate::client::Client;
use crate::error::{ClientError, ClientErrorKind};
use crate::profile::{self, ConfiguredDevice, ProfileStore};
use crate::resource::{DeviceId, DeviceSelector};
use crate::{AuvContext, ContextError};

/// Operating-system family reported by a Device.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevicePlatform {
  /// The daemon did not report a platform.
  Unspecified,
  /// A Linux Device.
  Linux,
  /// A macOS Device.
  Macos,
  /// A Windows Device.
  Windows,
}

/// A Device visible through the selected daemon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
  /// Canonical Device identity.
  pub id: DeviceId,
  /// Human-facing Device name.
  pub name: String,
  /// Reported operating-system family.
  pub platform: DevicePlatform,
  /// Whether the Device is local to the selected daemon.
  pub local: bool,
  /// Operator-defined labels.
  pub labels: HashMap<String, String>,
}

/// Availability of a configured paired Device profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceAvailability {
  /// The configured Device responded successfully.
  Online,
  /// The configured Device could not be reached.
  Offline,
  /// The stored credential was rejected.
  Unauthorized,
  /// The configured profile or remote identity is inconsistent.
  Invalid,
  /// Observation failed for another reason.
  Error,
}

/// One configured profile together with its live observation, when reachable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfiguredDeviceObservation {
  /// Stored non-secret profile metadata.
  pub profile: ConfiguredDevice,
  /// Classified live availability.
  pub availability: DeviceAvailability,
  /// Live Device returned by the remote daemon.
  pub remote: Option<Device>,
}

impl ConfiguredDeviceObservation {
  /// Returns whether this profile satisfies a validated Device selector.
  pub fn matches(&self, selector: &DeviceSelector) -> bool {
    self.profile.device_id().parse::<DeviceId>().is_ok_and(|id| selector.matches(&id, self.profile.device_name()))
  }
}

impl Device {
  /// Validates that this resource is the Device independently selected by a
  /// frontend root context.
  pub fn validate_selection(&self, selected: Option<&Device>) -> Result<(), DeviceError> {
    if let Some(selected) = selected
      && selected.id != self.id
    {
      return Err(DeviceError::SelectionConflict {
        actual: self.id.to_string(),
        selected: selected.id.to_string(),
      });
    }
    Ok(())
  }
}

/// Failure from Device inventory or selector resolution.
#[derive(Debug, thiserror::Error)]
pub enum DeviceError {
  /// The daemon client request failed.
  #[error(transparent)]
  Client(#[from] ClientError),
  /// A Device identity or selector is malformed.
  #[error(transparent)]
  Identity(#[from] crate::resource::IdentityError),
  /// The daemon omitted the canonical Device identity.
  #[error("Device response omitted its canonical ID")]
  MissingIdentity,
  /// No Device satisfies the selector.
  #[error("Device selector matched no Device")]
  NotFound,
  /// More than one Device satisfies the selector.
  #[error("Device selector is ambiguous; candidate IDs: {candidate_ids}")]
  Ambiguous {
    /// Canonical IDs of the matching Devices.
    candidate_ids: String,
  },
  /// The operation target differs from the root-selected Device.
  #[error("Device {actual:?} conflicts with selected Device {selected:?}")]
  SelectionConflict {
    /// Device selected by the operation.
    actual: String,
    /// Device selected by the frontend root.
    selected: String,
  },
  /// Reading or validating a configured profile failed.
  #[error(transparent)]
  Profile(#[from] profile::ProfileError),
}

/// Device inventory operations bound to one selected daemon.
#[derive(Clone, Debug)]
pub struct Devices {
  client: Client,
}

impl Devices {
  pub(crate) fn new(client: Client) -> Self {
    Self { client }
  }

  /// Lists Devices exposed by the selected daemon.
  pub async fn list(&self) -> Result<Vec<Device>, DeviceError> {
    self
      .client
      .grpc_client()
      .devices()
      .list_devices()
      .await
      .map_err(|status| ClientError::from_status("ListDevices", status))?
      .into_iter()
      .map(Device::try_from)
      .collect()
  }

  /// Resolves exactly one Device using the shared selector policy.
  pub async fn get(&self, selector: &DeviceSelector) -> Result<Device, DeviceError> {
    let devices = self.list().await?;
    let matches = devices.iter().filter(|device| selector.matches(&device.id, &device.name)).collect::<Vec<_>>();
    match matches.as_slice() {
      [] => Err(DeviceError::NotFound),
      [device] => Ok((*device).clone()),
      _ => Err(DeviceError::Ambiguous {
        candidate_ids: matches.iter().map(|device| device.id.to_string()).collect::<Vec<_>>().join(", "),
      }),
    }
  }

  /// Observes all configured paired profiles without failing the whole list
  /// when an individual remote is offline or unauthorized.
  pub async fn observe_configured(store: &ProfileStore) -> Result<Vec<ConfiguredDeviceObservation>, DeviceError> {
    let configured = match store.list_devices() {
      Ok(configured) => configured,
      Err(profile::ProfileError::Open { source, .. }) if source.kind() == std::io::ErrorKind::NotFound => Vec::new(),
      Err(error) => return Err(error.into()),
    };
    Ok(join_all(configured.into_iter().map(|profile| observe_profile(store, profile))).await)
  }
}

async fn observe_profile(store: &ProfileStore, profile: ConfiguredDevice) -> ConfiguredDeviceObservation {
  let context = AuvContext {
    config_profile: Some(profile.config_profile().to_string()),
    ..AuvContext::default()
  };
  match Client::from_context_with_profiles(context, store).await {
    Ok(client) => match client.devices().list().await {
      Ok(devices) => {
        let remote = devices.into_iter().find(|device| device.id.as_str() == profile.device_id());
        ConfiguredDeviceObservation {
          availability: if remote.is_some() {
            DeviceAvailability::Online
          } else {
            DeviceAvailability::Invalid
          },
          profile,
          remote,
        }
      }
      Err(error) => ConfiguredDeviceObservation {
        availability: availability_from_device_error(&error),
        profile,
        remote: None,
      },
    },
    Err(error) => ConfiguredDeviceObservation {
      availability: availability_from_context_error(&error),
      profile,
      remote: None,
    },
  }
}

fn availability_from_device_error(error: &DeviceError) -> DeviceAvailability {
  match error {
    DeviceError::Client(error) if error.kind() == ClientErrorKind::Unauthorized => DeviceAvailability::Unauthorized,
    DeviceError::Client(error) if error.kind() == ClientErrorKind::Unavailable => DeviceAvailability::Offline,
    DeviceError::Identity(_) | DeviceError::MissingIdentity => DeviceAvailability::Invalid,
    _ => DeviceAvailability::Error,
  }
}

fn availability_from_context_error(error: &ContextError) -> DeviceAvailability {
  match error {
    ContextError::Connect(_) | ContextError::PairedConnect(_) => DeviceAvailability::Offline,
    ContextError::RemoteDeviceList(error) if error.kind() == ClientErrorKind::Unauthorized => DeviceAvailability::Unauthorized,
    ContextError::RemoteDeviceList(error) if error.kind() == ClientErrorKind::Unavailable => DeviceAvailability::Offline,
    ContextError::Profile(_) | ContextError::ProfileEndpointMismatch { .. } | ContextError::CanonicalDeviceMissing(_) => {
      DeviceAvailability::Invalid
    }
    _ => DeviceAvailability::Error,
  }
}

impl TryFrom<proto::Device> for Device {
  type Error = DeviceError;

  fn try_from(device: proto::Device) -> Result<Self, Self::Error> {
    let id = device.r#ref.ok_or(DeviceError::MissingIdentity)?.device_id;
    let platform = match proto::DevicePlatform::try_from(device.platform).unwrap_or(proto::DevicePlatform::Unspecified) {
      proto::DevicePlatform::Unspecified => DevicePlatform::Unspecified,
      proto::DevicePlatform::Linux => DevicePlatform::Linux,
      proto::DevicePlatform::Macos => DevicePlatform::Macos,
      proto::DevicePlatform::Windows => DevicePlatform::Windows,
    };
    Ok(Self {
      id: DeviceId::from_str(&id)?,
      name: device.name,
      platform,
      local: device.local,
      labels: device.labels,
    })
  }
}
