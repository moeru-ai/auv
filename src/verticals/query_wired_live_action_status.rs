//! Shared wiring→OperationStatus/message mapping for query-wired live action glue.
//!
//! Domain-specific builders (`build_*_operation_result`) stay in each vertical;
//! this module only owns the duplicated admission/dispatch status state machine.

use crate::contract::OperationStatus;
use auv_game_minecraft::QueryActionWiringOutcome;
use auv_game_osu::VisualTruthQueryActionWiringOutcome;

pub(crate) struct QueryWiredLiveActionStatusLabels {
  pub attempted_without_summary_or_refusal: &'static str,
  pub refused_before_dispatch_default: &'static str,
}

pub(crate) const MINECRAFT_LABELS: QueryWiredLiveActionStatusLabels = QueryWiredLiveActionStatusLabels {
  attempted_without_summary_or_refusal: "query wired live action attempted without click summary or refusal",
  refused_before_dispatch_default: "query wired live action refused before dispatch",
};

pub(crate) const OSU_LABELS: QueryWiredLiveActionStatusLabels = QueryWiredLiveActionStatusLabels {
  attempted_without_summary_or_refusal: "osu query wired live action attempted without click summary or refusal",
  refused_before_dispatch_default: "osu query wired live action refused before dispatch",
};

pub(crate) trait QueryWiredLiveActionWiringView {
  fn attempted(&self) -> bool;
  fn click_summary(&self) -> Option<&str>;
  fn refusal_reason(&self) -> Option<&str>;
}

impl QueryWiredLiveActionWiringView for QueryActionWiringOutcome {
  fn attempted(&self) -> bool {
    self.attempted
  }

  fn click_summary(&self) -> Option<&str> {
    self.click_summary.as_deref()
  }

  fn refusal_reason(&self) -> Option<&str> {
    self.refusal_reason.as_deref()
  }
}

impl QueryWiredLiveActionWiringView for VisualTruthQueryActionWiringOutcome {
  fn attempted(&self) -> bool {
    self.attempted
  }

  fn click_summary(&self) -> Option<&str> {
    self.click_summary.as_deref()
  }

  fn refusal_reason(&self) -> Option<&str> {
    self.refusal_reason.as_deref()
  }
}

pub(crate) fn operation_status_and_message(
  wiring: &impl QueryWiredLiveActionWiringView,
  labels: &QueryWiredLiveActionStatusLabels,
) -> (OperationStatus, String) {
  if wiring.attempted() {
    if let Some(summary) = wiring.click_summary() {
      return (OperationStatus::Completed, summary.to_string());
    }
    if let Some(refusal) = wiring.refusal_reason() {
      return (OperationStatus::Failed, refusal.to_string());
    }
    return (OperationStatus::Failed, labels.attempted_without_summary_or_refusal.to_string());
  }

  let message = wiring.refusal_reason().map(str::to_string).unwrap_or_else(|| labels.refused_before_dispatch_default.to_string());
  (OperationStatus::Completed, message)
}

#[cfg(test)]
mod query_wired_live_action_status_tests {
  use super::{
    MINECRAFT_LABELS, OSU_LABELS, QueryWiredLiveActionStatusLabels, QueryWiredLiveActionWiringView, operation_status_and_message,
  };
  use crate::contract::OperationStatus;

  struct TestWiringView {
    attempted: bool,
    click_summary: Option<String>,
    refusal_reason: Option<String>,
  }

  impl QueryWiredLiveActionWiringView for TestWiringView {
    fn attempted(&self) -> bool {
      self.attempted
    }

    fn click_summary(&self) -> Option<&str> {
      self.click_summary.as_deref()
    }

    fn refusal_reason(&self) -> Option<&str> {
      self.refusal_reason.as_deref()
    }
  }

  fn assert_mapping(
    wiring: TestWiringView,
    labels: &QueryWiredLiveActionStatusLabels,
    expected_status: OperationStatus,
    expected_message: &str,
  ) {
    let (status, message) = operation_status_and_message(&wiring, labels);
    assert_eq!(status, expected_status);
    assert_eq!(message, expected_message);
  }

  #[test]
  fn query_wired_live_action_status_click_succeeded() {
    for labels in [&MINECRAFT_LABELS, &OSU_LABELS] {
      assert_mapping(
        TestWiringView {
          attempted: true,
          click_summary: Some("clicked at (1,2)".to_string()),
          refusal_reason: None,
        },
        labels,
        OperationStatus::Completed,
        "clicked at (1,2)",
      );
    }
  }

  #[test]
  fn query_wired_live_action_status_dispatch_refused() {
    for labels in [&MINECRAFT_LABELS, &OSU_LABELS] {
      assert_mapping(
        TestWiringView {
          attempted: true,
          click_summary: None,
          refusal_reason: Some("window not found".to_string()),
        },
        labels,
        OperationStatus::Failed,
        "window not found",
      );
    }
  }

  #[test]
  fn query_wired_live_action_status_defensive_attempted_gap() {
    assert_mapping(
      TestWiringView {
        attempted: true,
        click_summary: None,
        refusal_reason: None,
      },
      &MINECRAFT_LABELS,
      OperationStatus::Failed,
      "query wired live action attempted without click summary or refusal",
    );
    assert_mapping(
      TestWiringView {
        attempted: true,
        click_summary: None,
        refusal_reason: None,
      },
      &OSU_LABELS,
      OperationStatus::Failed,
      "osu query wired live action attempted without click summary or refusal",
    );
  }

  #[test]
  fn query_wired_live_action_status_pre_dispatch_refusal() {
    for labels in [&MINECRAFT_LABELS, &OSU_LABELS] {
      assert_mapping(
        TestWiringView {
          attempted: false,
          click_summary: None,
          refusal_reason: Some("visibility=outside_window".to_string()),
        },
        labels,
        OperationStatus::Completed,
        "visibility=outside_window",
      );
    }
  }

  #[test]
  fn query_wired_live_action_status_pre_dispatch_default() {
    assert_mapping(
      TestWiringView {
        attempted: false,
        click_summary: None,
        refusal_reason: None,
      },
      &MINECRAFT_LABELS,
      OperationStatus::Completed,
      "query wired live action refused before dispatch",
    );
    assert_mapping(
      TestWiringView {
        attempted: false,
        click_summary: None,
        refusal_reason: None,
      },
      &OSU_LABELS,
      OperationStatus::Completed,
      "osu query wired live action refused before dispatch",
    );
  }
}
