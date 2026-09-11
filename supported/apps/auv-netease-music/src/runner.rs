//! Daemon-owned NetEase Music Runner implementation.

use std::pin::Pin;

use crate::DEFAULT_APP_ID;
use crate::api::v1 as proto;
use crate::api::v1::application_service_server::{ApplicationService, ApplicationServiceServer};
use crate::api::v1::player_service_server::{PlayerService, PlayerServiceServer};
use crate::api::v1::playlist_service_server::{PlaylistService, PlaylistServiceServer};
use crate::api::v1::recommendation_service_server::{RecommendationService, RecommendationServiceServer};
use crate::api::v1::song_service_server::{SongService, SongServiceServer};
use futures_util::Stream;
use tonic::{Request, Response, Status};

#[derive(Default)]
struct Service;

#[tonic::async_trait]
impl PlayerService for Service {
  async fn get_now_playing(&self, request: Request<proto::GetNowPlayingRequest>) -> Result<Response<proto::GetNowPlayingResponse>, Status> {
    let requested = app_id(request.into_inner().application_bundle_id);
    let state = blocking("now-playing read", auv_media_macos::now_playing).await?;
    let state = if state.source_bundle_id.as_deref() == Some(requested.as_str()) {
      state
    } else {
      auv_media_macos::NowPlayingState::default()
    };
    Ok(Response::new(proto::GetNowPlayingResponse {
      present: state.present,
      is_playing: state.is_playing,
      source_bundle_id: state.source_bundle_id,
      title: state.title,
      artist: state.artist,
      album: state.album,
      duration_seconds: state.duration_seconds,
      elapsed_seconds: state.elapsed_seconds,
      playback_rate: state.playback_rate,
      content_item_id: state.content_item_id,
      supports_like: state.supports_like,
      is_liked: state.is_liked,
    }))
  }

  async fn play(&self, request: Request<proto::PlayRequest>) -> Result<Response<proto::PlayResponse>, Status> {
    let delivery = deliver_playback_control(request.into_inner().application_bundle_id, auv_media_macos::MediaCommand::Play).await?;
    Ok(Response::new(proto::PlayResponse { delivery }))
  }

  async fn pause(&self, request: Request<proto::PauseRequest>) -> Result<Response<proto::PauseResponse>, Status> {
    let delivery = deliver_playback_control(request.into_inner().application_bundle_id, auv_media_macos::MediaCommand::Pause).await?;
    Ok(Response::new(proto::PauseResponse { delivery }))
  }

  async fn toggle_player(&self, request: Request<proto::TogglePlayerRequest>) -> Result<Response<proto::TogglePlayerResponse>, Status> {
    let delivery =
      deliver_playback_control(request.into_inner().application_bundle_id, auv_media_macos::MediaCommand::TogglePlayPause).await?;
    Ok(Response::new(proto::TogglePlayerResponse { delivery }))
  }

  async fn next(&self, request: Request<proto::NextRequest>) -> Result<Response<proto::NextResponse>, Status> {
    let delivery = deliver_playback_control(request.into_inner().application_bundle_id, auv_media_macos::MediaCommand::NextTrack).await?;
    Ok(Response::new(proto::NextResponse { delivery }))
  }

  async fn previous(&self, request: Request<proto::PreviousRequest>) -> Result<Response<proto::PreviousResponse>, Status> {
    let delivery =
      deliver_playback_control(request.into_inner().application_bundle_id, auv_media_macos::MediaCommand::PreviousTrack).await?;
    Ok(Response::new(proto::PreviousResponse { delivery }))
  }

  async fn seek(&self, request: Request<proto::SeekRequest>) -> Result<Response<proto::SeekResponse>, Status> {
    let request = request.into_inner();
    if !request.position_seconds.is_finite() || request.position_seconds < 0.0 {
      return Err(Status::invalid_argument("position_seconds must be a non-negative finite number"));
    }
    let position = std::time::Duration::try_from_secs_f64(request.position_seconds)
      .map_err(|_| Status::invalid_argument("position_seconds is outside the representable range"))?;
    let requested_app = app_id(request.application_bundle_id);
    blocking("playback seek", move || seek_playback(position, &requested_app)).await?;
    Ok(Response::new(proto::SeekResponse {
      position_seconds: request.position_seconds,
    }))
  }

  async fn get_status(&self, request: Request<proto::GetStatusRequest>) -> Result<Response<proto::GetStatusResponse>, Status> {
    let request = request.into_inner();
    let mut inputs = crate::PlaybackStatusInputs::with_defaults();
    inputs.app_id = app_id(request.application_bundle_id);
    if let Some(value) = request.settle_milliseconds {
      inputs.settle_ms = value;
    }
    let result = blocking("playback status", move || crate::run_playback_status_probe(&inputs)).await?;
    Ok(Response::new(proto::GetStatusResponse {
      playback_exists: result.playback_exists,
      was_playing: result.was_playing,
      control_state: result.control_state.map(playback_control_state).unwrap_or(proto::PlaybackControlState::Unspecified) as i32,
      detail_screen_detected: result.detail_screen_detected,
      source: result.source,
      diagnostics: diagnostics(&result.diagnostics),
      known_limits: result.known_limits,
    }))
  }
}

#[tonic::async_trait]
impl PlaylistService for Service {
  type ListPlaylistsStream = Pin<Box<dyn Stream<Item = Result<proto::ListPlaylistsStreamResponse, Status>> + Send>>;

  async fn list_playlists(&self, request: Request<proto::ListPlaylistsRequest>) -> Result<Response<Self::ListPlaylistsStream>, Status> {
    let request = request.into_inner();
    let inputs = scan_inputs(request.scan)?;
    let query = non_empty(request.query);
    // TODO(netease-progressive-scan-stream): The app-owned scan currently
    // returns one aggregate after its blocking UI traversal. Emit events during
    // traversal and stop work when the receiver is dropped after that operation
    // exposes a typed event sink and cancellation boundary.
    let scan = blocking("playlist scan", move || match query {
      Some(query) => crate::run_live_scan_until_query(&inputs, &query),
      None => crate::run_live_scan(&inputs),
    })
    .await?;
    let playlists = scan
      .projection()
      .sections
      .iter()
      .filter(|section| matches!(section.kind, crate::SidebarSectionKind::MyPlaylists | crate::SidebarSectionKind::FavoritePlaylists))
      .flat_map(|section| {
        section.items.iter().map(|item| proto::Playlist {
          reference: Some(playlist_ref_to_proto(section.kind, &item.label)),
          id: item.id.clone(),
          candidate_id: item.candidate_id.clone(),
          anchor_id: item.anchor_id.clone(),
        })
      })
      .collect::<Vec<_>>();
    let emitted = u32::try_from(playlists.len()).unwrap_or(u32::MAX);
    let observations = u32::try_from(scan.observations_len()).unwrap_or(u32::MAX);
    let mut events: Vec<Result<proto::ListPlaylistsStreamResponse, Status>> =
      Vec::with_capacity(playlists.len() + scan.diagnostics().len() + 1);
    events.extend(playlists.into_iter().map(|playlist| {
      Ok(proto::ListPlaylistsStreamResponse {
        event: Some(proto::list_playlists_stream_response::Event::Item(proto::ListPlaylistsItem {
          playlist: Some(playlist),
        })),
      })
    }));
    events.extend(diagnostics(scan.diagnostics()).into_iter().map(|diagnostic| {
      Ok(proto::ListPlaylistsStreamResponse {
        event: Some(proto::list_playlists_stream_response::Event::Diagnostic(proto::ListPlaylistsDiagnostic {
          diagnostic: Some(diagnostic),
        })),
      })
    }));
    events.push(Ok(proto::ListPlaylistsStreamResponse {
      event: Some(proto::list_playlists_stream_response::Event::Completed(proto::ListPlaylistsCompleted {
        items_emitted: emitted,
        known_limits: scan.known_limits().to_vec(),
        observations,
      })),
    }));
    Ok(Response::new(Box::pin(futures_util::stream::iter(events))))
  }

  async fn select_playlist(
    &self,
    request: Request<proto::SelectPlaylistRequest>,
  ) -> Result<Response<proto::SelectPlaylistResponse>, Status> {
    let request = request.into_inner();
    let reference = playlist_ref_from_proto(request.playlist)?;
    let inputs = scan_inputs(request.scan)?;
    let result = blocking("playlist select", move || crate::run_playlist_select_ref(&inputs, &reference)).await?;
    Ok(Response::new(proto::SelectPlaylistResponse {
      playlist: Some(playlist_from_target(&result.target)),
      verified: result.verification.passed(),
      observed_title: result.verification.observed_title().map(ToOwned::to_owned),
      diagnostics: diagnostics(&result.diagnostics),
      known_limits: result.known_limits,
    }))
  }

  async fn play_playlist(&self, request: Request<proto::PlayPlaylistRequest>) -> Result<Response<proto::PlayPlaylistResponse>, Status> {
    let request = request.into_inner();
    let reference = playlist_ref_from_proto(request.playlist)?;
    let inputs = scan_inputs(request.scan)?;
    let result = blocking("playlist play", move || crate::run_playlist_play_ref(&inputs, &reference)).await?;
    Ok(Response::new(proto::PlayPlaylistResponse {
      playlist: Some(playlist_from_target(&result.select.target)),
      verified: result.verification.passed(),
      control_state: playback_control_state(result.verification.control_state()) as i32,
      observed_bottom_text: result.verification.observed_bottom_text().map(ToOwned::to_owned),
      diagnostics: diagnostics(&result.diagnostics),
      known_limits: result.known_limits,
    }))
  }
}

#[tonic::async_trait]
impl RecommendationService for Service {
  async fn play_daily_recommended(
    &self,
    request: Request<proto::PlayDailyRecommendedRequest>,
  ) -> Result<Response<proto::PlayDailyRecommendedResponse>, Status> {
    let request = request.into_inner();
    let mut inputs = crate::DailyRecommendedPlayInputs::with_defaults();
    inputs.app_id = app_id(request.application_bundle_id);
    if let Some(value) = request.max_top_scrolls {
      inputs.max_top_scrolls = value as usize;
    }
    if let Some(value) = request.top_scroll_amount {
      if !value.is_finite() || value <= 0.0 {
        return Err(Status::invalid_argument("top_scroll_amount must be a positive finite number"));
      }
      inputs.top_scroll_amount = value;
    }
    if let Some(value) = request.settle_milliseconds {
      inputs.settle_ms = value;
    }
    let result = blocking("daily recommended play", move || crate::run_daily_recommended_play(&inputs)).await?;
    Ok(Response::new(proto::PlayDailyRecommendedResponse {
      verified: result.verification.passed(),
      diagnostics: diagnostics(&result.diagnostics),
      known_limits: result.known_limits,
    }))
  }
}

#[tonic::async_trait]
impl SongService for Service {
  type ListSongsStream = Pin<Box<dyn Stream<Item = Result<proto::ListSongsStreamResponse, Status>> + Send>>;

  async fn list_songs(&self, request: Request<proto::ListSongsRequest>) -> Result<Response<Self::ListSongsStream>, Status> {
    let request = request.into_inner();
    let source = match request.source {
      Some(proto::list_songs_request::Source::DailyRecommended(_)) => crate::SongSource::from(crate::DailyRecommendedRef),
      Some(proto::list_songs_request::Source::Playlist(reference)) => crate::SongSource::from(playlist_ref_from_proto(Some(reference))?),
      None => return Err(Status::invalid_argument("song source must be specified")),
    };
    let inputs = scan_inputs(request.scan)?;
    // TODO(netease-progressive-scan-stream): See the matching playlist scan
    // marker. Song scanning needs the same typed event and cancellation seam
    // before this transport stream can emit while the UI traversal is running.
    let result = blocking("song scan", move || crate::run_songs_scan(&inputs, source)).await?;
    let emitted = u32::try_from(result.items.len()).unwrap_or(u32::MAX);
    let mut events: Vec<Result<proto::ListSongsStreamResponse, Status>> =
      Vec::with_capacity(result.items.len() + result.diagnostics.len() + 1);
    events.extend(result.items.into_iter().map(|song| {
      Ok(proto::ListSongsStreamResponse {
        event: Some(proto::list_songs_stream_response::Event::Item(proto::ListSongsItem {
          song: Some(proto::Song {
            id: song.id,
            index: song.index,
            title: song.title,
            row_text: song.row_text,
          }),
        })),
      })
    }));
    events.extend(diagnostics(&result.diagnostics).into_iter().map(|diagnostic| {
      Ok(proto::ListSongsStreamResponse {
        event: Some(proto::list_songs_stream_response::Event::Diagnostic(proto::ListSongsDiagnostic {
          diagnostic: Some(diagnostic),
        })),
      })
    }));
    events.push(Ok(proto::ListSongsStreamResponse {
      event: Some(proto::list_songs_stream_response::Event::Completed(proto::ListSongsCompleted {
        items_emitted: emitted,
        known_limits: result.known_limits,
      })),
    }));
    Ok(Response::new(Box::pin(futures_util::stream::iter(events))))
  }
}

#[tonic::async_trait]
impl ApplicationService for Service {
  async fn open_window(&self, request: Request<proto::OpenWindowRequest>) -> Result<Response<proto::OpenWindowResponse>, Status> {
    let request = request.into_inner();
    let mut inputs = crate::OpenWindowInputs::default();
    if let Some(value) = request.settle_milliseconds {
      inputs.settle_ms = value;
    }
    inputs.executable = non_empty(request.executable).map(Into::into);
    let result = blocking("open NetEase window", move || crate::run_open_window(&inputs)).await?;
    Ok(Response::new(proto::OpenWindowResponse {
      window_found: result.window_found,
      window_title: result.window_title,
      process_name: result.process_name,
      executable: result.executable,
    }))
  }
}

async fn blocking<T, E, F>(operation: &'static str, callback: F) -> Result<T, Status>
where
  T: Send + 'static,
  E: std::fmt::Display + Send + 'static,
  F: FnOnce() -> Result<T, E> + Send + 'static,
{
  tokio::task::spawn_blocking(callback)
    .await
    .map_err(|error| Status::internal(format!("{operation} worker failed: {error}")))?
    .map_err(|error| operation_status(operation, error))
}

async fn deliver_playback_control(application_bundle_id: Option<String>, command: auv_media_macos::MediaCommand) -> Result<String, Status> {
  let requested_app = app_id(application_bundle_id);
  blocking("playback control", move || control_playback(command, &requested_app)).await
}

fn operation_status(operation: &str, error: impl std::fmt::Display) -> Status {
  let message = format!("{operation} failed: {error}");
  if message.contains("only supported on") || message.contains("not available through the Windows") {
    Status::unimplemented(message)
  } else {
    Status::failed_precondition(message)
  }
}

fn app_id(value: Option<String>) -> String {
  non_empty(value).unwrap_or_else(|| DEFAULT_APP_ID.to_string())
}

fn non_empty(value: Option<String>) -> Option<String> {
  value.and_then(|value| (!value.trim().is_empty()).then(|| value.trim().to_string()))
}

fn scan_inputs(options: Option<proto::ScanOptions>) -> Result<crate::Inputs, Status> {
  let mut inputs = crate::Inputs::with_defaults();
  let Some(options) = options else {
    return Ok(inputs);
  };
  inputs.app_id = app_id(options.application_bundle_id);
  if let Some(value) = options.max_scrolls {
    inputs.max_scrolls = value as usize;
  }
  if let Some(value) = options.scroll_amount {
    if !value.is_finite() || value <= 0.0 {
      return Err(Status::invalid_argument("scan.scroll_amount must be a positive finite number"));
    }
    inputs.scroll_amount = value;
  }
  if let Some(value) = options.scroll_settle_milliseconds {
    inputs.scroll_settle_ms = value;
  }
  Ok(inputs)
}

fn diagnostics(values: &[auv_view::ParserDiagnostic]) -> Vec<proto::Diagnostic> {
  values
    .iter()
    .map(|value| proto::Diagnostic {
      code: value.code.clone(),
      message: value.message.clone(),
      node_id: value.node_id.clone(),
    })
    .collect()
}

fn playlist_ref_from_proto(value: Option<proto::PlaylistRef>) -> Result<crate::PlaylistRef, Status> {
  let value = value.ok_or_else(|| Status::invalid_argument("playlist must be specified"))?;
  let section = match proto::PlaylistSection::try_from(value.section).unwrap_or(proto::PlaylistSection::Unspecified) {
    proto::PlaylistSection::Created => crate::PlaylistSection::Created,
    proto::PlaylistSection::Favorite => crate::PlaylistSection::Favorite,
    proto::PlaylistSection::Unspecified => {
      return Err(Status::invalid_argument("playlist.section must be CREATED or FAVORITE"));
    }
  };
  crate::PlaylistRef::new(section, value.label).map_err(Status::invalid_argument)
}

fn playlist_ref_to_proto(section: crate::SidebarSectionKind, label: &str) -> proto::PlaylistRef {
  let section = match section {
    crate::SidebarSectionKind::MyPlaylists => proto::PlaylistSection::Created,
    crate::SidebarSectionKind::FavoritePlaylists => proto::PlaylistSection::Favorite,
    // List and selection only expose playlist collection sections. Keep the
    // fallback explicit so a future caller cannot silently become replayable.
    _ => proto::PlaylistSection::Unspecified,
  };
  proto::PlaylistRef {
    label: label.to_string(),
    section: section as i32,
  }
}

fn playlist_from_target(value: &crate::PlaylistSelectTarget) -> proto::Playlist {
  proto::Playlist {
    reference: Some(playlist_ref_to_proto(value.section_kind, &value.label)),
    id: value.item_id.clone(),
    candidate_id: value.candidate_id.clone(),
    anchor_id: value.anchor_id.clone(),
  }
}

fn playback_control_state(value: crate::PlaybackControlState) -> proto::PlaybackControlState {
  match value {
    crate::PlaybackControlState::PlayVisible => proto::PlaybackControlState::PlayVisible,
    crate::PlaybackControlState::PauseVisible => proto::PlaybackControlState::PauseVisible,
    crate::PlaybackControlState::Unknown => proto::PlaybackControlState::Unknown,
  }
}

#[cfg(target_os = "macos")]
fn require_media_owner(app_id: &str) -> Result<(), String> {
  let state = auv_media_macos::now_playing().map_err(|error| error.to_string())?;
  if state.source_bundle_id.as_deref() == Some(app_id) {
    Ok(())
  } else {
    Err(format!("{app_id} is not the current now-playing application"))
  }
}

#[cfg(target_os = "macos")]
fn control_playback(command: auv_media_macos::MediaCommand, app_id: &str) -> Result<String, String> {
  require_media_owner(app_id)?;
  auv_media_macos::send_command(command).map_err(|error| error.to_string())?;
  Ok("macos_media_remote".to_string())
}

#[cfg(target_os = "windows")]
fn control_playback(command: auv_media_macos::MediaCommand, _app_id: &str) -> Result<String, String> {
  let action = match command {
    auv_media_macos::MediaCommand::TogglePlayPause => crate::TransportAction::PlayPause,
    auv_media_macos::MediaCommand::NextTrack => crate::TransportAction::Next,
    auv_media_macos::MediaCommand::PreviousTrack => crate::TransportAction::Previous,
    auv_media_macos::MediaCommand::Play | auv_media_macos::MediaCommand::Pause => {
      return Err("idempotent play and pause are not available through the Windows UIA slice; use toggle".to_string());
    }
  };
  let result = crate::run_transport_action(&crate::TransportInputs::new(action))?;
  Ok(format!("windows_uia:{:?}", result.delivery.selected_path))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn control_playback(_command: auv_media_macos::MediaCommand, _app_id: &str) -> Result<String, String> {
  Err("NetEase playback control is only supported on macOS and Windows".to_string())
}

#[cfg(target_os = "macos")]
fn seek_playback(position: std::time::Duration, app_id: &str) -> Result<(), String> {
  require_media_owner(app_id)?;
  auv_media_macos::seek(position).map_err(|error| error.to_string())
}

#[cfg(not(target_os = "macos"))]
fn seek_playback(_position: std::time::Duration, _app_id: &str) -> Result<(), String> {
  Err("NetEase playback seek is only supported on macOS".to_string())
}

#[cfg(unix)]
pub async fn serve_inherited() -> Result<(), String> {
  let (incoming, parent_disconnected) = auv_api_server::runner_transport::inherited_transport()?.into_parts();
  let (health_reporter, health) = tonic_health::server::health_reporter();
  health_reporter.set_serving::<ApplicationServiceServer<Service>>().await;
  health_reporter.set_serving::<PlayerServiceServer<Service>>().await;
  health_reporter.set_serving::<PlaylistServiceServer<Service>>().await;
  health_reporter.set_serving::<RecommendationServiceServer<Service>>().await;
  health_reporter.set_serving::<SongServiceServer<Service>>().await;
  let reflection = auv_api_server::reflection::service(crate::api::FILE_DESCRIPTOR_SET)
    .map_err(|error| format!("failed to build NetEase Runner reflection: {error}"))?;
  tonic::transport::Server::builder()
    .add_service(health)
    .add_service(reflection)
    .add_service(ApplicationServiceServer::new(Service))
    .add_service(PlayerServiceServer::new(Service))
    .add_service(PlaylistServiceServer::new(Service))
    .add_service(RecommendationServiceServer::new(Service))
    .add_service(SongServiceServer::new(Service))
    .serve_with_incoming_shutdown(incoming, parent_disconnected)
    .await
    .map_err(|error| format!("NetEase Runner transport failed: {error}"))
}

#[cfg(not(unix))]
pub async fn serve_inherited() -> Result<(), String> {
  // TODO(netease-runner-windows-ipc): enable after the API server transport
  // supports daemon-owned inherited named-pipe handles.
  Err("the NetEase Runner currently requires Unix inherited IPC".to_string())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[tokio::test]
  async fn reflected_playlist_selection_rejects_an_empty_reference_label_before_ui_work() {
    let status = Service
      .select_playlist(Request::new(proto::SelectPlaylistRequest {
        playlist: Some(proto::PlaylistRef {
          label: "  ".to_string(),
          section: proto::PlaylistSection::Created as i32,
        }),
        scan: None,
      }))
      .await
      .expect_err("empty playlist reference label must be rejected");

    assert_eq!(status.code(), tonic::Code::InvalidArgument);
  }

  #[test]
  fn reflected_playlist_reference_requires_a_supported_section() {
    let status = playlist_ref_from_proto(Some(proto::PlaylistRef {
      label: "Focus".to_string(),
      section: proto::PlaylistSection::Unspecified as i32,
    }))
    .expect_err("unspecified section must be rejected");

    assert_eq!(status.code(), tonic::Code::InvalidArgument);
  }

  #[tokio::test]
  async fn reflected_seek_rejects_a_negative_position_before_media_work() {
    let status = Service
      .seek(Request::new(proto::SeekRequest {
        position_seconds: -1.0,
        application_bundle_id: None,
      }))
      .await
      .expect_err("negative seek position must be rejected");

    assert_eq!(status.code(), tonic::Code::InvalidArgument);
  }

  #[tokio::test]
  async fn reflected_song_list_requires_an_explicit_source() {
    let result = Service
      .list_songs(Request::new(proto::ListSongsRequest {
        source: None,
        scan: None,
      }))
      .await;
    let Err(status) = result else {
      panic!("missing source must be rejected");
    };

    assert_eq!(status.code(), tonic::Code::InvalidArgument);
  }

  #[test]
  fn unsupported_platform_errors_map_to_unimplemented() {
    let status = operation_status("open NetEase window", "operation is only supported on Windows");
    assert_eq!(status.code(), tonic::Code::Unimplemented);
  }
}
