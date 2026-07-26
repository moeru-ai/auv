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
