use crate::{CommandRole, Platform, RunnerSupport, SupportState, default_registry};

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
    assert_eq!(alias.runner, canonical.runner, "{} must share the Runner route of {}", alias.id, canonical.id);
  }
}

// ROOT CAUSE:
//
// If a command could register without a Runner declaration, help and MCP
// metadata would silently under-report the selected-Runner route, the gap
// the 2026-10-13 design note called "route undeclared".
//
// The `invoke_command` macro makes `runner =` required, so the declaration is
// also a compile-time fact; this test pins the registry-side completeness
// contract: fixtures declare no route, everything else declares one.
#[test]
fn every_command_declares_its_runner_route() {
  let registry = default_registry();
  assert!(!registry.all().is_empty(), "the registry is unexpectedly empty");

  for command in registry.all() {
    match command.role {
      CommandRole::Fixture => assert_eq!(
        command.runner,
        RunnerSupport::NoRoute,
        "{} is a fixture, so it must declare no Runner route, matching the dispatch refusal",
        command.id
      ),
      CommandRole::Desktop | CommandRole::Alias { .. } => assert_ne!(
        command.runner,
        RunnerSupport::NoRoute,
        "{} drives the desktop, so it must declare a Runner route (Direct or explicit cells)",
        command.id
      ),
    }
  }
}

/// Explicit Runner cells exist to state differences from the direct path; where
/// the cells are equal, the declaration must use the audited `Direct` shorthand
/// so the equality stays visible as a decision.
#[test]
fn explicit_runner_cells_differ_from_the_direct_cells() {
  for command in default_registry().all() {
    let RunnerSupport::Cells(cells) = command.runner else {
      continue;
    };
    assert_ne!(cells, command.support, "{} declares Runner cells identical to its direct cells; use runner = Direct instead", command.id);
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
