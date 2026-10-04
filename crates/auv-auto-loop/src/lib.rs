//! AUV Auto-Loop (Dual-Loop Auto Mode v0.1).
//!
//! Implements:
//! - Optimistic compilation + Pessimistic execution.
//! - Deterministic trajectory cleaning (state diff + backward slicing, zero LLM-as-judge).
//! - Parameter lifting via anti-unification.
//! - Three automatic compilation gates (Cleaning, Parameter, Blast-Radius).
//! - Template-based gate derivation (SMTC, CoreAudio float ±0.05, WGC SSIM/histogram, unverified-step fallback).
//! - Fast-loop scheduler (exact key first, embedding top-3 fallback, strict precondition interception).
//! - Step-by-step runtime verification with auto-isolation (consecutive failures >= 2 -> isolation & VLM escalation).
//! - Structured decision logging enforcing ZERO silent errors.

pub mod compiler;
pub mod decision_log;
pub mod models;
pub mod runtime;
pub mod scheduler;

pub use compiler::AutoCompiler;
pub use decision_log::DecisionLogger;
pub use models::{
  CompilationMetadata, DecisionAction, DecisionCategory, DecisionLog, ManualReviewItem, OperationDef, OperationStepDef, ParameterDef,
  PreconditionDef, ReasonCode, TargetMetadata, TrajectoryRecord, TrajectoryStep, VerificationGateDef,
};
pub use runtime::{ExecutionResult, RuntimeEnvironment, RuntimeExecutor};
pub use scheduler::{FastLoopScheduler, OperationCatalog, SchedulingOutcome, TaskRequest};
