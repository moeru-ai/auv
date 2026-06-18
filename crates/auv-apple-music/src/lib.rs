//! Apple Music Windows app integration: window resolution and launch.

pub mod app;
pub mod cli;
pub mod commands;

pub use app::{AppleMusicWindow, resolve_window};
pub use commands::launch::{LaunchResult, LaunchStep, run_open_window};
pub use commands::playback::{
  MetadataSource, PlaybackState, PlaybackStatus, PlaybackStatusInputs, run_playback_status,
};
pub use commands::transport::{
  TransportAction, TransportInputs, TransportResult, run_transport_action,
};
