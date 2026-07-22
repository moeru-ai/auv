//! Product read-side helpers shared by canonical inspect projections.

mod query_wired_live_action;

pub(crate) use self::query_wired_live_action::{operation_result_verification_claims, project_verification_outcome_from_claims};
