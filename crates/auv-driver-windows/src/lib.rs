//! Windows driver capabilities for AUV.
//!
//! This crate exposes Windows-native capabilities behind narrow,
//! capability-oriented modules, mirroring the macOS driver crate. The first
//! capability is system OCR backed by `Windows.Media.Ocr`.

pub mod accessibility;
mod application;
mod background_input;
pub mod capture;
pub mod clipboard;
mod descriptor;
pub mod desktop;
pub mod device_session;
pub mod device_unlock_host;
mod driver;
mod error;
pub mod input;
pub mod latency;
pub mod media;
pub mod mutation;
pub mod ocr;
#[cfg(feature = "overlay")]
mod overlay_follow;
pub mod permission;
pub mod playback_guard;
pub mod production_executor;
mod readiness;
mod session;
pub mod track_identity;
pub mod vision;
pub mod wgc;
pub mod window;

pub use accessibility::{AxNode, AxTreeSnapshot, focus_node, select_node, snapshot_window};
pub use application::ApplicationControl;
pub use auv_driver_common::vision::{OcrMatch, OcrMatches};
pub use auv_driver_common::{ProcessActivationResult, ProcessActivationVerification};
pub use clipboard::ClipboardSnapshot;
pub use descriptor::{WINDOWS_DESKTOP_CAPABILITIES, WindowsDriverDescriptor, windows_driver_descriptor};
pub use desktop::ensure_input_desktop;
pub use driver::{WindowsDriver, WindowsDriverSession};
pub use media::{
  AudioLookupStats, AudioLookupStatus, AudioVolumeController, MediaPlaybackStatus, MediaTrackMetadata, NowPlayingState, ProcessAudioVolume,
  SmtcMediaManager, SmtcSession,
};
pub use ocr::{OcrError, recognize_text_in_rgba};
#[cfg(feature = "overlay")]
pub use overlay_follow::OperationFollower;
pub use permission::{WindowsPermissionProbe, probe as probe_permissions};
pub use playback_guard::{
  DEFAULT_PLAY_POLL_TIMEOUT, DEFAULT_TARGET_VOLUME, DEFAULT_VOLUME_TOLERANCE, MockPlaybackSink, PlaybackActionSink, RealPlaybackSink,
  Step2CommandCounts, Step2Executor, Step2Options, Step2Plan, Step2Result, execute_step2_real, execute_step2_real_with_prestate,
};
pub use production_executor::{DiscoveryTimings, WindowsOperationContext, WindowsProductionExecutor};
pub use readiness::assess_readiness;
pub use session::{AccessibilityApi, ClipboardApi, DisplayApi, InputApi, PermissionApi, VisionApi, WindowApi};
pub use track_identity::{
  NormalizedTrackIdentity, TrackChangeVerdict, TrackIdentity, TrackIdentityLevel, evaluate_track_change, normalize_track_field,
};
pub use wgc::{
  FastWindowVerification, WindowHealth, capture_window_health, capture_window_health_cached, capture_window_health_strict,
  capture_window_wgc, check_window_liveness, clear_health_cache, prewarm_wgc, prewarm_wgc_window, reset_d3d_context,
};
