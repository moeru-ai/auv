//! Windows-specific NetEase Cloud Music window identity and resolution.
//!
//! Product selectors live here rather than in `auv-driver-windows`: the driver
//! owns generic Win32/UIA capabilities, while this crate owns the knowledge
//! that NetEase normally runs as `cloudmusic.exe`.

use auv_driver::window::Window;

pub const DEFAULT_PROCESS_NAME: &str = "cloudmusic.exe";
pub const DEFAULT_WINDOW_TITLE: &str = "网易云音乐";
pub const ENGLISH_WINDOW_TITLE: &str = "NetEase Cloud Music";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolveOptions {
  pub process_name: String,
  pub title: String,
}

impl Default for ResolveOptions {
  fn default() -> Self {
    Self {
      process_name: DEFAULT_PROCESS_NAME.to_string(),
      title: DEFAULT_WINDOW_TITLE.to_string(),
    }
  }
}

#[cfg(target_os = "windows")]
pub fn resolve_window(options: &ResolveOptions) -> Result<Option<Window>, String> {
  use auv_driver::error::DriverError;
  use auv_driver::selector::{App, Window as WindowSelector};

  let session = auv_driver::open_local().map_err(|error| format!("failed to open Windows driver: {error}"))?;

  let by_process = WindowSelector::main_visible().owned_by(App::name(options.process_name.clone()));
  match session.window().resolve(by_process) {
    Ok(window) => return Ok(Some(window)),
    Err(DriverError::NotFound { .. }) => {}
    Err(error) => {
      return Err(format!("failed to resolve NetEase window by process name: {error}"));
    }
  }

  let mut titles = candidate_titles(options);
  titles.dedup();
  for title in titles {
    match session.window().resolve(WindowSelector::title_contains(title)) {
      Ok(window) => return Ok(Some(window)),
      Err(DriverError::NotFound { .. }) => {}
      Err(error) => {
        return Err(format!("failed to resolve NetEase window by title {title:?}: {error}"));
      }
    }
  }

  Ok(None)
}

/// Builds the ordered list of window-title candidates to try, preferring the
/// caller-supplied title before falling back to NetEase's known localized and
/// English window titles.
///
/// `dedup()` only removes *consecutive* duplicates, so this only collapses
/// the common case where `options.title` already equals `DEFAULT_WINDOW_TITLE`;
/// it does not deduplicate non-adjacent repeats.
fn candidate_titles(options: &ResolveOptions) -> Vec<&str> {
  vec![
    options.title.as_str(),
    DEFAULT_WINDOW_TITLE,
    ENGLISH_WINDOW_TITLE,
    "CloudMusic",
  ]
}

#[cfg(not(target_os = "windows"))]
pub fn resolve_window(_options: &ResolveOptions) -> Result<Option<Window>, String> {
  Ok(None)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn candidate_titles_dedups_when_option_title_matches_the_default() {
    let options = ResolveOptions::default();
    let titles = candidate_titles(&options);
    let mut deduped = titles.clone();
    deduped.dedup();

    assert_eq!(
      titles,
      [
        DEFAULT_WINDOW_TITLE,
        DEFAULT_WINDOW_TITLE,
        ENGLISH_WINDOW_TITLE,
        "CloudMusic"
      ]
    );
    assert_eq!(deduped, [DEFAULT_WINDOW_TITLE, ENGLISH_WINDOW_TITLE, "CloudMusic"]);
  }

  #[test]
  fn candidate_titles_keeps_a_custom_option_title_first() {
    let options = ResolveOptions {
      process_name: DEFAULT_PROCESS_NAME.to_string(),
      title: "My NetEase".to_string(),
    };

    let titles = candidate_titles(&options);

    assert_eq!(
      titles,
      [
        "My NetEase",
        DEFAULT_WINDOW_TITLE,
        ENGLISH_WINDOW_TITLE,
        "CloudMusic"
      ]
    );
  }

  #[test]
  fn candidate_titles_does_not_dedup_non_adjacent_repeats() {
    // NOTICE: dedup() only removes consecutive duplicates. An option title
    // equal to `ENGLISH_WINDOW_TITLE` (not adjacent to it in this list) is not
    // deduplicated; this documents that existing, unchanged behavior.
    let options = ResolveOptions {
      process_name: DEFAULT_PROCESS_NAME.to_string(),
      title: ENGLISH_WINDOW_TITLE.to_string(),
    };

    let mut titles = candidate_titles(&options);
    titles.dedup();

    assert_eq!(
      titles,
      [
        ENGLISH_WINDOW_TITLE,
        DEFAULT_WINDOW_TITLE,
        ENGLISH_WINDOW_TITLE,
        "CloudMusic"
      ]
    );
  }
}
