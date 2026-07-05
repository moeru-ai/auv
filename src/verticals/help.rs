//! Help-only index for reference verticals (`auv verticals --help`).

pub fn render_verticals_help() -> String {
  String::from(
    "\
auv verticals — reference / research verticals

USAGE
  auv verticals [--help]
  auv verticals help

VERTICALS
  auv minecraft --help
  auv osu --help

NOTES
  - Reference verticals support spatial-result consumption research; they are not the default AUV product path.
  - `verticals` is a help-only index, not an execution namespace. Run commands via `auv minecraft ...` or `auv osu ...`.
",
  )
}

#[cfg(test)]
mod tests {
  use super::render_verticals_help;

  #[test]
  fn verticals_help_lists_minecraft_and_osu() {
    let help = render_verticals_help();
    assert!(help.contains("auv minecraft --help"));
    assert!(help.contains("auv osu --help"));
    assert!(help.contains("help-only index"));
  }
}
