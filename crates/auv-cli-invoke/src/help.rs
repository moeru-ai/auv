use crate::{ArgSpec, CommandGroup, CommandNode, InvokeCommand, InvokeRegistry};

pub fn render_help_index(registry: &InvokeRegistry) -> String {
  let mut help = String::from(
    "USAGE\n  auv invoke <command> [options]\n\nUse auv invoke <command> --help for command-specific options.\n",
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
      CommandNode::Command(command) => {
        help.push_str(&"  ".repeat(depth + 1));
        help.push_str(command.id);
        help.push_str("  ");
        help.push_str(command.summary);
        help.push('\n');
      }
      CommandNode::Group(group) => render_group_index(help, group, depth + 1),
    }
  }
}

fn has_commands(group: &CommandGroup) -> bool {
  group.children.iter().any(|child| match child {
    CommandNode::Command(_) => true,
    CommandNode::Group(group) => has_commands(group),
  })
}

pub fn render_command_help(command: &InvokeCommand) -> String {
  let mut help = format!(
    "COMMAND\n  {}\n\nUSAGE\n  auv invoke {}{}\n\nSUMMARY\n  {}\n",
    command.id,
    command.id,
    render_usage_args(command.args),
    command.summary
  );

  help.push_str("\nOPTIONS\n");
  if command.args.is_empty() {
    help.push_str("  none\n");
  } else {
    for arg in command.args {
      help.push_str("  ");
      help.push_str(arg.flag);
      help.push(' ');
      help.push_str(arg.value_name);
      if arg.required {
        help.push_str("  required  ");
      } else {
        help.push_str("  optional  ");
      }
      help.push_str(arg.help);
      help.push('\n');
    }
  }

  help
}

fn render_usage_args(args: &[ArgSpec]) -> String {
  let mut usage = String::new();
  for arg in args {
    usage.push(' ');
    if !arg.required {
      usage.push('[');
    }
    usage.push_str(arg.flag);
    usage.push(' ');
    usage.push_str(arg.value_name);
    if !arg.required {
      usage.push(']');
    }
  }
  usage
}
