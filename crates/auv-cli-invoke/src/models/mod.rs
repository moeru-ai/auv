use std::collections::BTreeMap;

mod invoke_report;
mod invoke_result;

pub use invoke_report::{InvokeReport, InvokeReportField, InvokeReportSection, InvokeReportTable, InvokeReportTableRow};
pub(crate) use invoke_report::{InvokeReportLabels, InvokeReportValue, InvokeSignalValue, OptionalReportText};
pub use invoke_result::{InvokeResult, RunStatus};

#[derive(Clone, Debug, Default)]
pub struct ExecutionTarget {
  pub application_id: Option<String>,
  pub target_label: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct InvokeRequest {
  pub command_id: String,
  pub target: ExecutionTarget,
  pub inputs: BTreeMap<String, String>,
  pub dry_run: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvokeOutputOptions {
  pub json: bool,
  pub detail: bool,
  pub wide: bool,
}

impl Default for InvokeOutputOptions {
  fn default() -> Self {
    Self {
      json: false,
      detail: false,
      wide: false,
    }
  }
}
