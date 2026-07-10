use std::cell::RefCell;
use std::collections::HashMap;
use std::os::fd::OwnedFd as StdOwnedFd;
use std::rc::Rc;
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

use super::request::{close_session, create_session, portal_proxy, response_signal, session_connection, session_request, wait_response};

const SCREENCAST_INTERFACE: &str = "org.freedesktop.portal.ScreenCast";
const SOURCE_MONITOR: u32 = 1;
const SOURCE_WINDOW: u32 = 2;
const CURSOR_HIDDEN: u32 = 1;
const PIPEWIRE_FRAME_TIMEOUT: Duration = Duration::from_secs(5);

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
  select_sources(connection, session_handle, SOURCE_MONITOR, true)?;
  Ok(())
}

pub fn select_window_sources(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<()> {
  select_sources(connection, session_handle, SOURCE_WINDOW, false)?;
  Ok(())
}

fn select_sources(connection: &Connection, session_handle: &OwnedObjectPath, source_type: u32, multiple: bool) -> DriverResult<()> {
  let mut options = HashMap::new();
  options.insert("types", Value::from(source_type));
  options.insert("multiple", Value::from(multiple));
  options.insert("cursor_mode", Value::from(CURSOR_HIDDEN));
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
}

impl ScreenCastSession {
  pub fn open_monitor() -> DriverResult<Self> {
    let connection = session_connection()?;
    let session_handle = create_session(&connection, SCREENCAST_INTERFACE)?;
    match start_session(connection, session_handle, select_monitor_sources) {
      Ok(session) => Ok(session),
      Err(error) => Err(error),
    }
  }

  pub fn open_window() -> DriverResult<Self> {
    let connection = session_connection()?;
    let session_handle = create_session(&connection, SCREENCAST_INTERFACE)?;
    match start_session(connection, session_handle, select_window_sources) {
      Ok(session) => Ok(session),
      Err(error) => Err(error),
    }
  }

  pub fn capture_monitor_frame(&mut self, target_bounds: Option<Rect>) -> DriverResult<ScreenCastFrame> {
    let stream = select_stream(&self.streams, target_bounds)?.clone();
    let fd = open_pipewire_remote(&self.connection, &self.session_handle)?;
    let image = read_pipewire_frame(fd.into(), stream.id)?;
    Ok(ScreenCastFrame { stream, image })
  }

  pub fn capture_window_frame(&mut self) -> DriverResult<ScreenCastFrame> {
    let stream = select_window_stream(&self.streams)?.clone();
    let fd = open_pipewire_remote(&self.connection, &self.session_handle)?;
    let image = read_pipewire_frame(fd.into(), stream.id)?;
    Ok(ScreenCastFrame { stream, image })
  }
}

impl Drop for ScreenCastSession {
  fn drop(&mut self) {
    let _ = close_session(&self.connection, &self.session_handle);
  }
}

fn start_session(
  connection: Connection,
  session_handle: OwnedObjectPath,
  select_sources: fn(&Connection, &OwnedObjectPath) -> DriverResult<()>,
) -> DriverResult<ScreenCastSession> {
  let result = select_sources(&connection, &session_handle)
    .and_then(|()| start_screencast(&connection, &session_handle))
    .and_then(|results| decode_streams(&results));
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
  })
}

pub fn capture_window_frame() -> DriverResult<ScreenCastFrame> {
  let mut session = ScreenCastSession::open_window()?;
  session.capture_window_frame()
}

fn start_screencast(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<HashMap<String, OwnedValue>> {
  let handle_token = super::request::portal_token("start");
  let request = super::request::portal_request_proxy(connection, &handle_token)?;
  let mut responses = response_signal(&request, SCREENCAST_INTERFACE, "Start")?;
  let proxy = portal_proxy(connection, SCREENCAST_INTERFACE)?;
  let mut options = HashMap::new();
  options.insert("handle_token", Value::from(handle_token.as_str()));
  proxy
    .call_method("Start", &(session_handle, "", options))
    .map_err(|error| backend(format!("failed to start screencast portal session: {error}")))?;
  wait_response(&mut responses, SCREENCAST_INTERFACE, "Start")
}

fn open_pipewire_remote(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<ZbusOwnedFd> {
  let proxy = portal_proxy(connection, SCREENCAST_INTERFACE)?;
  let options: HashMap<&str, Value<'_>> = HashMap::new();
  proxy
    .call("OpenPipeWireRemote", &(session_handle, options))
    .map_err(|error| backend(format!("failed to open portal PipeWire remote: {error}")))
}

fn select_stream<'a>(streams: &'a [ScreenCastStream], target_bounds: Option<Rect>) -> DriverResult<&'a ScreenCastStream> {
  if let Some(target_bounds) = target_bounds {
    return streams
      .iter()
      .find(|stream| stream.logical_rect().is_some_and(|rect| rect_contains_rect(rect, target_bounds)))
      .ok_or_else(|| backend(format!("no screencast stream contains target bounds {:?}; streams={streams:?}", target_bounds)));
  }
  streams.first().ok_or_else(|| backend("screencast start response contained no streams"))
}

fn select_window_stream(streams: &[ScreenCastStream]) -> DriverResult<&ScreenCastStream> {
  streams
    .iter()
    .find(|stream| stream.source_type == Some(SOURCE_WINDOW))
    .or_else(|| streams.first())
    .ok_or_else(|| backend("screencast window source response contained no streams"))
}

fn rect_contains_rect(container: Rect, candidate: Rect) -> bool {
  candidate.origin.x >= container.origin.x
    && candidate.origin.y >= container.origin.y
    && candidate.origin.x + candidate.size.width <= container.origin.x + container.size.width
    && candidate.origin.y + candidate.size.height <= container.origin.y + container.size.height
}

struct PipeWireCaptureState {
  format: spa::param::video::VideoInfoRaw,
  result: Rc<RefCell<Option<DriverResult<image::RgbaImage>>>>,
}

fn read_pipewire_frame(fd: StdOwnedFd, node_id: u32) -> DriverResult<image::RgbaImage> {
  pw::init();
  let mainloop = pw::main_loop::MainLoop::new(None).map_err(|error| backend(format!("failed to create PipeWire mainloop: {error}")))?;
  let context = pw::context::Context::new(&mainloop).map_err(|error| backend(format!("failed to create PipeWire context: {error}")))?;
  let core = context.connect_fd(fd, None).map_err(|error| backend(format!("failed to connect to portal PipeWire remote: {error}")))?;
  let result = Rc::new(RefCell::new(None));
  let state = PipeWireCaptureState {
    format: Default::default(),
    result: Rc::clone(&result),
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
        *state.result.borrow_mut() = Some(Err(backend(format!("PipeWire stream error: {error}"))));
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
        *state.result.borrow_mut() = Some(Err(backend("failed to parse PipeWire stream format")));
        return;
      };
      if media_type != spa::param::format::MediaType::Video || media_subtype != spa::param::format::MediaSubtype::Raw {
        *state.result.borrow_mut() = Some(Err(backend(format!("unsupported PipeWire stream media type {media_type:?}/{media_subtype:?}"))));
        return;
      }
      if let Err(error) = state.format.parse(param) {
        *state.result.borrow_mut() = Some(Err(backend(format!("failed to parse PipeWire raw video format: {error}"))));
      }
    })
    .process(|stream, state| {
      if state.result.borrow().is_some() {
        return;
      }
      let Some(mut buffer) = stream.dequeue_buffer() else {
        return;
      };
      let datas = buffer.datas_mut();
      let Some(data) = datas.first_mut() else {
        return;
      };
      let frame = decode_pipewire_frame(data, state.format);
      *state.result.borrow_mut() = Some(frame);
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

  let deadline = Instant::now() + PIPEWIRE_FRAME_TIMEOUT;
  while result.borrow().is_none() && Instant::now() < deadline {
    mainloop.loop_().iterate(Duration::from_millis(100));
  }
  result.borrow_mut().take().unwrap_or_else(|| Err(backend("timed out waiting for PipeWire screencast frame")))
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
mod tests {
  use super::*;

  #[test]
  fn stream_maps_global_point_to_local_point() {
    let stream = ScreenCastStream {
      id: 7,
      position: Some((100, 50)),
      size: Some((800, 600)),
      source_type: Some(SOURCE_MONITOR),
      mapping_id: None,
      pipewire_serial: None,
    };

    let point = stream.local_point(Point::new(120.0, 80.0)).expect("point maps into stream");

    assert_eq!(point, Point::new(20.0, 30.0));
  }

  #[test]
  fn stream_rejects_outside_point() {
    let stream = ScreenCastStream {
      id: 7,
      position: Some((100, 50)),
      size: Some((800, 600)),
      source_type: Some(SOURCE_MONITOR),
      mapping_id: None,
      pipewire_serial: None,
    };

    assert!(stream.local_point(Point::new(50.0, 80.0)).is_err());
  }

  #[test]
  fn bgrx_pixel_converts_to_rgba() {
    let mut dest = [0, 0, 0, 0];

    write_rgba_pixel(spa::param::video::VideoFormat::BGRx, &[3, 2, 1, 0], &mut dest).expect("BGRx converts");

    assert_eq!(dest, [1, 2, 3, 255]);
  }

  #[test]
  fn xrgb_pixel_converts_to_rgba() {
    let mut dest = [0, 0, 0, 0];

    write_rgba_pixel(spa::param::video::VideoFormat::xRGB, &[0, 1, 2, 3], &mut dest).expect("xRGB converts");

    assert_eq!(dest, [1, 2, 3, 255]);
  }

  #[test]
  fn window_stream_prefers_window_source_type() {
    let monitor = ScreenCastStream {
      id: 1,
      position: None,
      size: None,
      source_type: Some(SOURCE_MONITOR),
      mapping_id: None,
      pipewire_serial: None,
    };
    let window = ScreenCastStream {
      id: 2,
      position: None,
      size: None,
      source_type: Some(SOURCE_WINDOW),
      mapping_id: None,
      pipewire_serial: None,
    };

    let streams = [monitor, window];
    let selected = select_window_stream(&streams).expect("stream selected");

    assert_eq!(selected.id, 2);
  }
}
