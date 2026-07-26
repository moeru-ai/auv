use auv_cli_invoke::commands::app::activate_application;

#[test]
fn activation_requires_a_target() {
  let error = futures_executor::block_on(activate_application(None)).expect_err("missing activation target should fail");

  assert_eq!(error, "app.activate requires --target");
}
