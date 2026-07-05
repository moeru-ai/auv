pub mod mouse;
pub mod system_details;

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::interaction::InteractionStep;
use crate::windows::{OpenWindowReport, ResolveOptions, open_or_resolve};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenInputs {
  pub settle_ms: u64,
}

impl Default for OpenInputs {
  fn default() -> Self {
    Self { settle_ms: 8_000 }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OpenResult {
  pub command: &'static str,
  pub window: OpenWindowReport,
  pub steps: Vec<InteractionStep>,
}

pub fn run_open(inputs: &OpenInputs) -> Result<OpenResult, String> {
  let (_, window) = open_or_resolve(&ResolveOptions {
    settle: Duration::from_millis(inputs.settle_ms),
  })?;
  Ok(OpenResult {
    command: "open",
    steps: window.steps.clone(),
    window,
  })
}
