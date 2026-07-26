use super::{MINECRAFT_USAGE_LINES, render_minecraft_help};

#[test]
fn minecraft_help_lists_all_subcommands() {
  let help = render_minecraft_help();
  assert_eq!(MINECRAFT_USAGE_LINES.len(), 8);
  for line in MINECRAFT_USAGE_LINES {
    assert!(help.contains(line), "minecraft help should include usage line: {line}");
  }
  assert!(!help.contains("auv minecraft "));
  assert!(!help.contains("--inspect-server-token"), "Minecraft help must not advertise retired Inspect credentials");
}
