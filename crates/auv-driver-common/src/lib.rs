pub mod accessibility;
pub mod capture;
pub mod display;
pub mod error;
pub mod geometry;
pub mod input;
pub mod permission;
pub mod readiness;
pub mod selector;
pub mod traits;
pub mod vision;
pub mod window;

pub use accessibility::{AxFocusResult, AxTextRead};
pub use capture::{Activation, Capture, CaptureOptions, DisplayCapture, ImageView, RegionCapture};
pub use display::{Display, ObservedDisplays};
pub use error::{DriverError, DriverResult};
pub use geometry::{
  CameraPoint, CoordinateSpace, Point, Point3, ProjectionBasis, ProjectionDerivationFamily, ProjectionSourceSpace, RatioRect, Rect,
  ScreenPoint, Size, WindowPoint, WorldPoint,
};
pub use input::{
  ActivationPolicy, Click, ClickOptions, DisturbanceLevel, INPUT_ACTION_RESULT_PURPOSE, InputActionResult, InputAttempt, InputDeliveryPath,
  InputPolicy, InputPreparationLease, KeyPressOptions, PasteTextOptions, PrepareForInputOptions, Scroll, ScrollDeliveryCandidate,
  ScrollDeliveryStrategy, ScrollOptions, TextSubmit, TypeTextOptions, WaitOptions, WindowClickStrategy, WindowInput,
};
pub use permission::{PermissionProbe, PermissionStatus};
pub use readiness::{ReadinessCheck, ReadinessCheckStatus, ReadinessProbeInput, ReadinessReport, ReadinessStatus};
pub use selector::{App, AppSelector, TextMatcher, WindowSelector};
pub use traits::{Driver, DriverDescriptor, DriverSession, PlatformKind};
pub use vision::{ImageMatch, ImageMatchResult, OcrMatch, OcrMatches, RecognizedText, TextRecognition, TextRecognitionOptions};
pub use window::{
  ObservedWindows, Window, WindowMutationAttempt, WindowMutationCandidate, WindowMutationKind, WindowMutationOptions, WindowMutationPath,
  WindowMutationPolicy, WindowMutationResult, WindowMutationStrategy, WindowMutationVerification, WindowRef, WindowState,
};

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
