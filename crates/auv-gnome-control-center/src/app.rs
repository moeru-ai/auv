use std::borrow::Cow;

pub const APP_ID: &str = "org.gnome.Settings";
pub const PROCESS_NAME: &str = "gnome-control-center";
pub const DISPLAY_NAME: &str = "GNOME Control Center";

pub const SETTINGS_WINDOW: LabelSet = LabelSet::new(&["设置", "Settings", "GNOME Control Center"]);
pub const SYSTEM_PAGE: LabelSet = LabelSet::new(&["系统", "System"]);
pub const ABOUT_PAGE: LabelSet = LabelSet::new(&["关于", "About"]);
pub const SYSTEM_DETAILS_PAGE: LabelSet = LabelSet::new(&["系统详情", "System Details"]);
pub const COPY_BUTTON: LabelSet = LabelSet::new(&["复制", "Copy"]);
pub const MOUSE_PAGE: LabelSet =
  LabelSet::new(&["鼠标与触摸板", "鼠标", "Mouse & Touchpad", "Mouse"]);
pub const POINTER_SPEED: LabelSet = LabelSet::new(&["指针速度", "Pointer Speed"]);
pub const NATURAL_SCROLLING: LabelSet = LabelSet::new(&["自然", "Natural"]);
pub const TRADITIONAL_SCROLLING: LabelSet = LabelSet::new(&["传统", "Traditional"]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LabelSet {
  labels: &'static [&'static str],
}

impl LabelSet {
  pub const fn new(labels: &'static [&'static str]) -> Self {
    Self { labels }
  }

  pub const fn labels(self) -> &'static [&'static str] {
    self.labels
  }

  pub fn best_match<'a>(self, value: &'a str) -> Option<&'static str> {
    let normalized = normalize(value);
    self
      .labels
      .iter()
      .copied()
      .find(|label| normalize(label) == normalized)
      .or_else(|| {
        self.labels.iter().copied().find(|label| {
          let label = normalize(label);
          !label.is_empty() && normalized.contains(label.as_ref())
        })
      })
  }

  pub fn display(self) -> String {
    self.labels.join(" | ")
  }
}

pub fn normalize(value: &str) -> Cow<'_, str> {
  let normalized = value
    .chars()
    .filter(|ch| !ch.is_whitespace() && !ch.is_ascii_punctuation())
    .flat_map(char::to_lowercase)
    .collect::<String>();
  Cow::Owned(normalized)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn label_set_matches_exact_or_containing_text() {
    assert_eq!(SYSTEM_PAGE.best_match("系统"), Some("系统"));
    assert_eq!(SYSTEM_PAGE.best_match("System Settings"), Some("System"));
    assert_eq!(COPY_BUTTON.best_match("Copy"), Some("Copy"));
  }
}
