mod help;
mod select_proof;
mod sidebar_scan_proof;

use std::collections::BTreeMap;
use std::process::ExitCode;

use auv_cli_invoke::{CommandGroup, InvokeCommandInput, InvokeNamespace, InvokeRegistry, command};

pub use help::{render_command_help, render_help_index};
pub use select_proof::{
  SELECT_PROOF_COMMAND_ID, build_select_result_from_fixture_dir, hermetic_select_proof_fixture_dir, select_proof_handler,
};
pub use sidebar_scan_proof::{
  SIDEBAR_SCAN_PROOF_COMMAND_ID, build_scan_from_fixture_dir, hermetic_sidebar_scan_proof_fixture_dir, sidebar_scan_proof_handler,
};

pub fn netease_registry() -> InvokeRegistry {
  InvokeRegistry::from_groups(vec![
    CommandGroup::new("netease", "NETEASE").group(
      CommandGroup::new("playlist", "Playlist")
        .command(command::spec(
          select_proof::SELECT_PROOF_COMMAND_ID,
          InvokeNamespace::Fixture,
          "Hermetic playlist select proof from fixture dir",
          select_proof::SELECT_PROOF_ARGS,
          select_proof::select_proof_handler,
        ))
        .command(command::spec(
          sidebar_scan_proof::SIDEBAR_SCAN_PROOF_COMMAND_ID,
          InvokeNamespace::Fixture,
          "Hermetic playlist sidebar scan proof from fixture dir",
          sidebar_scan_proof::SIDEBAR_SCAN_PROOF_ARGS,
          sidebar_scan_proof::sidebar_scan_proof_handler,
        )),
    ),
  ])
}

/// Dispatch `auv-netease-music invoke …` without touching root `default_registry()`.
pub fn run(tokens: &[String]) -> ExitCode {
  let registry = netease_registry();

  if tokens.is_empty() || tokens == ["--help"] || tokens == ["-h"] {
    print!("{}", render_help_index(&registry));
    return ExitCode::SUCCESS;
  }

  let mut index = 0usize;
  let mut dry_run = false;
  while index < tokens.len() {
    match tokens[index].as_str() {
      "--dry-run" => {
        dry_run = true;
        index += 1;
      }
      "--help" | "-h" if index == 0 => {
        print!("{}", render_help_index(&registry));
        return ExitCode::SUCCESS;
      }
      _ => break,
    }
  }

  let Some(command_id) = tokens.get(index) else {
    eprintln!("error: invoke requires a command id");
    print!("{}", render_help_index(&registry));
    return ExitCode::from(2);
  };

  if tokens.get(index + 1).is_some_and(|token| token == "--help" || token == "-h") {
    let Some(command) = registry.resolve(command_id) else {
      eprintln!("error: unknown command {command_id}");
      print!("{}", render_help_index(&registry));
      return ExitCode::from(2);
    };
    print!("{}", render_command_help(command));
    return ExitCode::SUCCESS;
  }

  let Some(command) = registry.resolve(command_id) else {
    eprintln!("error: unknown command {command_id}");
    print!("{}", render_help_index(&registry));
    return ExitCode::from(2);
  };

  let mut inputs = BTreeMap::new();
  let mut cursor = index + 1;
  while cursor < tokens.len() {
    let flag = tokens[cursor].as_str();
    if flag == "--help" || flag == "-h" {
      print!("{}", render_command_help(command));
      return ExitCode::SUCCESS;
    }
    if !flag.starts_with("--") {
      eprintln!("error: unexpected argument {flag}");
      return ExitCode::from(2);
    }
    let key = flag.trim_start_matches("--");
    let Some(value) = tokens.get(cursor + 1) else {
      eprintln!("error: flag {flag} requires a value");
      return ExitCode::from(2);
    };
    inputs.insert(key.to_string(), value.clone());
    cursor += 2;
  }

  match futures_executor::block_on(command.invoke(InvokeCommandInput {
    command_id: command.id.to_string(),
    target_application_id: None,
    inputs,
    dry_run,
    cancellation: auv_cli_invoke::InvokeCancellation::new(),
  })) {
    Ok(output) => {
      println!("{}", output.summary);
      if let Some(run_id) = output.signals.get("run_id") {
        println!("run_id={run_id}");
      }
      if let Some(store_root) = output.signals.get("store_root") {
        println!("store_root={store_root}");
      }
      for limit in &output.known_limits {
        println!("known_limit: {limit}");
      }
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::from(1)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::{netease_registry, render_command_help, render_help_index, run};
  use crate::invoke::select_proof::SELECT_PROOF_COMMAND_ID;

  #[test]
  fn invoke_help_uses_app_binary_prefix() {
    let registry = netease_registry();
    let help = render_help_index(&registry);
    assert!(help.contains("auv-netease-music invoke"));
    assert!(!help.contains("USAGE\n  auv invoke <command>"));

    let command = registry.resolve(SELECT_PROOF_COMMAND_ID).expect("command");
    let command_help = render_command_help(command);
    assert!(command_help.contains("auv-netease-music invoke netease.playlist.selectProof"));
    assert!(!command_help.contains("USAGE\n  auv invoke netease.playlist.selectProof"));
  }

  #[test]
  fn invoke_run_without_caller_context_preserves_direct_result() {
    use std::process::ExitCode;

    let fixture_dir = crate::invoke::hermetic_select_proof_fixture_dir();
    let exit = run(&[
      SELECT_PROOF_COMMAND_ID.to_string(),
      "--fixture-dir".to_string(),
      fixture_dir.display().to_string(),
    ]);
    assert_eq!(exit, ExitCode::SUCCESS);
  }

  #[test]
  fn invoke_run_allows_command_help_after_command_flags() {
    use std::process::ExitCode;

    let fixture_dir = crate::invoke::hermetic_select_proof_fixture_dir();
    let exit = run(&[
      SELECT_PROOF_COMMAND_ID.to_string(),
      "--fixture-dir".to_string(),
      fixture_dir.display().to_string(),
      "--help".to_string(),
    ]);
    assert_eq!(exit, ExitCode::SUCCESS);
  }
}
