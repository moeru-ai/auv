use std::collections::HashMap;

use auv_driver::error::DriverResult;
use auv_driver::geometry::{Point, Rect};
use zbus::blocking::Connection;
use zbus::zvariant::{DeserializeDict, OwnedObjectPath, OwnedValue, Type, Value};

use crate::error::{backend, invalid_input};

use super::request::session_request;

const SCREENCAST_INTERFACE: &str = "org.freedesktop.portal.ScreenCast";
const SOURCE_MONITOR: u32 = 1;
const CURSOR_HIDDEN: u32 = 1;

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
    Some(Rect::new(
      f64::from(x),
      f64::from(y),
      f64::from(width),
      f64::from(height),
    ))
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
    let rect = self
      .logical_rect()
      .ok_or_else(|| backend("screencast stream is missing logical position/size"))?;
    if !self.contains(point) {
      return Err(invalid_input(format!(
        "point {:?} is outside screencast stream {:?}",
        point, rect
      )));
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

pub fn select_monitor_sources(
  connection: &Connection,
  session_handle: &OwnedObjectPath,
) -> DriverResult<()> {
  select_sources(connection, session_handle)?;
  Ok(())
}

fn select_sources(connection: &Connection, session_handle: &OwnedObjectPath) -> DriverResult<()> {
  let mut options = HashMap::new();
  options.insert("types", Value::from(SOURCE_MONITOR));
  options.insert("multiple", Value::from(true));
  options.insert("cursor_mode", Value::from(CURSOR_HIDDEN));
  session_request(
    connection,
    SCREENCAST_INTERFACE,
    "SelectSources",
    session_handle,
    options,
  )?;
  Ok(())
}

pub fn decode_streams(
  results: &HashMap<String, OwnedValue>,
) -> DriverResult<Vec<ScreenCastStream>> {
  let Some(value) = results.get("streams") else {
    return Err(backend("screencast start response missing streams"));
  };
  let streams =
    <Vec<(u32, StartStreamProperties)>>::try_from(value.try_clone().map_err(|error| {
      backend(format!(
        "failed to clone screencast stream metadata: {error}"
      ))
    })?)
    .map_err(|error| {
      backend(format!(
        "failed to decode screencast stream metadata: {error}"
      ))
    })?;
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

    let point = stream
      .local_point(Point::new(120.0, 80.0))
      .expect("point maps into stream");

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
}
