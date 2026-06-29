use std::collections::HashMap;

use auv_driver::error::DriverResult;
use auv_driver::geometry::{CoordinateSpace, Rect};
use auv_driver::window::{Window, WindowRef};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

use crate::error::{backend, invalid_input};

pub const WINDOW_REF_PREFIX: &str = "atspi:";
const REGISTRY_DEST: &str = "org.a11y.atspi.Registry";
const ROOT_PATH: &str = "/org/a11y/atspi/accessible/root";
const ACCESSIBLE_IFACE: &str = "org.a11y.atspi.Accessible";
const COMPONENT_IFACE: &str = "org.a11y.atspi.Component";
const STATE_FOCUSED: u32 = 12;

pub const MAX_DEPTH: usize = 40;
pub const MAX_NODES: usize = 2_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
  pub depth: usize,
  pub path: String,
  pub role: String,
  pub name: String,
  pub description: String,
  pub accessible_id: String,
  pub value: Option<String>,
  pub focused: bool,
  pub bounds: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TreeSnapshot {
  pub window_ref: String,
  pub nodes: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectRef {
  pub dest: String,
  pub path: String,
}

impl ObjectRef {
  pub fn encode(&self) -> String {
    format!("{WINDOW_REF_PREFIX}{}{}", self.dest, self.path)
  }

  pub fn decode(raw: &str) -> DriverResult<Self> {
    let rest = raw.strip_prefix(WINDOW_REF_PREFIX).ok_or_else(|| {
      invalid_input(format!(
        "window reference {raw:?} is not an AT-SPI window reference"
      ))
    })?;
    let Some(path_start) = rest.find('/') else {
      return Err(invalid_input(format!(
        "AT-SPI window reference {raw:?} is missing an object path"
      )));
    };
    let dest = &rest[..path_start];
    let path = &rest[path_start..];
    if dest.is_empty() || path.is_empty() {
      return Err(invalid_input(format!(
        "AT-SPI window reference {raw:?} is incomplete"
      )));
    }
    Ok(Self {
      dest: dest.to_string(),
      path: path.to_string(),
    })
  }
}

#[derive(Clone, Debug)]
struct Application {
  reference: ObjectRef,
  name: String,
  accessible_id: String,
}

#[derive(Clone, Debug)]
struct Accessible {
  reference: ObjectRef,
  name: String,
  description: String,
  accessible_id: String,
  role: String,
  child_count: i32,
  focused: bool,
  bounds: Rect,
}

pub fn list_windows() -> DriverResult<Vec<Window>> {
  let connection = connect()?;
  let applications = root_children(&connection)?
    .into_iter()
    .filter_map(|reference| application(&connection, reference).transpose())
    .collect::<DriverResult<Vec<_>>>()?;
  let mut windows = Vec::new();

  for app in applications {
    for child in children(&connection, &app.reference)? {
      let accessible = accessible(&connection, child)?;
      if accessible.role == "window" && rect_has_area(accessible.bounds) {
        windows.push(window_from_accessible(
          &app,
          &accessible,
          windows.is_empty(),
        ));
      }
    }
  }

  Ok(windows)
}

pub fn snapshot_window(window: &Window) -> DriverResult<TreeSnapshot> {
  let root = ObjectRef::decode(&window.reference.id)?;
  let connection = connect()?;
  let mut nodes = Vec::new();
  walk(&connection, &root, 0, "0".to_string(), &mut nodes)?;
  if nodes.is_empty() {
    return Err(backend("AT-SPI tree snapshot contained no nodes"));
  }
  Ok(TreeSnapshot {
    window_ref: window.reference.id.clone(),
    nodes,
  })
}

pub fn focus_window(window: &Window) -> DriverResult<()> {
  let reference = ObjectRef::decode(&window.reference.id)?;
  let connection = connect()?;
  component_proxy(&connection, &reference)?
    .call_method("GrabFocus", &())
    .map_err(|error| backend(format!("failed to focus AT-SPI window: {error}")))?;
  Ok(())
}

fn connect() -> DriverResult<Connection> {
  let session = Connection::session()
    .map_err(|error| backend(format!("failed to connect to session bus: {error}")))?;
  let bus = Proxy::new(&session, "org.a11y.Bus", "/org/a11y/bus", "org.a11y.Bus")
    .map_err(|error| backend(format!("failed to create AT-SPI bus proxy: {error}")))?;
  let address: String = bus
    .call("GetAddress", &())
    .map_err(|error| backend(format!("failed to get AT-SPI bus address: {error}")))?;
  zbus::blocking::connection::Builder::address(address.as_str())
    .map_err(|error| {
      backend(format!(
        "failed to configure AT-SPI bus connection: {error}"
      ))
    })?
    .build()
    .map_err(|error| backend(format!("failed to connect to AT-SPI bus: {error}")))
}

fn root_children(connection: &Connection) -> DriverResult<Vec<ObjectRef>> {
  let root = ObjectRef {
    dest: REGISTRY_DEST.to_string(),
    path: ROOT_PATH.to_string(),
  };
  children(connection, &root)
}

fn application(connection: &Connection, reference: ObjectRef) -> DriverResult<Option<Application>> {
  let proxy = accessible_proxy(connection, &reference)?;
  let child_count = proxy.get_property::<i32>("ChildCount").unwrap_or_default();
  if child_count <= 0 {
    return Ok(None);
  }
  let name = property_string(&proxy, "Name")?;
  let accessible_id = property_string(&proxy, "AccessibleId").unwrap_or_default();
  drop(proxy);
  Ok(Some(Application {
    name,
    accessible_id,
    reference,
  }))
}

fn accessible(connection: &Connection, reference: ObjectRef) -> DriverResult<Accessible> {
  let proxy = accessible_proxy(connection, &reference)?;
  let role: String = proxy
    .call("GetRoleName", &())
    .map_err(|error| backend(format!("failed to read AT-SPI role: {error}")))?;
  let child_count = proxy.get_property::<i32>("ChildCount").unwrap_or_default();
  let name = property_string(&proxy, "Name").unwrap_or_default();
  let description = property_string(&proxy, "Description").unwrap_or_default();
  let accessible_id = property_string(&proxy, "AccessibleId").unwrap_or_default();
  let focused = state_contains(&proxy, STATE_FOCUSED).unwrap_or(false);
  drop(proxy);
  let bounds = extents(connection, &reference).unwrap_or_default();
  Ok(Accessible {
    name,
    description,
    accessible_id,
    role,
    child_count,
    focused,
    bounds,
    reference,
  })
}

fn children(connection: &Connection, reference: &ObjectRef) -> DriverResult<Vec<ObjectRef>> {
  let proxy = accessible_proxy(connection, reference)?;
  let children: Vec<(String, OwnedObjectPath)> = proxy
    .call("GetChildren", &())
    .map_err(|error| backend(format!("failed to read AT-SPI children: {error}")))?;
  Ok(
    children
      .into_iter()
      .map(|(dest, path)| ObjectRef {
        dest,
        path: path.to_string(),
      })
      .collect(),
  )
}

fn extents(connection: &Connection, reference: &ObjectRef) -> DriverResult<Rect> {
  let proxy = component_proxy(connection, reference)?;
  let (x, y, width, height): (i32, i32, i32, i32) = proxy
    .call("GetExtents", &(0u32,))
    .map_err(|error| backend(format!("failed to read AT-SPI extents: {error}")))?;
  Ok(Rect::new(
    f64::from(x),
    f64::from(y),
    f64::from(width),
    f64::from(height),
  ))
}

fn walk(
  connection: &Connection,
  reference: &ObjectRef,
  depth: usize,
  path: String,
  nodes: &mut Vec<Node>,
) -> DriverResult<()> {
  if nodes.len() >= MAX_NODES {
    return Ok(());
  }
  let node = accessible(connection, reference.clone())?;
  let child_count = node.child_count;
  nodes.push(Node {
    depth,
    path: path.clone(),
    role: node.role,
    name: node.name,
    description: node.description,
    accessible_id: node.accessible_id,
    value: value(connection, reference)
      .ok()
      .filter(|value| !value.is_empty()),
    focused: node.focused,
    bounds: node.bounds,
  });
  if depth >= MAX_DEPTH || child_count <= 0 {
    return Ok(());
  }

  for (index, child) in children(connection, reference)?.into_iter().enumerate() {
    if nodes.len() >= MAX_NODES {
      break;
    }
    walk(
      connection,
      &child,
      depth + 1,
      format!("{path}/{index}"),
      nodes,
    )?;
  }
  Ok(())
}

fn value(connection: &Connection, reference: &ObjectRef) -> DriverResult<String> {
  let proxy = Proxy::new(
    connection,
    reference.dest.as_str(),
    reference.path.as_str(),
    ACCESSIBLE_IFACE,
  )
  .map_err(|error| backend(format!("failed to create AT-SPI Accessible proxy: {error}")))?;
  let attributes: HashMap<String, String> = proxy
    .call("GetAttributes", &())
    .map_err(|error| backend(format!("failed to read AT-SPI attributes: {error}")))?;
  Ok(attributes.get("value").cloned().unwrap_or_default())
}

fn accessible_proxy<'a>(
  connection: &'a Connection,
  reference: &'a ObjectRef,
) -> DriverResult<Proxy<'a>> {
  Proxy::new(
    connection,
    reference.dest.as_str(),
    object_path(reference.path.as_str())?,
    ACCESSIBLE_IFACE,
  )
  .map_err(|error| backend(format!("failed to create AT-SPI Accessible proxy: {error}")))
}

fn component_proxy<'a>(
  connection: &'a Connection,
  reference: &'a ObjectRef,
) -> DriverResult<Proxy<'a>> {
  Proxy::new(
    connection,
    reference.dest.as_str(),
    object_path(reference.path.as_str())?,
    COMPONENT_IFACE,
  )
  .map_err(|error| backend(format!("failed to create AT-SPI Component proxy: {error}")))
}

fn object_path(path: &str) -> DriverResult<ObjectPath<'_>> {
  ObjectPath::try_from(path)
    .map_err(|error| invalid_input(format!("invalid AT-SPI object path {path:?}: {error}")))
}

fn property_string(proxy: &Proxy<'_>, name: &str) -> DriverResult<String> {
  proxy
    .get_property::<String>(name)
    .map_err(|error| backend(format!("failed to read AT-SPI property {name}: {error}")))
}

fn state_contains(proxy: &Proxy<'_>, state: u32) -> DriverResult<bool> {
  let states: Vec<u32> = proxy
    .call("GetState", &())
    .map_err(|error| backend(format!("failed to read AT-SPI state: {error}")))?;
  Ok(states.contains(&state))
}

fn window_from_accessible(app: &Application, accessible: &Accessible, is_main: bool) -> Window {
  Window {
    reference: WindowRef {
      id: accessible.reference.encode(),
    },
    title: non_empty(accessible.name.clone()),
    app_name: non_empty(app.name.clone()),
    // NOTICE(linux-atspi-app-id): AT-SPI exposes Linux desktop application ids
    // through AccessibleId. This reuses the shared app_bundle_id field until a
    // platform-neutral app identity field is approved in auv-driver.
    app_bundle_id: non_empty(app.accessible_id.clone()),
    process_id: None,
    frame: accessible.bounds,
    coordinate_space: CoordinateSpace::Screen,
    is_main,
    is_visible: rect_has_area(accessible.bounds),
  }
}

fn rect_has_area(rect: Rect) -> bool {
  rect.size.width > 0.0 && rect.size.height > 0.0
}

fn non_empty(value: String) -> Option<String> {
  (!value.trim().is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn atspi_window_ref_roundtrips() {
    let reference = ObjectRef {
      dest: ":1.75".to_string(),
      path: "/org/gnome/Settings/a11y/window".to_string(),
    };

    assert_eq!(ObjectRef::decode(&reference.encode()).unwrap(), reference);
  }

  #[test]
  fn atspi_window_ref_rejects_non_atspi_reference() {
    assert!(ObjectRef::decode("42").is_err());
  }

  #[test]
  fn window_from_accessible_projects_linux_app_id() {
    let app = Application {
      reference: ObjectRef {
        dest: ":1.1".to_string(),
        path: ROOT_PATH.to_string(),
      },
      name: "gnome-control-center".to_string(),
      accessible_id: "org.gnome.Settings".to_string(),
    };
    let accessible = Accessible {
      reference: ObjectRef {
        dest: ":1.1".to_string(),
        path: "/window".to_string(),
      },
      name: "Settings".to_string(),
      description: String::new(),
      accessible_id: "CcWindow".to_string(),
      role: "window".to_string(),
      child_count: 0,
      focused: true,
      bounds: Rect::new(1.0, 2.0, 300.0, 400.0),
    };

    let window = window_from_accessible(&app, &accessible, true);

    assert_eq!(window.title.as_deref(), Some("Settings"));
    assert_eq!(window.app_name.as_deref(), Some("gnome-control-center"));
    assert_eq!(window.app_bundle_id.as_deref(), Some("org.gnome.Settings"));
    assert!(window.reference.id.starts_with(WINDOW_REF_PREFIX));
  }
}
