use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextMatcher {
  Exact(String),
  Contains(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSelector {
  pub bundle: Option<TextMatcher>,
  pub name: Option<TextMatcher>,
  pub process_id: Option<u32>,
  pub frontmost: bool,
}

pub struct App;

impl App {
  pub fn bundle(bundle: impl Into<String>) -> AppSelector {
    AppSelector {
      bundle: Some(TextMatcher::Exact(bundle.into())),
      ..AppSelector::default()
    }
  }

  pub fn bundle_id(bundle_id: impl Into<String>) -> AppSelector {
    Self::bundle(bundle_id)
  }

  pub fn name(name: impl Into<String>) -> AppSelector {
    AppSelector {
      name: Some(TextMatcher::Exact(name.into())),
      ..AppSelector::default()
    }
  }

  pub fn pid(process_id: u32) -> AppSelector {
    AppSelector {
      process_id: Some(process_id),
      ..AppSelector::default()
    }
  }

  pub fn process_id(process_id: u32) -> AppSelector {
    Self::pid(process_id)
  }

  pub fn frontmost() -> AppSelector {
    AppSelector {
      frontmost: true,
      ..AppSelector::default()
    }
  }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSelector {
  pub app: Option<AppSelector>,
  pub title: Option<TextMatcher>,
  pub main_visible: bool,
}

impl WindowSelector {
  pub fn owned_by(mut self, app: AppSelector) -> Self {
    self.app = Some(app);
    self
  }

  pub fn title_contains(mut self, title: impl Into<String>) -> Self {
    self.title = Some(TextMatcher::Contains(title.into()));
    self
  }

  pub fn title_exact(mut self, title: impl Into<String>) -> Self {
    self.title = Some(TextMatcher::Exact(title.into()));
    self
  }
}

pub struct Window;

impl Window {
  pub fn main_visible() -> WindowSelector {
    WindowSelector {
      main_visible: true,
      ..WindowSelector::default()
    }
  }

  pub fn titled(title: impl Into<String>) -> WindowSelector {
    Self::title_exact(title)
  }

  pub fn title_contains(title: impl Into<String>) -> WindowSelector {
    WindowSelector {
      title: Some(TextMatcher::Contains(title.into())),
      ..WindowSelector::default()
    }
  }

  pub fn title_exact(title: impl Into<String>) -> WindowSelector {
    WindowSelector {
      title: Some(TextMatcher::Exact(title.into())),
      ..WindowSelector::default()
    }
  }
}
