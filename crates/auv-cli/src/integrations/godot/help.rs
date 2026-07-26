//! Godot donor help (`auv-godot --help`).

pub fn render_godot_help() -> String {
  let mut help = String::from(
    "\
auv-godot — Godot donor product binary

USAGE
",
  );

  for line in GODOT_USAGE_LINES {
    help.push_str("  ");
    help.push_str(line);
    help.push('\n');
  }

  help
}

const GODOT_USAGE_LINES: &[&str] = &[
  "auv-godot capability-query [--json]",
  "auv-godot render-observe --output-dir <dir> [--stage <stage>]... [--json]",
];

/// Live usage line for `render-observe` (shared with CLI parse errors).
pub fn render_observe_usage_line() -> &'static str {
  GODOT_USAGE_LINES[1]
}

#[cfg(test)]
#[path = "help_test.rs"]
mod tests;
