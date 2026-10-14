//! Desktop-entry application ids for X11 windows.
//!
//! X11 windows carry WM_CLASS, not the desktop-file id that launchers and
//! models use (`gimp`, `libreoffice-calc`, `google-chrome`). A desktop entry
//! links the two through `StartupWMClass`, else through its `Exec` program,
//! whose basename is the usual WM_CLASS instance (`gimp-2.10`). This module
//! maps a requested id to those WM_CLASS aliases; matching stays in `window`.

use std::{
  env, fs,
  path::{Path, PathBuf},
};

/// WM_CLASS names that the desktop entry `app_id` (with or without the
/// `.desktop` suffix) declares for its windows. Empty when no entry exists.
pub(crate) fn wm_aliases(app_id: &str) -> Vec<String> {
  aliases_in(&data_dirs(), app_id)
}

/// XDG base directories in lookup order: the user's data home, then the
/// system data dirs.
// NOTICE(xdg-data-defaults): `/usr/local/share` and `/usr/share` are the
// defaults the XDG Base Directory specification defines for an unset or
// empty `XDG_DATA_DIRS`; they are platform-defined, not configuration.
fn data_dirs() -> Vec<PathBuf> {
  let mut dirs = Vec::new();
  match env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
    Some(home) => dirs.push(PathBuf::from(home)),
    None => {
      if let Some(home) = env::var_os("HOME").filter(|value| !value.is_empty()) {
        dirs.push(Path::new(&home).join(".local").join("share"));
      }
    }
  }
  match env::var_os("XDG_DATA_DIRS").filter(|value| !value.is_empty()) {
    Some(system) => dirs.extend(env::split_paths(&system)),
    None => dirs.extend([
      PathBuf::from("/usr/local/share"),
      PathBuf::from("/usr/share"),
    ]),
  }
  dirs
}

fn aliases_in(dirs: &[PathBuf], app_id: &str) -> Vec<String> {
  let id = app_id.strip_suffix(".desktop").unwrap_or(app_id);
  // A desktop id never contains a path separator; refuse to look outside the
  // applications directories.
  if id.is_empty() || id.contains(['/', '\\']) || id == "." || id == ".." {
    return Vec::new();
  }
  let file_name = format!("{id}.desktop");
  for dir in dirs {
    let applications = dir.join("applications");
    // Exact file first, then a case-insensitive match: callers spell ids
    // with arbitrary case (`Gimp`, `LibreOffice-Calc`).
    let path = applications.join(&file_name);
    let path = if path.is_file() {
      Some(path)
    } else {
      fs::read_dir(&applications).ok().and_then(|entries| {
        entries
          .flatten()
          .map(|entry| entry.path())
          .find(|path| path.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.eq_ignore_ascii_case(&file_name)))
      })
    };
    if let Some(text) = path.and_then(|path| fs::read_to_string(path).ok()) {
      return parse_aliases(&text);
    }
  }
  Vec::new()
}

/// `StartupWMClass` and the `Exec` program basename from `[Desktop Entry]`.
fn parse_aliases(text: &str) -> Vec<String> {
  let mut aliases = Vec::new();
  let mut in_entry = false;
  for line in text.lines().map(str::trim) {
    if line.starts_with('[') {
      in_entry = line == "[Desktop Entry]";
      continue;
    }
    if !in_entry {
      continue;
    }
    let Some((key, value)) = line.split_once('=') else {
      continue;
    };
    let value = value.trim();
    match key.trim() {
      "StartupWMClass" if !value.is_empty() => aliases.push(value.to_string()),
      "Exec" => {
        // Skip `env VAR=value` prefixes; field codes (%U) follow the program.
        let program = value.split_whitespace().map(|part| part.trim_matches('"')).find(|part| *part != "env" && !part.contains('='));
        if let Some(name) = program.and_then(|program| Path::new(program).file_name()).and_then(|name| name.to_str()) {
          aliases.push(name.to_string());
        }
      }
      _ => {}
    }
  }
  aliases.dedup();
  aliases
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn startup_wm_class_and_exec_basename_become_aliases() {
    let text = "[Desktop Entry]\nName=GIMP\nExec=env GDK_BACKEND=x11 /usr/bin/gimp-2.10 %U\nStartupWMClass=Gimp-2.10\n\n[Desktop Action new]\nExec=other\n";
    assert_eq!(parse_aliases(text), ["gimp-2.10", "Gimp-2.10"]);
    assert_eq!(parse_aliases("[Desktop Entry]\nExec=\"google-chrome-stable\" %U\n"), ["google-chrome-stable"]);
    assert!(parse_aliases("[Other]\nStartupWMClass=x\n").is_empty());
  }

  #[test]
  fn desktop_ids_resolve_case_insensitively_in_xdg_order() {
    let root = env::temp_dir().join(format!("auv-x11-desktop-test-{}", std::process::id()));
    let user = root.join("user");
    let system = root.join("system");
    fs::create_dir_all(user.join("applications")).unwrap();
    fs::create_dir_all(system.join("applications")).unwrap();
    fs::write(
      system.join("applications").join("libreoffice-calc.desktop"),
      "[Desktop Entry]\nExec=libreoffice --calc %U\nStartupWMClass=libreoffice-calc\n",
    )
    .unwrap();
    fs::write(system.join("applications").join("gimp.desktop"), "[Desktop Entry]\nExec=gimp-2.10 %U\n").unwrap();
    fs::write(user.join("applications").join("gimp.desktop"), "[Desktop Entry]\nExec=my-gimp\n").unwrap();
    let dirs = [user, system];

    assert_eq!(aliases_in(&dirs, "LibreOffice-Calc.desktop"), ["libreoffice", "libreoffice-calc"]);
    // The user's entry shadows the system one, as in XDG lookup.
    assert_eq!(aliases_in(&dirs, "gimp"), ["my-gimp"]);
    assert!(aliases_in(&dirs, "missing").is_empty());
    assert!(aliases_in(&dirs, "../system/applications/gimp").is_empty());
    fs::remove_dir_all(root).unwrap();
  }
}
