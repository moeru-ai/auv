//! System Media Transport Controls (SMTC) and CoreAudio session media controller.
//!
//! Exposes background media playback control (Play, Pause, Next, Previous, Toggle),
//! structured now-playing metadata (Title, Artist, Album, Status), and process-targeted
//! audio session volume control for Windows applications (including QQ Music).
//!
//! All operations operate entirely in the background via WinRT SMTC and Windows
//! CoreAudio COM interfaces without stealing foreground window focus or injecting
//! simulated input.

use serde::{Deserialize, Serialize};

/// Current playback state of an SMTC session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaPlaybackStatus {
  Closed,
  Opened,
  Changing,
  Stopped,
  Playing,
  Paused,
  Unknown(i32),
}

/// Metadata for the currently loaded or playing track.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct MediaTrackMetadata {
  pub title: String,
  pub artist: String,
  pub album_title: String,
  pub album_artist: String,
  pub genres: Vec<String>,
}

/// Structured summary of a media session and its current playback state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NowPlayingState {
  pub app_id: String,
  pub status: MediaPlaybackStatus,
  pub track: Option<MediaTrackMetadata>,
}

#[cfg(target_os = "windows")]
mod native {
  use windows::Media::Control::{GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager};
  use windows::Win32::Media::Audio::{
    Endpoints::IAudioEndpointVolume, IAudioSessionControl2, IAudioSessionEnumerator, IAudioSessionManager2, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator, eMultimedia, eRender,
  };
  use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
  use windows::core::Interface;

  use super::{MediaPlaybackStatus, MediaTrackMetadata, NowPlayingState};
  use crate::error::backend;
  use auv_driver_common::error::DriverResult;

  /// A wrapper around WinRT [`GlobalSystemMediaTransportControlsSession`].
  #[derive(Clone)]
  pub struct SmtcSession {
    inner: GlobalSystemMediaTransportControlsSession,
    app_id: String,
  }

  impl SmtcSession {
    pub fn new(session: GlobalSystemMediaTransportControlsSession) -> Self {
      let app_id = session.SourceAppUserModelId().map(|h| h.to_string()).unwrap_or_default();
      Self {
        inner: session,
        app_id,
      }
    }

    pub fn app_id(&self) -> &str {
      &self.app_id
    }

    pub fn playback_status(&self) -> DriverResult<MediaPlaybackStatus> {
      let info = self.inner.GetPlaybackInfo().map_err(|e| backend(format!("GetPlaybackInfo failed for {}: {e}", self.app_id)))?;
      let raw_status = info.PlaybackStatus().map_err(|e| backend(format!("PlaybackStatus failed for {}: {e}", self.app_id)))?;
      Ok(match raw_status.0 {
        0 => MediaPlaybackStatus::Closed,
        1 => MediaPlaybackStatus::Opened,
        2 => MediaPlaybackStatus::Changing,
        3 => MediaPlaybackStatus::Stopped,
        4 => MediaPlaybackStatus::Playing,
        5 => MediaPlaybackStatus::Paused,
        other => MediaPlaybackStatus::Unknown(other),
      })
    }

    pub fn track_metadata(&self) -> DriverResult<MediaTrackMetadata> {
      let op = self
        .inner
        .TryGetMediaPropertiesAsync()
        .map_err(|e| backend(format!("TryGetMediaPropertiesAsync failed for {}: {e}", self.app_id)))?;
      let props = op.get().map_err(|e| backend(format!("Failed to retrieve media properties for {}: {e}", self.app_id)))?;

      let title = props.Title().map(|h| h.to_string()).unwrap_or_default();
      let artist = props.Artist().map(|h| h.to_string()).unwrap_or_default();
      let album_title = props.AlbumTitle().map(|h| h.to_string()).unwrap_or_default();
      let album_artist = props.AlbumArtist().map(|h| h.to_string()).unwrap_or_default();
      let genres = if let Ok(genres_vec) = props.Genres() {
        let count = genres_vec.Size().unwrap_or(0);
        let mut list = Vec::with_capacity(count as usize);
        for i in 0..count {
          if let Ok(g) = genres_vec.GetAt(i) {
            list.push(g.to_string());
          }
        }
        list
      } else {
        Vec::new()
      };

      Ok(MediaTrackMetadata {
        title,
        artist,
        album_title,
        album_artist,
        genres,
      })
    }

    pub fn play(&self) -> DriverResult<bool> {
      let op = self.inner.TryPlayAsync().map_err(|e| backend(format!("TryPlayAsync call failed for {}: {e}", self.app_id)))?;
      op.get().map_err(|e| backend(format!("TryPlayAsync execution failed for {}: {e}", self.app_id)))
    }

    pub fn pause(&self) -> DriverResult<bool> {
      let op = self.inner.TryPauseAsync().map_err(|e| backend(format!("TryPauseAsync call failed for {}: {e}", self.app_id)))?;
      op.get().map_err(|e| backend(format!("TryPauseAsync execution failed for {}: {e}", self.app_id)))
    }

    pub fn toggle_play_pause(&self) -> DriverResult<bool> {
      let op = self
        .inner
        .TryTogglePlayPauseAsync()
        .map_err(|e| backend(format!("TryTogglePlayPauseAsync call failed for {}: {e}", self.app_id)))?;
      op.get().map_err(|e| backend(format!("TryTogglePlayPauseAsync execution failed for {}: {e}", self.app_id)))
    }

    pub fn skip_next(&self) -> DriverResult<bool> {
      let op = self.inner.TrySkipNextAsync().map_err(|e| backend(format!("TrySkipNextAsync call failed for {}: {e}", self.app_id)))?;
      op.get().map_err(|e| backend(format!("TrySkipNextAsync execution failed for {}: {e}", self.app_id)))
    }

    pub fn skip_previous(&self) -> DriverResult<bool> {
      let op =
        self.inner.TrySkipPreviousAsync().map_err(|e| backend(format!("TrySkipPreviousAsync call failed for {}: {e}", self.app_id)))?;
      op.get().map_err(|e| backend(format!("TrySkipPreviousAsync execution failed for {}: {e}", self.app_id)))
    }

    pub fn stop(&self) -> DriverResult<bool> {
      let op = self.inner.TryStopAsync().map_err(|e| backend(format!("TryStopAsync call failed for {}: {e}", self.app_id)))?;
      op.get().map_err(|e| backend(format!("TryStopAsync execution failed for {}: {e}", self.app_id)))
    }

    pub fn snapshot(&self) -> DriverResult<NowPlayingState> {
      let status = self.playback_status().unwrap_or(MediaPlaybackStatus::Closed);
      let track = self.track_metadata().ok();
      Ok(NowPlayingState {
        app_id: self.app_id.clone(),
        status,
        track,
      })
    }
  }

  /// Manages discovery and selection of SMTC media sessions.
  pub struct SmtcMediaManager {
    manager: GlobalSystemMediaTransportControlsSessionManager,
  }

  impl SmtcMediaManager {
    pub fn new() -> DriverResult<Self> {
      let op = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
        .map_err(|e| backend(format!("Failed to request SMTC session manager: {e}")))?;
      let manager = op.get().map_err(|e| backend(format!("Failed to retrieve SMTC session manager: {e}")))?;
      Ok(Self { manager })
    }

    pub fn current_session(&self) -> DriverResult<Option<SmtcSession>> {
      match self.manager.GetCurrentSession() {
        Ok(session) => Ok(Some(SmtcSession::new(session))),
        Err(_) => Ok(None),
      }
    }

    pub fn list_sessions(&self) -> DriverResult<Vec<SmtcSession>> {
      let sessions = self.manager.GetSessions().map_err(|e| backend(format!("Failed to get SMTC sessions: {e}")))?;
      let count = sessions.Size().unwrap_or(0);
      let mut result = Vec::with_capacity(count as usize);
      for i in 0..count {
        if let Ok(s) = sessions.GetAt(i) {
          result.push(SmtcSession::new(s));
        }
      }
      Ok(result)
    }

    pub fn find_session(&self, query: &str) -> DriverResult<Option<SmtcSession>> {
      let lower = query.to_lowercase();
      let sessions = self.list_sessions()?;
      for s in sessions {
        if s.app_id().to_lowercase().contains(&lower) {
          return Ok(Some(s));
        }
      }
      Ok(None)
    }

    pub fn now_playing(&self) -> DriverResult<Option<NowPlayingState>> {
      if let Some(current) = self.current_session()? {
        return Ok(Some(current.snapshot()?));
      }
      let all = self.list_sessions()?;
      if let Some(first) = all.first() {
        return Ok(Some(first.snapshot()?));
      }
      Ok(None)
    }
  }

  /// CoreAudio volume control for master and per-process audio sessions.
  pub struct AudioVolumeController;

  impl AudioVolumeController {
    /// Returns master system volume as a float in `[0.0, 1.0]`.
    pub fn get_master_volume() -> DriverResult<f32> {
      unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
          .map_err(|e| backend(format!("Failed to instantiate MMDeviceEnumerator: {e}")))?;
        let device =
          enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).map_err(|e| backend(format!("GetDefaultAudioEndpoint failed: {e}")))?;
        let endpoint_vol: IAudioEndpointVolume =
          device.Activate(CLSCTX_ALL, None).map_err(|e| backend(format!("Activate IAudioEndpointVolume failed: {e}")))?;
        endpoint_vol.GetMasterVolumeLevelScalar().map_err(|e| backend(format!("GetMasterVolumeLevelScalar failed: {e}")))
      }
    }

    /// Sets master system volume to a float in `[0.0, 1.0]`.
    pub fn set_master_volume(level: f32) -> DriverResult<()> {
      let clamped = level.clamp(0.0, 1.0);
      unsafe {
        let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
          .map_err(|e| backend(format!("Failed to instantiate MMDeviceEnumerator: {e}")))?;
        let device =
          enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).map_err(|e| backend(format!("GetDefaultAudioEndpoint failed: {e}")))?;
        let endpoint_vol: IAudioEndpointVolume =
          device.Activate(CLSCTX_ALL, None).map_err(|e| backend(format!("Activate IAudioEndpointVolume failed: {e}")))?;
        endpoint_vol
          .SetMasterVolumeLevelScalar(clamped, std::ptr::null())
          .map_err(|e| backend(format!("SetMasterVolumeLevelScalar failed: {e}")))
      }
    }

    /// Returns process volume for `target_pid` in `[0.0, 1.0]`.
    pub fn get_process_volume(target_pid: u32) -> DriverResult<f32> {
      let vol = find_process_simple_volume(target_pid)?;
      unsafe { vol.GetMasterVolume().map_err(|e| backend(format!("GetMasterVolume failed for PID {target_pid}: {e}"))) }
    }

    /// Sets process volume for `target_pid` to a float in `[0.0, 1.0]`.
    pub fn set_process_volume(target_pid: u32, level: f32) -> DriverResult<()> {
      let clamped = level.clamp(0.0, 1.0);
      let vol = find_process_simple_volume(target_pid)?;
      unsafe {
        vol.SetMasterVolume(clamped, std::ptr::null()).map_err(|e| backend(format!("SetMasterVolume failed for PID {target_pid}: {e}")))
      }
    }

    /// Returns process mute status for `target_pid`.
    pub fn get_process_mute(target_pid: u32) -> DriverResult<bool> {
      let vol = find_process_simple_volume(target_pid)?;
      unsafe { vol.GetMute().map(|b| b.as_bool()).map_err(|e| backend(format!("GetMute failed for PID {target_pid}: {e}"))) }
    }

    /// Sets process mute status for `target_pid`.
    pub fn set_process_mute(target_pid: u32, muted: bool) -> DriverResult<()> {
      let vol = find_process_simple_volume(target_pid)?;
      unsafe {
        vol
          .SetMute(windows::Win32::Foundation::BOOL(if muted { 1 } else { 0 }), std::ptr::null())
          .map_err(|e| backend(format!("SetMute failed for PID {target_pid}: {e}")))
      }
    }
  }

  fn find_process_simple_volume(target_pid: u32) -> DriverResult<ISimpleAudioVolume> {
    unsafe {
      let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        .map_err(|e| backend(format!("Failed to instantiate MMDeviceEnumerator: {e}")))?;
      let device =
        enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia).map_err(|e| backend(format!("GetDefaultAudioEndpoint failed: {e}")))?;
      let mgr: IAudioSessionManager2 =
        device.Activate(CLSCTX_ALL, None).map_err(|e| backend(format!("Activate IAudioSessionManager2 failed: {e}")))?;
      let session_enum: IAudioSessionEnumerator =
        mgr.GetSessionEnumerator().map_err(|e| backend(format!("GetSessionEnumerator failed: {e}")))?;
      let count = session_enum.GetCount().map_err(|e| backend(format!("GetCount failed: {e}")))?;

      for i in 0..count {
        if let Ok(session_ctrl) = session_enum.GetSession(i)
          && let Ok(ctrl2) = session_ctrl.cast::<IAudioSessionControl2>()
          && ctrl2.GetProcessId().ok() == Some(target_pid)
          && let Ok(vol) = session_ctrl.cast::<ISimpleAudioVolume>()
        {
          return Ok(vol);
        }
      }
      Err(backend(format!("No active audio session found for PID {target_pid}")))
    }
  }
}

#[cfg(target_os = "windows")]
pub use native::{AudioVolumeController, SmtcMediaManager, SmtcSession};

#[cfg(not(target_os = "windows"))]
use auv_driver_common::error::{DriverError, DriverResult};

#[cfg(not(target_os = "windows"))]
pub struct SmtcMediaManager;

#[cfg(not(target_os = "windows"))]
impl SmtcMediaManager {
  pub fn new() -> DriverResult<Self> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }
}

#[cfg(not(target_os = "windows"))]
pub struct AudioVolumeController;

#[cfg(test)]
#[path = "media_test.rs"]
mod tests;
