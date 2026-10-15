//! # AUV osu! Windows Benchmark & Actuator Spike (`auv-bench-osu`)
//!
//! NOTICE(auv-bench-osu-spike):
//! This crate is an isolated, standalone spike carrier for Phase 0 actuator fidelity
//! (`SendInput` + QPC hybrid wait) and S1 improv plan synthesis.
//! Following S2, if this capability graduates, it will fold back into
//! `supported/games/auv-game-osu` to avoid maintaining a duplicate beatmap codebase.

pub mod curve;
pub mod injector;
pub mod loopback;
pub mod model;
pub mod parser;
pub mod projection;
pub mod reconciliation;
pub mod simulator;
pub mod synthesizer;

pub use curve::SliderCurve;
pub use injector::{
  ActuatorBenchmarkReport, BenchmarkEdgeRecord, CheckStatus, OfflineSafetyReport, PrecisionClock, SafetyError, TimePeriodGuard,
  Win32InputActuator, default_osu_dir, run_actuator_smoke_bench, verify_lazer_ini_file, verify_lazer_offline_safety_guard_at_dir,
  verify_offline_safety_guard, verify_offline_safety_guard_at_path,
};
pub use loopback::{LoopbackBenchmarkReport, run_loopback_receiver_bench};
pub use model::{BeatmapModel, CurveType, Difficulty, EvidenceLevel, HitObject, TimingPoint};
pub use parser::{parse_beatmap_file, parse_beatmap_str};
pub use projection::{
  CalibrationPoint, CalibrationStatus, DualCandidateGateReport, EmpiricalSample, PlayfieldProjection, ProjectionBounds,
  ProjectionCalibrationReport, ProjectionDerivationMethod, ProjectionStrategy, circle_radius_playfield, evaluate_empirical_calibration,
  evaluate_layout_self_check, run_projection_dual_candidate_gate,
};
pub use reconciliation::{
  GroundTruthStats, MetadataReconciliationReport, ReconciliationResult, official_tear_rain_normal_ground_truth, reconcile_beatmap,
  reconcile_tear_rain_fixture,
};
pub use simulator::{Judgement, ObjectJudgement, SimulationReport, simulate_plan_offline};
pub use synthesizer::{
  ActionPlan, CursorTrajectoryPoint, KeyCode, KeyEventType, PlanHeader, PlanValidationError, ScheduledKeyEvent, synthesize_plan,
};
