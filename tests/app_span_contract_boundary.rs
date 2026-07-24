use std::fs;
use std::path::{Path, PathBuf};

#[test]
fn app_instrumentation_uses_operation_spans_instead_of_cli_command_metadata() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  let files = [
    "crates/auv-apple-music/src/lib.rs",
    "crates/auv-apple-notes/src/lib.rs",
    "crates/auv-apple-textedit/src/lib.rs",
    "crates/auv-qqmusic/src/lib.rs",
    "crates/auv-gnome-control-center/src/lib.rs",
  ];
  let forbidden = [
    "auv.command.id",
    "CommandSpan",
    "fn command<T>",
    ".command\"",
  ];

  let violations = files
    .into_iter()
    .flat_map(|relative| {
      let path = root.join(relative);
      let source = fs::read_to_string(&path).unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
      forbidden
        .into_iter()
        .filter(move |token| source.contains(token))
        .map(move |token| format!("{} contains {token:?}", display_path(root, &path)))
    })
    .collect::<Vec<_>>();

  assert!(
    violations.is_empty(),
    "app libraries must emit stable app-operation spans without CLI command metadata:\n{}",
    violations.join("\n")
  );
}

#[test]
fn reusable_app_crates_keep_auv_tracing_opt_in() {
  let root = Path::new(env!("CARGO_MANIFEST_DIR"));
  for crate_name in [
    "auv-apple-music",
    "auv-apple-notes",
    "auv-apple-textedit",
    "auv-qqmusic",
    "auv-gnome-control-center",
    "auv-media-macos",
    "auv-netease-music",
    "auv-game-balatro",
    "auv-game-minecraft",
    "auv-game-osu",
  ] {
    let relative = format!("crates/{crate_name}/Cargo.toml");
    let source = fs::read_to_string(root.join(&relative)).unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
    assert!(
      feature_contains(&source, "tracing", "dep:auv-tracing"),
      "{crate_name} must activate auv-tracing only through its tracing feature"
    );
    assert!(dependency_is_optional(&source, "auv-tracing"), "{crate_name} must declare auv-tracing as optional");
  }
}

fn feature_contains(manifest: &str, feature: &str, member: &str) -> bool {
  let Some(features) = manifest.split("[features]").nth(1) else {
    return false;
  };
  let features = features.split("\n[").next().unwrap_or(features);
  let prefix = format!("{feature} =");
  let Some(declaration) = features.lines().position(|line| line.trim_start().starts_with(&prefix)) else {
    return false;
  };
  let mut declaration_text = String::new();
  for line in features.lines().skip(declaration) {
    declaration_text.push_str(line);
    if line.contains(']') {
      break;
    }
  }
  declaration_text.contains(&format!("\"{member}\""))
}

fn dependency_is_optional(manifest: &str, dependency: &str) -> bool {
  let inline_prefix = format!("{dependency} =");
  if manifest.lines().find(|line| line.trim_start().starts_with(&inline_prefix)).is_some_and(|line| line.contains("optional = true")) {
    return true;
  }

  let table = format!("[dependencies.{dependency}]");
  let Some(section) = manifest.split(&table).nth(1) else {
    return false;
  };
  section.lines().skip(1).take_while(|line| !line.trim_start().starts_with('[')).any(|line| line.trim() == "optional = true")
}

fn display_path(root: &Path, path: &PathBuf) -> String {
  path.strip_prefix(root).unwrap_or(path).display().to_string()
}
