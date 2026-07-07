//! Apple Music Windows app integration: window resolution and launch.

pub mod app;
pub mod cli;
pub mod commands;

pub use app::{AppleMusicWindow, resolve_window};
pub use commands::launch::{LaunchResult, LaunchStep, run_open_window};
pub use commands::playback::{MetadataSource, PlaybackState, PlaybackStatus, PlaybackStatusInputs, run_playback_status};
pub use commands::search::{
  DEFAULT_RESULT_SELECTION_TIMEOUT_MS, DEFAULT_SEARCH_SETTLE_MS, DEFAULT_SEARCH_VERIFICATION_TIMEOUT_MS, SearchInputs, SearchResult,
  SearchResultMatch, SearchResultSelectInputs, SearchResultSelection, SearchVerification, SearchVerificationStatus, run_search,
  run_search_result_select,
};
pub use commands::transport::{TransportAction, TransportInputs, TransportResult, run_transport_action};
