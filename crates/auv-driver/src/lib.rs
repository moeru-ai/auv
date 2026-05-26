pub mod capture;
pub mod display;
pub mod error;
pub mod geometry;
pub mod input;
pub mod selector;
pub mod traits;
pub mod vision;
pub mod window;

pub use capture::{Activation, Capture, CaptureOptions, ImageView};
pub use display::{Display, ObservedDisplays};
pub use error::{DriverError, DriverResult};
pub use geometry::{CoordinateSpace, Point, RatioRect, Rect, ScreenPoint, Size, WindowPoint};
pub use input::{
  ActivationPolicy, Click, ClickOptions, DisturbanceLevel, InputActionResult, InputAttempt,
  InputDeliveryPath, InputPolicy, InputPreparationLease, PasteTextOptions, PrepareForInputOptions,
  TextSubmit, TypeTextOptions, WaitOptions,
};
pub use selector::{App, AppSelector, TextMatcher, WindowSelector};
pub use traits::{Driver, DriverDescriptor, DriverSession, PlatformKind};
pub use vision::{ImageMatch, ImageMatchResult, RecognizedText, TextRecognition};
pub use window::{ObservedWindows, Window, WindowRef};

#[cfg(test)]
mod tests {
  use std::time::Duration;

  use crate::{
    capture::{Activation, Capture, CaptureOptions, ImageView},
    display::{Display, ObservedDisplays},
    error::{DriverError, DriverResult},
    geometry::{CoordinateSpace, Point, RatioRect, Rect, ScreenPoint, Size, WindowPoint},
    input::{
      ActivationPolicy, Click, ClickOptions, DisturbanceLevel, InputActionResult, InputAttempt,
      InputDeliveryPath, InputPolicy, InputPreparationLease, PasteTextOptions,
      PrepareForInputOptions, TextSubmit, TypeTextOptions, WaitOptions,
    },
    selector::{App, AppSelector, TextMatcher, Window as SelectWindow, WindowSelector},
    traits::{Driver, DriverDescriptor, DriverSession, PlatformKind},
    vision::{ImageMatch, ImageMatchResult, RecognizedText, TextRecognition},
    window::{ObservedWindows, Window, WindowRef},
  };

  #[test]
  fn public_api_exports_agreed_driver_names() {
    let app = App::bundle("com.example.App");
    let _app_pid = App::pid(1234);
    let _frontmost = App::frontmost();
    let _window_selector = SelectWindow::main_visible()
      .owned_by(app)
      .title_contains("Inbox")
      .title_exact("Inbox - AUV");

    let _activation = Activation::ActivateFirst {
      settle: Duration::from_millis(50),
    };
    let _keep_current = Activation::KeepCurrent;
    let _click = Click::Double {
      interval: Duration::from_millis(100),
    };
    let _screen_point = ScreenPoint::new(10.0, 20.0);
    let _window_point = WindowPoint::new(3.0, 4.0);
    let _click_options = ClickOptions {
      policy: InputPolicy::BackgroundOnly,
      click: Click::Single,
    };
    let _type_options = TypeTextOptions {
      policy: InputPolicy::BackgroundPreferred,
      replace_existing: true,
      submit: TextSubmit::Return,
      inter_char_delay: Duration::from_millis(8),
      allow_clipboard_fallback: false,
      settle: Duration::from_millis(50),
    };
    let _prepare_options = PrepareForInputOptions {
      activation: ActivationPolicy::NoChange,
      preserve_frontmost: true,
      install_focus_guard: false,
      settle: Duration::from_millis(0),
    };
    let _lease = InputPreparationLease::noop();
    let _attempt = InputAttempt::success(InputDeliveryPath::WindowTargetedKeyboard);
    let _result = InputActionResult::single_success(InputDeliveryPath::WindowTargetedKeyboard);
    let _ = DisturbanceLevel::None;
    let _ = ActivationPolicy::Background;
    let _ = ActivationPolicy::FocusWithoutRaise;
    let _ = ActivationPolicy::Foreground {
      settle: Duration::from_millis(100),
    };
    let _ = InputPolicy::ForegroundPreferred;

    let _ = std::any::type_name::<Capture>();
    let _ = std::any::type_name::<CaptureOptions>();
    let _ = std::any::type_name::<ImageView<'static>>();
    let _ = std::any::type_name::<Display>();
    let _ = std::any::type_name::<ObservedDisplays>();
    let _ = std::any::type_name::<DriverError>();
    let _ = std::any::type_name::<DriverResult<()>>();
    let _ = std::any::type_name::<CoordinateSpace>();
    let _ = std::any::type_name::<Point>();
    let _ = std::any::type_name::<RatioRect>();
    let _ = std::any::type_name::<Rect>();
    let _ = std::any::type_name::<Size>();
    let _ = std::any::type_name::<PasteTextOptions>();
    let _ = std::any::type_name::<TextSubmit>();
    let _ = std::any::type_name::<WaitOptions>();
    let _ = std::any::type_name::<AppSelector>();
    let _ = std::any::type_name::<TextMatcher>();
    let _ = std::any::type_name::<WindowSelector>();
    let _ = std::any::type_name::<DriverDescriptor>();
    let _ = std::any::type_name::<PlatformKind>();
    let _ = std::any::type_name::<ImageMatch>();
    let _ = std::any::type_name::<ImageMatchResult>();
    let _ = std::any::type_name::<RecognizedText>();
    let _ = std::any::type_name::<TextRecognition>();
    let _ = std::any::type_name::<ObservedWindows>();
    let _ = std::any::type_name::<Window>();
    let _ = std::any::type_name::<WindowRef>();
  }

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
}
