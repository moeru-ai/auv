use std::fmt;

use auv_view::{ParserDiagnostic, ScanAppContext, ScanWindowContext, ViewBounds};
use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use crate::run_live_scan_until_query;
use crate::{Inputs, PlaybackControlState, PlaylistSelectTarget};

const PLAYLIST_SELECT_BOTTOM_SAFE_PADDING: f64 = 128.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistSelectResult {
  pub command: String,
  pub query: String,
  pub app: ScanAppContext,
  pub window: ScanWindowContext,
  pub target: PlaylistSelectTarget,
  pub verification: PlaylistSelectVerification,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
}

impl PlaylistSelectResult {
  pub fn to_human_readable(&self) -> PlaylistSelectHumanSummary<'_> {
    PlaylistSelectHumanSummary { result: self }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PlaylistSelectVerification {
  Passed {
    observed_title: String,
    evidence: PlaylistSelectVerificationEvidence,
  },
  Failed {
    evidence: PlaylistSelectVerificationEvidence,
  },
}

impl PlaylistSelectVerification {
  pub fn passed(&self) -> bool {
    matches!(self, Self::Passed { .. })
  }

  pub fn observed_title(&self) -> Option<&str> {
    match self {
      Self::Passed { observed_title, .. } => Some(observed_title),
      Self::Failed { .. } => None,
    }
  }

  fn used_sidebar_row_echo(&self) -> bool {
    matches!(
      self,
      Self::Passed {
        evidence: PlaylistSelectVerificationEvidence::SidebarRowEcho { .. },
        ..
      }
    )
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum PlaylistSelectVerificationEvidence {
  TitleOcr {
    tier: PlaylistSelectTitleOcrTier,
    recognized_region_count: usize,
    main_pane_match_count: usize,
    sidebar_echo_attempted: bool,
  },
  SidebarRowEcho {
    recognized_region_count: usize,
    main_pane_match_count: usize,
  },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlaylistSelectTitleOcrTier {
  TitleBand,
  HeroHeader,
  MainBand,
  FullWindow,
}

const PLAYLIST_SELECT_VERIFICATION_SIDEBAR_ECHO_LIMIT: &str = "verification_used_sidebar_row_echo_for_numeric_title";
const PLAYLIST_SELECT_VERIFICATION_ROW_ECHO_MARGIN: f64 = 16.0;
pub struct PlaylistSelectHumanSummary<'a> {
  result: &'a PlaylistSelectResult,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlaylistPlayResult {
  pub command: String,
  pub query: String,
  pub select: PlaylistSelectResult,
  pub verification: PlaylistPlayVerification,
  pub diagnostics: Vec<ParserDiagnostic>,
  pub known_limits: Vec<String>,
}

impl PlaylistPlayResult {
  pub fn to_human_readable(&self) -> PlaylistPlayHumanSummary<'_> {
    PlaylistPlayHumanSummary { result: self }
  }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PlaylistPlayVerification {
  Passed {
    control_state: PlaybackControlState,
    observed_bottom_text: Option<String>,
  },
  Failed {
    control_state: PlaybackControlState,
    observed_bottom_text: Option<String>,
  },
}

impl PlaylistPlayVerification {
  pub fn passed(&self) -> bool {
    matches!(self, Self::Passed { .. })
  }

  pub fn control_state(&self) -> PlaybackControlState {
    match self {
      Self::Passed { control_state, .. } | Self::Failed { control_state, .. } => *control_state,
    }
  }

  pub fn observed_bottom_text(&self) -> Option<&str> {
    match self {
      Self::Passed {
        observed_bottom_text,
        ..
      }
      | Self::Failed {
        observed_bottom_text,
        ..
      } => observed_bottom_text.as_deref(),
    }
  }
}

#[derive(Serialize)]
struct PlaylistPlayVerificationArtifact<'a> {
  before_bottom_text: Option<&'a str>,
  verification: &'a PlaylistPlayVerification,
}

pub struct PlaylistPlayHumanSummary<'a> {
  result: &'a PlaylistPlayResult,
}

impl fmt::Display for PlaylistSelectHumanSummary<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let result = self.result;
    writeln!(f, "NetEase playlist select")?;
    writeln!(f, "query: {}", result.query)?;
    writeln!(f, "target: {}", result.target.label)?;
    writeln!(
      f,
      "verification: {}{}",
      if result.verification.passed() {
        "passed"
      } else {
        "failed"
      },
      result.verification.observed_title().map(|title| format!(" observed_title={title}")).unwrap_or_default()
    )?;
    if result.known_limits.is_empty() {
      writeln!(f, "known_limits: (none)")?;
    } else {
      writeln!(f, "known_limits:")?;
      for limit in &result.known_limits {
        writeln!(f, "  - {limit}")?;
      }
    }
    if result.diagnostics.is_empty() {
      write!(f, "diagnostics: (none)")
    } else {
      writeln!(f, "diagnostics:")?;
      for diagnostic in &result.diagnostics {
        writeln!(f, "  - {}: {}", diagnostic.code, diagnostic.message)?;
      }
      Ok(())
    }
  }
}

impl fmt::Display for PlaylistPlayHumanSummary<'_> {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    let result = self.result;
    writeln!(f, "NetEase playlist play")?;
    writeln!(f, "query: {}", result.query)?;
    writeln!(f, "target: {}", result.select.target.label)?;
    writeln!(
      f,
      "verification: {} control={}",
      if result.verification.passed() {
        "passed"
      } else {
        "failed"
      },
      format!("{:?}", result.verification.control_state())
    )?;
    if result.known_limits.is_empty() {
      writeln!(f, "known_limits: (none)")?;
    } else {
      writeln!(f, "known_limits:")?;
      for limit in &result.known_limits {
        writeln!(f, "  - {limit}")?;
      }
    }
    if result.diagnostics.is_empty() {
      write!(f, "diagnostics: (none)")
    } else {
      writeln!(f, "diagnostics:")?;
      for diagnostic in &result.diagnostics {
        writeln!(f, "  - {}: {}", diagnostic.code, diagnostic.message)?;
      }
      Ok(())
    }
  }
}

fn playlist_click_options() -> auv_driver::ClickOptions {
  auv_driver::ClickOptions {
    policy: auv_driver::InputPolicy::BackgroundPreferred,
    click: auv_driver::Click::Single,
    window_strategy: auv_driver::WindowClickStrategy::ChromiumCompatible,
  }
}

fn playlist_play_verified_from_bottom_probe(
  control_state: PlaybackControlState,
  before_bottom_text: Option<&str>,
  observed_bottom_text: Option<&str>,
) -> bool {
  if control_state != PlaybackControlState::PauseVisible {
    return false;
  }

  let before = before_bottom_text.and_then(normalized_non_empty);
  let observed = observed_bottom_text.and_then(normalized_non_empty);
  match (before, observed) {
    (Some(before), Some(observed)) if before == observed => false,
    (Some(_), None) => false,
    _ => true,
  }
}

fn normalized_non_empty(input: &str) -> Option<String> {
  let normalized = crate::normalize_identity(input);
  (!normalized.is_empty()).then_some(normalized)
}

fn playlist_select_bottom_padding_scroll_needed(target_bounds: ViewBounds, sidebar_bounds: ViewBounds) -> bool {
  target_bounds.y + target_bounds.height > sidebar_bounds.y + sidebar_bounds.height - PLAYLIST_SELECT_BOTTOM_SAFE_PADDING
}

fn playlist_select_verification_main_pane_guard(bounds: ViewBounds, sidebar_bounds: ViewBounds, window_size: auv_driver::Size) -> bool {
  let main_pane_min_x = sidebar_bounds.x + sidebar_bounds.width * 0.85;
  let nav_band_max_y = window_size.height * 0.12;
  let main_content_max_y = window_size.height * 0.55;
  bounds.x >= main_pane_min_x && bounds.y > nav_band_max_y && bounds.y < main_content_max_y
}

fn playlist_select_verification_region_matches_target(target_label: &str, region_text: &str) -> bool {
  use crate::views::query_match::{PlaylistLabelMatchTier, playlist_label_match_tier};

  let target_identity = crate::normalize_identity(target_label);
  let label_identity = crate::normalize_identity(region_text);
  match playlist_label_match_tier(&label_identity, &target_identity) {
    PlaylistLabelMatchTier::Exact => true,
    PlaylistLabelMatchTier::Contains => !crate::view_parsers::sidebar::parse::is_single_ascii_digit_query(target_label),
    PlaylistLabelMatchTier::None => false,
  }
}

fn playlist_select_verification_title(
  recognition: &auv_driver::vision::TextRecognition,
  window_size: auv_driver::Size,
  sidebar_bounds: ViewBounds,
  target_label: &str,
) -> Option<String> {
  recognition
    .regions
    .iter()
    .filter(|region| {
      playlist_select_verification_main_pane_guard(
        ViewBounds::new(region.bounds.origin.x, region.bounds.origin.y, region.bounds.size.width, region.bounds.size.height),
        sidebar_bounds,
        window_size,
      )
    })
    .filter(|region| playlist_select_verification_region_matches_target(target_label, &region.text))
    .min_by(|left, right| left.bounds.origin.y.partial_cmp(&right.bounds.origin.y).unwrap_or(std::cmp::Ordering::Equal))
    .map(|region| region.text.trim().to_string())
}

fn build_playlist_select_verification_ocr_options(inputs: &Inputs, target_label: &str) -> auv_driver::vision::TextRecognitionOptions {
  if crate::view_parsers::sidebar::parse::is_single_ascii_digit_query(target_label) {
    crate::view_parsers::sidebar::target_probe::build_sidebar_target_probe_ocr_options(&inputs.ocr_options, target_label, target_label)
  } else {
    inputs.ocr_options.clone()
  }
}

fn playlist_select_verification_horizontal_ratio(sidebar_bounds: ViewBounds, window_size: auv_driver::Size) -> (f64, f64) {
  let window_width = window_size.width.max(1.0);
  let x_start = ((sidebar_bounds.x + sidebar_bounds.width) / window_width).clamp(0.24, 0.45);
  let width = (1.0 - x_start - 0.02).clamp(0.40, 0.76);
  (x_start, width)
}

fn playlist_select_verification_ratio(
  tier: PlaylistSelectTitleOcrTier,
  sidebar_bounds: ViewBounds,
  window_size: auv_driver::Size,
) -> auv_driver::RatioRect {
  if tier == PlaylistSelectTitleOcrTier::FullWindow {
    return auv_driver::RatioRect::new(0.0, 0.0, 1.0, 1.0);
  }

  let (x_start, width) = playlist_select_verification_horizontal_ratio(sidebar_bounds, window_size);
  match tier {
    // NOTICE(a6c-11): narrow band aligned with main_pane_guard nav floor (12% height).
    PlaylistSelectTitleOcrTier::TitleBand => auv_driver::RatioRect::new(x_start, 0.12, width, 0.22),
    // NOTICE(a6c-12): hero header above metadata line (1812 live y≈139 on 890px window).
    PlaylistSelectTitleOcrTier::HeroHeader => auv_driver::RatioRect::new(x_start, 0.08, width, 0.10),
    PlaylistSelectTitleOcrTier::MainBand => auv_driver::RatioRect::new(x_start, 0.10, width, 0.45),
    PlaylistSelectTitleOcrTier::FullWindow => unreachable!("full-window OCR ratio returns before main-pane geometry"),
  }
}

fn playlist_select_verification_region_overlaps_row_bounds(region_bounds: ViewBounds, row_bounds: ViewBounds, margin: f64) -> bool {
  let expanded =
    ViewBounds::new(row_bounds.x - margin, row_bounds.y - margin, row_bounds.width + margin * 2.0, row_bounds.height + margin * 2.0);
  region_bounds.x < expanded.x + expanded.width
    && region_bounds.x + region_bounds.width > expanded.x
    && region_bounds.y < expanded.y + expanded.height
    && region_bounds.y + region_bounds.height > expanded.y
}

fn playlist_select_verification_detail_chrome_present(
  recognition: &auv_driver::vision::TextRecognition,
  window_size: auv_driver::Size,
  sidebar_bounds: ViewBounds,
) -> bool {
  let play_all = crate::normalize_identity("播放全部");
  let song = crate::normalize_identity("歌曲");
  let comment = crate::normalize_identity("评论");
  let mut has_song = false;
  let mut has_comment = false;
  let mut has_play_all = false;

  for region in &recognition.regions {
    let region_bounds = ViewBounds::new(region.bounds.origin.x, region.bounds.origin.y, region.bounds.size.width, region.bounds.size.height);
    if !playlist_select_verification_main_pane_guard(region_bounds, sidebar_bounds, window_size) {
      continue;
    }
    let normalized = crate::normalize_identity(&region.text);
    if normalized.contains(&play_all) {
      has_play_all = true;
    }
    if normalized.contains(&song) {
      has_song = true;
    }
    if normalized.contains(&comment) {
      has_comment = true;
    }
  }

  has_play_all || (has_song && has_comment)
}

fn playlist_select_verification_sidebar_row_echo_from_recognition(
  sidebar_recognition: &auv_driver::vision::TextRecognition,
  main_recognition: &auv_driver::vision::TextRecognition,
  row_bounds: ViewBounds,
  target_label: &str,
  window_size: auv_driver::Size,
  sidebar_bounds: ViewBounds,
) -> Option<String> {
  if !crate::view_parsers::sidebar::parse::is_single_ascii_digit_query(target_label) {
    return None;
  }
  if !playlist_select_verification_detail_chrome_present(main_recognition, window_size, sidebar_bounds) {
    return None;
  }

  sidebar_recognition
    .regions
    .iter()
    .filter(|region| {
      playlist_select_verification_region_overlaps_row_bounds(
        ViewBounds::new(region.bounds.origin.x, region.bounds.origin.y, region.bounds.size.width, region.bounds.size.height),
        row_bounds,
        PLAYLIST_SELECT_VERIFICATION_ROW_ECHO_MARGIN,
      )
    })
    .find(|region| playlist_select_verification_region_matches_target(target_label, &region.text))
    .map(|region| region.text.trim().to_string())
}

#[cfg(test)]
#[path = "playlist_test.rs"]
mod tests;

#[cfg(not(target_os = "macos"))]
pub fn run_playlist_select(_inputs: &Inputs, _query: &str) -> Result<PlaylistSelectResult, String> {
  Err("live NetEase playlist select is only supported on macOS".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn run_playlist_play(_inputs: &Inputs, _query: &str) -> Result<PlaylistPlayResult, String> {
  Err("live NetEase playlist play is only supported on macOS".to_string())
}

#[cfg(target_os = "macos")]
pub fn run_playlist_select(inputs: &Inputs, query: &str) -> Result<PlaylistSelectResult, String> {
  let scan = run_live_scan_until_query(inputs, query)?;
  let target = scan.select_target(query)?;
  run_playlist_select_resolved(inputs, query, scan, target)
}

#[cfg(target_os = "macos")]
fn run_playlist_select_resolved(
  inputs: &Inputs,
  query: &str,
  scan: crate::PlaylistSidebarScan,
  target: PlaylistSelectTarget,
) -> Result<PlaylistSelectResult, String> {
  use crate::LIVE_TOP_SEEK_SCROLL_DELTA_MULTIPLIER;
  use crate::telemetry::{PlaylistSelectInputDelivered, PlaylistTargetResolved};
  use crate::view_parsers::sidebar::region::{broad_sidebar_probe_bounds, sidebar_scroll_anchor};
  use crate::view_parsers::sidebar::{
    PrecedingScrollContext, SidebarTargetProbeScrollContext, SidebarTargetSeekStep, capture_sidebar_target_probe,
    next_sidebar_target_seek_step, sidebar_rescan_target_seek_budget, sidebar_target_probe_diagnostic_message, top_seek_scroll_budget,
  };
  use auv_driver::selector::{App, Window};
  use auv_driver::{
    ActivationPolicy, Click, InputActionResult, InputDeliveryPath, InputPolicy, PrepareForInputOptions, Scroll, ScrollOptions, Size,
    WindowPoint,
  };

  let target_bounds =
    target.bounds.ok_or_else(|| format!("playlist target {:?} did not carry live bounds; rerun playlist ls/select", target.label))?;
  let target_observation_index = target.observation_index.unwrap_or(0);

  let session = auv_driver::open_local().map_err(|error| format!("failed to open macOS driver: {error}"))?;
  let app = App::bundle(inputs.app_id.clone());
  let window =
    session.window().resolve(Window::main_visible().owned_by(app)).map_err(|error| format!("failed to resolve NetEase window: {error}"))?;
  let window_size = Size::new(window.frame.size.width, window.frame.size.height);
  let sidebar_bounds = scan.sidebar_region().bounds.unwrap_or_else(|| broad_sidebar_probe_bounds(window_size));
  let sidebar_anchor = sidebar_scroll_anchor(sidebar_bounds);
  let mut diagnostics = scan.diagnostics().to_vec();
  let mut known_limits = scan.known_limits().to_vec();
  let mut click_bounds = target_bounds;
  {
    // NOTICE(netease-view-memory-retired): the default-off SceneBridge memory
    // experiment was removed because it had no current production consumer.
    // Reintroduce a memory shortcut only with an owner-approved runtime/read-side contract.
    // Rescan replay rewinds to the top and scroll-seeks the target label instead
    // of replaying a stale observation-index page count.
    // NOTICE(a6c-5): top rewind step size matches live top seek; motion stop deferred.
    let top_scroll_delta = inputs.scroll_amount * LIVE_TOP_SEEK_SCROLL_DELTA_MULTIPLIER;
    let top_scrolls = top_seek_scroll_budget(inputs.max_scrolls);
    let mut last_scroll_context = None;
    for index in 0..top_scrolls {
      match session.window().scroll(
        &window,
        sidebar_anchor,
        Scroll::new(0.0, top_scroll_delta),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: std::time::Duration::from_millis(inputs.scroll_settle_ms),
          ..ScrollOptions::default()
        },
      ) {
        Ok(result) => {
          last_scroll_context = Some(PrecedingScrollContext {
            step_name: format!("scroll-sidebar-top-{index}"),
            delta_y: top_scroll_delta,
            policy: "background_preferred".to_string(),
            settle_ms: inputs.scroll_settle_ms,
            delivery_path: Some(result.selected_path.as_str().to_string()),
            fallback_reason: result.fallback_reason().map(str::to_string),
          });
          auv_tracing::emit_event!(PlaylistSelectInputDelivered::SeekSidebarTop {
            attempt: index,
            bounds: sidebar_bounds,
            delivery: result,
          });
        }
        Err(error) => {
          diagnostics.push(ParserDiagnostic {
            code: "playlist_select_top_scroll_failed".to_string(),
            message: error.to_string(),
            node_id: target.candidate_id.clone(),
          });
          known_limits.push("playlist select top seek stopped after scroll failure".to_string());
          break;
        }
      }
    }

    let seek_budget = sidebar_rescan_target_seek_budget(inputs.max_scrolls, target_observation_index);
    let mut rescan_target_found = false;
    let mut last_rescan_probe_summary = None;
    let mut previous_sidebar_crop = None;
    for index in 0..seek_budget {
      let scroll_context = SidebarTargetProbeScrollContext {
        phase: "rescan".to_string(),
        attempt: index,
        scroll_anchor: (sidebar_anchor.0.x, sidebar_anchor.0.y),
        preceding_scroll: last_scroll_context.clone(),
      };
      let outcome = match capture_sidebar_target_probe(
        &session,
        &window,
        sidebar_bounds,
        inputs,
        index,
        &target.label,
        query,
        scroll_context,
        &mut previous_sidebar_crop,
      ) {
        Ok(outcome) => outcome,
        Err(error) => {
          diagnostics.push(ParserDiagnostic {
            code: "playlist_select_rescan_reobserve_failed".to_string(),
            message: error,
            node_id: target.candidate_id.clone(),
          });
          known_limits.push("playlist select rescan replay could not reobserve target before click".to_string());
          break;
        }
      };
      diagnostics.push(ParserDiagnostic {
        code: "playlist_select_rescan_reobserve_probe".to_string(),
        message: sidebar_target_probe_diagnostic_message("rescan", index, &outcome),
        node_id: target.candidate_id.clone(),
      });
      last_rescan_probe_summary = Some(sidebar_target_probe_diagnostic_message("rescan", index, &outcome));
      let found = outcome.probe.result.is_some();
      match next_sidebar_target_seek_step(index, seek_budget, found) {
        Some(SidebarTargetSeekStep::Found(_)) => {
          click_bounds = outcome.probe.result.expect("found step requires bounds");
          auv_tracing::emit_event!(PlaylistTargetResolved::RescanReplay {
            attempt: index,
            bounds: click_bounds,
          });
          rescan_target_found = true;
          break;
        }
        Some(SidebarTargetSeekStep::ScrollNext(_)) => {
          let result = session
            .window()
            .scroll(
              &window,
              sidebar_anchor,
              Scroll::new(0.0, -inputs.scroll_amount),
              ScrollOptions {
                policy: InputPolicy::ForegroundPreferred,
                settle: std::time::Duration::from_millis(inputs.scroll_settle_ms),
                ..ScrollOptions::default()
              },
            )
            .map_err(|error| format!("playlist select page scroll failed: {error}"))?;
          last_scroll_context = Some(PrecedingScrollContext {
            step_name: format!("scroll-sidebar-target-page-{index}"),
            delta_y: -inputs.scroll_amount,
            policy: "foreground_preferred".to_string(),
            settle_ms: inputs.scroll_settle_ms,
            delivery_path: Some(result.selected_path.as_str().to_string()),
            fallback_reason: result.fallback_reason().map(str::to_string),
          });
          auv_tracing::emit_event!(PlaylistSelectInputDelivered::SeekTargetPage {
            attempt: index,
            bounds: sidebar_bounds,
            delivery: result,
          });
        }
        None => break,
      }
    }

    if !rescan_target_found {
      diagnostics.push(ParserDiagnostic {
        code: "playlist_select_rescan_reobserve_missed_target".to_string(),
        message: format!(
          "target {:?} was not visible after rescan replay; last_probe={}",
          target.label,
          last_rescan_probe_summary.unwrap_or_else(|| "none".to_string())
        ),
        node_id: target.candidate_id.clone(),
      });
      known_limits.push("playlist select rescan replay could not reobserve target before click".to_string());
    }
  }

  for attempt in 0..2 {
    if !playlist_select_bottom_padding_scroll_needed(click_bounds, sidebar_bounds) {
      break;
    }

    let result = session
      .window()
      .scroll(
        &window,
        sidebar_anchor,
        Scroll::new(0.0, -inputs.scroll_amount),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: std::time::Duration::from_millis(inputs.scroll_settle_ms),
          ..ScrollOptions::default()
        },
      )
      .map_err(|error| format!("playlist select bottom padding scroll failed: {error}"))?;
    let bottom_padding_scroll = PrecedingScrollContext {
      step_name: format!("scroll-sidebar-bottom-padding-{attempt}"),
      delta_y: -inputs.scroll_amount,
      policy: "background_preferred".to_string(),
      settle_ms: inputs.scroll_settle_ms,
      delivery_path: Some(result.selected_path.as_str().to_string()),
      fallback_reason: result.fallback_reason().map(str::to_string),
    };
    auv_tracing::emit_event!(PlaylistSelectInputDelivered::SeekBottomPadding {
      attempt,
      bounds: sidebar_bounds,
      delivery: result,
    });

    let mut previous_sidebar_crop = None;
    match capture_sidebar_target_probe(
      &session,
      &window,
      sidebar_bounds,
      inputs,
      attempt,
      &target.label,
      query,
      SidebarTargetProbeScrollContext {
        phase: "bottom_padding".to_string(),
        attempt,
        scroll_anchor: (sidebar_anchor.0.x, sidebar_anchor.0.y),
        preceding_scroll: Some(bottom_padding_scroll),
      },
      &mut previous_sidebar_crop,
    ) {
      Ok(outcome) => {
        diagnostics.push(ParserDiagnostic {
          code: "playlist_select_bottom_padding_reobserve_probe".to_string(),
          message: sidebar_target_probe_diagnostic_message("bottom_padding", attempt, &outcome),
          node_id: target.candidate_id.clone(),
        });
        if let Some(bounds) = outcome.probe.result {
          click_bounds = bounds;
          auv_tracing::emit_event!(PlaylistTargetResolved::BottomPadding {
            attempt,
            bounds: click_bounds,
          });
        } else {
          diagnostics.push(ParserDiagnostic {
            code: "playlist_select_bottom_padding_reobserve_missed_target".to_string(),
            message: format!(
              "target {:?} was not visible after bottom padding scroll; probe={}",
              target.label,
              sidebar_target_probe_diagnostic_message("bottom_padding", attempt, &outcome)
            ),
            node_id: target.candidate_id.clone(),
          });
          known_limits.push("playlist select bottom padding could not reacquire target before click".to_string());
          break;
        }
      }
      Err(error) => {
        diagnostics.push(ParserDiagnostic {
          code: "playlist_select_bottom_padding_reobserve_failed".to_string(),
          message: error,
          node_id: target.candidate_id.clone(),
        });
        known_limits.push("playlist select bottom padding could not reacquire target before click".to_string());
        break;
      }
    }
  }

  let click_point = WindowPoint::new(click_bounds.x + click_bounds.width * 0.5, click_bounds.y + click_bounds.height * 0.5);
  let click = session
    .window()
    .click(&window, click_point, playlist_click_options())
    .map_err(|error| format!("playlist select click failed: {error}"))?;
  if inputs.scroll_settle_ms > 0 {
    std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
  }
  auv_tracing::emit_event!(PlaylistSelectInputDelivered::SelectPlaylist {
    bounds: click_bounds,
    delivery: click,
  });

  let mut verification = verify_playlist_select_title(&session, &window, window_size, sidebar_bounds, click_bounds, inputs, &target.label)?;

  if !verification.passed() {
    known_limits.push("background playlist row click did not verify; retried with foreground click".to_string());
    let screen_point = session
      .window()
      .to_screen_point(&window, click_point)
      .map_err(|error| format!("playlist select foreground point projection failed: {error}"))?;
    let lease = session
      .window()
      .prepare_for_input(
        &window,
        PrepareForInputOptions {
          activation: ActivationPolicy::Foreground {
            settle: std::time::Duration::from_millis(inputs.scroll_settle_ms),
          },
          preserve_frontmost: false,
          install_focus_guard: false,
          settle: std::time::Duration::from_millis(0),
        },
      )
      .map_err(|error| format!("playlist select foreground preparation failed: {error}"))?;
    let click_result = session.input().click_at(screen_point.point(), Click::Single);
    let restore_result = session.window().restore_input(lease);
    click_result.map_err(|error| format!("playlist select foreground click failed: {error}"))?;
    restore_result.map_err(|error| format!("playlist select foreground restore failed: {error}"))?;
    if inputs.scroll_settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
    }
    let delivery = InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents);
    auv_tracing::emit_event!(PlaylistSelectInputDelivered::SelectPlaylistForegroundRetry {
      bounds: click_bounds,
      delivery,
    });
    verification = verify_playlist_select_title(&session, &window, window_size, sidebar_bounds, click_bounds, inputs, &target.label)?;
  }

  if verification.used_sidebar_row_echo() {
    known_limits.push(PLAYLIST_SELECT_VERIFICATION_SIDEBAR_ECHO_LIMIT.to_string());
  }

  Ok(PlaylistSelectResult {
    command: "playlist.select".to_string(),
    query: query.to_string(),
    app: scan.app().clone(),
    window: scan.window().clone(),
    target,
    verification,
    diagnostics,
    known_limits,
  })
}

#[cfg(target_os = "macos")]
fn verify_playlist_select_title(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::Window,
  window_size: auv_driver::Size,
  sidebar_bounds: ViewBounds,
  row_bounds: ViewBounds,
  inputs: &Inputs,
  target_label: &str,
) -> Result<PlaylistSelectVerification, String> {
  auv_tracing::in_span!("auv.netease.playlist_select.verification", || {
    let capture = session.window().capture(window).map_err(|error| format!("playlist select verification capture failed: {error}"))?;
    crate::telemetry::png_artifact("auv.netease.playlist_select.verification_capture", &capture.image);

    let ocr_options = build_playlist_select_verification_ocr_options(inputs, target_label);
    let ocr_tiers = [
      PlaylistSelectTitleOcrTier::TitleBand,
      PlaylistSelectTitleOcrTier::HeroHeader,
      PlaylistSelectTitleOcrTier::MainBand,
      PlaylistSelectTitleOcrTier::FullWindow,
    ];

    let mut final_tier = PlaylistSelectTitleOcrTier::FullWindow;
    let mut observed_title = None;
    let mut last_recognition = None;

    for tier in ocr_tiers {
      final_tier = tier;
      let ocr_ratio = playlist_select_verification_ratio(tier, sidebar_bounds, window_size);
      let recognition = session
        .vision()
        .recognize_text_in_capture_with_options(&capture, ocr_ratio, ocr_options.clone())
        .map_err(|error| format!("playlist select verification OCR failed: {error}"))?;
      let recognition = crate::recognition_in_window_space(recognition, &capture);
      last_recognition = Some(recognition.clone());
      // NOTICE(a6c-4b): top-nav OCR in the upper band is not playlist detail title.
      observed_title = playlist_select_verification_title(&recognition, window_size, sidebar_bounds, target_label);
      if observed_title.is_some() {
        break;
      }
    }

    let recognition = last_recognition.ok_or_else(|| "playlist select verification OCR produced no recognition payload".to_string())?;
    let mut sidebar_echo_attempted = false;
    let mut used_sidebar_row_echo = false;

    if observed_title.is_none() && crate::view_parsers::sidebar::parse::is_single_ascii_digit_query(target_label) {
      sidebar_echo_attempted = true;
      let window_width = window_size.width.max(1.0);
      let window_height = window_size.height.max(1.0);
      let sidebar_ratio = auv_driver::RatioRect::new(
        sidebar_bounds.x / window_width,
        sidebar_bounds.y / window_height,
        sidebar_bounds.width / window_width,
        sidebar_bounds.height / window_height,
      );
      let sidebar_recognition = session
        .vision()
        .recognize_text_in_capture_with_options(&capture, sidebar_ratio, ocr_options.clone())
        .map_err(|error| format!("playlist select verification sidebar echo OCR failed: {error}"))?;
      let sidebar_recognition = crate::recognition_in_window_space(sidebar_recognition, &capture);
      crate::telemetry::json_artifact("auv.netease.playlist_select.sidebar_echo_recognition", &sidebar_recognition);
      if let Some(echo_title) = playlist_select_verification_sidebar_row_echo_from_recognition(
        &sidebar_recognition,
        &recognition,
        row_bounds,
        target_label,
        window_size,
        sidebar_bounds,
      ) {
        observed_title = Some(echo_title);
        used_sidebar_row_echo = true;
      }
    }

    crate::telemetry::json_artifact("auv.netease.playlist_select.recognition", &recognition);

    let recognized_region_count = recognition.regions.len();
    let main_pane_match_count = recognition
      .regions
      .iter()
      .filter(|region| {
        playlist_select_verification_main_pane_guard(
          ViewBounds::new(region.bounds.origin.x, region.bounds.origin.y, region.bounds.size.width, region.bounds.size.height),
          sidebar_bounds,
          window_size,
        )
      })
      .count();
    let evidence = if used_sidebar_row_echo {
      PlaylistSelectVerificationEvidence::SidebarRowEcho {
        recognized_region_count,
        main_pane_match_count,
      }
    } else {
      PlaylistSelectVerificationEvidence::TitleOcr {
        tier: final_tier,
        recognized_region_count,
        main_pane_match_count,
        sidebar_echo_attempted,
      }
    };

    Ok(match observed_title {
      Some(observed_title) => PlaylistSelectVerification::Passed {
        observed_title,
        evidence,
      },
      None => PlaylistSelectVerification::Failed { evidence },
    })
  })
}

#[cfg(target_os = "macos")]
pub fn run_playlist_play(inputs: &Inputs, query: &str) -> Result<PlaylistPlayResult, String> {
  let scan = run_live_scan_until_query(inputs, query)?;
  let target = scan.select_target(query)?;
  run_playlist_play_resolved(inputs, query, scan, target)
}

#[cfg(target_os = "macos")]
fn run_playlist_play_resolved(
  inputs: &Inputs,
  query: &str,
  scan: crate::PlaylistSidebarScan,
  target: PlaylistSelectTarget,
) -> Result<PlaylistPlayResult, String> {
  use crate::commands::daily_recommended::best_text_match;
  use crate::telemetry::PlaylistPlayInputDelivered;
  use auv_driver::selector::{App, Window};
  use auv_driver::{ActivationPolicy, Click, InputActionResult, InputDeliveryPath, PrepareForInputOptions, RatioRect, Size, WindowPoint};

  let select = run_playlist_select_resolved(inputs, query, scan, target)?;
  if !select.verification.passed() {
    return Err(format!("playlist select verification failed before play: observed_title={:?}", select.verification.observed_title()));
  }

  let session = auv_driver::open_local().map_err(|error| format!("failed to open macOS driver: {error}"))?;
  let app = App::bundle(inputs.app_id.clone());
  let window =
    session.window().resolve(Window::main_visible().owned_by(app)).map_err(|error| format!("failed to resolve NetEase window: {error}"))?;
  let window_size = Size::new(window.frame.size.width, window.frame.size.height);
  let diagnostics = select.diagnostics.clone();
  let mut known_limits = select.known_limits.clone();

  let capture = session.window().capture(&window).map_err(|error| format!("playlist play-all capture failed: {error}"))?;
  crate::telemetry::png_artifact("auv.netease.playlist_play.target_capture", &capture.image);
  let recognition = session
    .vision()
    .recognize_text_in_capture_with_options(&capture, RatioRect::new(0.0, 0.0, 1.0, 1.0), inputs.ocr_options.clone())
    .map_err(|error| format!("playlist play-all OCR failed: {error}"))?;
  let recognition = crate::recognition_in_window_space(recognition, &capture);
  let before_bottom_text = recognize_playlist_bottom_text(&session, &capture, inputs);
  let Some(target) = best_text_match(&recognition, "播放全部", window_size, |bounds, size| {
    bounds.x > size.width * 0.18 && bounds.y > size.height * 0.12 && bounds.y < size.height * 0.55
  }) else {
    return Err("playlist play-all text \"播放全部\" was not found".to_string());
  };
  let target_bounds = ViewBounds::new(target.bounds.origin.x, target.bounds.origin.y, target.bounds.size.width, target.bounds.size.height);
  let point = target.action_point();
  let click = session
    .window()
    .click(&window, WindowPoint::new(point.x, point.y), playlist_click_options())
    .map_err(|error| format!("playlist play-all click failed: {error}"))?;
  if inputs.scroll_settle_ms > 0 {
    std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
  }
  auv_tracing::emit_event!(PlaylistPlayInputDelivered::PlayAll {
    label: target.text,
    bounds: target_bounds,
    delivery: click,
  });

  let mut verification = capture_playlist_play_verification(&session, &window, inputs, before_bottom_text.as_deref())?;
  if !verification.passed() {
    known_limits.push("window-targeted Play All click did not verify playback; retried with foreground click".to_string());
    let screen_point = session
      .window()
      .to_screen_point(&window, WindowPoint::new(point.x, point.y))
      .map_err(|error| format!("playlist play-all foreground point projection failed: {error}"))?;
    let lease = session
      .window()
      .prepare_for_input(
        &window,
        PrepareForInputOptions {
          activation: ActivationPolicy::Foreground {
            settle: std::time::Duration::from_millis(inputs.scroll_settle_ms),
          },
          preserve_frontmost: false,
          install_focus_guard: false,
          settle: std::time::Duration::from_millis(0),
        },
      )
      .map_err(|error| format!("playlist play-all foreground preparation failed: {error}"))?;
    let click_result = session.input().click_at(screen_point.point(), Click::Single);
    let restore_result = session.window().restore_input(lease);
    click_result.map_err(|error| format!("playlist play-all foreground click failed: {error}"))?;
    restore_result.map_err(|error| format!("playlist play-all foreground restore failed: {error}"))?;
    if inputs.scroll_settle_ms > 0 {
      std::thread::sleep(std::time::Duration::from_millis(inputs.scroll_settle_ms));
    }
    let delivery = InputActionResult::single_success(InputDeliveryPath::ForegroundSystemEvents);
    auv_tracing::emit_event!(PlaylistPlayInputDelivered::PlayAllForegroundRetry {
      label: "播放全部".to_string(),
      bounds: target_bounds,
      delivery,
    });
    verification = capture_playlist_play_verification(&session, &window, inputs, before_bottom_text.as_deref())?;
  }
  if !verification.passed() {
    known_limits.push("playlist play-all click did not change the bottom player from its pre-click state".to_string());
  }

  Ok(PlaylistPlayResult {
    command: "playlist.play".to_string(),
    query: query.to_string(),
    select,
    verification,
    diagnostics,
    known_limits,
  })
}

#[cfg(target_os = "macos")]
fn capture_playlist_play_verification(
  session: &auv_driver_macos::MacosDriverSession,
  window: &auv_driver::Window,
  inputs: &Inputs,
  before_bottom_text: Option<&str>,
) -> Result<PlaylistPlayVerification, String> {
  use crate::views::player::classify_bottom_playback_control_state;

  auv_tracing::in_span!("auv.netease.playlist_play.verification", || {
    let capture = session.window().capture(window).map_err(|error| format!("playlist play verification capture failed: {error}"))?;
    crate::telemetry::png_artifact("auv.netease.playlist_play.verification_capture", &capture.image);
    let control_state = classify_bottom_playback_control_state(&capture.image);
    let bottom_text = recognize_playlist_bottom_text(session, &capture, inputs);
    let passed = playlist_play_verified_from_bottom_probe(control_state, before_bottom_text, bottom_text.as_deref());
    let verification = if passed {
      PlaylistPlayVerification::Passed {
        control_state,
        observed_bottom_text: bottom_text,
      }
    } else {
      PlaylistPlayVerification::Failed {
        control_state,
        observed_bottom_text: bottom_text,
      }
    };
    crate::telemetry::json_artifact(
      "auv.netease.playlist_play.verification",
      &PlaylistPlayVerificationArtifact {
        before_bottom_text,
        verification: &verification,
      },
    );

    Ok(verification)
  })
}

#[cfg(target_os = "macos")]
fn recognize_playlist_bottom_text(
  session: &auv_driver_macos::MacosDriverSession,
  capture: &auv_driver::Capture,
  inputs: &Inputs,
) -> Option<String> {
  use auv_driver::RatioRect;

  session
    .vision()
    .recognize_text_in_capture_with_options(capture, RatioRect::new(0.0, 0.88, 0.46, 0.12), inputs.ocr_options.clone())
    .ok()
    .map(|recognition| recognition.text.trim().to_string())
    .filter(|text| !text.is_empty())
}
