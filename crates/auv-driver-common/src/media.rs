//! The system now-playing contract shared by every platform's media provider.
//!
//! macOS reads the system Now Playing slot (`auv-media-macos`); Windows reads the current System
//! Media Transport Controls session (`auv-driver-windows::media`). Both report the same
//! [`NowPlayingState`], take the same [`MediaCommand`], and emit the same agent-facing
//! `now-playing-v0` output, so `mediaControl.*` has one result shape on every platform.

/// A structured snapshot of the system now-playing state.
///
/// [`Default`] is the idle state (nothing owns the slot) — useful for callers
/// that scope/filter the read to a specific app.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NowPlayingState {
  /// Whether an app currently owns the now-playing slot with valid content.
  pub present: bool,
  /// Identifier of the app that owns the now-playing slot: the bundle identifier on macOS, and
  /// the SMTC `SourceAppUserModelId` on Windows (for a desktop app often its executable name,
  /// such as `QQMusic.exe`).
  // TODO(now-playing-v1): the name keeps its `now-playing-v0` wire spelling although Windows
  // fills it with an AppUserModelID. Rename it to a platform-neutral `source_app_id` only with a
  // new schema version, when an owner approves breaking v0 readers.
  pub source_bundle_id: Option<String>,
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub duration_seconds: Option<f64>,
  pub elapsed_seconds: Option<f64>,
  pub playback_rate: Option<f64>,
  /// Whether playback is currently active.
  pub is_playing: bool,
  pub content_item_id: Option<String>,
  /// Whether the now-playing app exposes a like/favorite affordance for this
  /// track (`None` when unreported).
  ///
  /// LIMITATION: in practice only Apple Music **catalog/streaming** tracks
  /// populate this. It is `None` for NetEase, for local files (verified even in
  /// Music.app, even after pressing Favorite), and for apps that don't integrate
  /// MediaRemote's like affordance. There is no general, free way to set a
  /// "like" via MediaRemote (the vendored adapter doesn't expose `kMRLikeTrack`,
  /// and it would need track/station identifiers). Verified empirically on
  /// macOS 26.2 — see the design spec's "like/favorite" finding. Windows SMTC has
  /// no like state, so it is always `None` there.
  pub supports_like: Option<bool>,
  /// Whether this track is currently liked/favorited (`None` when unreported).
  /// Same limitation as [`Self::supports_like`] — effectively Apple Music
  /// catalog only; never set for NetEase, local tracks, or on Windows.
  pub is_liked: Option<bool>,
}

/// A transport command sent to whichever app owns the system now-playing slot.
///
/// Like the read, this is system-wide and app-agnostic — it acts on the
/// current now-playing app, not a specific one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaCommand {
  Play,
  Pause,
  TogglePlayPause,
  NextTrack,
  PreviousTrack,
}

impl MediaCommand {
  /// A short stable label for this command (`"play"`, `"pause"`, `"toggle"`,
  /// `"next"`, `"previous"`).
  pub fn label(self) -> &'static str {
    match self {
      MediaCommand::Play => "play",
      MediaCommand::Pause => "pause",
      MediaCommand::TogglePlayPause => "toggle",
      MediaCommand::NextTrack => "next",
      MediaCommand::PreviousTrack => "previous",
    }
  }
}

/// Stable schema identifier for the JSON output.
pub const SCHEMA_VERSION: &str = "now-playing-v0";

/// The stable JSON output object.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct NowPlayingOutput {
  pub schema_version: &'static str,
  pub present: bool,
  pub is_playing: bool,
  pub source_bundle_id: Option<String>,
  pub title: Option<String>,
  pub artist: Option<String>,
  pub album: Option<String>,
  pub duration_seconds: Option<f64>,
  pub elapsed_seconds: Option<f64>,
  pub playback_rate: Option<f64>,
  pub content_item_id: Option<String>,
  // Usually `null`: in practice only Apple Music catalog tracks report like
  // state — never NetEase or local files. See `NowPlayingState::supports_like`.
  pub supports_like: Option<bool>,
  pub is_liked: Option<bool>,
}

/// Observable result of one media transport command.
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct MediaControlOutcome {
  pub command: &'static str,
  pub before: NowPlayingOutput,
  pub after: NowPlayingOutput,
  pub verified: bool,
}

impl MediaControlOutcome {
  /// Builds the outcome of `command` from the state read before it was sent and after, and
  /// judges whether the after-state shows the command's effect.
  pub fn new(command: MediaCommand, before: &NowPlayingState, after: &NowPlayingState) -> Self {
    Self {
      command: command.label(),
      before: build_now_playing_output(before),
      after: build_now_playing_output(after),
      verified: command_verified(command, before, after),
    }
  }
}

fn command_verified(command: MediaCommand, before: &NowPlayingState, after: &NowPlayingState) -> bool {
  match command {
    MediaCommand::Play => after.present && after.is_playing,
    MediaCommand::Pause => after.present && !after.is_playing,
    MediaCommand::TogglePlayPause => before.present && after.present && before.is_playing != after.is_playing,
    MediaCommand::NextTrack | MediaCommand::PreviousTrack => {
      before.present
        && after.present
        && (before.content_item_id != after.content_item_id || before.title != after.title || before.artist != after.artist)
    }
  }
}

/// Build the versioned output object from a [`NowPlayingState`].
pub fn build_now_playing_output(state: &NowPlayingState) -> NowPlayingOutput {
  NowPlayingOutput {
    schema_version: SCHEMA_VERSION,
    present: state.present,
    is_playing: state.is_playing,
    source_bundle_id: state.source_bundle_id.clone(),
    title: state.title.clone(),
    artist: state.artist.clone(),
    album: state.album.clone(),
    duration_seconds: state.duration_seconds,
    elapsed_seconds: state.elapsed_seconds,
    playback_rate: state.playback_rate,
    content_item_id: state.content_item_id.clone(),
    supports_like: state.supports_like,
    is_liked: state.is_liked,
  }
}

#[cfg(test)]
#[path = "media_test.rs"]
mod tests;
