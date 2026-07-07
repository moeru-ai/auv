use crate::capture::types::{CaptureContract, CoordinateSpace, DisplayDescriptor, Rect, Scale2D, Size, capture_error};
use crate::types::AuvResult;

#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedRegion {
  pub display_index: usize,
  pub display_local_logical: Rect,
  pub source_global_logical_bounds: Rect,
}

pub fn scale_from_logical_and_physical(logical: &Rect, physical: &Size) -> AuvResult<(Scale2D, Scale2D)> {
  if logical.width <= 0.0 || logical.height <= 0.0 || physical.width <= 0.0 || physical.height <= 0.0 {
    return Err(format!("{}: logical and physical display sizes must be positive", capture_error::BACKEND_FAILED));
  }

  let logical_to_pixel = Scale2D {
    x: physical.width / logical.width,
    y: physical.height / logical.height,
  };
  let pixel_to_logical = Scale2D {
    x: logical.width / physical.width,
    y: logical.height / physical.height,
  };

  Ok((pixel_to_logical, logical_to_pixel))
}

pub fn resolve_display_index(
  displays: &[DisplayDescriptor],
  display_ref: Option<&str>,
  display_id: Option<&str>,
  main: bool,
) -> AuvResult<usize> {
  if let Some(display_ref) = display_ref {
    return displays
      .iter()
      .position(|display| display.display_ref == display_ref)
      .ok_or_else(|| format!("{}: display_ref {} was not reported by the capture backend", capture_error::DISPLAY_NOT_FOUND, display_ref));
  }

  if let Some(display_id) = display_id {
    return displays.iter().position(|display| display.native_display_id == display_id).ok_or_else(|| {
      format!("{}: native display id {} was not reported by the capture backend", capture_error::DISPLAY_NOT_FOUND, display_id)
    });
  }

  if main && let Some(index) = displays.iter().position(|display| display.is_main) {
    return Ok(index);
  }

  if displays.is_empty() {
    return Err(format!("{}: no displays were reported by the capture backend", capture_error::DISPLAY_NOT_FOUND));
  }

  Ok(0)
}

pub fn display_index_for_window(displays: &[DisplayDescriptor], window_bounds: &Rect) -> AuvResult<usize> {
  let containing_displays =
    displays.iter().enumerate().filter(|(_, display)| rect_contains_rect(&display.global_logical_bounds, window_bounds)).collect::<Vec<_>>();

  if containing_displays.len() != 1 {
    return Err(format!("{}: window bounds are not fully contained by exactly one display", capture_error::STALE_WINDOW_REF));
  }

  Ok(containing_displays[0].0)
}

pub fn resolve_region(
  displays: &[DisplayDescriptor],
  input: Rect,
  coordinate_space: CoordinateSpace,
  display_ref: Option<&str>,
  display_id: Option<&str>,
) -> AuvResult<ResolvedRegion> {
  if input.width <= 0.0 || input.height <= 0.0 {
    return Err(format!("{}: capture region size must be positive", capture_error::REGION_OUT_OF_BOUNDS));
  }

  match coordinate_space {
    CoordinateSpace::GlobalLogical => resolve_global_logical_region(displays, input),
    CoordinateSpace::DisplayLogical => {
      require_display_selector(display_ref, display_id, "display_logical")?;
      let display_index = resolve_display_index(displays, display_ref, display_id, false)?;
      resolve_display_logical_region(displays, display_index, input)
    }
    CoordinateSpace::DisplayPhysical => {
      require_display_selector(display_ref, display_id, "display_physical")?;
      let display_index = resolve_display_index(displays, display_ref, display_id, false)?;
      let display = displays.get(display_index).ok_or_else(|| {
        format!("{}: resolved display index {} is missing from the display descriptor list", capture_error::STALE_DISPLAY_REF, display_index)
      })?;
      resolve_display_logical_region(
        displays,
        display_index,
        Rect {
          x: input.x * display.pixel_to_logical_scale.x,
          y: input.y * display.pixel_to_logical_scale.y,
          width: input.width * display.pixel_to_logical_scale.x,
          height: input.height * display.pixel_to_logical_scale.y,
        },
      )
    }
  }
}

pub fn project_capture_pixel_to_global_logical(contract: &CaptureContract, screenshot_x: f64, screenshot_y: f64) -> AuvResult<(f64, f64)> {
  if !screenshot_x.is_finite() || !screenshot_y.is_finite() {
    return Err(format!("{}: screenshot point must be finite", capture_error::COORDINATE_CONTRACT_STALE));
  }
  if screenshot_x < 0.0
    || screenshot_y < 0.0
    || screenshot_x >= contract.screenshot_pixel_size.width
    || screenshot_y >= contract.screenshot_pixel_size.height
  {
    return Err(format!(
      "{}: screenshot point ({:.3}, {:.3}) is outside capture pixels {:.0}x{:.0}",
      capture_error::COORDINATE_CONTRACT_STALE,
      screenshot_x,
      screenshot_y,
      contract.screenshot_pixel_size.width,
      contract.screenshot_pixel_size.height
    ));
  }

  Ok((
    contract.source_global_logical_bounds.x + screenshot_x * contract.pixel_to_logical_scale.x,
    contract.source_global_logical_bounds.y + screenshot_y * contract.pixel_to_logical_scale.y,
  ))
}

fn resolve_global_logical_region(displays: &[DisplayDescriptor], input: Rect) -> AuvResult<ResolvedRegion> {
  let containing_displays =
    displays.iter().enumerate().filter(|(_, display)| rect_contains_rect(&display.global_logical_bounds, &input)).collect::<Vec<_>>();

  if containing_displays.len() != 1 {
    let intersecting_count = displays.iter().filter(|display| rect_intersects_rect(&display.global_logical_bounds, &input)).count();
    if intersecting_count > 1 {
      return Err(format!("{}: global logical region crosses display boundaries", capture_error::REGION_CROSSES_DISPLAYS));
    }
    return Err(format!("{}: global logical region is not fully contained by one display", capture_error::REGION_OUT_OF_BOUNDS));
  }

  let (display_index, display) = containing_displays[0];
  Ok(ResolvedRegion {
    display_index,
    display_local_logical: Rect {
      x: input.x - display.global_logical_bounds.x,
      y: input.y - display.global_logical_bounds.y,
      width: input.width,
      height: input.height,
    },
    source_global_logical_bounds: input,
  })
}

fn resolve_display_logical_region(displays: &[DisplayDescriptor], display_index: usize, input: Rect) -> AuvResult<ResolvedRegion> {
  let display = displays.get(display_index).ok_or_else(|| {
    format!("{}: resolved display index {} is missing from the display descriptor list", capture_error::STALE_DISPLAY_REF, display_index)
  })?;
  let display_local_bounds = Rect {
    x: 0.0,
    y: 0.0,
    width: display.global_logical_bounds.width,
    height: display.global_logical_bounds.height,
  };
  if !rect_contains_rect(&display_local_bounds, &input) {
    return Err(format!("{}: display-local region is outside {}", capture_error::REGION_OUT_OF_BOUNDS, display.display_ref));
  }

  Ok(ResolvedRegion {
    display_index,
    display_local_logical: input.clone(),
    source_global_logical_bounds: Rect {
      x: display.global_logical_bounds.x + input.x,
      y: display.global_logical_bounds.y + input.y,
      width: input.width,
      height: input.height,
    },
  })
}

fn rect_contains_rect(container: &Rect, candidate: &Rect) -> bool {
  candidate.x >= container.x
    && candidate.y >= container.y
    && candidate.x + candidate.width <= container.x + container.width
    && candidate.y + candidate.height <= container.y + container.height
}

fn rect_intersects_rect(a: &Rect, b: &Rect) -> bool {
  a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y
}

fn require_display_selector(display_ref: Option<&str>, display_id: Option<&str>, coordinate_space: &str) -> AuvResult<()> {
  if display_ref.is_some() || display_id.is_some() {
    return Ok(());
  }
  Err(format!("{}: {} coordinates require display_ref or display_id", capture_error::DISPLAY_NOT_FOUND, coordinate_space))
}
