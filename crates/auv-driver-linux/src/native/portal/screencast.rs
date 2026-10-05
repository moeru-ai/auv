use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::os::fd::OwnedFd as StdOwnedFd;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use ashpd::desktop::screencast::{CursorMode, Screencast, SelectSourcesOptions, SourceType, Stream};
use ashpd::desktop::{PersistMode, Session};
use ashpd::enumflags2::BitFlags;
use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::{Point, Rect};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use spa::pod::Pod;

use crate::error::{backend, invalid_input};

use super::persistence::{RestoreTokenKind, RestoreTokenStore};
use super::request::{run, session_connection};

const PIPEWIRE_FRAME_TIMEOUT: Duration = Duration::from_secs(5);
// NOTICE: GNOME's mutter honors the negotiated `maxFramerate` and re-records a
// frame skipped by that limit once the interval passes
// (`maybe_schedule_follow_up_frame` in `src/backends/meta-screen-cast-stream-src.c`).
// The cap therefore bounds the idle copy cost without losing the final screen
// state, and the refresh wait must exceed one capped interval so that a
// follow-up frame still lands inside it.
const PIPEWIRE_MAX_FRAMERATE: u32 = 10;
const PIPEWIRE_REFRESH_WAIT: Duration = Duration::from_millis(150);

#[derive(Debug)]
pub struct ScreenCastFrame {
  pub stream: ScreenCastStream,
  pub image: image::RgbaImage,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScreenCastStream {
  pub id: u32,
  pub position: Option<(i32, i32)>,
  pub size: Option<(i32, i32)>,
  pub source_type: Option<u32>,
  pub mapping_id: Option<String>,
}

impl ScreenCastStream {
  pub fn logical_rect(&self) -> Option<Rect> {
    let (x, y) = self.position?;
    let (width, height) = self.size?;
    if width <= 0 || height <= 0 {
      return None;
    }
    Some(Rect::new(f64::from(x), f64::from(y), f64::from(width), f64::from(height)))
  }

  pub fn contains(&self, point: Point) -> bool {
    self.logical_rect().is_some_and(|rect| {
      point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x <= rect.origin.x + rect.size.width
        && point.y <= rect.origin.y + rect.size.height
    })
  }

  pub fn local_point(&self, point: Point) -> DriverResult<Point> {
    let rect = self.logical_rect().ok_or_else(|| backend("screencast stream is missing logical position/size"))?;
    if !self.contains(point) {
      return Err(invalid_input(format!("point {:?} is outside screencast stream {:?}", point, rect)));
    }
    Ok(Point::new(point.x - rect.origin.x, point.y - rect.origin.y))
  }
}

// Map Portal stream metadata to AUV's logical desktop geometry. PipeWire serial
// metadata has no AUV consumer; connection routing uses the advertised node ID.
impl From<&Stream> for ScreenCastStream {
  fn from(stream: &Stream) -> Self {
    Self {
      id: stream.pipe_wire_node_id(),
      position: stream.position(),
      size: stream.size(),
      source_type: stream.source_type().map(|source| source as u32),
      mapping_id: stream.mapping_id().or(stream.id()).map(str::to_owned),
    }
  }
}

#[derive(Debug)]
pub struct ScreenCastSession {
  screencast: Screencast,
  session: Session<Screencast>,
  streams: Vec<ScreenCastStream>,
  receivers: FrameReceiverPool,
}

impl ScreenCastSession {
  pub fn open_monitor(restore_tokens: Option<&RestoreTokenStore>, app_id: Option<&ashpd::AppID>) -> DriverResult<Self> {
    let connection = session_connection(app_id)?;
    let screencast = run("open ScreenCast", Screencast::with_connection(connection))?;
    let session = run("create screencast session", screencast.create_session(Default::default()))?;
    let mut capture = Self {
      screencast,
      session,
      streams: Vec::new(),
      receivers: FrameReceiverPool::default(),
    };
    let persistent = restore_tokens.is_some() && capture.screencast.version() >= 4;
    let start = |restore: Option<&str>| {
      run("start screencast session", async {
        let mut options = SelectSourcesOptions::default()
          .set_sources(BitFlags::from(SourceType::Monitor))
          .set_multiple(true)
          .set_cursor_mode(CursorMode::Hidden);
        if persistent {
          options = options.set_persist_mode(PersistMode::ExplicitlyRevoked).set_restore_token(restore);
        }
        capture.screencast.select_sources(&capture.session, options).await?.response()?;
        capture.screencast.start(&capture.session, None, Default::default()).await?.response()
      })
    };
    let selected = if let Some(store) = restore_tokens.filter(|_| persistent) {
      store.rotate(RestoreTokenKind::ScreenCast, |current| {
        let selected = start(current)?;
        let replacement = selected.restore_token().map(str::to_owned);
        Ok((selected, replacement))
      })?
    } else {
      start(None)?
    };
    capture.streams = selected.streams().iter().map(ScreenCastStream::from).collect();
    if capture.streams.is_empty() {
      return Err(backend("screencast portal started without streams"));
    }
    Ok(capture)
  }

  pub fn capture_monitor_frame(&mut self, target_bounds: Option<Rect>) -> DriverResult<ScreenCastFrame> {
    let stream = select_stream(&self.streams, target_bounds)?.clone();
    let image = self.receivers.capture(stream.id, || {
      let fd = run("open PipeWire remote", self.screencast.open_pipe_wire_remote(&self.session, Default::default()))?;
      Ok(Box::new(PipeWireFrameReceiver::open(fd, stream.id)?))
    })?;
    Ok(ScreenCastFrame { stream, image })
  }
}

impl Drop for ScreenCastSession {
  fn drop(&mut self) {
    self.receivers.clear();
    let _ = run("close screencast session", self.session.close());
  }
}

fn select_stream(streams: &[ScreenCastStream], target_bounds: Option<Rect>) -> DriverResult<&ScreenCastStream> {
  if let Some(target_bounds) = target_bounds {
    return streams
      .iter()
      .find(|stream| stream.logical_rect().is_some_and(|rect| rect_contains_rect(rect, target_bounds)))
      .ok_or_else(|| backend(format!("no screencast stream contains target bounds {:?}; streams={streams:?}", target_bounds)));
  }
  streams.first().ok_or_else(|| backend("screencast start response contained no streams"))
}

fn rect_contains_rect(container: Rect, candidate: Rect) -> bool {
  candidate.origin.x >= container.origin.x
    && candidate.origin.y >= container.origin.y
    && candidate.origin.x + candidate.size.width <= container.origin.x + container.size.width
    && candidate.origin.y + candidate.size.height <= container.origin.y + container.size.height
}

struct PipeWireCaptureState {
  format: spa::param::video::VideoInfoRaw,
  latest: Rc<RefCell<LatestFrame>>,
  pending: Rc<RefCell<Option<PendingFrameRequest>>>,
  terminal_error: Rc<RefCell<Option<String>>>,
}

type WorkerFrameResult = Result<Arc<image::RgbaImage>, String>;

struct PendingFrameRequest {
  sender: mpsc::SyncSender<WorkerFrameResult>,
  stale_after: Option<Instant>,
}

/// Newest frame the stream delivered.
///
/// Frames that arrive while no capture waits keep only a compact raw copy (one
/// memcpy, no pixel conversion). Conversion happens when a capture needs the
/// frame. Invariant: `decoded`, when present, is the newest frame; storing a
/// raw frame clears it.
#[derive(Default)]
struct LatestFrame {
  raw: Option<RawFrame>,
  decoded: Option<Arc<image::RgbaImage>>,
}

struct RawFrame {
  bytes: Vec<u8>,
  layout: FrameLayout,
}

impl LatestFrame {
  fn is_empty(&self) -> bool {
    self.raw.is_none() && self.decoded.is_none()
  }

  /// Copies the frame region of a mapped buffer, reusing the previous allocation.
  fn store_raw(&mut self, source: &[u8], layout: FrameLayout) {
    let mut bytes = self.raw.take().map(|raw| raw.bytes).unwrap_or_default();
    bytes.clear();
    bytes.extend_from_slice(&source[layout.offset..layout.end()]);
    self.raw = Some(RawFrame {
      bytes,
      layout: FrameLayout {
        offset: 0,
        ..layout
      },
    });
    self.decoded = None;
  }

  fn store_decoded(&mut self, image: Arc<image::RgbaImage>) {
    self.decoded = Some(image);
  }

  fn clear(&mut self) {
    self.raw = None;
    self.decoded = None;
  }

  /// The newest frame as RGBA, converting a raw copy at most once.
  fn image(&mut self) -> Option<DriverResult<Arc<image::RgbaImage>>> {
    if let Some(image) = &self.decoded {
      return Some(Ok(Arc::clone(image)));
    }
    let raw = self.raw.as_ref()?;
    Some(raw.layout.convert(&raw.bytes).map(|image| {
      let image = Arc::new(image);
      self.decoded = Some(Arc::clone(&image));
      image
    }))
  }
}

fn take_stale_frame_response(
  pending: &RefCell<Option<PendingFrameRequest>>,
  latest: &RefCell<LatestFrame>,
  now: Instant,
) -> Option<(mpsc::SyncSender<WorkerFrameResult>, WorkerFrameResult)> {
  let is_stale = pending.borrow().as_ref().and_then(|request| request.stale_after).is_some_and(|deadline| now >= deadline);
  if !is_stale {
    return None;
  }
  let image = latest.borrow_mut().image()?.map_err(|error| error.to_string());
  let sender = pending.borrow_mut().take()?.sender;
  Some((sender, image))
}

trait FrameReceiver: Send {
  fn capture_frame(&mut self) -> DriverResult<image::RgbaImage>;
}

#[derive(Default)]
struct FrameReceiverPool {
  receivers: HashMap<u32, Box<dyn FrameReceiver>>,
}

impl fmt::Debug for FrameReceiverPool {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.debug_struct("FrameReceiverPool").field("stream_ids", &self.receivers.keys()).finish()
  }
}

impl FrameReceiverPool {
  fn capture(&mut self, stream_id: u32, create: impl FnOnce() -> DriverResult<Box<dyn FrameReceiver>>) -> DriverResult<image::RgbaImage> {
    if let std::collections::hash_map::Entry::Vacant(entry) = self.receivers.entry(stream_id) {
      entry.insert(create()?);
    }
    let result = self.receivers.get_mut(&stream_id).expect("receiver was inserted above").capture_frame();
    if result.is_err() {
      self.receivers.remove(&stream_id);
    }
    result
  }

  fn clear(&mut self) {
    self.receivers.clear();
  }
}

enum PipeWireWorkerCommand {
  Capture(mpsc::SyncSender<WorkerFrameResult>),
  Stop,
}

struct PipeWireFrameReceiver {
  commands: mpsc::Sender<PipeWireWorkerCommand>,
  worker: Option<thread::JoinHandle<()>>,
}

impl fmt::Debug for PipeWireFrameReceiver {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.debug_struct("PipeWireFrameReceiver").finish_non_exhaustive()
  }
}

impl PipeWireFrameReceiver {
  fn open(fd: StdOwnedFd, node_id: u32) -> DriverResult<Self> {
    let (commands, command_receiver) = mpsc::channel();
    let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
    let worker = thread::Builder::new()
      .name(format!("auv-pipewire-{node_id}"))
      .spawn(move || {
        if let Err(error) = run_pipewire_receiver(fd, node_id, command_receiver, &ready_sender) {
          let _ = ready_sender.try_send(Err(error.to_string()));
        }
      })
      .map_err(|error| backend(format!("failed to start PipeWire capture worker: {error}")))?;
    let mut receiver = Self {
      commands,
      worker: Some(worker),
    };
    match ready_receiver.recv_timeout(PIPEWIRE_FRAME_TIMEOUT) {
      Ok(Ok(())) => Ok(receiver),
      Ok(Err(error)) => {
        receiver.stop();
        Err(backend(error))
      }
      Err(mpsc::RecvTimeoutError::Timeout) => {
        receiver.stop();
        Err(backend("timed out initializing PipeWire screencast receiver"))
      }
      Err(mpsc::RecvTimeoutError::Disconnected) => {
        receiver.stop();
        Err(backend("PipeWire screencast receiver stopped during initialization"))
      }
    }
  }

  fn stop(&mut self) {
    let _ = self.commands.send(PipeWireWorkerCommand::Stop);
    if let Some(worker) = self.worker.take() {
      let _ = worker.join();
    }
  }
}

impl FrameReceiver for PipeWireFrameReceiver {
  fn capture_frame(&mut self) -> DriverResult<image::RgbaImage> {
    let (frame_sender, frame_receiver) = mpsc::sync_channel(1);
    self.commands.send(PipeWireWorkerCommand::Capture(frame_sender)).map_err(|_| backend("PipeWire screencast receiver stopped"))?;
    match frame_receiver.recv_timeout(PIPEWIRE_FRAME_TIMEOUT) {
      Ok(Ok(image)) => Ok((*image).clone()),
      Ok(Err(error)) => Err(backend(error)),
      Err(mpsc::RecvTimeoutError::Timeout) => Err(backend("timed out waiting for PipeWire screencast frame")),
      Err(mpsc::RecvTimeoutError::Disconnected) => Err(backend("PipeWire screencast receiver stopped while waiting for a frame")),
    }
  }
}

impl Drop for PipeWireFrameReceiver {
  fn drop(&mut self) {
    self.stop();
  }
}

fn run_pipewire_receiver(
  fd: StdOwnedFd,
  node_id: u32,
  commands: mpsc::Receiver<PipeWireWorkerCommand>,
  ready: &mpsc::SyncSender<Result<(), String>>,
) -> DriverResult<()> {
  // TODO(pipewire-serial-target): ScreenCast v6 deprecates reusable numeric
  // node IDs in favor of `pipewire-serial` plus PW_KEY_TARGET_OBJECT. Keep the
  // current node ID until the minimum portal/PipeWire compatibility boundary
  // is explicitly raised and tested.
  let mainloop = pw::main_loop::MainLoop::new(None).map_err(|error| backend(format!("failed to create PipeWire mainloop: {error}")))?;
  let context = pw::context::Context::new(&mainloop).map_err(|error| backend(format!("failed to create PipeWire context: {error}")))?;
  let core = context.connect_fd(fd, None).map_err(|error| backend(format!("failed to connect to portal PipeWire remote: {error}")))?;
  let latest = Rc::new(RefCell::new(LatestFrame::default()));
  let pending = Rc::new(RefCell::new(None));
  let terminal_error = Rc::new(RefCell::new(None));
  let state = PipeWireCaptureState {
    format: Default::default(),
    latest: Rc::clone(&latest),
    pending: Rc::clone(&pending),
    terminal_error: Rc::clone(&terminal_error),
  };
  let stream = pw::stream::Stream::new(
    &core,
    "auv-screen-capture",
    properties! {
      *pw::keys::MEDIA_TYPE => "Video",
      *pw::keys::MEDIA_CATEGORY => "Capture",
      *pw::keys::MEDIA_ROLE => "Screen",
    },
  )
  .map_err(|error| backend(format!("failed to create PipeWire stream: {error}")))?;
  let _listener = stream
    .add_local_listener_with_user_data(state)
    .state_changed(|_, state, _, new| {
      if let pw::stream::StreamState::Error(error) = new {
        let error = format!("PipeWire stream error: {error}");
        *state.terminal_error.borrow_mut() = Some(error.clone());
        if let Some(pending) = state.pending.borrow_mut().take() {
          let _ = pending.sender.send(Err(error));
        }
      }
    })
    .param_changed(|_, state, id, param| {
      let Some(param) = param else {
        return;
      };
      if id != spa::param::ParamType::Format.as_raw() {
        return;
      }
      let Ok((media_type, media_subtype)) = spa::param::format_utils::parse_format(param) else {
        *state.terminal_error.borrow_mut() = Some("failed to parse PipeWire stream format".to_string());
        return;
      };
      if media_type != spa::param::format::MediaType::Video || media_subtype != spa::param::format::MediaSubtype::Raw {
        *state.terminal_error.borrow_mut() = Some(format!("unsupported PipeWire stream media type {media_type:?}/{media_subtype:?}"));
        return;
      }
      if let Err(error) = state.format.parse(param) {
        *state.terminal_error.borrow_mut() = Some(format!("failed to parse PipeWire raw video format: {error}"));
      }
    })
    .process(|stream, state| {
      let pending = state.pending.borrow_mut().take();
      let Some(mut buffer) = stream.dequeue_buffer() else {
        if let Some(pending) = pending {
          *state.pending.borrow_mut() = Some(pending);
        }
        return;
      };
      let datas = buffer.datas_mut();
      let frame = match datas.first_mut() {
        Some(data) => mapped_frame(data, state.format),
        None => Err(backend("PipeWire frame contained no data planes")),
      };
      if pending.is_none() && !state.latest.borrow().is_empty() {
        // Keep a raw copy of frames that arrive while nobody is waiting. The
        // stream only sends frames on damage, so a dropped frame could be the
        // last one before the screen goes static; a later capture would then
        // return an outdated image. Conversion waits until a capture asks.
        match frame {
          Ok((source, layout)) => state.latest.borrow_mut().store_raw(source, layout),
          // An unreadable newest frame must not leave an older one looking current.
          Err(_) => state.latest.borrow_mut().clear(),
        }
        return;
      }
      match frame.and_then(|(source, layout)| layout.convert(source)) {
        Ok(image) => {
          let image = Arc::new(image);
          state.latest.borrow_mut().store_decoded(Arc::clone(&image));
          if let Some(pending) = pending {
            let _ = pending.sender.send(Ok(image));
          }
        }
        Err(error) => {
          if let Some(pending) = pending {
            let _ = pending.sender.send(Err(error.to_string()));
          }
        }
      }
    })
    .register()
    .map_err(|error| backend(format!("failed to register PipeWire stream listener: {error}")))?;

  let enum_format = pipewire_raw_video_format_param();
  let mut params = [Pod::from_bytes(&enum_format).ok_or_else(|| backend("failed to build PipeWire raw video format param"))?];
  stream
    .connect(
      spa::utils::Direction::Input,
      Some(node_id),
      pw::stream::StreamFlags::AUTOCONNECT | pw::stream::StreamFlags::MAP_BUFFERS,
      &mut params,
    )
    .map_err(|error| backend(format!("failed to connect PipeWire stream {node_id}: {error}")))?;

  ready.send(Ok(())).map_err(|_| backend("PipeWire receiver initializer stopped waiting"))?;
  loop {
    match commands.try_recv() {
      Ok(PipeWireWorkerCommand::Capture(sender)) => {
        if let Some(error) = terminal_error.borrow().clone() {
          let _ = sender.send(Err(error));
        } else if pending.borrow().is_some() {
          let _ = sender.send(Err("PipeWire receiver already has a pending frame request".to_string()));
        } else {
          *pending.borrow_mut() = Some(PendingFrameRequest {
            sender,
            stale_after: (!latest.borrow().is_empty()).then(|| Instant::now() + PIPEWIRE_REFRESH_WAIT),
          });
        }
      }
      Ok(PipeWireWorkerCommand::Stop) | Err(mpsc::TryRecvError::Disconnected) => break,
      Err(mpsc::TryRecvError::Empty) => {}
    }
    mainloop.loop_().iterate(Duration::from_millis(20));
    if let Some((sender, image)) = take_stale_frame_response(&pending, &latest, Instant::now()) {
      let _ = sender.send(image);
    }
  }
  Ok(())
}

fn pipewire_raw_video_format_param() -> Vec<u8> {
  let object = spa::pod::object!(
    spa::utils::SpaTypes::ObjectParamFormat,
    spa::param::ParamType::EnumFormat,
    spa::pod::property!(spa::param::format::FormatProperties::MediaType, Id, spa::param::format::MediaType::Video),
    spa::pod::property!(spa::param::format::FormatProperties::MediaSubtype, Id, spa::param::format::MediaSubtype::Raw),
    spa::pod::property!(
      spa::param::format::FormatProperties::VideoFormat,
      Choice,
      Enum,
      Id,
      spa::param::video::VideoFormat::RGBx,
      spa::param::video::VideoFormat::RGBx,
      spa::param::video::VideoFormat::RGBA,
      spa::param::video::VideoFormat::BGRx,
      spa::param::video::VideoFormat::BGRA,
      spa::param::video::VideoFormat::xRGB,
      spa::param::video::VideoFormat::RGB,
      spa::param::video::VideoFormat::BGR,
    ),
    spa::pod::property!(
      spa::param::format::FormatProperties::VideoSize,
      Choice,
      Range,
      Rectangle,
      spa::utils::Rectangle {
        width: 1920,
        height: 1080
      },
      spa::utils::Rectangle {
        width: 1,
        height: 1
      },
      spa::utils::Rectangle {
        width: 8192,
        height: 8192
      }
    ),
    spa::pod::property!(
      spa::param::format::FormatProperties::VideoFramerate,
      Choice,
      Range,
      Fraction,
      spa::utils::Fraction { num: 30, denom: 1 },
      spa::utils::Fraction { num: 0, denom: 1 },
      spa::utils::Fraction { num: 120, denom: 1 }
    ),
    spa::pod::property!(
      spa::param::format::FormatProperties::VideoMaxFramerate,
      Choice,
      Range,
      Fraction,
      spa::utils::Fraction {
        num: PIPEWIRE_MAX_FRAMERATE,
        denom: 1
      },
      spa::utils::Fraction { num: 1, denom: 1 },
      spa::utils::Fraction {
        num: PIPEWIRE_MAX_FRAMERATE,
        denom: 1
      }
    ),
  );
  spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &spa::pod::Value::Object(object))
    .expect("PipeWire format pod serialization should be valid")
    .0
    .into_inner()
}

/// Validated geometry of one frame inside a mapped buffer.
#[derive(Clone, Copy, Debug)]
struct FrameLayout {
  offset: usize,
  stride: usize,
  width: usize,
  height: usize,
  bytes_per_pixel: usize,
  format: spa::param::video::VideoFormat,
}

impl FrameLayout {
  fn new(format: spa::param::video::VideoInfoRaw, offset: u32, stride: i32, available: usize) -> DriverResult<Self> {
    let size = format.size();
    if size.width == 0 || size.height == 0 {
      return Err(backend("PipeWire stream reported empty video size"));
    }
    let video_format = format.format();
    let bytes_per_pixel = pipewire_bytes_per_pixel(video_format)?;
    if stride <= 0 {
      return Err(backend(format!("unsupported PipeWire frame stride {stride}")));
    }
    let layout = Self {
      offset: usize::try_from(offset).map_err(|error| backend(format!("invalid PipeWire frame offset: {error}")))?,
      stride: usize::try_from(stride).map_err(|error| backend(format!("invalid PipeWire frame stride: {error}")))?,
      width: usize::try_from(size.width).map_err(|error| backend(format!("invalid PipeWire frame width: {error}")))?,
      height: usize::try_from(size.height).map_err(|error| backend(format!("invalid PipeWire frame height: {error}")))?,
      bytes_per_pixel,
      format: video_format,
    };
    let required = layout.checked_end()?;
    if required > available {
      return Err(backend(format!("PipeWire frame buffer is too small: need {required} bytes, have {available}")));
    }
    Ok(layout)
  }

  fn checked_end(&self) -> DriverResult<usize> {
    let row_bytes = self.width.checked_mul(self.bytes_per_pixel).ok_or_else(|| backend("PipeWire frame row size overflowed"))?;
    self
      .offset
      .checked_add(self.stride.checked_mul(self.height - 1).ok_or_else(|| backend("PipeWire frame stride overflowed"))?)
      .and_then(|start| start.checked_add(row_bytes))
      .ok_or_else(|| backend("PipeWire frame bounds overflowed"))
  }

  /// End of the last pixel row; `new` already checked it fits the buffer.
  fn end(&self) -> usize {
    self.offset + self.stride * (self.height - 1) + self.width * self.bytes_per_pixel
  }

  fn convert(&self, source: &[u8]) -> DriverResult<image::RgbaImage> {
    let image_len = self
      .width
      .checked_mul(self.height)
      .and_then(|pixels| pixels.checked_mul(4))
      .ok_or_else(|| backend("PipeWire RGBA image size overflowed"))?;
    if self.end() > source.len() {
      return Err(backend(format!("PipeWire frame buffer is too small: need {} bytes, have {}", self.end(), source.len())));
    }
    let mut rgba = vec![0; image_len];
    for y in 0..self.height {
      let source_row = self.offset + y * self.stride;
      let dest_row = y * self.width * 4;
      for x in 0..self.width {
        let source_pixel = source_row + x * self.bytes_per_pixel;
        let dest_pixel = dest_row + x * 4;
        write_rgba_pixel(self.format, &source[source_pixel..source_pixel + self.bytes_per_pixel], &mut rgba[dest_pixel..dest_pixel + 4])?;
      }
    }
    image::RgbaImage::from_raw(
      u32::try_from(self.width).expect("width came from u32"),
      u32::try_from(self.height).expect("height came from u32"),
      rgba,
    )
    .ok_or_else(|| backend("failed to build RGBA image from PipeWire frame"))
  }
}

/// The mapped bytes of one buffer plane with its validated frame layout.
fn mapped_frame(data: &mut spa::buffer::Data, format: spa::param::video::VideoInfoRaw) -> DriverResult<(&[u8], FrameLayout)> {
  let (offset, stride) = (data.chunk().offset(), data.chunk().stride());
  let source = data.data().ok_or_else(|| backend("PipeWire frame buffer is not memory-mapped"))?;
  let layout = FrameLayout::new(format, offset, stride, source.len())?;
  Ok((source, layout))
}

fn pipewire_bytes_per_pixel(format: spa::param::video::VideoFormat) -> DriverResult<usize> {
  if format == spa::param::video::VideoFormat::RGB || format == spa::param::video::VideoFormat::BGR {
    Ok(3)
  } else if format == spa::param::video::VideoFormat::RGBx
    || format == spa::param::video::VideoFormat::RGBA
    || format == spa::param::video::VideoFormat::BGRx
    || format == spa::param::video::VideoFormat::BGRA
    || format == spa::param::video::VideoFormat::xRGB
  {
    Ok(4)
  } else {
    Err(backend(format!("unsupported PipeWire raw video format {format:?}")))
  }
}

fn write_rgba_pixel(format: spa::param::video::VideoFormat, source: &[u8], dest: &mut [u8]) -> DriverResult<()> {
  if format == spa::param::video::VideoFormat::RGB {
    dest.copy_from_slice(&[source[0], source[1], source[2], 255]);
  } else if format == spa::param::video::VideoFormat::BGR {
    dest.copy_from_slice(&[source[2], source[1], source[0], 255]);
  } else if format == spa::param::video::VideoFormat::RGBx {
    dest.copy_from_slice(&[source[0], source[1], source[2], 255]);
  } else if format == spa::param::video::VideoFormat::RGBA {
    dest.copy_from_slice(&[source[0], source[1], source[2], source[3]]);
  } else if format == spa::param::video::VideoFormat::BGRx {
    dest.copy_from_slice(&[source[2], source[1], source[0], 255]);
  } else if format == spa::param::video::VideoFormat::BGRA {
    dest.copy_from_slice(&[source[2], source[1], source[0], source[3]]);
  } else if format == spa::param::video::VideoFormat::xRGB {
    dest.copy_from_slice(&[source[1], source[2], source[3], 255]);
  } else {
    return Err(backend(format!("unsupported PipeWire raw video format {format:?}")));
  }
  Ok(())
}

#[cfg(test)]
#[path = "screencast_test.rs"]
mod tests;
