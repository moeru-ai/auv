use auv_cli::product_registry;

const CASH_OUT_COMMAND_ID: &str = "game.balatro.cash_out";
const DOCUMENT_WRITE_COMMAND_ID: &str = "app.textedit.document.write";

#[test]
fn product_registry_includes_app_commands_once() {
  let registry = product_registry();

  assert!(registry.resolve(CASH_OUT_COMMAND_ID).is_some());
  assert!(registry.resolve(DOCUMENT_WRITE_COMMAND_ID).is_some());
  assert_eq!(registry.all().iter().filter(|command| command.id == CASH_OUT_COMMAND_ID).count(), 1);
  assert_eq!(registry.all().iter().filter(|command| command.id == DOCUMENT_WRITE_COMMAND_ID).count(), 1);
}

#[test]
fn core_registry_excludes_textedit() {
  assert!(auv_cli_invoke::default_registry().resolve(DOCUMENT_WRITE_COMMAND_ID).is_none());
}
