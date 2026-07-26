use super::{OSU_USAGE_LINES, render_osu_help};

#[test]
fn osu_help_lists_all_5_subcommands() {
  let help = render_osu_help();
  assert_eq!(OSU_USAGE_LINES.len(), 5);
  for line in OSU_USAGE_LINES {
    assert!(help.contains(line), "osu help should include usage line: {line}");
  }
  assert!(!help.contains("auv osu "));
}
