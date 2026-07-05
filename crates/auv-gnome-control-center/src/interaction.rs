use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InteractionStep {
  pub name: String,
  pub outcome: StepOutcome,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub target: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub note: Option<String>,
}

impl InteractionStep {
  pub fn new(name: impl Into<String>, outcome: StepOutcome) -> Self {
    Self {
      name: name.into(),
      outcome,
      target: None,
      note: None,
    }
  }

  pub fn target(mut self, target: impl Into<String>) -> Self {
    self.target = Some(target.into());
    self
  }

  pub fn note(mut self, note: impl Into<String>) -> Self {
    self.note = Some(note.into());
    self
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepOutcome {
  Found,
  NotFound,
  Started,
  Selected,
  Clicked,
  Copied,
  Verified,
  Restored,
  Skipped,
}
