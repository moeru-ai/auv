//! Guards the core `auv-runtime` package against regaining game or Godot dependencies.
//!
//! Prefer this + `rg 'auv_game_' src/` over any `cargo tree -p auv-runtime --lib`
//! trick (that graph is not a reliable library-only proof).

fn package_dependency_table_bodies(cargo_toml: &str) -> Vec<String> {
  let mut tables = Vec::new();
  let mut current: Option<String> = None;
  let mut capturing = false;

  for line in cargo_toml.lines() {
    let trimmed = line.trim();
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
      if capturing {
        if let Some(body) = current.take() {
          tables.push(body);
        }
      }
      let name = &trimmed[1..trimmed.len() - 1];
      capturing = matches!(name, "dependencies" | "dev-dependencies" | "build-dependencies")
        || (name.starts_with("target.")
          && (name.ends_with(".dependencies") || name.ends_with(".dev-dependencies") || name.ends_with(".build-dependencies")));
      current = capturing.then(String::new);
      continue;
    }
    if let Some(body) = current.as_mut() {
      body.push_str(line);
      body.push('\n');
    }
  }
  if capturing {
    if let Some(body) = current.take() {
      tables.push(body);
    }
  }
  tables
}

fn dependency_keys(table_body: &str) -> Vec<String> {
  let mut keys = Vec::new();
  for line in table_body.lines() {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
      continue;
    }
    if trimmed.starts_with('[') {
      break;
    }
    let key = trimmed.split([' ', '=', '.']).next().unwrap_or("");
    if !key.is_empty() {
      keys.push(key.to_string());
    }
  }
  keys
}

#[test]
fn root_auv_runtime_package_dependencies_exclude_game_and_godot_crates() {
  let cargo_toml = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"));
  let mut offenders = Vec::new();

  for table in package_dependency_table_bodies(cargo_toml) {
    for key in dependency_keys(&table) {
      if key.starts_with("auv-game-") || key == "auv-godot" {
        offenders.push(key);
      }
    }
  }

  assert!(
    offenders.is_empty(),
    "auv-runtime package [dependencies]/[dev-dependencies]/[target.*.dependencies] must not list game/godot crates; found {offenders:?}. \
     Keep donor wiring in the product CLI package. Companion falsifier: rg 'auv_game_' src/"
  );
}
