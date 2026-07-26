use std::time::Duration;

use auv_driver::{Click, InputActionResult, Point, RatioRect};
use serde::{Deserialize, Serialize};

use crate::driver::QqMusicDriver;

pub const DEFAULT_APP_ID: &str = "com.tencent.QQMusicMac";
pub const DEFAULT_SEARCH_SHORTCUT: &str = "cmd+f";
pub const DEFAULT_SETTLE_MS: u64 = 250;
pub const DEFAULT_ANCHOR_TIMEOUT_MS: u64 = 5_000;
pub const DEFAULT_SEARCH_REGION: RatioRect = RatioRect {
  x: 0.0,
  y: 0.0,
  width: 1.0,
  height: 1.0,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SearchCommand {
  Search(SearchSubmit),
  Results(SearchResultsAction),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchSubmit {
  pub app_id: String,
  pub query: String,
  pub shortcut: String,
  pub settle_ms: u64,
}

impl SearchSubmit {
  pub fn defaults_with_query(query: impl Into<String>) -> Self {
    Self {
      app_id: DEFAULT_APP_ID.to_string(),
      query: query.into(),
      shortcut: DEFAULT_SEARCH_SHORTCUT.to_string(),
      settle_ms: DEFAULT_SETTLE_MS,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SearchResultsAction {
  Select(SearchResultsSelect),
  Click(SearchResultsClick),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResultsSelect {
  pub app_id: String,
  pub query: String,
  pub anchor: String,
  pub settle_ms: u64,
  pub anchor_timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResultsClick {
  pub app_id: String,
  pub query: Option<String>,
  pub anchor: Option<String>,
  pub row: Option<usize>,
  pub candidate_ref_json: Option<String>,
  pub settle_ms: u64,
  pub anchor_timeout_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchAnchorMatch {
  pub text: String,
  pub confidence: f64,
  pub point: Point,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchAction {
  Activate,
  FocusSearch,
  SubmitQuery,
  ClickResult,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchActionResult {
  pub action: SearchAction,
  pub input_action_result: Option<InputActionResult>,
}

impl SearchActionResult {
  pub fn completed(action: SearchAction) -> Self {
    Self {
      action,
      input_action_result: None,
    }
  }

  pub fn delivered(action: SearchAction, input_action_result: InputActionResult) -> Self {
    Self {
      action,
      input_action_result: Some(input_action_result),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchCommandReport {
  pub command: &'static str,
  pub actions: Vec<SearchActionResult>,
  pub anchor: Option<SearchAnchorMatch>,
  pub unsupported: Option<String>,
}

pub fn run_search_command(command: &SearchCommand, driver: &mut impl QqMusicDriver) -> Result<SearchCommandReport, String> {
  match command {
    SearchCommand::Search(command) => crate::tracing::search(|| run_search(command, driver)),
    SearchCommand::Results(SearchResultsAction::Select(command)) => crate::tracing::search_result_select(|| run_select(command, driver)),
    SearchCommand::Results(SearchResultsAction::Click(command)) => crate::tracing::search_result_click(|| run_click(command, driver)),
  }
}

fn run_search(command: &SearchSubmit, driver: &mut impl QqMusicDriver) -> Result<SearchCommandReport, String> {
  let actions = execute_search_phase(driver, &command.app_id, &command.query, &command.shortcut, command.settle_ms)?;
  Ok(SearchCommandReport {
    command: "search",
    actions,
    anchor: None,
    unsupported: None,
  })
}

fn run_select(command: &SearchResultsSelect, driver: &mut impl QqMusicDriver) -> Result<SearchCommandReport, String> {
  let mut actions = execute_search_phase(driver, &command.app_id, &command.query, DEFAULT_SEARCH_SHORTCUT, command.settle_ms)?;
  let anchor = driver.wait_anchor(&command.app_id, &command.anchor, Duration::from_millis(command.anchor_timeout_ms))?;
  actions.push(driver.click_anchor(&command.app_id, &anchor, Click::Single, Duration::from_millis(command.settle_ms))?);
  Ok(SearchCommandReport {
    command: "search.results.select",
    actions,
    anchor: Some(anchor),
    unsupported: None,
  })
}

fn run_click(command: &SearchResultsClick, driver: &mut impl QqMusicDriver) -> Result<SearchCommandReport, String> {
  if command.row.is_some() {
    // TODO(qqmusic-row-click): row selection is parsed for the agreed CLI shape,
    // but execution is deferred until a typed result-row detection API exists.
    return Ok(unsupported("search.results.click --row needs a typed row detection API"));
  }
  if command.candidate_ref_json.is_some() {
    // TODO(qqmusic-candidate-ref-click): CandidateRef execution is deferred until
    // QQMusic has a typed CandidateRef consumer instead of ad-hoc JSON parsing.
    return Ok(unsupported("search.results.click --candidate-ref needs a typed CandidateRef consumer API"));
  }
  let query = command.query.as_deref().ok_or_else(|| "search.results.click requires <query> unless --candidate-ref is used".to_string())?;
  let anchor_text =
    command.anchor.as_deref().ok_or_else(|| "search.results.click requires --anchor, --row, or --candidate-ref".to_string())?;
  let mut actions = execute_search_phase(driver, &command.app_id, query, DEFAULT_SEARCH_SHORTCUT, command.settle_ms)?;
  let anchor = driver.wait_anchor(&command.app_id, anchor_text, Duration::from_millis(command.anchor_timeout_ms))?;
  actions.push(driver.click_anchor(
    &command.app_id,
    &anchor,
    Click::Double {
      interval: Duration::from_millis(80),
    },
    Duration::from_millis(command.settle_ms),
  )?);
  Ok(SearchCommandReport {
    command: "search.results.click",
    actions,
    anchor: Some(anchor),
    unsupported: None,
  })
}

fn execute_search_phase(
  driver: &mut impl QqMusicDriver,
  app_id: &str,
  query: &str,
  shortcut: &str,
  settle_ms: u64,
) -> Result<Vec<SearchActionResult>, String> {
  Ok(vec![
    driver.activate_app(app_id, Duration::from_millis(settle_ms))?,
    driver.press_search_shortcut(shortcut, Duration::from_millis(settle_ms))?,
    driver.paste_query(query, Duration::from_millis(settle_ms))?,
  ])
}

fn unsupported(message: impl Into<String>) -> SearchCommandReport {
  SearchCommandReport {
    command: "search.results.click",
    actions: Vec::new(),
    anchor: None,
    unsupported: Some(message.into()),
  }
}
