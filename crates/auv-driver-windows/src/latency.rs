// NOTICE(native-owner-migration): preserve this public path and one shared
// telemetry buffer during the capture-only move. Revisit ownership during
// the approved desktop/input extraction; see the capability split design note.
pub use auv_driver_capture::windows::latency::*;
