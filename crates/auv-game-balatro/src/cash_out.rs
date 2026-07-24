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

#[cfg(all(test, feature = "tracing"))]
mod tests {
  use std::sync::Arc;

  use auv_driver::{InputDeliveryPath, WindowPoint};
  use auv_task_object_detection::BoundingBox;
  use auv_tracing::{AuthorityId, Context, MemoryRunStore, RunId, RunStore, configure, dispatcher};

  use super::*;

  #[test]
  fn completed_event_records_domain_confirmation_without_embedding_delivery() {
    futures_executor::block_on(async {
      let store = Arc::new(MemoryRunStore::new(AuthorityId::new()));
      let dispatch = configure().run_store(store.clone()).build().expect("memory dispatch");
      let run_id = RunId::new();
      let root = dispatcher::with_default(&dispatch, || Context::root(run_id));
      let result = CashOutResult {
        target: "Balatro".to_string(),
        selected_button: ButtonTarget {
          id: "button_cash_out".to_string(),
          label: "cash out".to_string(),
          bbox: BoundingBox {
            x1: 1.0,
            y1: 2.0,
            x2: 3.0,
            y2: 4.0,
          },
          confidence: 0.95,
        },
        window_point: WindowPoint::new(20.0, 30.0),
        delivery: InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse),
        confirmation: CashOutConfirmation::Confirmed {
          basis: CashOutConfirmationBasis::StoreObserved,
          before_phase: BalatroPhase::Playing,
          after_phase: BalatroPhase::Store,
        },
      };

      root.in_scope(|| emit_cash_out_completed(&result));
      dispatch.flush().await.expect("flush completed event");
      let snapshot = store.load_snapshot(run_id).await.expect("load snapshot").expect("cash-out run");
      let event = snapshot
        .events()
        .iter()
        .find(|event| event.schema().name().as_str() == "auv.balatro.cash_out.completed")
        .expect("cash-out completed event");

      assert!(event.payload().get().contains("\"confirmation\""));
      assert!(!event.payload().get().contains("\"delivery\""));
    });
  }
}
