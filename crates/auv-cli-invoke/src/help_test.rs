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
  assert!(help.contains("  Linux   partial: foreground delivery only; targeted delivery is unsupported\n"), "{help}");
}

#[test]
fn command_help_shows_an_unsupported_platform_as_no() {
  let registry = default_registry();
  let help = render_command_help(registry.resolve("screen.clickText").expect("screen.clickText is registered"));
  assert!(help.contains("  Windows no\n"), "{help}");
}

#[test]
fn an_alias_names_its_canonical_command_in_its_help() {
  let registry = default_registry();
  let help = render_command_help(registry.resolve("input.pressKeys").expect("input.pressKeys is registered"));
  assert!(help.contains("This command is an alias of input.keys."), "{help}");
}
