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
  ScrollDeliveryStrategy, ScrollOptions, TextSubmit, TypeTextOptions, WaitOptions, WindowClickStrategy,
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
mod tests {
  use crate::{Driver, DriverDescriptor, DriverResult, DriverSession, PlatformKind};

  #[derive(Clone, Copy)]
  struct TestDriver;

  #[derive(Clone, Copy)]
  struct TestSession;

  impl Driver for TestDriver {
    type Session = TestSession;

    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test",
        platform: PlatformKind::Fixture,
        description: "test driver",
      }
    }

    fn open_local(&self) -> DriverResult<Self::Session> {
      Ok(TestSession)
    }
  }

  impl DriverSession for TestSession {
    fn descriptor(&self) -> DriverDescriptor {
      DriverDescriptor {
        id: "test-session",
        platform: PlatformKind::Fixture,
        description: "test session",
      }
    }
  }

  #[test]
  fn driver_traits_use_typed_sessions() -> DriverResult<()> {
    let driver = TestDriver;
    let session = driver.open_local()?;

    assert_eq!(driver.descriptor().id, "test");
    assert_eq!(session.descriptor().description, "test session");

    let _ = PlatformKind::Macos;
    let _ = PlatformKind::Windows;
    let _ = PlatformKind::Linux;
    let _ = PlatformKind::Android;
    let _ = PlatformKind::Ios;
    let _ = PlatformKind::Browser;
    let _ = PlatformKind::Fixture;
    let _ = PlatformKind::Remote;

    Ok(())
  }
}
