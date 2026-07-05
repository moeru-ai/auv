//! Osu reference vertical help (`auv osu --help`).

pub fn render_osu_help() -> String {
  let mut help = String::from(
    "\
auv osu — reference vertical for spatial-result consumption research

USAGE
",
  );

  for line in OSU_USAGE_LINES {
    help.push_str("  ");
    help.push_str(line);
    help.push('\n');
  }

  help
}

const OSU_USAGE_LINES: &[&str] = &[
  "auv osu benchmark <beatmap.osu> [--output-dir <dir>]",
  "auv osu dispatch <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]",
  "auv osu export-dataset <run-artifact-dir> --output-dir <dir>",
  "auv osu eval-detections <run-artifact-dir> --detections <dir-or-json> [--output-dir <dir>]",
  "auv osu vision-demo <beatmap.osu> --target-app <name> [--output-dir <dir>] [--dispatch-limit <n>] [--capture-verify]",
];

#[cfg(test)]
mod tests {
  use super::{OSU_USAGE_LINES, render_osu_help};

  #[test]
  fn osu_help_lists_all_5_subcommands() {
    let help = render_osu_help();
    assert_eq!(OSU_USAGE_LINES.len(), 5);
    for line in OSU_USAGE_LINES {
      assert!(
        help.contains(line),
        "osu help should include usage line: {line}"
      );
    }
  }
}
