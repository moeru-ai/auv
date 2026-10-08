pub mod accessibility;
pub mod application;
pub mod capture;
pub mod display;
pub mod error;
pub mod geometry;
pub mod input;
pub mod input_cancellation;
pub mod keyboard;
pub mod keyboard_input;
pub mod mouse;
pub mod mouse_input;
pub mod permission;
pub mod readiness;
pub mod scroll_motion;
pub mod scroll_stream;
pub mod selector;
pub mod traits;
pub mod vision;
pub mod window;

pub use accessibility::{AxFocusResult, AxTextRead, AxTextSelector, FocusTextOptions};
pub use application::{
  ApplicationActivationResult, ApplicationActivationVerification, ProcessActivationResult, ProcessActivationVerification,
};
pub use capture::{Activation, Capture, CaptureOptions, CaptureResolution, DisplayCapture, ImageView, RegionCapture};
pub use display::{Display, ObservedDisplays};
pub use error::{DriverError, DriverResult};
pub use geometry::{
  CameraPoint, CoordinateSpace, PixelSize, Point, Point3, Position, Positional, Positioned, ProjectionBasis, ProjectionDerivationFamily,
  ProjectionSourceSpace, Rect, RelativeRect, ScreenPoint, Size, WindowPoint, WorldPoint,
};
pub use input::{
  ActivationPolicy, Click, ClickModifiers, ClickOptions, DisturbanceLevel, INPUT_ACTION_RESULT_PURPOSE, InputActionResult, InputAttempt,
  InputDeliveryPath, InputPolicy, InputPreparationLease, InputTarget, KeyPressOptions, KeyboardInput, KeyboardInputError,
  KeyboardInputProgress, MouseButton, PasteTextOptions, PrepareForInputOptions, PressKeysOptions, Scroll, ScrollDeliveryCandidate,
  ScrollDeliveryStrategy, ScrollOptions, TextSubmit, TypeTextOptions, WaitOptions, WindowClickStrategy, WindowInput,
};
pub use keyboard::{Key, Keysym, Modifier};
pub use keyboard_input::{KeyboardBackend, KeyboardHold, KeyboardHoldController, KeyboardHoldId, keyboard_hold_controller};
pub use mouse::{
  MouseCubicBezierSegment, MouseCurve, MouseCurveMapping, MouseMotionOptions, MouseMotionSample, MouseSamples, MouseStart, MoveMouseRequest,
};
pub use permission::{PermissionProbe, PermissionStatus};
pub use readiness::{ReadinessCheck, ReadinessCheckStatus, ReadinessProbeInput, ReadinessReport, ReadinessStatus};
pub use scroll_motion::{
  CumulativeQuantizer, MotionTiming, ScrollMotion, ScrollMotionProgress, ScrollMotionResult, ScrollMotionSchedule, TimingFunction,
};
pub use scroll_stream::{
  MAX_SCROLL_STREAM_LEASE, MAX_SCROLL_VELOCITY, ScrollStreamControl, ScrollStreamOptions, ScrollStreamProgress, ScrollStreamResult,
  ScrollStreamStopReason, ScrollVelocity,
};
pub use selector::{App, AppSelector, TextMatcher, WindowSelector};
pub use traits::{Driver, DriverDescriptor, DriverSession, PlatformKind};
pub use vision::{ImageMatch, ImageMatchResult, OcrMatch, OcrMatches, RecognizedText, TextRecognition, TextRecognitionOptions};
pub use window::{
  ObservedWindows, Window, WindowMutationAttempt, WindowMutationCandidate, WindowMutationKind, WindowMutationOptions, WindowMutationPath,
  WindowMutationPolicy, WindowMutationResult, WindowMutationStrategy, WindowMutationVerification, WindowRef, WindowState, find_window,
};

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;
