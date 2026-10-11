use crate::{CommandGroup, CommandNode, CommandRole, InvokeCommand, InvokeRegistry, Platform, PlatformSupport, SupportState};

/// Width of one platform column in the command index: the longest word, `Windows`, and a gap.
const SUPPORT_COLUMN_WIDTH: usize = 9;

pub fn render_help_index(registry: &InvokeRegistry) -> String {
  let mut commands = Vec::new();
  for group in registry.groups() {
    collect_commands(group, &mut commands);
  }
  let command_width = commands.iter().map(|command| command.id.len()).chain(["help".len()]).max().unwrap_or(0) + 2;

  let mut help = String::from(
    "Invoke typed computer-use operations through AUV's shared local and remote execution model.\n\nEach invocation creates or joins a Run, calls one registered operation, and records its result and artifacts through the same execution path used by other frontends.\n\nExamples:\n  # List displays on the local Device\n  auv invoke display.list\n\n  # Run the same operation on a paired Device\n  auv --device node1 invoke display.list\n\n  # Inspect command-specific arguments and examples\n  auv invoke screen.findText --help\n\nUsage:\n  auv invoke <COMMAND> [OPTIONS]\n\nCommands:\n",
  );
  let mut header = format!("  {:<command_width$}", "");
  for platform in Platform::ALL {
    header.push_str(&format!("{:<SUPPORT_COLUMN_WIDTH$}", platform.label()));
  }
  help.push_str(header.trim_end());
  help.push('\n');
  for command in commands {
    help.push_str(&format!("  {:<command_width$}", command.id));
    for platform in Platform::ALL {
      help.push_str(&format!("{:<SUPPORT_COLUMN_WIDTH$}", support_word(command.support.on(platform))));
    }
    help.push_str(command.description);
    help.push('\n');
  }
  help.push_str(&format!(
    "  {:<command_width$}{}Print help for invoke or one command\n",
    "help",
    " ".repeat(SUPPORT_COLUMN_WIDTH * Platform::ALL.len())
  ));
  help.push_str(
    "\nPlatform support is whether the command's direct local path exists in the current source. It is not a claim that the path was run.\n  yes      a path exists for everything the command accepts\n  partial  a path exists with limits; see \"auv invoke <COMMAND> --help\"\n  no       no path; the command refuses, or completes without doing the work and says so\n  ?        not audited\nEvidence for each claim is kept in docs/SUPPORT_MATRIX.md in the AUV repository.\n",
  );
  help.push_str(
    "\nOptions:\n  --target <TARGET>    Select a command-supported target; see command help for accepted types\n  --dry-run            Validate without performing the operation\n  --store-root <PATH>  Persist the recorded run under this directory\n  --no-overlay         Disable live visual overlay presentation\n  --json               Render machine-readable JSON output\n  --detail             Include diagnostic detail in human output\n  --wide               Include extra columns in human table output\n\nUse \"auv invoke <COMMAND> --help\" for command-specific options.\n",
  );

  help
}

fn collect_commands<'a>(group: &'a CommandGroup, commands: &mut Vec<&'a InvokeCommand>) {
  for child in &group.children {
    match child {
      CommandNode::Command(command) => commands.push(command),
      CommandNode::Group(group) => collect_commands(group, commands),
    }
  }
}

pub fn render_command_help(command: &InvokeCommand) -> String {
  let mut clap_command = crate::command::with_invoke_context(command.clap_command(), command.target);
  let mut help = clap_command.render_long_help().to_string();
  help.push_str("\nPlatform support:\n");
  for platform in Platform::ALL {
    let support = command.support.on(platform);
    help.push_str(&format!("  {:<8}{}", platform.label(), support_word(support)));
    if let Some(note) = support.note {
      help.push_str(&format!(": {note}"));
    }
    help.push('\n');
  }
  match command.role {
    CommandRole::Desktop => {}
    CommandRole::Alias { canonical } => help.push_str(&format!("This command is an alias of {canonical}.\n")),
    CommandRole::Fixture => help.push_str("This command processes fixture data you supply and does not control a desktop.\n"),
  }
  help
}

/// The word the command index and per-command help show for one cell.
fn support_word(support: PlatformSupport) -> &'static str {
  match support.state {
    SupportState::Supported => "yes",
    SupportState::Partial => "partial",
    SupportState::Unsupported => "no",
    SupportState::NotEvidenced => "?",
  }
}

#[cfg(test)]
#[path = "help_test.rs"]
mod tests;
