//! Bounded recent-frame capture resources owned by the local Driver Runner.

use std::{
  collections::{HashMap, VecDeque},
  sync::{Arc, Mutex, mpsc},
  thread,
  time::Duration,
};

use auv_api_proto::auv::api::driver::v1 as proto;
use auv_api_proto::auv::api::driver::v1::recent_frames_service_server::RecentFramesService;
use tonic::{Request, Response, Status};

use super::local_driver;

const MAX_ACTIVE_BUFFERS: usize = 8;
const MAX_CAPACITY: u32 = 120;
const MAX_TARGET_FPS: u32 = 120;
const MAX_BUFFER_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct Service {
  buffers: Arc<Mutex<HashMap<String, ActiveBuffer>>>,
  source_factory: Arc<dyn FrameSourceFactory>,
}

impl Service {
  pub(super) fn new(session: auv_driver::LocalDriverSession) -> Self {
    Self {
      buffers: Arc::new(Mutex::new(HashMap::new())),
      source_factory: Arc::new(DriverFrameSourceFactory { session }),
    }
  }

  pub(super) async fn shutdown(&self) -> Result<(), Status> {
    let buffers = self.buffers.lock().expect("recent-frame buffers mutex poisoned").drain().map(|(_, buffer)| buffer).collect::<Vec<_>>();
    for buffer in buffers {
      buffer.stop().await?;
    }
    Ok(())
  }

  #[cfg(test)]
  fn with_factory(source_factory: Arc<dyn FrameSourceFactory>) -> Self {
    Self {
      buffers: Arc::new(Mutex::new(HashMap::new())),
      source_factory,
    }
  }
}

struct ActiveBuffer {
  history: Arc<Mutex<History>>,
  stop: Option<mpsc::Sender<()>>,
  producer: Option<thread::JoinHandle<()>>,
}

impl ActiveBuffer {
  fn start(
    id: &str,
    target_fps: u32,
    capacity: usize,
    mut source: Box<dyn FrameSource>,
    first_capture: auv_driver::Capture,
    output_size: Option<PixelSize>,
  ) -> Result<Self, Status> {
    let history = Arc::new(Mutex::new(History::new(capacity)));
    history.lock().expect("recent-frame history mutex poisoned").push(first_capture);
    let (stop, stop_requested) = mpsc::channel();
    let producer_history = Arc::clone(&history);
    let producer = thread::Builder::new()
      .name(format!("auv-recent-frames-{id}"))
      .spawn(move || run_producer(&mut *source, target_fps, output_size, producer_history, stop_requested))
      .map_err(|error| Status::resource_exhausted(format!("failed to start recent-frame producer: {error}")))?;
    Ok(Self {
      history,
      stop: Some(stop),
      producer: Some(producer),
    })
  }

  async fn stop(mut self) -> Result<(), Status> {
    if let Some(stop) = self.stop.take() {
      let _ = stop.send(());
    }
    let producer = self.producer.take().expect("an active frame buffer owns its producer thread");
    tokio::task::spawn_blocking(move || producer.join())
      .await
      .map_err(|error| Status::internal(format!("failed to join recent-frame producer task: {error}")))?
      .map_err(|_| Status::internal("recent-frame producer thread panicked"))?;
    Ok(())
  }
}

impl Drop for ActiveBuffer {
  fn drop(&mut self) {
    if let Some(stop) = self.stop.take() {
      let _ = stop.send(());
    }
  }
}

struct History {
  capacity: usize,
  frames: VecDeque<proto::RecentFrame>,
  latest_sequence: u64,
  dropped_frames: u64,
  pause: Option<Pause>,
  fault: Option<String>,
}

/// The buffer's target cannot be captured faithfully right now, so captures
/// are skipped until one succeeds.
struct Pause {
  reason: String,
  skipped_captures: u64,
  since: std::time::SystemTime,
}

impl History {
  fn new(capacity: usize) -> Self {
    Self {
      capacity,
      frames: VecDeque::with_capacity(capacity),
      latest_sequence: 0,
      dropped_frames: 0,
      pause: None,
      fault: None,
    }
  }

  fn push(&mut self, capture: auv_driver::Capture) {
    self.pause = None;
    self.latest_sequence += 1;
    if self.frames.len() == self.capacity {
      self.frames.pop_front();
      self.dropped_frames += 1;
    }
    self.frames.push_back(proto::RecentFrame {
      sequence: self.latest_sequence,
      // TODO(recent-frames-capture-refs): buffered frames still carry pixels.
      // Move them into the capture store and return references together with
      // the planned video stream (capture-references-and-positions design).
      frame: Some(local_driver::image_frame_to_proto(capture)),
    });
  }

  fn skip(&mut self, reason: String) {
    let pause = self.pause.get_or_insert_with(|| Pause {
      reason: String::new(),
      skipped_captures: 0,
      since: std::time::SystemTime::now(),
    });
    pause.reason = reason;
    pause.skipped_captures += 1;
  }

  fn recent(&self, after_sequence: u64) -> Result<proto::GetRecentFramesResponse, Status> {
    if after_sequence > self.latest_sequence {
      return Err(Status::invalid_argument(format!(
        "after_sequence {after_sequence} is newer than latest_sequence {}",
        self.latest_sequence
      )));
    }
    let frames = self.frames.iter().filter(|frame| frame.sequence > after_sequence).cloned().collect::<Vec<_>>();
    if frames.is_empty()
      && let Some(reason) = &self.fault
    {
      return Err(Status::unavailable(format!("recent-frame producer stopped: {reason}")));
    }
    Ok(proto::GetRecentFramesResponse {
      frames,
      latest_sequence: self.latest_sequence,
      dropped_frames: self.dropped_frames,
      pause: self.pause.as_ref().map(|pause| proto::FrameBufferPause {
        reason: pause.reason.clone(),
        skipped_captures: pause.skipped_captures,
        since: Some(pause.since.into()),
      }),
    })
  }
}

#[derive(Clone, Copy)]
struct PixelSize {
  width: u32,
  height: u32,
}

#[derive(Clone, Debug, PartialEq)]
enum CaptureTarget {
  Window(proto::WindowRef),
  Display(Option<String>),
  Region {
    region: auv_driver::Rect,
    display: Option<String>,
  },
}

trait FrameSourceFactory: Send + Sync + 'static {
  fn open(&self, target: CaptureTarget) -> Result<Box<dyn FrameSource>, Status>;
}

trait FrameSource: Send + 'static {
  fn capture(&mut self) -> Result<auv_driver::Capture, auv_driver::DriverError>;
}

struct DriverFrameSourceFactory {
  session: auv_driver::LocalDriverSession,
}

impl FrameSourceFactory for DriverFrameSourceFactory {
  fn open(&self, target: CaptureTarget) -> Result<Box<dyn FrameSource>, Status> {
    let target = match target {
      CaptureTarget::Window(reference) => DriverCaptureTarget::Window(local_driver::resolve_window_ref(&self.session, reference)?),
      CaptureTarget::Display(display) => DriverCaptureTarget::Display(display),
      CaptureTarget::Region { region, display } => DriverCaptureTarget::Region { region, display },
    };
    Ok(Box::new(DriverFrameSource {
      session: self.session.clone(),
      target,
    }))
  }
}

enum DriverCaptureTarget {
  Window(auv_driver::Window),
  Display(Option<String>),
  Region {
    region: auv_driver::Rect,
    display: Option<String>,
  },
}

struct DriverFrameSource {
  session: auv_driver::LocalDriverSession,
  target: DriverCaptureTarget,
}

impl FrameSource for DriverFrameSource {
  fn capture(&mut self) -> Result<auv_driver::Capture, auv_driver::DriverError> {
    match &self.target {
      DriverCaptureTarget::Window(window) => self.session.window().capture(window),
      DriverCaptureTarget::Display(display) => self
        .session
        .display()
        .capture(auv_driver::CaptureOptions {
          display: display.clone(),
          ..Default::default()
        })
        .map(|captured| captured.capture),
      DriverCaptureTarget::Region { region, display } => self
        .session
        .display()
        .capture_region(auv_driver::CaptureOptions {
          display: display.clone(),
          region: Some(*region),
          ..Default::default()
        })
        .map(|captured| captured.capture),
    }
  }
}

fn run_producer(
  source: &mut dyn FrameSource,
  target_fps: u32,
  output_size: Option<PixelSize>,
  history: Arc<Mutex<History>>,
  stop: mpsc::Receiver<()>,
) {
  let period = Duration::from_secs_f64(1.0 / f64::from(target_fps));
  loop {
    match stop.recv_timeout(period) {
      Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => return,
      Err(mpsc::RecvTimeoutError::Timeout) => {}
    }
    match source.capture() {
      Ok(capture) => history.lock().expect("recent-frame history mutex poisoned").push(resize_capture(capture, output_size)),
      // The target is transiently not capturable (for example a window shown
      // by Mission Control or minimized). Keep the buffer alive and retry on
      // the next tick instead of stopping it.
      Err(error @ auv_driver::DriverError::StaleUiReference { .. }) => {
        history.lock().expect("recent-frame history mutex poisoned").skip(error.to_string());
      }
      Err(error) => {
        history.lock().expect("recent-frame history mutex poisoned").fault = Some(error.to_string());
        return;
      }
    }
  }
}

fn resize_capture(mut capture: auv_driver::Capture, output_size: Option<PixelSize>) -> auv_driver::Capture {
  let Some(size) = output_size else {
    return capture;
  };
  if capture.image.width() != size.width || capture.image.height() != size.height {
    capture.image = image::imageops::resize(&capture.image, size.width, size.height, image::imageops::FilterType::Triangle);
  }
  capture
}

#[tonic::async_trait]
impl RecentFramesService for Service {
  async fn open_frame_buffer(
    &self,
    request: Request<proto::OpenFrameBufferRequest>,
  ) -> Result<Response<proto::OpenFrameBufferResponse>, Status> {
    let (target, target_fps, capacity, output_size) = validate_open(request.into_inner())?;
    if self.buffers.lock().expect("recent-frame buffers mutex poisoned").len() >= MAX_ACTIVE_BUFFERS {
      return Err(Status::resource_exhausted(format!("at most {MAX_ACTIVE_BUFFERS} recent-frame buffers may be active")));
    }

    let mut source = self.source_factory.open(target)?;
    let first_capture =
      resize_capture(source.capture().map_err(|error| Status::unavailable(format!("initial capture failed: {error}")))?, output_size);
    validate_buffer_bytes(first_capture.image.len(), capacity)?;
    let id = uuid::Uuid::now_v7().to_string();
    let active = ActiveBuffer::start(&id, target_fps, capacity, source, first_capture, output_size)?;

    let rejected = {
      let mut buffers = self.buffers.lock().expect("recent-frame buffers mutex poisoned");
      if buffers.len() >= MAX_ACTIVE_BUFFERS {
        Some(active)
      } else {
        buffers.insert(id.clone(), active);
        None
      }
    };
    if let Some(active) = rejected {
      active.stop().await?;
      return Err(Status::resource_exhausted(format!("at most {MAX_ACTIVE_BUFFERS} recent-frame buffers may be active")));
    }
    Ok(Response::new(proto::OpenFrameBufferResponse {
      frame_buffer: Some(proto::FrameBufferRef {
        frame_buffer_id: id,
      }),
    }))
  }

  async fn get_recent_frames(
    &self,
    request: Request<proto::GetRecentFramesRequest>,
  ) -> Result<Response<proto::GetRecentFramesResponse>, Status> {
    let request = request.into_inner();
    let reference = validate_reference(request.frame_buffer)?;
    let history = {
      let buffers = self.buffers.lock().expect("recent-frame buffers mutex poisoned");
      Arc::clone(&buffers.get(&reference.frame_buffer_id).ok_or_else(|| Status::not_found("unknown frame buffer"))?.history)
    };
    let history = history.lock().expect("recent-frame history mutex poisoned");
    Ok(Response::new(history.recent(request.after_sequence)?))
  }

  async fn close_frame_buffer(
    &self,
    request: Request<proto::CloseFrameBufferRequest>,
  ) -> Result<Response<proto::CloseFrameBufferResponse>, Status> {
    let reference = validate_reference(request.into_inner().frame_buffer)?;
    let active = self
      .buffers
      .lock()
      .expect("recent-frame buffers mutex poisoned")
      .remove(&reference.frame_buffer_id)
      .ok_or_else(|| Status::not_found("unknown frame buffer"))?;
    active.stop().await?;
    Ok(Response::new(proto::CloseFrameBufferResponse {}))
  }
}

fn validate_open(request: proto::OpenFrameBufferRequest) -> Result<(CaptureTarget, u32, usize, Option<PixelSize>), Status> {
  if request.target_fps == 0 || request.target_fps > MAX_TARGET_FPS {
    return Err(Status::invalid_argument(format!("target_fps must be within 1..={MAX_TARGET_FPS}")));
  }
  if request.frame_capacity == 0 || request.frame_capacity > MAX_CAPACITY {
    return Err(Status::invalid_argument(format!("frame_capacity must be within 1..={MAX_CAPACITY}")));
  }
  let capacity = request.frame_capacity as usize;
  let output_size = request.output_size.map(|size| validate_output_size(size, capacity)).transpose()?;
  let target = match request.target.and_then(|target| target.target) {
    Some(proto::capture_target::Target::Window(window)) if !window.window_id.trim().is_empty() => CaptureTarget::Window(window),
    Some(proto::capture_target::Target::Window(_)) => return Err(Status::invalid_argument("target.window.window_id is required")),
    Some(proto::capture_target::Target::Display(display)) => {
      CaptureTarget::Display(local_driver::display_selector_from_proto(display.selector)?)
    }
    Some(proto::capture_target::Target::Region(region)) => CaptureTarget::Region {
      region: local_driver::rect_from_proto(
        region.region.ok_or_else(|| Status::invalid_argument("target.region.region is required"))?,
        "target.region.region",
      )?,
      display: local_driver::display_selector_from_proto(region.selector)?,
    },
    None => return Err(Status::invalid_argument("target is required")),
  };
  Ok((target, request.target_fps, capacity, output_size))
}

fn validate_output_size(size: auv_api_proto::auv::api::image::v1::PixelSize, capacity: usize) -> Result<PixelSize, Status> {
  if size.width == 0 || size.height == 0 {
    return Err(Status::invalid_argument("output_size width and height must be positive"));
  }
  let bytes = usize::try_from(size.width)
    .ok()
    .and_then(|width| usize::try_from(size.height).ok().and_then(|height| width.checked_mul(height)))
    .and_then(|pixels| pixels.checked_mul(4))
    .ok_or_else(|| Status::resource_exhausted("output_size is too large"))?;
  validate_buffer_bytes(bytes, capacity)?;
  Ok(PixelSize {
    width: size.width,
    height: size.height,
  })
}

fn validate_buffer_bytes(frame_bytes: usize, capacity: usize) -> Result<(), Status> {
  let buffer_bytes = frame_bytes.checked_mul(capacity).ok_or_else(|| Status::resource_exhausted("frame buffer is too large"))?;
  if buffer_bytes > MAX_BUFFER_BYTES {
    return Err(Status::resource_exhausted(format!(
      "frame buffer would use {buffer_bytes} bytes; limit is {MAX_BUFFER_BYTES}. Reduce frame_capacity or pass output_size"
    )));
  }
  Ok(())
}

fn validate_reference(reference: Option<proto::FrameBufferRef>) -> Result<proto::FrameBufferRef, Status> {
  let reference = reference.ok_or_else(|| Status::invalid_argument("frame_buffer is required"))?;
  if reference.frame_buffer_id.trim().is_empty() {
    return Err(Status::invalid_argument("frame_buffer.frame_buffer_id is required"));
  }
  Ok(reference)
}

#[cfg(test)]
#[path = "recent_frames_test.rs"]
mod tests;
