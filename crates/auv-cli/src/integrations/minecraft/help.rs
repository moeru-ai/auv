//! Minecraft app help (`auv-minecraft --help`).

const INSPECT_OPTIONS: &str = " [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>]";

pub fn render_minecraft_help() -> String {
  let mut help = String::from(
    "\
auv-minecraft — Minecraft integration for spatial-result consumption research

USAGE
",
  );

  for line in MINECRAFT_USAGE_LINES {
    help.push_str("  ");
    help.push_str(line);
    help.push('\n');
  }

  help.push_str("\nCOMMON OPTIONS\n");
  help.push_str("  Most subcommands accept:");
  help.push_str(INSPECT_OPTIONS);
  help.push('\n');

  help
}

const MINECRAFT_USAGE_LINES: &[&str] = &[
  "auv-minecraft bridge --sample <telemetry.jsonl> (--screenshot <frame.png> | --capture-target-app <bundle-id> [--capture-target-title <window-title-substring>]) --target-block <x,y,z> [--capture-skew-ms <ms>] [--screenshot-is-minecraft-window true|false]",
  "auv-minecraft calibrate-projection --frame <minecraft-spatial-frame.json> --screenshot <frame.png> --target-block <x,y,z> [--target-semantics hit_face_center|block_center] [--screenshot-is-minecraft-window true|false]",
  "auv-minecraft live-click --sample <telemetry.jsonl> --screenshot <frame.png> --target-block <x,y,z> --target-app <application-id> --target-title <window title> [--post-sample <telemetry.jsonl>] [--capture-skew-ms <ms>] [--screenshot-is-minecraft-window true|false]",
  "auv-minecraft export-spatial-bundle <run-id> --output-dir <dir>",
  "auv-minecraft export-3dgs-scene-packet --bundle-manifest <bundle/run.json>... --output-dir <dir>",
  "auv-minecraft prepare-texture-sweep --sidecar-run-dir <dir> --output-dir <dir>",
  "auv-minecraft build-texture-sweep-samples --bundle-manifest <bundle/run.json>... --output <samples.json>",
  "auv-minecraft eval-texture-sweep --samples <samples.json> --output-dir <dir> [--require-real-source]",
];

#[cfg(test)]
mod tests {
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
}
