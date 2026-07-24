mod accessibility;
mod application;
mod descriptor;
mod driver;
mod readiness;
mod session;

// TODO(driver-crates): These modules are temporarily public so the root
// command adapter can build while command-facing code migrates to typed
// session APIs. Do not treat them as stable crate API.
#[doc(hidden)]
pub mod capture;
#[doc(hidden)]
pub mod constants;
#[doc(hidden)]
pub mod observe;
#[doc(hidden)]
pub mod support;
#[doc(hidden)]
pub mod types;

// TODO(driver-crates): This is a temporary compatibility surface for the root
// crate while legacy macOS command code is moved behind typed session APIs.
#[doc(hidden)]
pub mod native;

pub use accessibility::{AxFocusResult, AxTextRead, DEFAULT_AX_MAX_CHILDREN, DEFAULT_AX_MAX_DEPTH};
pub use application::ApplicationControl;
pub use auv_driver_common::vision::{OcrMatch, OcrMatches};
pub use descriptor::{MacosDriverDescriptor, macos_driver_descriptor};
pub use driver::{MacosDriver, MacosDriverSession};
pub use readiness::assess_readiness;
pub use session::{AccessibilityApi, ClipboardApi, InputApi, PermissionApi, VisionApi, WindowApi};
pub use types::{ObservedAxNode, ObservedAxTreeSnapshot};

#[cfg(test)]
mod tests {
  #[test]
  fn observe_api_does_not_expose_legacy_signal_map_helpers() {
    let observe_source = include_str!("observe.rs");

    for helper in [
      "verify_now_playing_title_signals",
      "verify_ax_text_signals",
      "ocr_detection_signals",
      "wait_ocr_detection_signals",
      "row_detection_signals",
      "wait_row_detection_signals",
      "insert_optional_signal",
      "preferred_ax_signal_text",
    ] {
      assert!(!observe_source.contains(&format!("pub fn {helper}")), "observe API still exposes legacy signal-map helper `{helper}`");
    }
  }

  #[test]
  fn descriptor_api_exposes_no_legacy_metadata_bag() {
    let descriptor_source = include_str!("descriptor.rs");
    let crate_source = include_str!("lib.rs");

    for forbidden in [
      "MacosLegacyDescriptorMetadata",
      "macos_legacy_descriptor_metadata",
      "MACOS_DESKTOP_CAPABILITIES",
      "donor_boundary",
    ] {
      assert!(!descriptor_source.contains(forbidden), "descriptor module still contains `{forbidden}`");
      assert!(!crate_source.contains(&format!("pub use descriptor::{{{forbidden}")), "crate API still exports `{forbidden}`");
    }
  }

  #[test]
  fn accessibility_api_does_not_export_observation_wrappers() {
    let accessibility_source = include_str!("accessibility.rs");

    for forbidden in ["AxFocusObservation", "AxTextObservation"] {
      assert!(!accessibility_source.contains(&format!("pub struct {forbidden}")), "accessibility API still exports `{forbidden}`");
    }
  }

  #[test]
  fn typed_driver_results_do_not_embed_or_round_trip_human_reports() {
    let types_source = include_str!("types.rs");
    let native_ocr_source = include_str!("native/ocr.rs");
    let native_tree_source = include_str!("native/tree.rs");
    let support_source = include_str!("support.rs");

    assert!(!types_source.contains("pub report:"), "typed driver result still embeds rendered report text");
    for forbidden in ["render_ocr_text_report", "render_visual_rows_report"] {
      assert!(!native_ocr_source.contains(&format!("fn {forbidden}")), "native OCR still exposes legacy report renderer `{forbidden}`");
    }
    assert!(!native_tree_source.contains("fn render_ax_tree_report"), "native AX still exposes a legacy report renderer");
    assert!(!support_source.contains("mod parse;"), "driver support still compiles legacy text-report decoders");
  }
}
