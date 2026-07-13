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
mod tests {
  use super::{GODOT_USAGE_LINES, render_godot_help};

  #[test]
  fn godot_help_lists_live_bin_usage() {
    let help = render_godot_help();
    assert_eq!(GODOT_USAGE_LINES.len(), 2);
    for line in GODOT_USAGE_LINES {
      assert!(help.contains(line), "godot help should include usage line: {line}");
    }
    assert!(!help.contains("auv godot "));
  }
}
