//! Unresolved resource selection shared by interface adapters.

/// Root resource selectors shared by command-like interface adapters.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RootSelection {
  /// Exact Device name selector.
  pub device_name: Option<String>,
  /// Device ID-prefix selector.
  pub device_id: Option<String>,
  /// Run ID-prefix selector.
  pub run_id: Option<String>,
}

/// Canonical resources resolved from a root selection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResolvedSelection {
  /// Selected Device, including one inherited from a single-Device Run.
  pub device: Option<crate::devices::Device>,
  /// Selected Run.
  pub run: Option<crate::runs::Run>,
}

/// Failure while resolving a root resource selection.
#[derive(Debug, thiserror::Error)]
pub enum SelectionError {
  /// Device selection failed.
  #[error(transparent)]
  Device(#[from] crate::devices::DeviceError),
  /// Run selection failed.
  #[error(transparent)]
  Run(#[from] crate::runs::RunError),
  /// The Device name and ID selectors resolve to different Devices.
  #[error("--device and --device-id select different Devices ({by_name:?} and {by_id:?})")]
  DeviceConflict {
    /// Canonical Device selected by name.
    by_name: String,
    /// Canonical Device selected by ID prefix.
    by_id: String,
  },
}

/// Failure while connecting and resolving a selected client.
#[derive(Debug, thiserror::Error)]
pub enum SelectedClientError {
  /// Resolving the connection context failed.
  #[error(transparent)]
  Context(#[from] crate::ContextError),
  /// Run or Device placement failed.
  #[error(transparent)]
  Placement(#[from] crate::client::PlacementError),
  /// Root resource selection failed.
  #[error(transparent)]
  Selection(#[from] SelectionError),
}

impl RootSelection {
  /// Returns whether no root resource selector is present.
  pub fn is_empty(&self) -> bool {
    self.device_name.is_none() && self.device_id.is_none() && self.run_id.is_none()
  }

  /// Builds the non-secret context used to resolve this selection.
  pub fn context(&self, endpoint: Option<&str>) -> crate::AuvContext {
    crate::AuvContext {
      device_id: self.device_id.clone(),
      device_name: self.device_name.clone(),
      run_id: self.run_id.clone(),
      daemon_endpoint: endpoint.map(str::to_string),
      ..crate::AuvContext::default()
    }
  }

  /// Builds the validated Device selector represented by the root fields.
  pub fn device_selector(&self) -> Result<Option<crate::resource::DeviceSelector>, crate::resource::IdentityError> {
    match (&self.device_id, &self.device_name) {
      (Some(id), Some(name)) => Ok(Some(crate::resource::DeviceSelector::parse_id(id)?.with_name(name.clone()))),
      (Some(id), None) => Ok(Some(crate::resource::DeviceSelector::parse_id(id)?)),
      (None, Some(name)) => Ok(Some(crate::resource::DeviceSelector::by_name(name.clone()))),
      (None, None) => Ok(None),
    }
  }
}

impl crate::Client {
  /// Connects using explicit root selection or optional local discovery, then
  /// resolves the selected resources with the same policy for every frontend.
  pub async fn selected(
    endpoint: Option<&str>,
    selection: &RootSelection,
  ) -> Result<Option<(Self, ResolvedSelection)>, SelectedClientError> {
    let client = if selection.is_empty() {
      let Some(client) = Self::discover(endpoint).await? else {
        return Ok(None);
      };
      client
    } else {
      Self::from_context(selection.context(endpoint)).await?
    };
    let resolved = client.resolve(selection).await?;
    Ok(Some((client, resolved)))
  }

  /// Resolves canonical resources against this connected client.
  pub async fn resolve(&self, selection: &RootSelection) -> Result<ResolvedSelection, SelectionError> {
    let by_id = match selection.device_id.as_deref() {
      Some(value) => {
        let selector = crate::resource::DeviceSelector::parse_id(value).map_err(crate::devices::DeviceError::from)?;
        Some(self.devices().get(&selector).await?)
      }
      None => None,
    };
    let by_name = match selection.device_name.as_deref() {
      Some(value) => Some(self.devices().get(&crate::resource::DeviceSelector::by_name(value)).await?),
      None => None,
    };
    if let (Some(by_id), Some(by_name)) = (&by_id, &by_name)
      && by_id.id != by_name.id
    {
      return Err(SelectionError::DeviceConflict {
        by_name: by_name.id.to_string(),
        by_id: by_id.id.to_string(),
      });
    }
    let run = match selection.run_id.as_deref() {
      Some(value) => {
        let selector = crate::resource::RunSelector::parse(value).map_err(crate::runs::RunError::from)?;
        Some(self.runs().get(&selector).await?)
      }
      None => None,
    };
    let mut device = by_id.or(by_name);
    if device.is_none()
      && let Some(run) = &run
      && let [device_id] = run.devices.as_slice()
    {
      device = Some(self.devices().get(&crate::resource::DeviceSelector::by_id(device_id.as_str())).await?);
    }
    if let (Some(run), Some(device)) = (&run, &device) {
      run.validate_device(Some(&device.id))?;
    }
    Ok(ResolvedSelection { device, run })
  }
}
