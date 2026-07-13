//! Capture contract renderers for JSON artifacts and human-readable output.

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
mod tests {
  use super::super::types::{CaptureBackend, Rect, Scale2D, Size};
  use super::*;

  fn sample_display_contract() -> CaptureContract {
    CaptureContract {
      coordinate_contract_version: 1,
      capture_source: CaptureSource::Display {
        display_ref: "display_0".to_string(),
        native_display_id: "main-native-display".to_string(),
      },
      capture_backend: CaptureBackend::XcapMacos,
      include_shadow: false,
      source_global_logical_bounds: Rect {
        x: 0.0,
        y: 0.0,
        width: 3008.0,
        height: 1692.0,
      },
      source_physical_pixel_bounds: Rect {
        x: 0.0,
        y: 0.0,
        width: 6016.0,
        height: 3384.0,
      },
      screenshot_pixel_size: Size {
        width: 6016.0,
        height: 3384.0,
      },
      pixel_to_logical_scale: Scale2D { x: 0.5, y: 0.5 },
      logical_to_pixel_scale: Scale2D { x: 2.0, y: 2.0 },
      captured_at_unix_ms: 1779090000000,
    }
  }

  #[test]
  fn json_contains_contract_fields() {
    let rendered = render_capture_contract_json(&sample_display_contract()).unwrap();

    assert!(rendered.contains("\"coordinate_contract_version\": 1"));
    assert!(rendered.contains("\"display_ref\": \"display_0\""));
    assert!(rendered.contains("\"native_display_id\": \"main-native-display\""));
    assert!(rendered.contains("\"pixel_to_logical_scale\""));
  }

  #[test]
  fn text_report_is_human_readable() {
    let rendered = render_capture_contract_text(&sample_display_contract());

    assert!(rendered.contains("captureSource=display:display_0"));
    assert!(rendered.contains("nativeDisplayId=main-native-display"));
    assert!(rendered.contains("includeShadow=false"));
    assert!(rendered.contains("screenshotPixels=6016x3384"));
  }
}
