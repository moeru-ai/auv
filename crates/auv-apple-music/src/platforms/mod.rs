//! Platform-specific Apple Music integrations.
//!
//! Each module owns one cohesive app capability for one platform, including
//! driver calls, platform UI interpretation, and platform-only verification.
//! Platform-neutral frontends consume the typed results re-exported here.

// TODO(apple-music-view-parser-platform): Apple Music has no approved view
// parser slice yet. When one is approved, keep its platform acquisition and
// adapter in a `<capability>_<platform>.rs` module here while shared parser IR
// remains platform-neutral.

mod launch_windows;
mod playback_windows;
mod probe_macos;
mod search_windows;
mod transport_windows;
mod window_windows;

pub use launch_windows::{LaunchEvent, LaunchResult, OpenWindowInputs, run_open_window};
pub use playback_windows::{MetadataSource, PlaybackState, PlaybackStatus, PlaybackStatusInputs, run_playback_status};
pub use probe_macos::{
  DEFAULT_ACTIVATE_SETTLE_MS, DEFAULT_MUSIC_APP_BUNDLE_ID, DiscoveredNode, ProbeInputs, ProbeResult, ToolbarChildCounts, ToolbarInspection,
  run_probe,
};
pub use search_windows::{
  DEFAULT_RESULT_SELECTION_TIMEOUT_MS, DEFAULT_SEARCH_SETTLE_MS, DEFAULT_SEARCH_VERIFICATION_TIMEOUT_MS, SearchInputs, SearchResult,
  SearchResultMatch, SearchResultSelectInputs, SearchResultSelection, SearchVerification, SearchVerificationStatus, run_search,
  run_search_result_select,
};
pub use transport_windows::{TransportAction, TransportInputs, TransportResult, run_transport_action};
pub use window_windows::{AppleMusicWindow, ResolveOptions, resolve_window};
