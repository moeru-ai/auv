use auv_cli_invoke::{CommandGroup, InvokeCommand, InvokeRegistry};

const BINARY_USAGE: &str = "auv-qqmusic invoke";

pub fn render_help_index(registry: &InvokeRegistry) -> String {
  let mut help = String::from(
    "USAGE\n  auv-qqmusic invoke <command> [options]\n\nOPTIONS\n  --dry-run  Validate inputs without writing a store proof\n\nUse auv-qqmusic invoke <command> --help for command-specific options.\n",
  );

  for group in registry.groups() {
    if !has_commands(group) {
      continue;
    }
    render_group_index(&mut help, group, 0);
  }

  help
}

fn render_group_index(help: &mut String, group: &CommandGroup, depth: usize) {
  if !has_commands(group) {
    return;
  }

  help.push('\n');
  if depth == 0 {
    help.push_str(group.heading);
  } else {
    help.push_str(&"  ".repeat(depth));
    help.push_str(group.heading);
  }
  help.push('\n');

  for child in &group.children {
    match child {
      auv_cli_invoke::CommandNode::Command(command) => {
        help.push_str(&"  ".repeat(depth + 1));
        help.push_str(command.id);
        help.push_str("  ");
        help.push_str(command.summary);
        help.push('\n');
      }
      auv_cli_invoke::CommandNode::Group(group) => render_group_index(help, group, depth + 1),
    }
  }
}

fn has_commands(group: &CommandGroup) -> bool {
  group.children.iter().any(|child| match child {
    auv_cli_invoke::CommandNode::Command(_) => true,
    auv_cli_invoke::CommandNode::Group(group) => has_commands(group),
  })
}

pub fn render_command_help(command: &InvokeCommand) -> String {
  let mut help = format!("USAGE\n  {BINARY_USAGE} {}", command.id);
  for arg in command.args {
    if arg.required {
      help.push_str(&format!(
        " --{} <{}>",
        arg.flag.trim_start_matches("--"),
        arg.value_name
      ));
    } else {
      help.push_str(&format!(
        " [--{} <{}>]",
        arg.flag.trim_start_matches("--"),
        arg.value_name
      ));
    }
  }
  help.push_str("\n\n");
  help.push_str(command.summary);
  help.push_str("\n\nOPTIONS\n");
  for arg in command.args {
    help.push_str(&format!(
      "  {} <{}>{}\n    {}\n",
      arg.flag,
      arg.value_name,
      if arg.required { "" } else { "  (optional)" },
      arg.help
    ));
  }
  help
}
