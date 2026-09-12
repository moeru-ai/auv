#[cfg(target_os = "macos")]
use auv_cli_invoke::commands::app::activate_application;
use auv_cli_invoke::commands::app::{activate_process_application, require_activation_target};

#[test]
fn activation_requires_a_target() {
  let error = require_activation_target(None).expect_err("missing activation target should fail");
  assert_eq!(error, "app.activate requires --target");

  #[cfg(target_os = "macos")]
  {
    let error = futures_executor::block_on(activate_application(None)).expect_err("missing activation target should fail");
    assert_eq!(error, "app.activate requires --target");
  }

  #[cfg(target_os = "windows")]
  {
    let error = futures_executor::block_on(activate_process_application(None)).expect_err("missing activation target should fail");
    assert_eq!(error, "app.activate requires --target");
  }
}
