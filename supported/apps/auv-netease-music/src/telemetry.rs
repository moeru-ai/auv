//! NetEase-specific tracing events and best-effort evidence artifacts.

use auv_driver::InputActionResult;
use auv_tracing::{Attributes, ByteLength};
use auv_view::ViewBounds;
use serde::Serialize;

use crate::scroll::policies::detection_motion::MotionEvidence;
use crate::{SidebarScanStopReason, SidebarViewportObservation};

const JSON_ARTIFACT_BYTE_LIMIT: u64 = 4 * 1024 * 1024;

#[derive(Serialize)]
struct ArtifactPreparationFailed {
  purpose: &'static str,
  error: String,
}

impl auv_tracing::EventPayload for ArtifactPreparationFailed {
  const NAME: &'static str = "auv.netease.artifact_preparation_failed";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
struct SidebarObserved {
  observation_index: usize,
  viewport_fingerprint: String,
}

impl auv_tracing::EventPayload for SidebarObserved {
  const NAME: &'static str = "auv.netease.sidebar.observed";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
struct SidebarScrolled {
  from_observation: usize,
  to_observation: usize,
  requested_delta: f64,
  settle_ms: u64,
  delivery_path: Option<String>,
  motion: Option<MotionEvidence>,
}

impl auv_tracing::EventPayload for SidebarScrolled {
  const NAME: &'static str = "auv.netease.sidebar.scrolled";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
struct SidebarScanStopped {
  reason: SidebarScanStopReason,
}

impl auv_tracing::EventPayload for SidebarScanStopped {
  const NAME: &'static str = "auv.netease.sidebar.scan_stopped";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum PlaylistSelectInputDelivered {
  SeekSidebarTop {
    attempt: usize,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  SeekTargetPage {
    attempt: usize,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  SeekBottomPadding {
    attempt: usize,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  SelectPlaylist {
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  SelectPlaylistForegroundRetry {
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
}

impl auv_tracing::EventPayload for PlaylistSelectInputDelivered {
  const NAME: &'static str = "auv.netease.playlist_select.input_delivered";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub(crate) enum PlaylistTargetResolved {
  RescanReplay { attempt: usize, bounds: ViewBounds },
  BottomPadding { attempt: usize, bounds: ViewBounds },
}

impl auv_tracing::EventPayload for PlaylistTargetResolved {
  const NAME: &'static str = "auv.netease.playlist_select.target_resolved";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum PlaylistPlayInputDelivered {
  PlayAll {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  PlayAllForegroundRetry {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
}

impl auv_tracing::EventPayload for PlaylistPlayInputDelivered {
  const NAME: &'static str = "auv.netease.playlist_play.input_delivered";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum DailyRecommendedInputDelivered {
  SeekSidebarTop {
    attempt: usize,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  SelectSidebarRecommend {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  OpenDailyRecommendedCard {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  OpenDailyRecommendedTitleForegroundRetry {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  PlayAll {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
  PlayAllForegroundRetry {
    label: String,
    bounds: ViewBounds,
    delivery: InputActionResult,
  },
}

impl auv_tracing::EventPayload for DailyRecommendedInputDelivered {
  const NAME: &'static str = "auv.netease.daily_recommended.input_delivered";
  const VERSION: u32 = 1;
}

#[derive(Serialize)]
pub(crate) struct DailyRecommendedPlayAllChecked {
  pub visible: bool,
}

impl auv_tracing::EventPayload for DailyRecommendedPlayAllChecked {
  const NAME: &'static str = "auv.netease.daily_recommended.play_all_checked";
  const VERSION: u32 = 1;
}

pub(crate) fn emit_sidebar_scan_events(
  observations: &[SidebarViewportObservation],
  scroll_amount: f64,
  scroll_settle_ms: u64,
  stop_reason: Option<SidebarScanStopReason>,
) {
  for (index, observation) in observations.iter().enumerate() {
    auv_tracing::emit_event!(SidebarObserved {
      observation_index: observation.observation_index,
      viewport_fingerprint: observation.viewport_fingerprint.clone(),
    });
    if let Some(next) = observations.get(index + 1) {
      auv_tracing::emit_event!(SidebarScrolled {
        from_observation: observation.observation_index,
        to_observation: next.observation_index,
        requested_delta: scroll_amount,
        settle_ms: scroll_settle_ms,
        delivery_path: next.incoming_scroll_delivery_path.clone(),
        motion: next.scroll_motion.clone(),
      });
    }
  }
  if let Some(reason) = stop_reason {
    auv_tracing::emit_event!(SidebarScanStopped { reason });
  }
}

pub(crate) fn json_artifact<T: Serialize>(purpose: &'static str, value: &T) {
  let result = auv_tracing::emit_json_artifact(
    purpose,
    Attributes::empty(),
    ByteLength::new(JSON_ARTIFACT_BYTE_LIMIT).expect("static NetEase JSON limit is valid"),
    value,
  )
  .map(drop)
  .map_err(|error| error.to_string());
  if let Err(error) = result {
    preparation_failed(purpose, error);
  }
}

pub(crate) fn image_artifact(purpose: &'static str, image: &image::RgbaImage) {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return;
  }
  match auv_tracing::image_artifact(auv_tracing::EmitBytesOptions::new().with_purpose(purpose), image) {
    Ok(artifact) => drop(auv_tracing::emit_artifact(artifact)),
    Err(error) => preparation_failed(purpose, error.to_string()),
  }
}

pub(crate) fn preparation_failed(purpose: &'static str, error: impl Into<String>) {
  auv_tracing::emit_event!(ArtifactPreparationFailed {
    purpose,
    error: error.into()
  });
}

pub(crate) fn spawn_artifact_task(operation: impl FnOnce() + Send + 'static) -> Option<std::thread::JoinHandle<()>> {
  if !auv_tracing::Context::current().can_publish_artifacts() {
    return None;
  }
  let context = auv_tracing::Context::current();
  Some(std::thread::spawn(move || context.in_scope(operation)))
}
