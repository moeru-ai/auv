use std::path::{Path, PathBuf};

#[test]
fn game_integrations_do_not_export_legacy_artifact_roles() {
  let repo = repository_root();

  for relative in [
    "crates/auv-game-balatro/src/artifact_roles.rs",
    "crates/auv-game-osu/src/artifact_roles.rs",
    "crates/auv-game-minecraft/src/artifact_roles.rs",
  ] {
    assert!(!repo.join(relative).is_file(), "legacy artifact role module still exists: {relative}");
  }

  assert_source_excludes(&repo.join("crates/auv-game-balatro/src/lib.rs"), &["artifact_roles", "_ROLE"]);
  assert_source_excludes(&repo.join("crates/auv-game-osu/src/lib.rs"), &["artifact_roles", "_ROLE"]);
  assert_source_excludes(&repo.join("crates/auv-game-minecraft/src/lib.rs"), &["artifact_roles", "_ARTIFACT_ROLE"]);
  assert_source_excludes(&repo.join("crates/auv-cli/src/integrations/balatro/mod.rs"), &["_ROLE"]);
  assert_source_excludes(&repo.join("crates/auv-cli/src/integrations/osu/mod.rs"), &["_ROLE"]);
  assert_source_excludes(&repo.join("crates/auv-cli/src/integrations/minecraft/mod.rs"), &["artifact_roles", "_ARTIFACT_ROLE"]);
}

#[test]
fn shared_domain_crates_do_not_export_run_artifact_roles() {
  let repo = repository_root();

  assert_source_excludes(&repo.join("crates/auv-driver-common/src/input.rs"), &["INPUT_ACTION_RESULT_ARTIFACT_ROLE"]);
  assert_source_excludes(&repo.join("crates/auv-driver-common/src/lib.rs"), &["INPUT_ACTION_RESULT_ARTIFACT_ROLE"]);
  assert_source_excludes(&repo.join("crates/auv-view/src/memory/mod.rs"), &["VIEW_MEMORY_ARTIFACT_ROLE"]);
  assert_source_excludes(&repo.join("crates/auv-scan/src/coverage_artifact.rs"), &["SCAN_COVERAGE_ARTIFACT_ROLE"]);
  assert_source_excludes(&repo.join("crates/auv-scan/src/lib.rs"), &["SCAN_COVERAGE_ARTIFACT_ROLE"]);
}

#[test]
fn artifact_read_integrity_mechanics_live_only_in_auv_tracing() {
  let repo = repository_root();
  let readers = [
    "src/run_read/mod.rs",
    "crates/auv-game-balatro/src/run_read.rs",
    "crates/auv-game-osu/src/run_read.rs",
    "crates/auv-game-minecraft/src/run_read.rs",
    "crates/auv-netease-music/src/run_artifacts.rs",
    "crates/auv-cli/src/integrations/minecraft/mod.rs",
  ];

  for relative in readers {
    assert_source_excludes(
      &repo.join(relative),
      &[
        "try_reserve_exact(expected_capacity)",
        "while let Some(chunk) = reader.next().await",
        "Sha256::digest(&bytes)",
      ],
    );
  }
}

fn repository_root() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repository root")
}

fn assert_source_excludes(path: &Path, forbidden: &[&str]) {
  let source = std::fs::read_to_string(path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
  for needle in forbidden {
    assert!(!source.contains(needle), "{} still contains legacy export `{needle}`", path.display());
  }
}
