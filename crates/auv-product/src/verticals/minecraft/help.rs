//! Minecraft donor help (`auv-minecraft --help`).
//!
//! NOTICE(inspect-composition / S2): Live usage strings name the donor bin
//! (`auv-minecraft`). Root `auv minecraft` is a tombstone only.

const INSPECT_OPTIONS: &str = " [--store-root <path>] [--inspect-local-write true|false|default] [--inspect-server-write true|false|default] [--require-inspect-server-write] [--inspect-server-url <url>] [--inspect-server-token <token>] [--inspect-server-token-file <path>]";

pub fn render_minecraft_help() -> String {
  let mut help = String::from(
    "\
auv-minecraft — reference vertical for spatial-result consumption research

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
  help.push_str("\n");

  help
}

const MINECRAFT_USAGE_LINES: &[&str] = &[
  "auv-minecraft bridge --sample <telemetry.jsonl> (--screenshot <frame.png> | --capture-target-app <bundle-id> [--capture-target-title <window-title-substring>]) --target-block <x,y,z> [--capture-skew-ms <ms>] [--screenshot-is-minecraft-window true|false]",
  "auv-minecraft calibrate-projection --frame <minecraft-spatial-frame.json> --screenshot <frame.png> --target-block <x,y,z> [--target-semantics hit_face_center|block_center] [--screenshot-is-minecraft-window true|false]",
  "auv-minecraft live-click --sample <telemetry.jsonl> --screenshot <frame.png> --target-block <x,y,z> --target-app <application-id> --target-title <window title> [--post-sample <telemetry.jsonl>] [--capture-skew-ms <ms>] [--screenshot-is-minecraft-window true|false]",
  "auv-minecraft query-wired-live-click --training-result-semantic-manifest <semantic.json> --target-block <x,y,z> [--target-face <up|down|north|south|east|west>] [--target-semantics hit_face_center|block_center] [--query-provider checkpoint-native|closed-scene-toy] [--closed-scene-fixture <path>] [--query-command <command>] --output-dir <dir> --target-app <application-id> --target-title <window title> [--sample <telemetry.jsonl>] [--post-sample <telemetry.jsonl>] [--verification-expected-item-id <id>]",
  "auv-minecraft export-spatial-bundle <run-id> --output-dir <dir>",
  "auv-minecraft export-3dgs-scene-packet --bundle-manifest <bundle/run.json>... --output-dir <dir>",
  "auv-minecraft export-3dgs-training-package --scene-packet-manifest <scene-packet/run.json> --output-dir <dir>",
  "auv-minecraft prepare-3dgs-training --training-package-manifest <training-package/run.json> --output-dir <dir>",
  "auv-minecraft launch-3dgs-training-job --training-launch-plan <training-launch-plan.json> --output-dir <dir> [--training-job-endpoint <url>] [--training-job-token <token>] [--training-job-submit-command <command>]",
  "auv-minecraft collect-3dgs-training-job-result --training-job-manifest <training-job.json> --output-dir <dir> [--training-job-endpoint <url>] [--training-job-token <token>] [--training-job-status-command <command>]",
  "auv-minecraft fetch-3dgs-training-result-artifacts --training-result-manifest <training-result.json> --output-dir <dir> [--training-job-endpoint <url>] [--training-job-token <token>] [--artifact-fetch-command <command>]",
  "auv-minecraft validate-3dgs-training-result --training-result-artifact-manifest <d11-manifest.json> --output-dir <dir>",
  "auv-minecraft query-3dgs-training-result --training-result-semantic-manifest <semantic.json> --target-block <x,y,z> [--target-face <up|down|north|south|east|west>] [--target-semantics hit_face_center|block_center] [--query-command <command>] --output-dir <dir>",
  "auv-minecraft inspect-3dgs-training-result-holdout --training-result-semantic-manifest <semantic.json> [--holdout-frame-index <n>] [--holdout-render-command <command>] --output-dir <dir>",
  "auv-minecraft measure-3dgs-holdout-render-quality --training-result-semantic-manifest <semantic.json> --holdout-preview-manifest <mc16.json> --render-command <command> --output-dir <dir>",
  "auv-minecraft prepare-texture-sweep --sidecar-run-dir <dir> --output-dir <dir>",
  "auv-minecraft build-texture-sweep-samples --bundle-manifest <bundle/run.json>... --output <samples.json>",
  "auv-minecraft eval-texture-sweep --samples <samples.json> --output-dir <dir> [--require-real-source]",
];

#[cfg(test)]
mod tests {
  use super::{MINECRAFT_USAGE_LINES, render_minecraft_help};

  #[test]
  fn minecraft_help_lists_all_18_subcommands() {
    let help = render_minecraft_help();
    assert_eq!(MINECRAFT_USAGE_LINES.len(), 18);
    for line in MINECRAFT_USAGE_LINES {
      assert!(help.contains(line), "minecraft help should include usage line: {line}");
    }
    assert!(!help.contains("auv minecraft "));
  }
}
