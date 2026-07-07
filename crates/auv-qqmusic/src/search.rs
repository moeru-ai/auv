use std::time::Duration;

use auv_driver::{Click, InputActionResult, Point, RatioRect};
use serde::{Deserialize, Serialize};

use crate::driver::{OperationResult, QqMusicDriver};

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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchStep {
  pub step_id: &'static str,
  pub summary: String,
  pub input_action_result: Option<InputActionResult>,
}

impl SearchStep {
  pub fn new(step_id: &'static str, summary: impl Into<String>) -> Self {
    Self {
      step_id,
      summary: summary.into(),
      input_action_result: None,
    }
  }

  pub fn with_input(step_id: &'static str, summary: impl Into<String>, input_action_result: InputActionResult) -> Self {
    Self {
      step_id,
      summary: summary.into(),
      input_action_result: Some(input_action_result),
    }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchCommandReport {
  pub command: &'static str,
  pub steps: Vec<SearchStep>,
  pub anchor: Option<SearchAnchorMatch>,
  pub unsupported: Option<String>,
}

pub fn run_search_command(command: &SearchCommand, driver: &mut impl QqMusicDriver) -> OperationResult<SearchCommandReport> {
  match command {
    SearchCommand::Search(command) => run_search(command, driver),
    SearchCommand::Results(SearchResultsAction::Select(command)) => run_select(command, driver),
    SearchCommand::Results(SearchResultsAction::Click(command)) => run_click(command, driver),
  }
}

fn run_search(command: &SearchSubmit, driver: &mut impl QqMusicDriver) -> OperationResult<SearchCommandReport> {
  let steps = execute_search_phase(driver, &command.app_id, &command.query, &command.shortcut, command.settle_ms)?;
  Ok(SearchCommandReport {
    command: "search",
    steps,
    anchor: None,
    unsupported: None,
  })
}

fn run_select(command: &SearchResultsSelect, driver: &mut impl QqMusicDriver) -> OperationResult<SearchCommandReport> {
  let mut steps = execute_search_phase(driver, &command.app_id, &command.query, DEFAULT_SEARCH_SHORTCUT, command.settle_ms)?;
  let anchor = driver.wait_anchor(&command.app_id, &command.anchor, Duration::from_millis(command.anchor_timeout_ms))?;
  steps.push(driver.click_anchor(&command.app_id, &anchor, Click::Single, Duration::from_millis(command.settle_ms))?);
  Ok(SearchCommandReport {
    command: "search.results.select",
    steps,
    anchor: Some(anchor),
    unsupported: None,
  })
}

fn run_click(command: &SearchResultsClick, driver: &mut impl QqMusicDriver) -> OperationResult<SearchCommandReport> {
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
  let mut steps = execute_search_phase(driver, &command.app_id, query, DEFAULT_SEARCH_SHORTCUT, command.settle_ms)?;
  let anchor = driver.wait_anchor(&command.app_id, anchor_text, Duration::from_millis(command.anchor_timeout_ms))?;
  steps.push(driver.click_anchor(
    &command.app_id,
    &anchor,
    Click::Double {
      interval: Duration::from_millis(80),
    },
    Duration::from_millis(command.settle_ms),
  )?);
  Ok(SearchCommandReport {
    command: "search.results.click",
    steps,
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
) -> OperationResult<Vec<SearchStep>> {
  Ok(vec![
    driver.activate_app(app_id, Duration::from_millis(settle_ms))?,
    driver.press_search_shortcut(shortcut, Duration::from_millis(settle_ms))?,
    driver.paste_query(query, Duration::from_millis(settle_ms))?,
  ])
}

fn unsupported(message: impl Into<String>) -> SearchCommandReport {
  SearchCommandReport {
    command: "search.results.click",
    steps: Vec::new(),
    anchor: None,
    unsupported: Some(message.into()),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use auv_driver::{InputActionResult, InputDeliveryPath};

  #[derive(Default)]
  struct RecordingQqMusicDriver {
    calls: Vec<String>,
  }

  impl QqMusicDriver for RecordingQqMusicDriver {
    fn activate_app(&mut self, app_id: &str, settle: Duration) -> OperationResult<SearchStep> {
      self.calls.push(format!("activate:{app_id}:{}", settle.as_millis()));
      Ok(SearchStep::new("activate", "activated"))
    }

    fn press_search_shortcut(&mut self, shortcut: &str, settle: Duration) -> OperationResult<SearchStep> {
      self.calls.push(format!("shortcut:{shortcut}:{}", settle.as_millis()));
      Ok(SearchStep::with_input(
        "shortcut",
        "pressed shortcut",
        InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents),
      ))
    }

    fn paste_query(&mut self, query: &str, settle: Duration) -> OperationResult<SearchStep> {
      self.calls.push(format!("paste:{query}:{}", settle.as_millis()));
      Ok(SearchStep::new("paste", "pasted query"))
    }

    fn wait_anchor(&mut self, app_id: &str, anchor: &str, timeout: Duration) -> OperationResult<SearchAnchorMatch> {
      self.calls.push(format!("anchor:{app_id}:{anchor}:{}", timeout.as_millis()));
      Ok(SearchAnchorMatch {
        text: anchor.to_string(),
        confidence: 0.95,
        point: Point::new(100.0, 200.0),
      })
    }

    fn click_anchor(&mut self, app_id: &str, anchor: &SearchAnchorMatch, click: Click, settle: Duration) -> OperationResult<SearchStep> {
      self.calls.push(format!("click:{app_id}:{}:{click:?}:{}", anchor.text, settle.as_millis()));
      Ok(SearchStep::with_input("click", "clicked", InputActionResult::single_success(InputDeliveryPath::WindowTargetedMouse)))
    }
  }

  #[test]
  fn search_command_runs_search_phase() {
    let command = SearchCommand::Search(SearchSubmit::defaults_with_query("周杰伦"));
    let mut driver = RecordingQqMusicDriver::default();

    let report = run_search_command(&command, &mut driver).expect("command should run");

    assert_eq!(report.command, "search");
    assert_eq!(
      driver.calls,
      vec![
        "activate:com.tencent.QQMusicMac:250",
        "shortcut:cmd+f:250",
        "paste:周杰伦:250",
      ]
    );
  }

  #[test]
  fn search_results_select_searches_and_single_clicks_anchor() {
    let command = SearchCommand::Results(SearchResultsAction::Select(SearchResultsSelect {
      app_id: DEFAULT_APP_ID.to_string(),
      query: "周杰伦".to_string(),
      anchor: "晴天".to_string(),
      settle_ms: DEFAULT_SETTLE_MS,
      anchor_timeout_ms: DEFAULT_ANCHOR_TIMEOUT_MS,
    }));
    let mut driver = RecordingQqMusicDriver::default();

    let report = run_search_command(&command, &mut driver).expect("command should run");

    assert_eq!(report.command, "search.results.select");
    assert_eq!(report.anchor.as_ref().map(|anchor| anchor.text.as_str()), Some("晴天"));
    assert!(driver.calls.iter().any(|call| call.contains("click:com.tencent.QQMusicMac:晴天:Single")));
  }

  #[test]
  fn search_results_click_anchor_double_clicks_anchor() {
    let command = SearchCommand::Results(SearchResultsAction::Click(SearchResultsClick {
      app_id: DEFAULT_APP_ID.to_string(),
      query: Some("周杰伦".to_string()),
      anchor: Some("晴天".to_string()),
      row: None,
      candidate_ref_json: None,
      settle_ms: DEFAULT_SETTLE_MS,
      anchor_timeout_ms: DEFAULT_ANCHOR_TIMEOUT_MS,
    }));
    let mut driver = RecordingQqMusicDriver::default();

    let report = run_search_command(&command, &mut driver).expect("command should run");

    assert_eq!(report.command, "search.results.click");
    assert!(driver.calls.iter().any(|call| call.contains("click:com.tencent.QQMusicMac:晴天:Double")));
  }

  #[test]
  fn search_results_click_row_is_explicitly_unsupported() {
    let command = SearchCommand::Results(SearchResultsAction::Click(SearchResultsClick {
      app_id: DEFAULT_APP_ID.to_string(),
      query: Some("周杰伦".to_string()),
      anchor: None,
      row: Some(2),
      candidate_ref_json: None,
      settle_ms: DEFAULT_SETTLE_MS,
      anchor_timeout_ms: DEFAULT_ANCHOR_TIMEOUT_MS,
    }));
    let mut driver = RecordingQqMusicDriver::default();

    let report = run_search_command(&command, &mut driver).expect("unsupported is reported");

    assert_eq!(report.unsupported.as_deref(), Some("search.results.click --row needs a typed row detection API"));
    assert!(driver.calls.is_empty());
  }

  #[test]
  fn search_results_click_candidate_ref_is_explicitly_unsupported() {
    let command = SearchCommand::Results(SearchResultsAction::Click(SearchResultsClick {
      app_id: DEFAULT_APP_ID.to_string(),
      query: None,
      anchor: None,
      row: None,
      candidate_ref_json: Some("{\"candidate\":\"ref\"}".to_string()),
      settle_ms: DEFAULT_SETTLE_MS,
      anchor_timeout_ms: DEFAULT_ANCHOR_TIMEOUT_MS,
    }));
    let mut driver = RecordingQqMusicDriver::default();

    let report = run_search_command(&command, &mut driver).expect("unsupported is reported");

    assert_eq!(report.unsupported.as_deref(), Some("search.results.click --candidate-ref needs a typed CandidateRef consumer API"));
    assert!(driver.calls.is_empty());
  }
}
