use auv_driver::display::Display;
use auv_driver::error::DriverResult;
use auv_driver::geometry::{CoordinateSpace, Rect};

use crate::error::{backend, not_found};

const WAYLAND_DISPLAY_BACKEND: &str = "wayland.xdg-output";

#[cfg(target_os = "linux")]
use wayland_client::globals::GlobalListContents;
#[cfg(target_os = "linux")]
use wayland_client::protocol::wl_output::WlOutput;
#[cfg(target_os = "linux")]
use wayland_client::{
  Connection, Dispatch, QueueHandle, delegate_noop,
  protocol::{
    wl_output::{self},
    wl_registry::{self, WlRegistry},
  },
};
#[cfg(target_os = "linux")]
use wayland_protocols::xdg::xdg_output::zv1::client::{
  zxdg_output_manager_v1::ZxdgOutputManagerV1,
  zxdg_output_v1::{self, ZxdgOutputV1},
};

#[derive(Clone, Debug)]
pub struct DisplayTarget {
  pub display: Display,
}

pub fn list_targets() -> DriverResult<Vec<DisplayTarget>> {
  wayland_display_targets()
}

pub fn resolve_target(
  targets: &[DisplayTarget],
  selector: Option<&str>,
) -> DriverResult<DisplayTarget> {
  if let Some(selector) = selector {
    let selector = selector.trim();
    return targets
      .iter()
      .find(|target| {
        target.display.id == selector
          || target
            .display
            .name
            .as_deref()
            .is_some_and(|display_ref| display_ref == selector)
      })
      .cloned()
      .ok_or_else(|| not_found(format!("display {selector:?}")));
  }

  targets
    .iter()
    .find(|target| target.display.is_primary)
    .or_else(|| targets.first())
    .cloned()
    .ok_or_else(|| not_found("primary display"))
}

pub fn resolve_for_region(
  targets: &[DisplayTarget],
  selector: Option<&str>,
  region: Rect,
) -> DriverResult<DisplayTarget> {
  let selected = if selector.is_some() {
    vec![resolve_target(targets, selector)?]
  } else {
    targets.to_vec()
  };
  selected
    .into_iter()
    .find(|target| rect_contains_rect(target.display.frame, region))
    .ok_or_else(|| not_found("display containing region"))
}

pub fn selected_target_or_none(selector: Option<&str>) -> DriverResult<Option<DisplayTarget>> {
  let targets = match list_targets() {
    Ok(targets) => targets,
    Err(error) if selector.is_some() => {
      return Err(backend(format!(
        "failed to resolve selected display via {WAYLAND_DISPLAY_BACKEND}: {error}"
      )));
    }
    Err(_) => return Ok(None),
  };
  resolve_target(&targets, selector).map(Some)
}

fn rect_contains_rect(container: Rect, candidate: Rect) -> bool {
  candidate.origin.x >= container.origin.x
    && candidate.origin.y >= container.origin.y
    && candidate.origin.x + candidate.size.width <= container.origin.x + container.size.width
    && candidate.origin.y + candidate.size.height <= container.origin.y + container.size.height
}

#[cfg(target_os = "linux")]
#[derive(Debug, Default)]
struct WaylandDisplayState {
  outputs: Vec<WaylandOutputState>,
}

#[cfg(target_os = "linux")]
#[derive(Debug)]
struct WaylandOutputState {
  wl_output: WlOutput,
  name: Option<String>,
  description: Option<String>,
  physical_size: Option<(i32, i32)>,
  logical_position: Option<(i32, i32)>,
  logical_size: Option<(i32, i32)>,
  scale: i32,
}

#[cfg(target_os = "linux")]
impl Dispatch<WlRegistry, GlobalListContents> for WaylandDisplayState {
  fn event(
    state: &mut Self,
    registry: &WlRegistry,
    event: wl_registry::Event,
    _: &GlobalListContents,
    _: &Connection,
    qh: &QueueHandle<Self>,
  ) {
    let wl_registry::Event::Global {
      name,
      interface,
      version,
    } = event
    else {
      return;
    };

    if interface != "wl_output" {
      return;
    }
    state.outputs.push(WaylandOutputState {
      wl_output: registry.bind::<WlOutput, _, _>(name, version.min(4), qh, ()),
      name: None,
      description: None,
      physical_size: None,
      logical_position: None,
      logical_size: None,
      scale: 1,
    });
  }
}

#[cfg(target_os = "linux")]
impl Dispatch<WlOutput, ()> for WaylandDisplayState {
  fn event(
    state: &mut Self,
    wl_output: &WlOutput,
    event: wl_output::Event,
    _: &(),
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    let Some(output) = state
      .outputs
      .iter_mut()
      .find(|output| output.wl_output == *wl_output)
    else {
      return;
    };

    match event {
      wl_output::Event::Name { name } => output.name = Some(name),
      wl_output::Event::Description { description } => output.description = Some(description),
      wl_output::Event::Mode { width, height, .. } => {
        output.physical_size = Some((width, height));
      }
      wl_output::Event::Scale { factor } => output.scale = factor,
      _ => {}
    }
  }
}

#[cfg(target_os = "linux")]
delegate_noop!(WaylandDisplayState: ignore ZxdgOutputManagerV1);

#[cfg(target_os = "linux")]
impl Dispatch<ZxdgOutputV1, usize> for WaylandDisplayState {
  fn event(
    state: &mut Self,
    _: &ZxdgOutputV1,
    event: zxdg_output_v1::Event,
    output_index: &usize,
    _: &Connection,
    _: &QueueHandle<Self>,
  ) {
    let Some(output) = state.outputs.get_mut(*output_index) else {
      return;
    };

    match event {
      zxdg_output_v1::Event::LogicalPosition { x, y } => {
        output.logical_position = Some((x, y));
      }
      zxdg_output_v1::Event::LogicalSize { width, height } => {
        output.logical_size = Some((width, height));
      }
      zxdg_output_v1::Event::Name { name } => {
        output.name.get_or_insert(name);
      }
      zxdg_output_v1::Event::Description { description } => {
        output.description.get_or_insert(description);
      }
      _ => {}
    }
  }
}

#[cfg(target_os = "linux")]
fn wayland_display_targets() -> DriverResult<Vec<DisplayTarget>> {
  use wayland_client::globals::registry_queue_init;

  let connection = Connection::connect_to_env()
    .map_err(|error| backend(format!("failed to connect to Wayland compositor: {error}")))?;
  let (globals, mut event_queue) = registry_queue_init::<WaylandDisplayState>(&connection)
    .map_err(|error| backend(format!("failed to initialize Wayland registry: {error}")))?;
  let qh = event_queue.handle();
  let output_manager = globals
    .bind::<ZxdgOutputManagerV1, _, _>(&qh, 3..=3, ())
    .map_err(|error| backend(format!("failed to bind xdg-output manager: {error}")))?;

  let mut state = WaylandDisplayState::default();
  for global in globals
    .contents()
    .clone_list()
    .into_iter()
    .filter(|global| global.interface == "wl_output")
  {
    state.outputs.push(WaylandOutputState {
      wl_output: globals.registry().bind::<WlOutput, _, _>(
        global.name,
        global.version.min(4),
        &qh,
        (),
      ),
      name: None,
      description: None,
      physical_size: None,
      logical_position: None,
      logical_size: None,
      scale: 1,
    });
  }

  event_queue
    .roundtrip(&mut state)
    .map_err(|error| backend(format!("failed to read Wayland output metadata: {error}")))?;

  let xdg_outputs = state
    .outputs
    .iter()
    .enumerate()
    .map(|(index, output)| output_manager.get_xdg_output(&output.wl_output, &qh, index))
    .collect::<Vec<_>>();
  event_queue.roundtrip(&mut state).map_err(|error| {
    backend(format!(
      "failed to read xdg-output logical geometry: {error}"
    ))
  })?;
  for output in xdg_outputs {
    output.destroy();
  }

  if state.outputs.is_empty() {
    return Err(not_found("display"));
  }

  state
    .outputs
    .into_iter()
    .enumerate()
    .map(output_target)
    .collect()
}

fn output_target((index, output): (usize, WaylandOutputState)) -> DriverResult<DisplayTarget> {
  let (x, y) = output
    .logical_position
    .ok_or_else(|| backend("Wayland output is missing xdg-output logical_position"))?;
  let (width, height) = output
    .logical_size
    .ok_or_else(|| backend("Wayland output is missing xdg-output logical_size"))?;
  if width <= 0 || height <= 0 {
    return Err(backend(format!(
      "Wayland output has invalid logical size {width}x{height}"
    )));
  }

  let id = output
    .name
    .unwrap_or_else(|| format!("wayland-output-{index}"));
  // NOTICE(wayland-primary-output): Wayland/xdg-output does not expose a
  // compositor-generic primary-output flag. AUV marks the first advertised
  // output primary only to preserve existing default display resolution.
  let is_primary = index == 0;
  let scale_factor = output
    .physical_size
    .filter(|(physical_width, _)| *physical_width > 0)
    .map(|(physical_width, _)| f64::from(physical_width) / f64::from(width))
    .unwrap_or_else(|| f64::from(output.scale.max(1)));

  Ok(DisplayTarget {
    display: Display {
      id,
      name: output.description,
      frame: Rect::new(
        f64::from(x),
        f64::from(y),
        f64::from(width),
        f64::from(height),
      ),
      coordinate_space: CoordinateSpace::Screen,
      scale_factor,
      is_primary,
      is_builtin: None,
    },
  })
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn display_resolution_prefers_primary() {
    let targets = vec![
      DisplayTarget {
        display: display("left", false),
      },
      DisplayTarget {
        display: display("primary", true),
      },
    ];

    let selected = resolve_target(&targets, None).expect("display resolves");

    assert_eq!(selected.display.id, "primary");
  }

  fn display(id: &str, is_primary: bool) -> Display {
    Display {
      id: id.to_string(),
      name: None,
      frame: Rect::new(0.0, 0.0, 100.0, 100.0),
      coordinate_space: CoordinateSpace::Screen,
      scale_factor: 1.0,
      is_primary,
      is_builtin: None,
    }
  }
}
