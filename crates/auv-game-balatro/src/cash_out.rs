use auv_driver::{InputActionResult, WindowPoint};
use serde::{Deserialize, Serialize};

use crate::model::{BalatroPhase, BalatroState, ButtonTarget};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CashOutConfirmationRequest {
  None,
  Targeted,
  Weak,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CashOutRequest {
  pub target: String,
  pub confirmation: CashOutConfirmationRequest,
  pub timeout_ms: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CashOutConfirmationBasis {
  StoreObserved,
  CashOutButtonDisappeared,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CashOutConfirmationFailure {
  NoStoreTransition,
  NoObservableChange,
  ObservationFailed { message: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CashOutConfirmation {
  NotRequested,
  Confirmed {
    basis: CashOutConfirmationBasis,
    before_phase: BalatroPhase,
    after_phase: BalatroPhase,
  },
  NotConfirmed {
    before_phase: BalatroPhase,
    after_phase: Option<BalatroPhase>,
    reason: CashOutConfirmationFailure,
  },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct CashOutResult {
  pub target: String,
  pub selected_button: ButtonTarget,
  pub window_point: WindowPoint,
  pub delivery: InputActionResult,
  pub confirmation: CashOutConfirmation,
}

#[cfg(feature = "tracing")]
#[derive(Serialize)]
struct CashOutCompleted<'a> {
  target: &'a str,
  selected_button: &'a ButtonTarget,
  window_point: WindowPoint,
  confirmation: &'a CashOutConfirmation,
}

#[cfg(feature = "tracing")]
impl auv_tracing::EventPayload for CashOutCompleted<'_> {
  const NAME: &'static str = "auv.balatro.cash_out.completed";
  const VERSION: u32 = 1;
}

#[cfg(feature = "tracing")]
pub(crate) fn emit_cash_out_completed(result: &CashOutResult) {
  auv_tracing::emit_event!(CashOutCompleted {
    target: &result.target,
    selected_button: &result.selected_button,
    window_point: result.window_point,
    confirmation: &result.confirmation,
  });
}

#[cfg(not(feature = "tracing"))]
pub(crate) fn emit_cash_out_completed(_result: &CashOutResult) {}

pub(crate) fn evaluate_cash_out_confirmation(
  request: CashOutConfirmationRequest,
  before: &BalatroState,
  after: Result<&BalatroState, String>,
) -> CashOutConfirmation {
  if request == CashOutConfirmationRequest::None {
    return CashOutConfirmation::NotRequested;
  }

  let after = match after {
    Ok(after) => after,
    Err(message) => {
      return CashOutConfirmation::NotConfirmed {
        before_phase: before.phase,
        after_phase: None,
        reason: CashOutConfirmationFailure::ObservationFailed { message },
      };
    }
  };

  if after.phase == BalatroPhase::Store || after.store.is_store {
    return CashOutConfirmation::Confirmed {
      basis: CashOutConfirmationBasis::StoreObserved,
      before_phase: before.phase,
      after_phase: after.phase,
    };
  }

  let button_disappeared =
    before.buttons.iter().any(|button| button.id == "button_cash_out") && after.buttons.iter().all(|button| button.id != "button_cash_out");
  if request == CashOutConfirmationRequest::Weak && button_disappeared {
    return CashOutConfirmation::Confirmed {
      basis: CashOutConfirmationBasis::CashOutButtonDisappeared,
      before_phase: before.phase,
      after_phase: after.phase,
    };
  }

  CashOutConfirmation::NotConfirmed {
    before_phase: before.phase,
    after_phase: Some(after.phase),
    reason: match request {
      CashOutConfirmationRequest::Targeted => CashOutConfirmationFailure::NoStoreTransition,
      CashOutConfirmationRequest::Weak => CashOutConfirmationFailure::NoObservableChange,
      CashOutConfirmationRequest::None => unreachable!("none returns before observing"),
    },
  }
}
