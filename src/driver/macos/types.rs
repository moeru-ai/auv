// TODO(driver-crates): temporary root compatibility while legacy command
// handlers migrate to `auv-driver-macos` typed session APIs.
pub(crate) use auv_driver_macos::types::{
  CoordinateReadinessAssessment, DetectedScreenRows, ObservedAxNode, ObservedAxTreeSnapshot,
  ObservedDisplay, ObservedDisplaySnapshot, ObservedOcrRow, ObservedPointResolution, ObservedRect,
  ObservedWindow, ObservedWindowSnapshot, OcrTextMatch, OcrTextSnapshot, ScreenshotDimensions,
  WindowCandidate, WindowRef, WindowSelection,
};
