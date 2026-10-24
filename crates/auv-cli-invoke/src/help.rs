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
    "\nSelected Runner support is the same source-level claim for the route a selected Run or Device takes; see it in \"auv invoke <COMMAND> --help\".\n",
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
  push_support_lines(&mut help, command.support);
  help.push_str("\nSelected Runner support:\n");
  match command.runner.cells(command.support) {
    None => help.push_str("  no Runner route; run it without a selected Run or Device\n"),
    Some(cells) if cells == command.support => help.push_str("  same as the direct local path above\n"),
    Some(cells) => push_support_lines(&mut help, cells),
  }
  match command.role {
    CommandRole::Desktop => {}
    CommandRole::Alias { canonical } => help.push_str(&format!("This command is an alias of {canonical}.\n")),
    CommandRole::Fixture => help.push_str("This command processes fixture data you supply and does not control a desktop.\n"),
  }
  help
}

/// The per-platform cell lines one support block shows: the word for each
/// state and, where a cell has one, its note.
fn push_support_lines(help: &mut String, support: crate::CommandSupport) {
  for platform in Platform::ALL {
    let cell = support.on(platform);
    help.push_str(&format!("  {:<8}{}", platform.label(), support_word(cell)));
    if let Some(note) = cell.note {
      help.push_str(&format!(": {note}"));
    }
    help.push('\n');
  }
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

/// One documented call of a command, parsed from its CLI help example.
///
/// CLI `--help` examples are the single source: each `auv invoke <id> ...`
/// line goes through the command's own argument parser, so every example a
/// frontend shows (CLI text, MCP JSON) is a call the command accepts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InvokeExample {
  /// The `# ...` comment line written above the example, if any.
  pub summary: Option<String>,
  /// The example exactly as the CLI help prints it.
  pub cli: String,
  pub target: Option<crate::ExecutionTarget>,
  /// Protocol inputs the example resolves to; MCP `invoke` takes the same map.
  pub inputs: std::collections::BTreeMap<String, String>,
}

impl InvokeCommand {
  /// Parses the `Examples:` section of this command's long help.
  ///
  /// Returns an error naming the first example the command rejects, so a
  /// stale example fails tests instead of misleading a caller.
  pub fn examples(&self) -> Result<Vec<InvokeExample>, String> {
    let help = self.clap_command().get_after_long_help().map(ToString::to_string).unwrap_or_default();
    let mut examples = Vec::new();
    let mut summary = None;
    for line in help.lines().map(str::trim) {
      if let Some(comment) = line.strip_prefix('#') {
        summary = Some(comment.trim().to_string());
        continue;
      }
      let Some(tokens) = shlex::split(line) else {
        continue;
      };
      // Aliases share one argument struct (input.keys and input.pressKeys),
      // so an example may name either id; it parses against this command.
      let Some(position) = tokens.iter().position(|token| token == "invoke") else {
        continue;
      };
      if tokens.len() < position + 2 {
        continue;
      }
      if tokens.first().map(String::as_str) != Some("auv") {
        continue;
      }
      let parsed =
        self.parse_cli_args(&tokens[position + 2..]).map_err(|error| format!("{} example `{line}` does not parse: {error}", self.id))?;
      let crate::command::InvokeCommandCliParse::Invoke { target, inputs, .. } = parsed else {
        return Err(format!("{} example `{line}` asks for help instead of invoking", self.id));
      };
      let named = &tokens[position + 1];
      examples.push(InvokeExample {
        summary: summary.take(),
        cli: if named == self.id {
          line.to_string()
        } else {
          line.replacen(named.as_str(), self.id, 1)
        },
        target,
        inputs,
      });
    }
    Ok(examples)
  }
}

#[cfg(test)]
#[path = "help_test.rs"]
mod tests;
