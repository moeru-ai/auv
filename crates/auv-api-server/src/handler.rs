//! Transport-independent owner of the daemon control plane.

use std::path::PathBuf;

use crate::control_plane::ControlPlane;

pub struct ApiHandler {
  control_plane: ControlPlane,
}

impl ApiHandler {
  pub fn new(store_root: PathBuf) -> Result<Self, String> {
    Ok(Self {
      control_plane: ControlPlane::open(&store_root)?,
    })
  }

  pub fn new_with_runner_providers(
    store_root: PathBuf,
    first_party_runners: crate::runner_provider::FirstPartyRunnerRuntimes,
    runner_providers: Vec<crate::runner_provider::RunnerProviderConfig>,
  ) -> Result<Self, String> {
    let control_plane = ControlPlane::open_with_runner_providers(&store_root, first_party_runners, runner_providers)?;
    Ok(Self { control_plane })
  }

  pub(crate) fn control_plane(&self) -> &ControlPlane {
    &self.control_plane
  }

  pub(crate) fn has_live_resources(&self) -> bool {
    self.control_plane.has_live_runners()
  }

  pub(crate) async fn shutdown(&self) {
    self.control_plane.shutdown().await;
  }
}
