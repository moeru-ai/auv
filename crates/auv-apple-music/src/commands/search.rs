//! Search submission for Apple Music on Windows.
//!
//! The command composes the typed Windows driver surfaces:
//!
//! 1. resolve and restore the Apple Music window,
//! 2. locate and focus the UIA search edit,
//! 3. replace and submit the query,
//! 4. verify the query through UI Automation, with OCR as a fallback.
//!
//! Search activation and search verification remain separate in the result.
//! A delivered query is not reported as semantically verified unless UIA or
//! fallback OCR sees the normalized query after submission.

use std::fmt;
use std::path::PathBuf;
use std::time::Duration;

use auv_driver::input::InputActionResult;
use auv_driver::window::WindowMutationResult;
use serde::{Deserialize, Serialize};

use crate::app::ResolveOptions;

pub const DEFAULT_SEARCH_SETTLE_MS: u64 = 300;
pub const DEFAULT_SEARCH_VERIFICATION_TIMEOUT_MS: u64 = 5_000;
pub const DEFAULT_RESULT_SELECTION_TIMEOUT_MS: u64 = 5_000;

/// Inputs for the `search` command.
#[derive(Clone, Debug)]
pub struct SearchInputs {
  pub query: String,
  pub settle_ms: u64,
  pub verification_timeout_ms: u64,
  pub artifact_dir: Option<PathBuf>,
  pub resolve: ResolveOptions,
}

impl SearchInputs {
  pub fn with_query(query: impl Into<String>) -> Self {
    Self {
      query: query.into(),
      settle_ms: DEFAULT_SEARCH_SETTLE_MS,
      verification_timeout_ms: DEFAULT_SEARCH_VERIFICATION_TIMEOUT_MS,
      artifact_dir: None,
      resolve: ResolveOptions::default(),
    }
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchVerificationStatus {
  Verified,
  Unverified,
}

impl fmt::Display for SearchVerificationStatus {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    match self {
      Self::Verified => write!(f, "verified"),
      Self::Unverified => write!(f, "unverified"),
    }
  }
}

/// Post-submit evidence that is deliberately separate from input delivery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchVerification {
  pub status: SearchVerificationStatus,
  pub method: String,
  pub observed_text: Option<String>,
  pub artifact: Option<String>,
}

/// Output produced by [`run_search`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchResult {
  pub command: String,
  pub query: String,
  pub window_title: Option<String>,
  pub window_preparation: WindowMutationResult,
  pub search_focus_input: InputActionResult,
  pub query_input: InputActionResult,
  pub verification: SearchVerification,
  pub diagnostics: Vec<String>,
}

impl SearchResult {
  pub fn is_verified(&self) -> bool {
    self.verification.status == SearchVerificationStatus::Verified
  }
}

/// Inputs for searching and selecting one uniquely matched result.
#[derive(Clone, Debug)]
pub struct SearchResultSelectInputs {
  pub search: SearchInputs,
  pub anchor: String,
  pub selection_timeout_ms: u64,
}

impl SearchResultSelectInputs {
  pub fn with_query_and_anchor(query: impl Into<String>, anchor: impl Into<String>) -> Self {
    Self {
      search: SearchInputs::with_query(query),
      anchor: anchor.into(),
      selection_timeout_ms: DEFAULT_RESULT_SELECTION_TIMEOUT_MS,
    }
  }
}

/// The UIA result item chosen for activation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchResultMatch {
  pub path: String,
  pub name: String,
  pub control_type: String,
  pub class_name: String,
}

/// Output produced by [`run_search_result_select`].
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SearchResultSelection {
  pub command: String,
  pub search: SearchResult,
  pub selected: SearchResultMatch,
  pub selection_input: InputActionResult,
  pub verification: SearchVerification,
}

impl SearchResultSelection {
  pub fn is_verified(&self) -> bool {
    self.verification.status == SearchVerificationStatus::Verified
  }
}

pub fn run_search(inputs: &SearchInputs) -> Result<SearchResult, String> {
  let mut driver = platform::open_driver()?;
  run_search_with_driver(inputs, &mut driver)
}

pub fn run_search_result_select(inputs: &SearchResultSelectInputs) -> Result<SearchResultSelection, String> {
  let anchor = inputs.anchor.trim();
  if anchor.is_empty() {
    return Err("search result anchor must not be empty".to_string());
  }
  let mut driver = platform::open_driver()?;
  let search = run_search_with_driver(&inputs.search, &mut driver)?;
  if !search.is_verified() {
    return Err("search result selection requires a verified search".to_string());
  }
  let (selected, selection_input, verification) =
    driver.select_result(anchor, Duration::from_millis(inputs.selection_timeout_ms), Duration::from_millis(inputs.search.settle_ms))?;
  Ok(SearchResultSelection {
    command: "search.results.select".to_string(),
    search,
    selected,
    selection_input,
    verification,
  })
}

trait SearchDriver {
  fn prepare_window(&mut self, resolve: &ResolveOptions, settle: Duration) -> Result<(Option<String>, WindowMutationResult), String>;

  fn focus_search(&mut self, settle: Duration) -> Result<InputActionResult, String>;

  fn submit_query(&mut self, query: &str, settle: Duration) -> Result<InputActionResult, String>;

  fn verify_query(&mut self, query: &str, timeout: Duration, artifact_dir: Option<&std::path::Path>) -> Result<SearchVerification, String>;

  fn select_result(
    &mut self,
    anchor: &str,
    timeout: Duration,
    settle: Duration,
  ) -> Result<(SearchResultMatch, InputActionResult, SearchVerification), String>;
}

fn run_search_with_driver(inputs: &SearchInputs, driver: &mut impl SearchDriver) -> Result<SearchResult, String> {
  let query = inputs.query.trim();
  if query.is_empty() {
    return Err("search query must not be empty".to_string());
  }
  let settle = Duration::from_millis(inputs.settle_ms);
  let (window_title, window_preparation) = driver.prepare_window(&inputs.resolve, settle)?;
  let search_focus_input = driver.focus_search(settle)?;
  let query_input = driver.submit_query(query, settle)?;
  let verification = driver.verify_query(query, Duration::from_millis(inputs.verification_timeout_ms), inputs.artifact_dir.as_deref())?;

  let diagnostics = if verification.status == SearchVerificationStatus::Unverified {
    vec!["query input was delivered, but neither UI Automation nor fallback OCR verified the query".to_string()]
  } else {
    Vec::new()
  };

  Ok(SearchResult {
    command: "search".to_string(),
    query: query.to_string(),
    window_title,
    window_preparation,
    search_focus_input,
    query_input,
    verification,
    diagnostics,
  })
}

// TODO(apple-music-search-candidate-ref): structured result listing and stable
// candidate-ref consumption are deferred because this slice selects one live
// UIA result by a unique accessible-name anchor. Add durable candidates when
// the owner approves a result-listing contract.

#[cfg(not(target_os = "windows"))]
mod platform {
  pub(super) struct UnsupportedSearchDriver;

  pub(super) fn open_driver() -> Result<UnsupportedSearchDriver, String> {
    Err("Apple Music search is only supported on Windows".to_string())
  }

  impl super::SearchDriver for UnsupportedSearchDriver {
    fn prepare_window(
      &mut self,
      _resolve: &crate::app::ResolveOptions,
      _settle: std::time::Duration,
    ) -> Result<(Option<String>, auv_driver::window::WindowMutationResult), String> {
      unreachable!("non-Windows search driver cannot be opened")
    }

    fn focus_search(&mut self, _settle: std::time::Duration) -> Result<auv_driver::input::InputActionResult, String> {
      unreachable!("non-Windows search driver cannot be opened")
    }

    fn submit_query(&mut self, _query: &str, _settle: std::time::Duration) -> Result<auv_driver::input::InputActionResult, String> {
      unreachable!("non-Windows search driver cannot be opened")
    }

    fn verify_query(
      &mut self,
      _query: &str,
      _timeout: std::time::Duration,
      _artifact_dir: Option<&std::path::Path>,
    ) -> Result<super::SearchVerification, String> {
      unreachable!("non-Windows search driver cannot be opened")
    }

    fn select_result(
      &mut self,
      _anchor: &str,
      _timeout: std::time::Duration,
      _settle: std::time::Duration,
    ) -> Result<(super::SearchResultMatch, auv_driver::input::InputActionResult, super::SearchVerification), String> {
      unreachable!("non-Windows search driver cannot be opened")
    }
  }
}

#[cfg(target_os = "windows")]
mod platform {
  use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

  use auv_driver::Driver;
  use auv_driver::geometry::RatioRect;
  use auv_driver::input::{InputPolicy, TextSubmit, TypeTextOptions};
  use auv_driver::window::{Window, WindowMutationOptions, WindowMutationVerification};
  use auv_driver_windows::{WindowsDriver, WindowsDriverSession};

  use super::{
    SearchDriver, SearchResultMatch, SearchVerification, SearchVerificationStatus, normalized, resolve_result_match, search_input_path,
    selection_navigation_evidence, uia_query_evidence,
  };
  use crate::app::{ResolveOptions, resolve_window};

  pub(super) struct WindowsSearchDriver {
    session: WindowsDriverSession,
    window: Option<Window>,
  }

  pub(super) fn open_driver() -> Result<WindowsSearchDriver, String> {
    let session = WindowsDriver::new().open_local().map_err(|error| format!("windows driver open failed: {error}"))?;
    Ok(WindowsSearchDriver {
      session,
      window: None,
    })
  }

  impl SearchDriver for WindowsSearchDriver {
    fn prepare_window(
      &mut self,
      resolve: &ResolveOptions,
      settle: Duration,
    ) -> Result<(Option<String>, auv_driver::window::WindowMutationResult), String> {
      let window = resolve_window(resolve)?.ok_or_else(|| "Apple Music window not found -- is the app running?".to_string())?.window;
      let title = window.title.clone();
      let result = self
        .session
        .window()
        .restore(
          &window,
          WindowMutationOptions {
            settle,
            verification: WindowMutationVerification::BestEffortState,
            ..WindowMutationOptions::default()
          },
        )
        .map_err(|error| format!("Apple Music window preparation failed: {error}"))?;
      self.window = Some(window);
      Ok((title, result))
    }

    fn focus_search(&mut self, settle: Duration) -> Result<auv_driver::input::InputActionResult, String> {
      let window = self.window.as_ref().ok_or_else(|| "search focus requires a prepared window".to_string())?;
      let snapshot =
        self.session.accessibility().snapshot_window(window).map_err(|error| format!("search input UIA snapshot failed: {error}"))?;
      let path = search_input_path(snapshot.nodes.iter().map(|node| {
        (node.path.as_str(), node.name.as_str(), node.automation_id.as_str(), node.class_name.as_str(), node.control_type.as_str())
      }))
      .ok_or_else(|| "Apple Music UIA search input was not found (expected edit/TextBox node)".to_string())?;
      let result =
        self.session.accessibility().focus_node(window, &path).map_err(|error| format!("search input UIA focus failed: {error}"))?;
      if !settle.is_zero() {
        std::thread::sleep(settle);
      }
      Ok(result)
    }

    fn submit_query(&mut self, query: &str, settle: Duration) -> Result<auv_driver::input::InputActionResult, String> {
      self
        .session
        .input()
        .type_text(
          query,
          TypeTextOptions {
            policy: InputPolicy::ForegroundPreferred,
            replace_existing: true,
            submit: TextSubmit::Return,
            allow_clipboard_fallback: false,
            settle,
            ..TypeTextOptions::default()
          },
        )
        .map_err(|error| format!("search query input failed: {error}"))
    }

    fn verify_query(
      &mut self,
      query: &str,
      timeout: Duration,
      artifact_dir: Option<&std::path::Path>,
    ) -> Result<SearchVerification, String> {
      let window = self.window.as_ref().ok_or_else(|| "search verification requires a prepared window".to_string())?;
      let deadline = Instant::now() + timeout;

      loop {
        let snapshot = self
          .session
          .accessibility()
          .snapshot_window(window)
          .map_err(|error| format!("search verification UIA snapshot failed: {error}"))?;
        if let Some((method, observed_text)) = uia_query_evidence(
          query,
          snapshot.nodes.iter().map(|node| (node.name.as_str(), node.value.as_deref(), node.control_type.as_str(), node.focused)),
        ) {
          let artifact = artifact_dir
            .map(|dir| {
              let capture = self.session.window().capture(window).map_err(|error| format!("search artifact capture failed: {error}"))?;
              save_artifact(dir, &capture)
            })
            .transpose()?;
          return Ok(SearchVerification {
            status: SearchVerificationStatus::Verified,
            method,
            observed_text: Some(observed_text),
            artifact,
          });
        }

        let timed_out = Instant::now() >= deadline;
        if timed_out || timeout.is_zero() {
          break;
        }

        std::thread::sleep(Duration::from_millis(100));
      }

      // OCR is deliberately a single fallback after UIA has exhausted the
      // verification window. This keeps the stable accessibility path primary
      // while preserving coverage for Apple Music surfaces that omit a usable
      // ValuePattern or accessible result label.
      let capture = self.session.window().capture(window).map_err(|error| format!("search fallback capture failed: {error}"))?;
      let recognition = self
        .session
        .vision()
        .recognize_text_in_capture(&capture, RatioRect::new(0.0, 0.0, 1.0, 1.0))
        .map_err(|error| format!("search fallback OCR failed: {error}"))?;
      let verified = normalized(&recognition.text).contains(&normalized(query));
      let artifact = artifact_dir.map(|dir| save_artifact(dir, &capture)).transpose()?;
      Ok(SearchVerification {
        status: if verified {
          SearchVerificationStatus::Verified
        } else {
          SearchVerificationStatus::Unverified
        },
        method: "window_capture_ocr_fallback".to_string(),
        observed_text: (!recognition.text.trim().is_empty()).then_some(recognition.text),
        artifact,
      })
    }

    fn select_result(
      &mut self,
      anchor: &str,
      timeout: Duration,
      settle: Duration,
    ) -> Result<(SearchResultMatch, auv_driver::input::InputActionResult, SearchVerification), String> {
      let window = self.window.as_ref().ok_or_else(|| "result selection requires a prepared window".to_string())?;
      let deadline = Instant::now() + timeout;

      loop {
        let snapshot =
          self.session.accessibility().snapshot_window(window).map_err(|error| format!("result selection UIA snapshot failed: {error}"))?;
        let matched = resolve_result_match(
          anchor,
          snapshot.nodes.iter().map(|node| (node.path.as_str(), node.name.as_str(), node.control_type.as_str(), node.class_name.as_str())),
        )?;
        if let Some(matched) = matched {
          let input = self
            .session
            .accessibility()
            .select_node(window, &matched.path)
            .map_err(|error| format!("result selection UIA action failed: {error}"))?;
          if !settle.is_zero() {
            std::thread::sleep(settle);
          }
          let verification = loop {
            let after = self
              .session
              .accessibility()
              .snapshot_window(window)
              .map_err(|error| format!("result verification UIA snapshot failed: {error}"))?;
            if let Some(observed_text) = selection_navigation_evidence(
              &matched.name,
              after.nodes.iter().map(|node| (node.name.as_str(), node.control_type.as_str(), node.class_name.as_str())),
            ) {
              break SearchVerification {
                status: SearchVerificationStatus::Verified,
                method: "ui_automation_navigation".to_string(),
                observed_text: Some(observed_text),
                artifact: None,
              };
            }
            if Instant::now() >= deadline || timeout.is_zero() {
              break SearchVerification {
                status: SearchVerificationStatus::Unverified,
                method: "ui_automation_navigation".to_string(),
                observed_text: None,
                artifact: None,
              };
            }
            std::thread::sleep(Duration::from_millis(100));
          };
          return Ok((matched, input, verification));
        }
        if Instant::now() >= deadline || timeout.is_zero() {
          return Err(format!("Apple Music search result matching {anchor:?} was not found"));
        }
        std::thread::sleep(Duration::from_millis(100));
      }
    }
  }

  fn save_artifact(dir: &std::path::Path, capture: &auv_driver::capture::Capture) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|error| format!("create search artifact directory failed: {error}"))?;
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_millis()).unwrap_or(0);
    let path = dir.join(format!("apple-music-search-{timestamp}.png"));
    capture.image.save(&path).map_err(|error| format!("save search artifact failed: {error}"))?;
    Ok(path.to_string_lossy().into_owned())
  }
}

fn normalized(value: &str) -> String {
  value.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn search_input_path<'a>(nodes: impl Iterator<Item = (&'a str, &'a str, &'a str, &'a str, &'a str)>) -> Option<String> {
  nodes
    .filter(|(_path, name, automation_id, class_name, control_type)| {
      let is_text_box = automation_id.eq_ignore_ascii_case("TextBox") && class_name.eq_ignore_ascii_case("TextBox");
      let is_search_edit =
        control_type.eq_ignore_ascii_case("edit") && (name.eq_ignore_ascii_case("Search") || automation_id.eq_ignore_ascii_case("TextBox"));
      is_text_box || is_search_edit
    })
    .map(|(path, _name, _automation_id, _class_name, _control_type)| path.to_string())
    .next()
}

fn resolve_result_match<'a>(
  anchor: &str,
  nodes: impl Iterator<Item = (&'a str, &'a str, &'a str, &'a str)>,
) -> Result<Option<SearchResultMatch>, String> {
  let anchor = normalized(anchor);
  let candidates = nodes
    .filter(|(_path, name, control_type, class_name)| {
      !name.trim().is_empty()
        && control_type.eq_ignore_ascii_case("list item")
        && class_name.eq_ignore_ascii_case("GridViewItem")
        && normalized(name).contains(&anchor)
    })
    .map(|(path, name, control_type, class_name)| SearchResultMatch {
      path: path.to_string(),
      name: name.to_string(),
      control_type: control_type.to_string(),
      class_name: class_name.to_string(),
    })
    .collect::<Vec<_>>();

  match candidates.as_slice() {
    [] => Ok(None),
    [matched] => Ok(Some(matched.clone())),
    matches => {
      let names = matches.iter().map(|matched| matched.name.as_str()).collect::<Vec<_>>().join("; ");
      Err(format!("search result anchor is ambiguous; {} UIA results matched: {names}", matches.len()))
    }
  }
}

fn selection_navigation_evidence<'a>(selected_name: &str, nodes: impl Iterator<Item = (&'a str, &'a str, &'a str)>) -> Option<String> {
  let selected = normalized(selected_name);
  let nodes = nodes.collect::<Vec<_>>();
  let selected_item_still_present = nodes.iter().any(|(name, control_type, class_name)| {
    control_type.eq_ignore_ascii_case("list item") && class_name.eq_ignore_ascii_case("GridViewItem") && normalized(name) == selected
  });
  if selected_item_still_present {
    return None;
  }

  let observed =
    nodes.iter().map(|(name, _control_type, _class_name)| *name).filter(|name| !name.trim().is_empty()).collect::<Vec<_>>().join(" ");
  let observed_normalized = normalized(&observed);
  let matched_terms =
    identity_terms(selected_name).iter().filter(|term| observed_normalized.contains(term.as_str())).cloned().collect::<Vec<_>>();
  (matched_terms.len() >= 2).then(|| format!("detail view matched identity terms: {}", matched_terms.join(", ")))
}

fn identity_terms(value: &str) -> Vec<String> {
  const IGNORED: &[&str] = &["the", "and", "with", "from", "into", "major", "minor"];
  normalized(value)
    .split(|character: char| !character.is_alphanumeric())
    .filter(|term| term.len() >= 4 && !IGNORED.contains(term))
    .map(ToOwned::to_owned)
    .collect()
}

fn uia_query_evidence<'a>(query: &str, nodes: impl Iterator<Item = (&'a str, Option<&'a str>, &'a str, bool)>) -> Option<(String, String)> {
  let query = normalized(query);
  let nodes = nodes.collect::<Vec<_>>();

  for (_name, value, control_type, focused) in &nodes {
    let Some(value) = value else {
      continue;
    };
    if normalized(value).contains(&query) {
      let method = if *focused || control_type.eq_ignore_ascii_case("edit") {
        "ui_automation_value"
      } else {
        "ui_automation_value_observation"
      };
      return Some((method.to_string(), (*value).to_string()));
    }
  }

  for (name, _value, _control_type, _focused) in nodes {
    if normalized(name).contains(&query) {
      return Some(("ui_automation_name".to_string(), name.to_string()));
    }
  }

  None
}

#[cfg(test)]
mod tests {
  use auv_driver::geometry::Rect;
  use auv_driver::input::{InputActionResult, InputDeliveryPath};
  use auv_driver::window::{WindowMutationAttempt, WindowMutationPath, WindowMutationResult, WindowState};

  use super::*;

  struct RecordingDriver {
    calls: Vec<String>,
    verification: SearchVerificationStatus,
    selected: Option<SearchResultMatch>,
  }

  impl RecordingDriver {
    fn new(verification: SearchVerificationStatus) -> Self {
      Self {
        calls: Vec::new(),
        verification,
        selected: None,
      }
    }
  }

  impl SearchDriver for RecordingDriver {
    fn prepare_window(&mut self, _resolve: &ResolveOptions, settle: Duration) -> Result<(Option<String>, WindowMutationResult), String> {
      self.calls.push(format!("prepare:{}", settle.as_millis()));
      Ok((Some("Apple Music".to_string()), mutation_result()))
    }

    fn focus_search(&mut self, settle: Duration) -> Result<InputActionResult, String> {
      self.calls.push(format!("focus:{}", settle.as_millis()));
      Ok(input_result(InputDeliveryPath::AxFocus))
    }

    fn submit_query(&mut self, query: &str, settle: Duration) -> Result<InputActionResult, String> {
      self.calls.push(format!("query:{query}:{}", settle.as_millis()));
      Ok(input_result(InputDeliveryPath::ForegroundSystemEvents))
    }

    fn verify_query(
      &mut self,
      query: &str,
      timeout: Duration,
      artifact_dir: Option<&std::path::Path>,
    ) -> Result<SearchVerification, String> {
      self.calls.push(format!("verify:{query}:{}:{}", timeout.as_millis(), artifact_dir.is_some()));
      Ok(SearchVerification {
        status: self.verification,
        method: "test".to_string(),
        observed_text: Some(query.to_string()),
        artifact: None,
      })
    }

    fn select_result(
      &mut self,
      anchor: &str,
      timeout: Duration,
      settle: Duration,
    ) -> Result<(SearchResultMatch, InputActionResult, SearchVerification), String> {
      self.calls.push(format!("select:{anchor}:{}:{}", timeout.as_millis(), settle.as_millis()));
      let selected = self.selected.clone().unwrap_or_else(|| SearchResultMatch {
        path: "0/2/0/0/6/0/3/0/1/3".to_string(),
        name: anchor.to_string(),
        control_type: "list item".to_string(),
        class_name: "GridViewItem".to_string(),
      });
      Ok((
        selected,
        input_result(InputDeliveryPath::AxPress),
        SearchVerification {
          status: SearchVerificationStatus::Verified,
          method: "test".to_string(),
          observed_text: Some(anchor.to_string()),
          artifact: None,
        },
      ))
    }
  }

  fn input_result(path: InputDeliveryPath) -> InputActionResult {
    InputActionResult::single_success(path)
  }

  fn mutation_result() -> WindowMutationResult {
    WindowMutationResult {
      selected_path: WindowMutationPath::PlatformNative,
      attempts: vec![WindowMutationAttempt::success(
        WindowMutationPath::PlatformNative,
        "restored",
      )],
      fallback_reason: None,
      before_frame: Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
      after_frame: Some(Rect::new(0.0, 0.0, 800.0, 600.0)),
      before_state: Some(WindowState {
        is_minimized: Some(false),
        is_visible: Some(true),
      }),
      after_state: Some(WindowState {
        is_minimized: Some(false),
        is_visible: Some(true),
      }),
      focus_disturbance: auv_driver::input::DisturbanceLevel::Foreground,
      mouse_disturbance: auv_driver::input::DisturbanceLevel::None,
    }
  }

  #[test]
  fn search_runs_typed_window_input_and_verification_sequence() {
    let inputs = SearchInputs::with_query("AURORA Cure For Me");
    let mut driver = RecordingDriver::new(SearchVerificationStatus::Verified);

    let result = run_search_with_driver(&inputs, &mut driver).expect("search should run");

    assert!(result.is_verified());
    assert_eq!(
      driver.calls,
      vec![
        "prepare:300",
        "focus:300",
        "query:AURORA Cure For Me:300",
        "verify:AURORA Cure For Me:5000:false",
      ]
    );
    assert_eq!(result.search_focus_input.selected_path, InputDeliveryPath::AxFocus);
  }

  #[test]
  fn search_keeps_unverified_delivery_inspectable() {
    let inputs = SearchInputs::with_query("AURORA");
    let mut driver = RecordingDriver::new(SearchVerificationStatus::Unverified);

    let result = run_search_with_driver(&inputs, &mut driver).expect("delivery should be reported");

    assert!(!result.is_verified());
    assert_eq!(result.diagnostics.len(), 1);
    assert_eq!(result.query_input.selected_path, InputDeliveryPath::ForegroundSystemEvents);
  }

  #[test]
  fn search_rejects_blank_query_before_driver_actions() {
    let inputs = SearchInputs::with_query("   ");
    let mut driver = RecordingDriver::new(SearchVerificationStatus::Verified);

    let error = run_search_with_driver(&inputs, &mut driver).expect_err("blank query should fail");

    assert_eq!(error, "search query must not be empty");
    assert!(driver.calls.is_empty());
  }

  #[test]
  fn normalized_collapses_whitespace_and_case() {
    assert_eq!(normalized("  Cure\nFOR   Me "), "cure for me");
  }

  #[test]
  fn search_input_path_matches_live_apple_music_uia_shape() {
    let path = search_input_path(
      [
        ("0/2/0/0/1/3", "", "", "AutoSuggestBox", "group"),
        ("0/2/0/0/1/3/0", "Search", "TextBox", "TextBox", "edit"),
      ]
      .into_iter(),
    );

    assert_eq!(path.as_deref(), Some("0/2/0/0/1/3/0"));
  }

  #[test]
  fn search_input_path_does_not_match_unrelated_text() {
    let path = search_input_path([("0/2/0/0/2/5/1/0", "Current Song", "", "TextBlock", "text")].into_iter());

    assert_eq!(path, None);
  }

  #[test]
  fn result_match_selects_unique_live_grid_item_anchor() {
    let matched = resolve_result_match(
      "Ballade No. 1 in G Minor, Op. 23 YUNDI",
      [
        ("0/2/0/0/6/0/3/0/1/3", "Ballade No. 1 in G Minor, Op. 23 YUNDI", "list item", "GridViewItem"),
        ("0/2/0/0/6/0/4", "Playlists", "list item", "ListViewItem"),
      ]
      .into_iter(),
    )
    .expect("selector should not be ambiguous")
    .expect("result should match");

    assert_eq!(matched.path, "0/2/0/0/6/0/3/0/1/3");
  }

  #[test]
  fn result_match_rejects_ambiguous_anchor() {
    let error = resolve_result_match(
      "Ballade No. 1",
      [
        ("0/1", "Ballade No. 1 in G Minor, Op. 23 YUNDI", "list item", "GridViewItem"),
        ("0/2", "Chopin: Ballade No. 1 (Piano) Healing Energy", "list item", "GridViewItem"),
      ]
      .into_iter(),
    )
    .expect_err("broad anchor should be ambiguous");

    assert!(error.contains("2 UIA results matched"));
  }

  #[test]
  fn selection_navigation_verifies_detail_view_from_live_shape() {
    let evidence = selection_navigation_evidence(
      "Ballade No. 1 in G Minor, Op. 23 YUNDI",
      [
        ("Chopin: Ballades, Berceuse & Mazurkas YUNDI Classical 2016", "group", "NamedContainerAutomationPeer"),
        ("YUNDI", "text", "TextBlock"),
      ]
      .into_iter(),
    );

    assert!(evidence.is_some());
  }

  #[test]
  fn selection_navigation_rejects_unchanged_result_grid() {
    let evidence = selection_navigation_evidence(
      "Ballade No. 1 in G Minor, Op. 23 YUNDI",
      [("Ballade No. 1 in G Minor, Op. 23 YUNDI", "list item", "GridViewItem")].into_iter(),
    );

    assert_eq!(evidence, None);
  }

  #[test]
  fn search_result_selection_runs_search_then_typed_selection() {
    let inputs = SearchResultSelectInputs::with_query_and_anchor("Chopin ballade no. 1", "Ballade No. 1 in G Minor, Op. 23 YUNDI");
    let mut driver = RecordingDriver::new(SearchVerificationStatus::Verified);

    let search = run_search_with_driver(&inputs.search, &mut driver).expect("search should complete first");
    let (selected, input, verification) = driver
      .select_result(&inputs.anchor, Duration::from_millis(inputs.selection_timeout_ms), Duration::from_millis(inputs.search.settle_ms))
      .expect("selection should succeed");

    assert!(search.is_verified());
    assert_eq!(selected.name, inputs.anchor);
    assert_eq!(input.selected_path, InputDeliveryPath::AxPress);
    assert_eq!(verification.status, SearchVerificationStatus::Verified);
    assert_eq!(driver.calls.last().map(String::as_str), Some("select:Ballade No. 1 in G Minor, Op. 23 YUNDI:5000:300"));
  }

  #[test]
  fn uia_verification_prefers_value_pattern() {
    let evidence = uia_query_evidence(
      "AURORA Cure For Me",
      [
        ("Search", Some("AURORA Cure For Me"), "edit", true),
        ("AURORA Cure For Me", None, "text", false),
      ]
      .into_iter(),
    );

    assert_eq!(evidence, Some(("ui_automation_value".to_string(), "AURORA Cure For Me".to_string(),)));
  }

  #[test]
  fn uia_verification_uses_accessible_name_when_value_is_absent() {
    let evidence = uia_query_evidence("Cure For Me", [("AURORA — Cure For Me", None, "text", false)].into_iter());

    assert_eq!(evidence, Some(("ui_automation_name".to_string(), "AURORA — Cure For Me".to_string(),)));
  }

  #[test]
  fn uia_verification_returns_none_without_query_evidence() {
    let evidence = uia_query_evidence("Cure For Me", [("Search", Some("Different Song"), "edit", true)].into_iter());

    assert_eq!(evidence, None);
  }
}
