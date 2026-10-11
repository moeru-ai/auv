use crate::{CommandRole, Platform, SupportState, default_registry};

#[test]
fn aliases_name_a_registered_canonical_command_and_share_its_support() {
  let registry = default_registry();
  let mut aliases = registry
    .all()
    .iter()
    .filter_map(|command| match command.role {
      CommandRole::Alias { canonical } => Some((command.id, canonical)),
      _ => None,
    })
    .collect::<Vec<_>>();
  aliases.sort_unstable();
  // A third alias must be a decision, not an accident: it changes how the registry counts commands.
  assert_eq!(
    aliases,
    [
      ("input.axFocusText", "input.focusText"),
      ("input.pressKeys", "input.keys")
    ]
  );

  for (alias, canonical) in aliases {
    let alias = registry.resolve(alias).expect("alias is registered");
    let canonical = registry.resolve(canonical).expect("canonical command is registered");
    assert_eq!(canonical.role, CommandRole::Desktop, "{} must not itself be an alias", canonical.id);
    assert_eq!(alias.support, canonical.support, "{} must share the support of {}", alias.id, canonical.id);
  }
}

#[test]
fn fixture_commands_are_supported_on_every_platform_without_a_limit() {
  let registry = default_registry();
  let mut fixtures = registry.all().iter().filter(|command| command.role == CommandRole::Fixture).collect::<Vec<_>>();
  fixtures.sort_by_key(|command| command.id);
  assert_eq!(fixtures.iter().map(|command| command.id).collect::<Vec<_>>(), ["scan.coverage", "scan.frame"]);

  for fixture in fixtures {
    for platform in Platform::ALL {
      let cell = fixture.support.on(platform);
      assert_eq!(cell.state, SupportState::Supported, "{} on {}", fixture.id, platform.label());
      assert_eq!(cell.note, None, "{} on {}", fixture.id, platform.label());
    }
  }
}

#[test]
fn a_partial_cell_always_names_its_limit() {
  for command in default_registry().all() {
    for platform in Platform::ALL {
      let cell = command.support.on(platform);
      if cell.state == SupportState::Partial {
        assert!(cell.note.is_some_and(|note| !note.trim().is_empty()), "{} on {} is partial without a limit", command.id, platform.label());
      }
    }
  }
}
