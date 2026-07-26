// File: src/driver/macos/capture/artifact.rs
use super::types::{CaptureContract, CaptureSource, CoordinateSpace};
use crate::types::AuvResult;

pub fn render_capture_contract_json(contract: &CaptureContract) -> AuvResult<String> {
  let mut rendered = serde_json::to_string_pretty(contract)
    .map_err(|error| format!("capture.backend_failed: failed to encode capture contract JSON: {error}"))?;
  rendered.push('\n');
  Ok(rendered)
}

pub fn render_capture_contract_text(contract: &CaptureContract) -> String {
  format!(
    concat!(
      "coordinateContractVersion={}\n",
      "captureSource={}\n",
      "captureBackend={:?}\n",
      "nativeDisplayId={}\n",
      "nativeWindowId={}\n",
      "includeShadow={}\n",
      "sourceGlobalLogicalBounds={:.3},{:.3},{:.3},{:.3}\n",
      "screenshotPixels={:.0}x{:.0}\n",
      "pixelToLogicalScale={:.6},{:.6}\n"
    ),
    contract.coordinate_contract_version,
    render_capture_source(&contract.capture_source),
    contract.capture_backend,
    native_display_id(&contract.capture_source).unwrap_or(""),
    native_window_id(&contract.capture_source).unwrap_or(""),
    contract.include_shadow,
    contract.source_global_logical_bounds.x,
    contract.source_global_logical_bounds.y,
    contract.source_global_logical_bounds.width,
    contract.source_global_logical_bounds.height,
    contract.screenshot_pixel_size.width,
    contract.screenshot_pixel_size.height,
    contract.pixel_to_logical_scale.x,
    contract.pixel_to_logical_scale.y
  )
}

fn render_capture_source(source: &CaptureSource) -> String {
  match source {
    CaptureSource::Display { display_ref, .. } => format!("display:{display_ref}"),
    CaptureSource::Region {
      display_ref,
      input_space,
      ..
    } => format!("region:{display_ref}:{}", render_coordinate_space(input_space)),
    CaptureSource::Window {
      window_ref,
      display_ref,
      ..
    } => format!("window:{window_ref}:{display_ref}"),
  }
}

fn native_display_id(source: &CaptureSource) -> Option<&str> {
  match source {
    CaptureSource::Display {
      native_display_id, ..
    }
    | CaptureSource::Region {
      native_display_id, ..
    }
    | CaptureSource::Window {
      native_display_id, ..
    } => Some(native_display_id),
  }
}

fn native_window_id(source: &CaptureSource) -> Option<&str> {
  match source {
    CaptureSource::Window {
      native_window_id, ..
    } => Some(native_window_id),
    CaptureSource::Display { .. } | CaptureSource::Region { .. } => None,
  }
}

fn render_coordinate_space(space: &CoordinateSpace) -> &'static str {
  match space {
    CoordinateSpace::GlobalLogical => "global_logical",
    CoordinateSpace::DisplayLogical => "display_logical",
    CoordinateSpace::DisplayPhysical => "display_physical",
  }
}

#[cfg(test)]
#[path = "artifact_test.rs"]
mod tests;
