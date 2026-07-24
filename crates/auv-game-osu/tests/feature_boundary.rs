use std::path::{Path, PathBuf};
use std::process::Command;

fn manifest_path() -> PathBuf {
  Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")
}

fn cargo_metadata() -> serde_json::Value {
  let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
    .args([
      "metadata",
      "--format-version",
      "1",
      "--no-deps",
      "--manifest-path",
    ])
    .arg(manifest_path())
    .output()
    .expect("run cargo metadata for auv-game-osu");
  assert!(output.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));
  serde_json::from_slice(&output.stdout).expect("decode cargo metadata")
}

fn osu_package(metadata: &serde_json::Value) -> &serde_json::Value {
  metadata["packages"]
    .as_array()
    .expect("metadata packages")
    .iter()
    .find(|package| package["name"] == "auv-game-osu")
    .expect("auv-game-osu package")
}

#[test]
fn tracing_feature_is_optional_and_disabled_by_default() {
  let metadata = cargo_metadata();
  let package = osu_package(&metadata);
  let features = package["features"].as_object().expect("package features");

  assert_eq!(features.get("default"), Some(&serde_json::json!([])));
  assert!(
    features
      .get("tracing")
      .and_then(serde_json::Value::as_array)
      .is_some_and(|members| members.iter().any(|member| member == "dep:auv-tracing")),
    "tracing feature must activate the optional auv-tracing dependency"
  );

  let tracing_dependency = package["dependencies"]
    .as_array()
    .expect("package dependencies")
    .iter()
    .find(|dependency| dependency["name"] == "auv-tracing")
    .expect("auv-tracing dependency");
  assert_eq!(tracing_dependency["optional"], true, "auv-tracing must be optional");
}

#[test]
fn domain_only_dependency_tree_excludes_auv_tracing() {
  let output = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
    .args([
      "tree",
      "--manifest-path",
      manifest_path().to_str().expect("UTF-8 manifest path"),
      "--package",
      "auv-game-osu",
      "--no-default-features",
      "--edges",
      "normal",
    ])
    .output()
    .expect("run cargo tree for domain-only auv-game-osu");
  assert!(output.status.success(), "cargo tree failed: {}", String::from_utf8_lossy(&output.stderr));

  let tree = String::from_utf8(output.stdout).expect("cargo tree output is UTF-8");
  assert!(!tree.lines().any(|line| line.contains("auv-tracing v")), "domain-only dependency tree still contains auv-tracing:\n{tree}");
}

#[test]
fn tracing_contract_test_requires_the_tracing_feature() {
  let metadata = cargo_metadata();
  let package = osu_package(&metadata);
  let target = package["targets"]
    .as_array()
    .expect("package targets")
    .iter()
    .find(|target| target["name"] == "tracing_contract")
    .expect("tracing_contract test target");
  let required_features = target["required-features"].as_array().expect("tracing_contract required-features");

  assert!(required_features.iter().any(|feature| feature == "tracing"));
}
