use std::time::Duration;

use auv_driver::LocalDriverSession;
use auv_driver::{
  ActivationPolicy, App, Click, ClickOptions, InputPolicy, KeyPressOptions, PasteTextOptions, PrepareForInputOptions, TextSubmit,
  WaitOptions, Window, WindowPoint, WindowSelector,
};
use auv_driver_macos::MacosDriverSession;

use crate::search::{DEFAULT_SEARCH_REGION, SearchAnchorMatch, SearchStep};

pub type OperationResult<T> = Result<T, String>;

pub trait QqMusicDriver {
  fn activate_app(&mut self, app_id: &str, settle: Duration) -> OperationResult<SearchStep>;

  fn press_search_shortcut(&mut self, shortcut: &str, settle: Duration) -> OperationResult<SearchStep>;

  fn paste_query(&mut self, query: &str, settle: Duration) -> OperationResult<SearchStep>;

  fn wait_anchor(&mut self, app_id: &str, anchor: &str, timeout: Duration) -> OperationResult<SearchAnchorMatch>;

  fn click_anchor(&mut self, app_id: &str, anchor: &SearchAnchorMatch, click: Click, settle: Duration) -> OperationResult<SearchStep>;
}

pub struct MacosQqMusicDriver {
  session: LocalDriverSession,
}

impl MacosQqMusicDriver {
  pub fn open_local() -> OperationResult<Self> {
    let session = auv_driver::open_local().map_err(|error| error.to_string())?;
    Ok(Self { session })
  }

  pub fn from_session(session: MacosDriverSession) -> Self {
    Self {
      session: LocalDriverSession::Macos(session),
    }
  }

  fn main_window(&self, app_id: &str) -> OperationResult<Window> {
    self.session.window().resolve(main_window_selector(app_id)).map_err(|error| error.to_string())
  }
}

impl QqMusicDriver for MacosQqMusicDriver {
  fn activate_app(&mut self, app_id: &str, settle: Duration) -> OperationResult<SearchStep> {
    let window = self.main_window(app_id)?;
    self
      .session
      .window()
      .prepare_for_input(
        &window,
        PrepareForInputOptions {
          activation: ActivationPolicy::Foreground { settle },
          preserve_frontmost: false,
          install_focus_guard: false,
          settle: Duration::ZERO,
        },
      )
      .map_err(|error| error.to_string())?;
    Ok(SearchStep::new("search.activate", "activated QQMusic"))
  }

  fn press_search_shortcut(&mut self, shortcut: &str, settle: Duration) -> OperationResult<SearchStep> {
    let result = self
      .session
      .input()
      .press_key(KeyPressOptions {
        key: shortcut.to_string(),
        settle,
      })
      .map_err(|error| error.to_string())?;
    Ok(SearchStep::with_input("search.shortcut", format!("pressed {shortcut}"), result))
  }

  fn paste_query(&mut self, query: &str, settle: Duration) -> OperationResult<SearchStep> {
    self
      .session
      .input()
      .paste_text(PasteTextOptions {
        text: query.to_string(),
        replace_existing: true,
        submit: TextSubmit::Return,
        settle,
      })
      .map_err(|error| error.to_string())?;
    Ok(SearchStep::new("search.query", "pasted and submitted search query"))
  }

  fn wait_anchor(&mut self, app_id: &str, anchor: &str, timeout: Duration) -> OperationResult<SearchAnchorMatch> {
    let window = self.main_window(app_id)?;
    let matches = self
      .session
      .window()
      .wait_text(
        &window,
        anchor,
        DEFAULT_SEARCH_REGION,
        WaitOptions {
          timeout,
          poll_interval: Duration::from_millis(100),
        },
      )
      .map_err(|error| error.to_string())?;
    let Some(best) = matches.best_match() else {
      return Err(format!("search result anchor {anchor:?} was not found"));
    };
    Ok(SearchAnchorMatch {
      text: best.text.clone(),
      confidence: best.confidence,
      point: best.action_point(),
    })
  }

  fn click_anchor(&mut self, app_id: &str, anchor: &SearchAnchorMatch, click: Click, settle: Duration) -> OperationResult<SearchStep> {
    let window = self.main_window(app_id)?;
    let result = self
      .session
      .window()
      .click(
        &window,
        WindowPoint::new(anchor.point.x, anchor.point.y),
        ClickOptions {
          policy: InputPolicy::ForegroundPreferred,
          click,
          ..ClickOptions::default()
        },
      )
      .map_err(|error| error.to_string())?;
    if !settle.is_zero() {
      std::thread::sleep(settle);
    }
    Ok(SearchStep::with_input("search.results.click", "clicked search result anchor", result))
  }
}

fn main_window_selector(app_id: &str) -> WindowSelector {
  WindowSelector {
    app: Some(App::bundle_id(app_id)),
    title: None,
    main_visible: true,
  }
}
