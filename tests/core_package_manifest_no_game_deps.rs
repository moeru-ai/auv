//! S4 falsifier: root `auv-cli` package must not regain game/godot deps.
//!
//! Prefer this + `rg 'auv_game_' src/` over any `cargo tree -p auv-cli --lib`
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
fn root_auv_cli_package_dependencies_exclude_game_and_godot_crates() {
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
    "auv-cli package [dependencies]/[dev-dependencies]/[target.*.dependencies] must not list game/godot crates; found {offenders:?}. \
     Keep donor wiring in auv-product. Companion falsifier: rg 'auv_game_' src/"
  );
}

#[test]
fn root_library_modules_have_zero_auv_game_crate_path_references() {
  let src_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
  let mut hits = Vec::new();
  scan_rs_for_auv_game(&src_root, &mut hits);
  assert!(hits.is_empty(), "library modules under src/ must not reference auv_game_*; hits={hits:?}");
}

fn scan_rs_for_auv_game(dir: &std::path::Path, hits: &mut Vec<String>) {
  let entries = std::fs::read_dir(dir).unwrap_or_else(|error| panic!("read_dir {}: {error}", dir.display()));
  for entry in entries {
    let entry = entry.expect("dir entry");
    let path = entry.path();
    if path.is_dir() {
      scan_rs_for_auv_game(&path, hits);
      continue;
    }
    if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
      continue;
    }
    let text = std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    if text.contains("auv_game_") {
      hits.push(path.display().to_string());
    }
  }
}
