pub mod capture;
pub mod display;
pub mod error;
pub mod geometry;
pub mod input;
pub mod operation;
pub mod permission;
pub mod readiness;
pub mod selector;
pub mod traits;
pub mod vision;
pub mod window;

pub use capture::{
  Activation, Capture, CaptureBinding, CaptureOptions, DisplayCapture, ImageView, RegionCapture,
};
pub use display::{Display, ObservedDisplays};
pub use error::{DriverError, DriverResult};
pub use geometry::{
  CameraPoint, CoordinateSpace, Point, Point3, ProjectionBasis, ProjectionDerivationFamily,
  ProjectionSourceSpace, RatioRect, Rect, ScreenPoint, Size, WindowPoint, WorldPoint,
};
pub use input::{
  ActivationPolicy, Click, ClickOptions, DisturbanceLevel, InputActionResult, InputAttempt,
  InputDeliveryPath, InputPolicy, InputPreparationLease, KeyPressOptions, PasteTextOptions,
  PrepareForInputOptions, Scroll, ScrollDeliveryCandidate, ScrollDeliveryStrategy, ScrollOptions,
  TextSubmit, TypeTextOptions, WaitOptions, WindowClickStrategy,
};
pub use operation::{OperationDisturbance, OperationNamespace, OperationSpec};
pub use permission::{PermissionProbe, PermissionStatus};
pub use readiness::{
  ReadinessCheck, ReadinessCheckStatus, ReadinessProbeInput, ReadinessReport, ReadinessStatus,
};
pub use selector::{App, AppSelector, TextMatcher, WindowSelector};
pub use traits::{Driver, DriverDescriptor, DriverSession, PlatformKind};
pub use vision::{
  ImageMatch, ImageMatchResult, RecognizedText, TextRecognition, TextRecognitionOptions,
};
pub use window::{
  ObservedWindows, Window, WindowMutationAttempt, WindowMutationCandidate, WindowMutationKind,
  WindowMutationOptions, WindowMutationPath, WindowMutationPolicy, WindowMutationResult,
  WindowMutationStrategy, WindowMutationVerification, WindowRef, WindowState,
};

#[cfg(test)]
mod tests {
  use crate::{
    Driver, DriverDescriptor, DriverResult, DriverSession, PlatformKind, WindowMutationAttempt,
    WindowMutationCandidate, WindowMutationKind, WindowMutationOptions, WindowMutationPath,
    WindowMutationPolicy, WindowMutationResult, WindowState,
  };

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
        summary: "test driver",
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
        summary: "test session",
      }
    }
  }

  #[test]
  fn driver_traits_use_typed_sessions() -> DriverResult<()> {
    let driver = TestDriver;
    let session = driver.open_local()?;

    assert_eq!(driver.descriptor().id, "test");
    assert_eq!(session.descriptor().summary, "test session");

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

  #[test]
  fn public_api_exports_agreed_driver_names() {
    let _window_mutation_options = WindowMutationOptions::default();
    let _window_mutation_attempt =
      WindowMutationAttempt::success(WindowMutationPath::AxWindowAttribute, "ok");
    let _ = std::any::type_name::<WindowMutationCandidate>();
    let _ = std::any::type_name::<WindowMutationKind>();
    let _ = std::any::type_name::<WindowMutationPolicy>();
    let _ = std::any::type_name::<WindowMutationResult>();
    let _ = std::any::type_name::<WindowState>();
  }
}
