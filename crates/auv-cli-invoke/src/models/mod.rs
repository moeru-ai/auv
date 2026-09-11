use std::collections::BTreeMap;

mod invoke_report;
mod invoke_result;

pub use invoke_report::{InvokeReport, InvokeReportField, InvokeReportSection, InvokeReportTable, InvokeReportTableRow};
pub(crate) use invoke_report::{InvokeReportValue, OptionalReportText};
pub use invoke_result::{InvokeResult, InvokeStatus};

/// Explicit resource selected by the shared invoke frontend.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutionTarget {
  Application { id: String },
  Window { id: String },
  Display { id: String },
}

impl ExecutionTarget {
  pub fn parse(value: &str) -> Result<Self, String> {
    let value = value.trim();
    if value.is_empty() {
      return Err("--target cannot be empty".to_string());
    }
    if let Some((kind, id)) = value.split_once(':') {
      let id = id.trim();
      if id.is_empty() {
        return Err(format!("--target {kind}: requires a non-empty id"));
      }
      return match kind {
        "app" => Ok(Self::Application { id: id.to_string() }),
        "window" => Ok(Self::Window { id: id.to_string() }),
        "display" => Ok(Self::Display { id: id.to_string() }),
        _ => Err(format!("--target has unknown kind {kind:?}; expected app:, window:, or display:")),
      };
    }

    // NOTICE(invoke-target-bare-app): Bare targets remain application ids so
    // existing invoke calls keep their meaning. Remove this form only through
    // an owner-approved CLI compatibility boundary.
    Ok(Self::Application {
      id: value.to_string(),
    })
  }

  pub fn application_id(&self) -> Option<&str> {
    match self {
      Self::Application { id } => Some(id),
      Self::Window { .. } | Self::Display { .. } => None,
    }
  }
}

#[derive(Clone, Debug, Default)]
pub struct InvokeRequest {
  pub command_id: String,
  pub target: Option<ExecutionTarget>,
  pub inputs: BTreeMap<String, String>,
  pub dry_run: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InvokeOutputOptions {
  pub json: bool,
  pub detail: bool,
  pub wide: bool,
}
