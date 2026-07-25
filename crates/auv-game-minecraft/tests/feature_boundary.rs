use std::process::Command;

use serde_json::Value;

#[test]
fn tracing_dependencies_are_opt_in_without_retired_training_contract_target() {
  let manifest_path = format!("{}/Cargo.toml", env!("CARGO_MANIFEST_DIR"));
  let output = Command::new(env!("CARGO"))
    .args([
      "metadata",
      "--format-version",
      "1",
      "--no-deps",
      "--manifest-path",
      &manifest_path,
    ])
    .output()
    .expect("run cargo metadata");
  assert!(output.status.success(), "cargo metadata failed: {}", String::from_utf8_lossy(&output.stderr));

  let metadata: Value = serde_json::from_slice(&output.stdout).expect("decode cargo metadata");
  let package = metadata["packages"]
    .as_array()
    .and_then(|packages| packages.iter().find(|package| package["name"] == "auv-game-minecraft"))
    .expect("auv-game-minecraft package");

  for dependency_name in ["auv-tracing", "auv-inspect-model"] {
    let dependency = package["dependencies"]
      .as_array()
      .and_then(|dependencies| dependencies.iter().find(|dependency| dependency["name"] == dependency_name && dependency["kind"].is_null()))
      .unwrap_or_else(|| panic!("{dependency_name} dependency"));
    assert_eq!(dependency["optional"], true, "{dependency_name} must be optional");
  }

  let tracing_features = package["features"]["tracing"].as_array().expect("tracing feature");
  for dependency_feature in ["dep:auv-tracing", "dep:auv-inspect-model"] {
    assert!(tracing_features.iter().any(|feature| feature == dependency_feature), "tracing feature must enable {dependency_feature}");
  }

  assert!(
    package["targets"].as_array().is_some_and(|targets| targets.iter().all(|target| target["name"] != "tracing_contract")),
    "the retired training artifact contract target must not remain"
  );
}
