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

/// Status of an audio session endpoint lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioLookupStatus {
  Hit,
  Miss,
  Invalidated,
}

/// Statistics from resolving an audio volume endpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioLookupStats {
  pub status: AudioLookupStatus,
  pub endpoint_id: Option<String>,
  pub endpoint_count: usize,
  pub session_count: usize,
}

#[cfg(target_os = "windows")]
mod native {
  use windows::Foundation::{EventRegistrationToken, TypedEventHandler};
  use windows::Media::Control::{GlobalSystemMediaTransportControlsSession, GlobalSystemMediaTransportControlsSessionManager};
  use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionEnumerator, IAudioSessionManager2, IMMDevice, IMMDeviceEnumerator,
    ISimpleAudioVolume, MMDeviceEnumerator, eCommunications, eMultimedia, eRender,
  };
  use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize};
  use windows::core::{Interface, PCWSTR};

  use super::{AudioLookupStats, AudioLookupStatus, MediaPlaybackStatus, MediaTrackMetadata, NowPlayingState};
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

    pub fn on_media_properties_changed<F>(&self, handler: F) -> DriverResult<EventRegistrationToken>
    where
      F: Fn() + Send + 'static,
    {
      self
        .inner
        .MediaPropertiesChanged(&TypedEventHandler::new(move |_, _| {
          handler();
          Ok(())
        }))
        .map_err(|e| backend(format!("Failed to register MediaPropertiesChanged for {}: {e}", self.app_id)))
    }

    pub fn remove_media_properties_changed(&self, token: EventRegistrationToken) -> DriverResult<()> {
      self
        .inner
        .RemoveMediaPropertiesChanged(token)
        .map_err(|e| backend(format!("Failed to unregister MediaPropertiesChanged for {}: {e}", self.app_id)))
    }

    /// Registers a best-effort callback on `PlaybackInfoChanged`.
    ///
    /// NOTE: During track skipping, playback status may remain `Playing`, so this
    /// event is an auxiliary wakeup signal, not a primary verification gate.
    pub fn on_playback_info_changed<F>(&self, handler: F) -> DriverResult<EventRegistrationToken>
    where
      F: Fn() + Send + 'static,
    {
      self
        .inner
        .PlaybackInfoChanged(&TypedEventHandler::new(move |_, _| {
          handler();
          Ok(())
        }))
        .map_err(|e| backend(format!("Failed to register PlaybackInfoChanged for {}: {e}", self.app_id)))
    }

    /// Unregisters a previously registered `PlaybackInfoChanged` token.
    pub fn remove_playback_info_changed(&self, token: EventRegistrationToken) -> DriverResult<()> {
      self
        .inner
        .RemovePlaybackInfoChanged(token)
        .map_err(|e| backend(format!("Failed to unregister PlaybackInfoChanged for {}: {e}", self.app_id)))
    }

    pub fn snapshot(&self) -> DriverResult<NowPlayingState> {
      let status = self.playback_status()?;
      let meta = self.track_metadata()?;
      let track = if meta.title.is_empty() && meta.artist.is_empty() && meta.album_title.is_empty() {
        None
      } else {
        Some(meta)
      };
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
        // WinRT returns S_OK (0x0) with a null interface pointer when there is no active
        // media session, which windows-rs converts into an Error with HRESULT(0).
        // Only HRESULT(0) represents a legitimate absence of an active session; non-zero
        // HRESULTs (e.g. RPC server unavailable, access denied) must be propagated.
        Err(e) if e.code().0 == 0 => Ok(None),
        Err(e) => Err(backend(format!("Failed to get current SMTC session: {e}"))),
      }
    }

    pub fn list_sessions(&self) -> DriverResult<Vec<SmtcSession>> {
      let sessions = self.manager.GetSessions().map_err(|e| backend(format!("Failed to get SMTC sessions: {e}")))?;
      let count = sessions.Size().map_err(|e| backend(format!("Failed to get SMTC sessions size: {e}")))?;
      let mut result = Vec::with_capacity(count as usize);
      for i in 0..count {
        let s = sessions.GetAt(i).map_err(|e| backend(format!("Failed to get SMTC session at index {i}: {e}")))?;
        result.push(SmtcSession::new(s));
      }
      Ok(result)
    }

    pub fn find_session(&self, query: &str) -> DriverResult<Option<SmtcSession>> {
      let lower = query.to_lowercase();
      if let Ok(Some(current)) = self.current_session()
        && current.app_id().to_lowercase().contains(&lower)
      {
        return Ok(Some(current));
      }
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

  /// Reusable process audio volume handle, avoiding repeated MMDevice and AudioSession enumeration.
  #[derive(Debug)]
  pub struct ProcessAudioVolume {
    pid: u32,
    endpoint_id: Option<String>,
    vol: ISimpleAudioVolume,
  }

  // SAFETY: Initialized in multithreaded COM apartment (COINIT_MULTITHREADED).
  unsafe impl Send for ProcessAudioVolume {}
  unsafe impl Sync for ProcessAudioVolume {}

  impl ProcessAudioVolume {
    pub fn endpoint_id(&self) -> Option<&str> {
      self.endpoint_id.as_deref()
    }

    pub fn is_alive(&self) -> bool {
      self.get_volume().is_ok()
    }

    pub fn get_volume(&self) -> DriverResult<f32> {
      unsafe { self.vol.GetMasterVolume().map_err(|e| backend(format!("GetMasterVolume failed for PID {}: {e}", self.pid))) }
    }

    pub fn set_volume(&self, level: f32) -> DriverResult<()> {
      let clamped = level.clamp(0.0, 1.0);
      unsafe {
        self.vol.SetMasterVolume(clamped, std::ptr::null()).map_err(|e| backend(format!("SetMasterVolume failed for PID {}: {e}", self.pid)))
      }
    }

    pub fn get_mute(&self) -> DriverResult<bool> {
      unsafe { self.vol.GetMute().map(|b| b.as_bool()).map_err(|e| backend(format!("GetMute failed for PID {}: {e}", self.pid))) }
    }

    pub fn set_mute(&self, muted: bool) -> DriverResult<()> {
      unsafe {
        self
          .vol
          .SetMute(windows::Win32::Foundation::BOOL(if muted { 1 } else { 0 }), std::ptr::null())
          .map_err(|e| backend(format!("SetMute failed for PID {}: {e}", self.pid)))
      }
    }
  }

  /// CoreAudio volume control for per-process audio sessions.
  pub struct AudioVolumeController;

  impl AudioVolumeController {
    /// Opens a reusable audio volume handle for `target_pid`.
    pub fn open_process(target_pid: u32) -> DriverResult<ProcessAudioVolume> {
      Self::open_process_cached(target_pid, None).map(|(vol, _)| vol)
    }

    /// Opens a reusable audio volume handle, prioritizing `cached_endpoint_id`.
    pub fn open_process_cached(target_pid: u32, cached_endpoint_id: Option<&str>) -> DriverResult<(ProcessAudioVolume, AudioLookupStats)> {
      let (vol, stats) = find_process_simple_volume(target_pid, cached_endpoint_id)?;
      Ok((
        ProcessAudioVolume {
          pid: target_pid,
          endpoint_id: stats.endpoint_id.clone(),
          vol,
        },
        stats,
      ))
    }

    /// Returns process volume for `target_pid` in `[0.0, 1.0]`.
    pub fn get_process_volume(target_pid: u32) -> DriverResult<f32> {
      Self::open_process(target_pid)?.get_volume()
    }

    /// Sets process volume for `target_pid` to a float in `[0.0, 1.0]`.
    pub fn set_process_volume(target_pid: u32, level: f32) -> DriverResult<()> {
      Self::open_process(target_pid)?.set_volume(level)
    }

    /// Returns process mute status for `target_pid`.
    pub fn get_process_mute(target_pid: u32) -> DriverResult<bool> {
      Self::open_process(target_pid)?.get_mute()
    }

    /// Sets process mute status for `target_pid`.
    pub fn set_process_mute(target_pid: u32, muted: bool) -> DriverResult<()> {
      Self::open_process(target_pid)?.set_mute(muted)
    }
  }

  struct ComGuard {
    uninit: bool,
  }

  impl Drop for ComGuard {
    fn drop(&mut self) {
      if self.uninit {
        unsafe { CoUninitialize() };
      }
    }
  }

  fn init_com() -> ComGuard {
    let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    ComGuard { uninit: hr.is_ok() }
  }

  unsafe fn get_device_id(device: &IMMDevice) -> Option<String> {
    unsafe {
      let pwstr = device.GetId().ok()?;
      let s = pwstr.to_string().ok();
      CoTaskMemFree(Some(pwstr.0 as _));
      s
    }
  }

  unsafe fn search_device_for_process_volume(device: &IMMDevice, target_pid: u32) -> Option<(ISimpleAudioVolume, usize)> {
    unsafe {
      let mgr: IAudioSessionManager2 = device.Activate(CLSCTX_ALL, None).ok()?;
      let session_enum: IAudioSessionEnumerator = mgr.GetSessionEnumerator().ok()?;
      let count = session_enum.GetCount().ok()?;
      for i in 0..count {
        if let Ok(session_ctrl) = session_enum.GetSession(i)
          && let Ok(ctrl2) = session_ctrl.cast::<IAudioSessionControl2>()
          && ctrl2.GetProcessId().ok() == Some(target_pid)
          && let Ok(vol) = session_ctrl.cast::<ISimpleAudioVolume>()
        {
          return Some((vol, count as usize));
        }
      }
      None
    }
  }

  fn find_process_simple_volume(target_pid: u32, cached_endpoint_id: Option<&str>) -> DriverResult<(ISimpleAudioVolume, AudioLookupStats)> {
    let _com = init_com();
    unsafe {
      let enumerator: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
        .map_err(|e| backend(format!("Failed to instantiate MMDeviceEnumerator: {e}")))?;

      let mut was_invalidated = false;

      // 0. Check cached endpoint first (prioritizing previously known endpoint)
      if let Some(cached_id) = cached_endpoint_id {
        let wide: Vec<u16> = cached_id.encode_utf16().chain(std::iter::once(0)).collect();
        if let Ok(device) = enumerator.GetDevice(PCWSTR(wide.as_ptr()))
          && let Some((vol, session_count)) = search_device_for_process_volume(&device, target_pid)
        {
          return Ok((
            vol,
            AudioLookupStats {
              status: AudioLookupStatus::Hit,
              endpoint_id: Some(cached_id.to_string()),
              endpoint_count: 1,
              session_count,
            },
          ));
        }
        was_invalidated = true;
      }

      let mut total_endpoints = 0usize;

      // 1. Check default multimedia render endpoint first (fast path for common case)
      if let Ok(default_device) = enumerator.GetDefaultAudioEndpoint(eRender, eMultimedia) {
        total_endpoints += 1;
        if let Some((vol, session_count)) = search_device_for_process_volume(&default_device, target_pid) {
          let ep_id = get_device_id(&default_device);
          return Ok((
            vol,
            AudioLookupStats {
              status: if was_invalidated {
                AudioLookupStatus::Invalidated
              } else {
                AudioLookupStatus::Miss
              },
              endpoint_id: ep_id,
              endpoint_count: total_endpoints,
              session_count,
            },
          ));
        }
      }

      // 2. Check default communications render endpoint (for voice / communication applications)
      if let Ok(comm_device) = enumerator.GetDefaultAudioEndpoint(eRender, eCommunications) {
        total_endpoints += 1;
        if let Some((vol, session_count)) = search_device_for_process_volume(&comm_device, target_pid) {
          let ep_id = get_device_id(&comm_device);
          return Ok((
            vol,
            AudioLookupStats {
              status: if was_invalidated {
                AudioLookupStatus::Invalidated
              } else {
                AudioLookupStatus::Miss
              },
              endpoint_id: ep_id,
              endpoint_count: total_endpoints,
              session_count,
            },
          ));
        }
      }

      // 3. Enumerate all active render endpoints (handles processes explicitly routed to non-default devices)
      let collection =
        enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE).map_err(|e| backend(format!("EnumAudioEndpoints failed: {e}")))?;
      let count = collection.GetCount().map_err(|e| backend(format!("EnumAudioEndpoints GetCount failed: {e}")))?;
      for i in 0..count {
        total_endpoints += 1;
        if let Ok(device) = collection.Item(i)
          && let Some((vol, session_count)) = search_device_for_process_volume(&device, target_pid)
        {
          let ep_id = get_device_id(&device);
          return Ok((
            vol,
            AudioLookupStats {
              status: if was_invalidated {
                AudioLookupStatus::Invalidated
              } else {
                AudioLookupStatus::Miss
              },
              endpoint_id: ep_id,
              endpoint_count: total_endpoints,
              session_count,
            },
          ));
        }
      }

      Err(backend(format!("No active audio session found for PID {target_pid}")))
    }
  }
}

#[cfg(target_os = "windows")]
pub use native::{AudioVolumeController, ProcessAudioVolume, SmtcMediaManager, SmtcSession};

#[cfg(not(target_os = "windows"))]
use auv_driver_common::error::{DriverError, DriverResult};

#[cfg(not(target_os = "windows"))]
#[derive(Debug)]
pub struct ProcessAudioVolume;

#[cfg(not(target_os = "windows"))]
impl ProcessAudioVolume {
  pub fn endpoint_id(&self) -> Option<&str> {
    None
  }

  pub fn is_alive(&self) -> bool {
    false
  }

  pub fn get_volume(&self) -> DriverResult<f32> {
    Err(DriverError::unsupported("Audio volume control is only supported on Windows"))
  }

  pub fn set_volume(&self, _level: f32) -> DriverResult<()> {
    Err(DriverError::unsupported("Audio volume control is only supported on Windows"))
  }

  pub fn get_mute(&self) -> DriverResult<bool> {
    Err(DriverError::unsupported("Audio volume control is only supported on Windows"))
  }

  pub fn set_mute(&self, _muted: bool) -> DriverResult<()> {
    Err(DriverError::unsupported("Audio volume control is only supported on Windows"))
  }
}

#[cfg(not(target_os = "windows"))]
pub struct SmtcMediaManager;

#[cfg(not(target_os = "windows"))]
impl SmtcMediaManager {
  pub fn new() -> DriverResult<Self> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn current_session(&self) -> DriverResult<Option<SmtcSession>> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn list_sessions(&self) -> DriverResult<Vec<SmtcSession>> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn find_session(&self, _query: &str) -> DriverResult<Option<SmtcSession>> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn now_playing(&self) -> DriverResult<Option<NowPlayingState>> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }
}

#[cfg(not(target_os = "windows"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRegistrationToken(pub i64);

#[cfg(not(target_os = "windows"))]
#[derive(Clone, Debug)]
pub struct SmtcSession;

#[cfg(not(target_os = "windows"))]
impl SmtcSession {
  pub fn app_id(&self) -> &str {
    ""
  }

  pub fn playback_status(&self) -> DriverResult<MediaPlaybackStatus> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn track_metadata(&self) -> DriverResult<MediaTrackMetadata> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn play(&self) -> DriverResult<bool> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn pause(&self) -> DriverResult<bool> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn toggle_play_pause(&self) -> DriverResult<bool> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn skip_next(&self) -> DriverResult<bool> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn skip_previous(&self) -> DriverResult<bool> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn stop(&self) -> DriverResult<bool> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn on_media_properties_changed<F>(&self, _handler: F) -> DriverResult<EventRegistrationToken>
  where
    F: Fn() + Send + 'static,
  {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn remove_media_properties_changed(&self, _token: EventRegistrationToken) -> DriverResult<()> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn on_playback_info_changed<F>(&self, _handler: F) -> DriverResult<EventRegistrationToken>
  where
    F: Fn() + Send + 'static,
  {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn remove_playback_info_changed(&self, _token: EventRegistrationToken) -> DriverResult<()> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }

  pub fn snapshot(&self) -> DriverResult<NowPlayingState> {
    Err(DriverError::unsupported("SMTC is only supported on Windows"))
  }
}

#[cfg(not(target_os = "windows"))]
pub struct AudioVolumeController;

#[cfg(not(target_os = "windows"))]
impl AudioVolumeController {
  pub fn open_process(_target_pid: u32) -> DriverResult<ProcessAudioVolume> {
    Err(DriverError::unsupported("Audio volume control is only supported on Windows"))
  }

  pub fn open_process_cached(_target_pid: u32, _cached_endpoint_id: Option<&str>) -> DriverResult<(ProcessAudioVolume, AudioLookupStats)> {
    Err(DriverError::unsupported("Audio volume control is only supported on Windows"))
  }

  pub fn get_process_volume(_target_pid: u32) -> DriverResult<f32> {
    Err(DriverError::unsupported("Audio volume control is only supported on Windows"))
  }

  pub fn set_process_volume(_target_pid: u32, _level: f32) -> DriverResult<()> {
    Err(DriverError::unsupported("Audio volume control is only supported on Windows"))
  }

  pub fn get_process_mute(_target_pid: u32) -> DriverResult<bool> {
    Err(DriverError::unsupported("Audio volume control is only supported on Windows"))
  }

  pub fn set_process_mute(_target_pid: u32, _muted: bool) -> DriverResult<()> {
    Err(DriverError::unsupported("Audio volume control is only supported on Windows"))
  }
}

#[cfg(test)]
#[path = "media_test.rs"]
mod tests;
