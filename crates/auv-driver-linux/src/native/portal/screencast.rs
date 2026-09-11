use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::os::fd::OwnedFd as StdOwnedFd;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use auv_driver_common::error::DriverResult;
use auv_driver_common::geometry::{Point, Rect};
use pipewire as pw;
use pw::properties::properties;
use pw::spa;
use spa::pod::Pod;
use zbus::blocking::Connection;
use zbus::zvariant::{DeserializeDict, OwnedFd as ZbusOwnedFd, OwnedObjectPath, OwnedValue, Type, Value};

use crate::error::{backend, invalid_input};

use super::persistence::{RestoreTokenKind, RestoreTokenStore};
use super::request::{
  close_session, create_session, interface_version, portal_proxy, response_signal, restore_token, session_connection, session_request,
  wait_response,
};

const SCREENCAST_INTERFACE: &str = "org.freedesktop.portal.ScreenCast";
const SOURCE_MONITOR: u32 = 1;
const CURSOR_HIDDEN: u32 = 1;
const PERSIST_UNTIL_REVOKED: u32 = 2;
const PERSISTENCE_INTERFACE_VERSION: u32 = 4;
const PIPEWIRE_FRAME_TIMEOUT: Duration = Duration::from_secs(5);
const PIPEWIRE_REFRESH_WAIT: Duration = Duration::from_millis(100);

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
  pub pipewire_serial: Option<u64>,
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

#[derive(DeserializeDict, Type, Debug, Value, OwnedValue)]
#[zvariant(signature = "dict")]
struct StartStreamProperties {
  pub id: Option<String>,
  pub position: Option<(i32, i32)>,
  pub size: Option<(i32, i32)>,
  pub source_type: Option<u32>,
  pub mapping_id: Option<String>,
  #[zvariant(rename = "pipewire-serial")]
  pub pipewire_serial: Option<u64>,
}

pub fn select_monitor_sources(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<()> {
  select_sources(connection, session_handle, SOURCE_MONITOR, true, None, false)?;
  Ok(())
}

fn select_sources(
  connection: &Connection,
  session_handle: &OwnedObjectPath,
  source_type: u32,
  multiple: bool,
  restore: Option<&str>,
  persistent: bool,
) -> DriverResult<()> {
  let mut options = HashMap::new();
  options.insert("types", Value::from(source_type));
  options.insert("multiple", Value::from(multiple));
  options.insert("cursor_mode", Value::from(CURSOR_HIDDEN));
  if persistent {
    options.insert("persist_mode", Value::from(PERSIST_UNTIL_REVOKED));
    if let Some(restore) = restore {
      options.insert("restore_token", Value::from(restore));
    }
  }
  session_request(connection, SCREENCAST_INTERFACE, "SelectSources", session_handle, options)?;
  Ok(())
}

pub fn decode_streams(results: &HashMap<String, OwnedValue>) -> DriverResult<Vec<ScreenCastStream>> {
  let Some(value) = results.get("streams") else {
    return Err(backend("screencast start response missing streams"));
  };
  let streams = <Vec<(u32, StartStreamProperties)>>::try_from(
    value.try_clone().map_err(|error| backend(format!("failed to clone screencast stream metadata: {error}")))?,
  )
  .map_err(|error| backend(format!("failed to decode screencast stream metadata: {error}")))?;
  Ok(
    streams
      .into_iter()
      .map(|(id, properties)| ScreenCastStream {
        id,
        position: properties.position,
        size: properties.size,
        source_type: properties.source_type,
        mapping_id: properties.mapping_id.or(properties.id),
        pipewire_serial: properties.pipewire_serial,
      })
      .collect(),
  )
}

#[derive(Debug)]
pub struct ScreenCastSession {
  connection: Connection,
  session_handle: OwnedObjectPath,
  streams: Vec<ScreenCastStream>,
  receivers: FrameReceiverPool,
}

impl ScreenCastSession {
  pub fn open_monitor(restore_tokens: Option<&RestoreTokenStore>) -> DriverResult<Self> {
    let connection = session_connection()?;
    let session_handle = create_session(&connection, SCREENCAST_INTERFACE)?;
    start_session(connection, session_handle, restore_tokens)
  }

  pub fn capture_monitor_frame(&mut self, target_bounds: Option<Rect>) -> DriverResult<ScreenCastFrame> {
    let stream = select_stream(&self.streams, target_bounds)?.clone();
    let image = self.receivers.capture(stream.id, || {
      let fd = open_pipewire_remote(&self.connection, &self.session_handle)?;
      Ok(Box::new(PipeWireFrameReceiver::open(fd.into(), stream.id)?))
    })?;
    Ok(ScreenCastFrame { stream, image })
  }
}

impl Drop for ScreenCastSession {
  fn drop(&mut self) {
    self.receivers.clear();
    let _ = close_session(&self.connection, &self.session_handle);
  }
}

fn start_session(
  connection: Connection,
  session_handle: OwnedObjectPath,
  restore_tokens: Option<&RestoreTokenStore>,
) -> DriverResult<ScreenCastSession> {
  let result = (|| {
    let persistent = restore_tokens.is_some() && interface_version(&connection, SCREENCAST_INTERFACE)? >= PERSISTENCE_INTERFACE_VERSION;
    let results = if let Some(restore_tokens) = restore_tokens.filter(|_| persistent) {
      restore_tokens.rotate(RestoreTokenKind::ScreenCast, |current| {
        select_sources(&connection, &session_handle, SOURCE_MONITOR, true, current, true)?;
        let results = start_screencast(&connection, &session_handle)?;
        let replacement = restore_token(&results, SCREENCAST_INTERFACE)?;
        Ok((results, replacement))
      })?
    } else {
      select_monitor_sources(&connection, &session_handle)?;
      start_screencast(&connection, &session_handle)?
    };
    decode_streams(&results)
  })();
  let streams = match result {
    Ok(streams) => streams,
    Err(error) => {
      let close_result = close_session(&connection, &session_handle);
      return match close_result {
        Ok(()) => Err(error),
        Err(close_error) => Err(backend(format!("{error}; also failed to close screencast portal session: {close_error}"))),
      };
    }
  };
  if streams.is_empty() {
    close_session(&connection, &session_handle)?;
    return Err(backend("screencast portal started without streams"));
  }
  Ok(ScreenCastSession {
    connection,
    session_handle,
    streams,
    receivers: FrameReceiverPool::default(),
  })
}

fn start_screencast(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<HashMap<String, OwnedValue>> {
  let handle_token = super::request::portal_token("start");
  let request = super::request::portal_request_proxy(connection, &handle_token)?;
  let mut responses = response_signal(&request, SCREENCAST_INTERFACE, "Start")?;
  let proxy = portal_proxy(connection, SCREENCAST_INTERFACE)?;
  let mut options = HashMap::new();
  options.insert("handle_token", Value::from(handle_token.as_str()));
  super::request::call_method(&proxy, SCREENCAST_INTERFACE, "Start", &(session_handle, "", options))?;
  wait_response(&mut responses, SCREENCAST_INTERFACE, "Start")
}

fn open_pipewire_remote(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<ZbusOwnedFd> {
  let proxy = portal_proxy(connection, SCREENCAST_INTERFACE)?;
  let options: HashMap<&str, Value<'_>> = HashMap::new();
  let response = super::request::call_method(&proxy, SCREENCAST_INTERFACE, "OpenPipeWireRemote", &(session_handle, options))?;
  response.body().deserialize().map_err(|error| backend(format!("failed to decode portal PipeWire remote: {error}")))
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
  latest: Rc<RefCell<Option<Arc<image::RgbaImage>>>>,
  pending: Rc<RefCell<Option<PendingFrameRequest>>>,
  terminal_error: Rc<RefCell<Option<String>>>,
}

type WorkerFrameResult = Result<Arc<image::RgbaImage>, String>;

struct PendingFrameRequest {
  sender: mpsc::SyncSender<WorkerFrameResult>,
  stale_after: Option<Instant>,
}

fn take_stale_frame_response(
  pending: &RefCell<Option<PendingFrameRequest>>,
  latest: &RefCell<Option<Arc<image::RgbaImage>>>,
  now: Instant,
) -> Option<(mpsc::SyncSender<WorkerFrameResult>, Arc<image::RgbaImage>)> {
  let is_stale = pending.borrow().as_ref().and_then(|request| request.stale_after).is_some_and(|deadline| now >= deadline);
  if !is_stale {
    return None;
  }
  let sender = pending.borrow_mut().take()?.sender;
  let image = latest.borrow().clone()?;
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
  let latest = Rc::new(RefCell::new(None));
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
      if pending.is_none() && state.latest.borrow().is_some() {
        // Dequeue and immediately release frames when nobody is waiting. This
        // keeps the persistent stream live without continuously converting a
        // full RGBA display while AUV is idle.
        return;
      }
      let datas = buffer.datas_mut();
      let Some(data) = datas.first_mut() else {
        if let Some(pending) = pending {
          let _ = pending.sender.send(Err("PipeWire frame contained no data planes".to_string()));
        }
        return;
      };
      match decode_pipewire_frame(data, state.format) {
        Ok(image) => {
          let image = Arc::new(image);
          *state.latest.borrow_mut() = Some(Arc::clone(&image));
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
            stale_after: latest.borrow().as_ref().map(|_| Instant::now() + PIPEWIRE_REFRESH_WAIT),
          });
        }
      }
      Ok(PipeWireWorkerCommand::Stop) | Err(mpsc::TryRecvError::Disconnected) => break,
      Err(mpsc::TryRecvError::Empty) => {}
    }
    mainloop.loop_().iterate(Duration::from_millis(20));
    if let Some((sender, latest)) = take_stale_frame_response(&pending, &latest, Instant::now()) {
      let _ = sender.send(Ok(latest));
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
  );
  spa::pod::serialize::PodSerializer::serialize(std::io::Cursor::new(Vec::new()), &spa::pod::Value::Object(object))
    .expect("PipeWire format pod serialization should be valid")
    .0
    .into_inner()
}

fn decode_pipewire_frame(data: &mut spa::buffer::Data, format: spa::param::video::VideoInfoRaw) -> DriverResult<image::RgbaImage> {
  let size = format.size();
  let width = size.width;
  let height = size.height;
  if width == 0 || height == 0 {
    return Err(backend("PipeWire stream reported empty video size"));
  }
  let video_format = format.format();
  let bytes_per_pixel = pipewire_bytes_per_pixel(video_format)?;
  let chunk = data.chunk();
  let stride = chunk.stride();
  if stride <= 0 {
    return Err(backend(format!("unsupported PipeWire frame stride {stride}")));
  }
  let offset = usize::try_from(chunk.offset()).map_err(|error| backend(format!("invalid PipeWire frame offset: {error}")))?;
  let stride = usize::try_from(stride).map_err(|error| backend(format!("invalid PipeWire frame stride: {error}")))?;
  let width = usize::try_from(width).map_err(|error| backend(format!("invalid PipeWire frame width: {error}")))?;
  let height = usize::try_from(height).map_err(|error| backend(format!("invalid PipeWire frame height: {error}")))?;
  let row_bytes = width.checked_mul(bytes_per_pixel).ok_or_else(|| backend("PipeWire frame row size overflowed"))?;
  let image_len =
    width.checked_mul(height).and_then(|pixels| pixels.checked_mul(4)).ok_or_else(|| backend("PipeWire RGBA image size overflowed"))?;
  let source = data.data().ok_or_else(|| backend("PipeWire frame buffer is not memory-mapped"))?;
  let required = offset
    .checked_add(stride.checked_mul(height.saturating_sub(1)).ok_or_else(|| backend("PipeWire frame stride overflowed"))?)
    .and_then(|start| start.checked_add(row_bytes))
    .ok_or_else(|| backend("PipeWire frame bounds overflowed"))?;
  if required > source.len() {
    return Err(backend(format!("PipeWire frame buffer is too small: need {required} bytes, have {}", source.len())));
  }
  let mut rgba = vec![0; image_len];
  for y in 0..height {
    let source_row = offset + y * stride;
    let dest_row = y * width * 4;
    for x in 0..width {
      let source_pixel = source_row + x * bytes_per_pixel;
      let dest_pixel = dest_row + x * 4;
      write_rgba_pixel(video_format, &source[source_pixel..source_pixel + bytes_per_pixel], &mut rgba[dest_pixel..dest_pixel + 4])?;
    }
  }
  image::RgbaImage::from_raw(u32::try_from(width).expect("width came from u32"), u32::try_from(height).expect("height came from u32"), rgba)
    .ok_or_else(|| backend("failed to build RGBA image from PipeWire frame"))
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
