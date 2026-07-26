use super::*;
use crate::TableRow;

#[derive(TableRow)]
struct ProcessRow<'a> {
  process_id: u32,
  name: &'a str,
  #[table(display_zero = "blocked", display_with = |_: &bool| "ready")]
  ready: bool,
  owner: Option<&'a str>,
  #[table(wide, header = "EXECUTABLE")]
  executable_path: &'a Path,
  #[table(hidden)]
  _internal_id: &'a str,
}

#[test]
fn derive_infers_headers_and_common_value_formats() {
  let rows = [ProcessRow {
    process_id: 42,
    name: "AUV",
    ready: true,
    owner: None,
    executable_path: Path::new("auv"),
    _internal_id: "not-present",
  }];

  assert_eq!(render(&rows, TableOptions::default()), "PROCESS ID  NAME  READY  OWNER\n42          AUV   ready  -");
  assert_eq!(
    render(&rows, TableOptions::default().wide(true)),
    "PROCESS ID  NAME  READY  OWNER  EXECUTABLE\n42          AUV   ready  -      auv"
  );
}

#[test]
fn empty_message_is_appended_after_the_schema() {
  let rows: [ProcessRow<'_>; 0] = [];

  assert_eq!(render(&rows, TableOptions::default().empty_message("(no processes)")), "PROCESS ID  NAME  READY  OWNER\n(no processes)");
}

#[derive(Clone, Copy)]
enum ReadinessOverride {
  ForceReady,
}

#[derive(TableRow)]
struct ContextRow {
  #[table(display_with = |ready: &bool| if *ready || self.override_state.is_some() { "ready" } else { "blocked" })]
  ready: bool,
  #[table(hidden)]
  override_state: Option<ReadinessOverride>,
}

#[test]
fn custom_formatter_can_read_another_row_field() {
  let rows = [ContextRow {
    ready: false,
    override_state: Some(ReadinessOverride::ForceReady),
  }];

  assert_eq!(render(&rows, TableOptions::default()), "READY\nready");
}
