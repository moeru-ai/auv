//! Scheduler module coordinating catalog, key matching, embedding fallback, and preconditions.

pub mod catalog;
pub mod matcher;

pub use catalog::OperationCatalog;
pub use matcher::{FastLoopScheduler, SchedulingOutcome, TaskRequest};
