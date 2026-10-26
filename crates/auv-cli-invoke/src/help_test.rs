use std::collections::BTreeMap;

use crate::{Platform, SupportState, default_registry, render_command_help, render_help_index};

fn word(state: SupportState) -> &'static str {
  match state {
    SupportState::Supported => "yes",
    SupportState::Partial => "partial",
    SupportState::Unsupported => "no",
    SupportState::NotEvidenced => "?",
  }
}

#[test]
fn the_index_shows_every_command_with_its_support_on_each_platform() {
  let registry = default_registry();
  let index = render_help_index(&registry);

  let header = index.lines().find(|line| line.trim() == "macOS    Linux    Windows").expect("platform header in the command list");
  assert!(header.starts_with("  "), "header is part of the indented command list:\n{index}");

  for command in registry.all() {
    let line = index
      .lines()
      .find(|line| line.split_whitespace().next() == Some(command.id))
      .unwrap_or_else(|| panic!("{} is missing from the index:\n{index}", command.id));
    let cells = line.split_whitespace().skip(1).take(3).collect::<Vec<_>>();
    let expected = Platform::ALL.map(|platform| word(command.support.on(platform).state));
    assert_eq!(cells, expected, "support columns of {}", command.id);
  }
}

#[test]
fn the_index_explains_that_support_is_a_source_path_and_not_a_behavior_claim() {
  let index = render_help_index(&default_registry());
  assert!(index.contains("It is not a claim that the path was run."), "{index}");
  assert!(index.contains("partial  a path exists with limits"), "{index}");
}

#[test]
fn command_help_names_the_limit_of_a_partial_platform() {
  let registry = default_registry();
  let help = render_command_help(registry.resolve("input.typeText").expect("input.typeText is registered"));
  assert!(help.contains("Platform support:\n  macOS   yes\n"), "{help}");
  assert!(help.contains("  Linux   partial: focused target only; cannot focus or raise a window\n"), "{help}");
}

#[test]
fn command_help_shows_an_unsupported_platform_as_no() {
  let registry = default_registry();
  let help = render_command_help(registry.resolve("screen.clickText").expect("screen.clickText is registered"));
  assert!(help.contains("  Linux   no\n"), "{help}");
  assert!(help.contains("  Windows partial: primary display only; foreground pointer input"), "{help}");
  assert!(help.contains("moves the real pointer"), "{help}");
}

#[test]
fn an_alias_names_its_canonical_command_in_its_help() {
  let registry = default_registry();
  let help = render_command_help(registry.resolve("input.pressKeys").expect("input.pressKeys is registered"));
  assert!(help.contains("This command is an alias of input.keys."), "{help}");
}

#[test]
fn every_command_help_states_its_selected_runner_route() {
  for command in default_registry().all() {
    let help = render_command_help(command);
    assert!(help.contains("\nSelected Runner support:\n"), "{} does not state its Runner route:\n{help}", command.id);
  }
}

#[test]
fn command_help_distinguishes_runner_cells_from_direct_cells() {
  let registry = default_registry();
  let help = render_command_help(registry.resolve("screen.clickText").expect("screen.clickText is registered"));
  // The direct path refuses Linux while the Runner route finds display text and
  // clicks through global ClickPoint, so one help must show both facts.
  assert!(help.contains("Platform support:\n  macOS   yes\n  Linux   no\n"), "{help}");
  assert!(
    help.contains("\nSelected Runner support:\n  macOS   yes\n  Linux   yes\n  Windows partial: primary display only; global ClickPoint ignores the requested input policy and Windows delivers it as foreground SendInput\n"),
    "{help}"
  );
}

#[test]
fn command_help_names_the_limit_of_a_partial_runner_cell() {
  let registry = default_registry();
  let help = render_command_help(registry.resolve("input.drag").expect("input.drag is registered"));
  assert!(
    help.contains(
      "\nSelected Runner support:\n  macOS   partial: foreground-preferred input policy is refused; DragMouse carries no window policy\n"
    ),
    "{help}"
  );
  assert!(
    help.contains("  Windows partial: foreground-preferred input policy is refused; background delivery posts window messages"),
    "{help}"
  );
}

#[test]
fn command_help_says_when_the_runner_route_matches_the_direct_path() {
  let registry = default_registry();
  let help = render_command_help(registry.resolve("input.typeText").expect("input.typeText is registered"));
  assert!(help.contains("\nSelected Runner support:\n  same as the direct local path above\n"), "{help}");
}

#[test]
fn fixture_help_declares_no_runner_route_instead_of_unsupported_platforms() {
  let registry = default_registry();
  for id in ["scan.frame", "scan.coverage"] {
    let help = render_command_help(registry.resolve(id).expect("scan fixture is registered"));
    assert!(help.contains("\nSelected Runner support:\n  no Runner route; run it without a selected Run or Device\n"), "{id}:\n{help}");
  }
}

#[test]
fn the_index_points_at_the_per_command_runner_route() {
  let index = render_help_index(&default_registry());
  assert!(index.contains("Selected Runner support is the same source-level claim for the route a selected Run or Device takes"), "{index}");
}

#[test]
fn every_command_documents_at_least_one_example_it_accepts() {
  for command in default_registry().all() {
    let examples = command.examples().unwrap_or_else(|error| panic!("{error}"));
    assert!(!examples.is_empty(), "{} has no `auv invoke` example in its long help", command.id);
  }
}

// ROOT CAUSE:
//
// If an MCP caller only saw the `invoke` schema, it could not tell which
// `inputs` keys a command takes or how list values are encoded. An OSWorld
// agent sent `{"keys": "CTRL+L"}` and `{"text": ...}` for `query`, and 84 of
// about 160 failed calls in one run were such argument mistakes.
//
// The fix derives protocol inputs from the CLI examples, so MCP can show
// calls the command accepts.
#[test]
fn examples_resolve_to_the_protocol_inputs_mcp_sends() {
  let registry = default_registry();
  let press_keys = registry.resolve("input.pressKeys").unwrap().examples().unwrap();
  assert_eq!(press_keys[0].inputs.get("keys").map(String::as_str), Some(r#"["cmd","shift","p"]"#));

  let wait = registry.resolve("screen.waitForText").unwrap().examples().unwrap();
  assert_eq!(wait[0].inputs, BTreeMap::from([("query".to_string(), "Ready".to_string())]));
  assert_eq!(wait[0].cli, r#"auv invoke screen.waitForText "Ready""#);
}

#[test]
fn example_comments_become_summaries() {
  let examples = default_registry().resolve("screen.findText").unwrap().examples().unwrap();
  assert_eq!(examples[0].summary.as_deref(), Some("Find text on the current screen"));
}
