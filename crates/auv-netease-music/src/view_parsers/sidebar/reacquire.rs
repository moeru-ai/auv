//! Live sidebar reacquire adapter for SceneBridge A3-min.
//!
//! TODO(parser-layer-traits-a4): replace with RegionParser/ItemParser extraction.

use auv_driver::{InputPolicy, Scroll, ScrollOptions, Window};
use auv_driver_macos::MacosDriverSession;
use auv_view::memory::{
  ReacquireConfig, ReacquireDriverAdapter, ReacquireObservation, ReacquireOutcome, ReacquireTarget,
  ViewMemory, outcome_label, reacquire, strategy_name,
};
use auv_view::{ParserDiagnostic, ViewBounds};

use crate::view_memory::PlaylistReacquireSummary;
use crate::{Inputs, PlaylistSelectTarget, SidebarCandidateKind, SidebarSectionKind};

pub struct LiveSidebarReacquireAdapter<'a> {
  session: &'a MacosDriverSession,
  window: &'a Window,
  sidebar_bounds: ViewBounds,
  inputs: &'a Inputs,
  sidebar_anchor: auv_driver::WindowPoint,
  observation_index: usize,
}

impl<'a> LiveSidebarReacquireAdapter<'a> {
  pub fn new(
    session: &'a MacosDriverSession,
    window: &'a Window,
    sidebar_bounds: ViewBounds,
    inputs: &'a Inputs,
    sidebar_anchor: auv_driver::WindowPoint,
  ) -> Self {
    Self {
      session,
      window,
      sidebar_bounds,
      inputs,
      sidebar_anchor,
      observation_index: 0,
    }
  }
}

impl ReacquireDriverAdapter for LiveSidebarReacquireAdapter<'_> {
  fn observe_viewport(&mut self) -> Result<ReacquireObservation, ParserDiagnostic> {
    let capture = self
      .session
      .window()
      .capture(self.window)
      .map_err(|error| ParserDiagnostic {
        code: "reacquire_capture_failed".into(),
        message: error.to_string(),
        node_id: None,
      })?;
    let recognition = self
      .session
      .vision()
      .recognize_text_in_capture_with_options(
        &capture,
        crate::bounds_to_ratio(self.sidebar_bounds, &capture),
        self.inputs.ocr_options.clone(),
      )
      .map_err(|error| ParserDiagnostic {
        code: "reacquire_ocr_failed".into(),
        message: error.to_string(),
        node_id: None,
      })?;
    let recognition = crate::recognition_in_window_space(recognition, &capture);
    let observation = crate::view_parsers::sidebar::parse_sidebar_viewport(
      self.observation_index,
      self.sidebar_bounds,
      &recognition,
    );
    self.observation_index += 1;

    let mut current_section: Option<SidebarSectionKind> = None;
    let candidates = observation
      .candidates
      .iter()
      .filter_map(|candidate| {
        if candidate.kind == SidebarCandidateKind::SectionHeader {
          if let Some(label) = candidate.label.as_deref() {
            current_section = Some(SidebarSectionKind::from_label(label));
          }
          return None;
        }
        if candidate.kind != SidebarCandidateKind::PlaylistItem {
          return None;
        }
        let label = candidate.label.clone()?;
        let bounds = candidate.bounds?;
        Some(auv_view::memory::ReacquireCandidate {
          node_id: Some(candidate.id.clone()),
          label,
          section_hint: current_section.map(|kind| kind.domain_kind().to_string()),
          bounds,
        })
      })
      .collect();

    Ok(ReacquireObservation {
      fingerprint: observation.viewport_fingerprint.clone(),
      candidates,
    })
  }

  fn scroll_down(&mut self) -> Result<(), ParserDiagnostic> {
    self
      .session
      .window()
      .scroll(
        self.window,
        self.sidebar_anchor,
        Scroll::new(0.0, -self.inputs.scroll_amount),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: std::time::Duration::from_millis(self.inputs.scroll_settle_ms),
          ..ScrollOptions::default()
        },
      )
      .map(|_| ())
      .map_err(|error| ParserDiagnostic {
        code: "reacquire_scroll_down_failed".into(),
        message: error.to_string(),
        node_id: None,
      })
  }

  fn scroll_up(&mut self) -> Result<(), ParserDiagnostic> {
    self
      .session
      .window()
      .scroll(
        self.window,
        self.sidebar_anchor,
        Scroll::new(0.0, self.inputs.scroll_amount),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle: std::time::Duration::from_millis(self.inputs.scroll_settle_ms),
          ..ScrollOptions::default()
        },
      )
      .map(|_| ())
      .map_err(|error| ParserDiagnostic {
        code: "reacquire_scroll_up_failed".into(),
        message: error.to_string(),
        node_id: None,
      })
  }
}

pub fn try_reacquire_for_target(
  inputs: &Inputs,
  session: &MacosDriverSession,
  window: &Window,
  sidebar_bounds: ViewBounds,
  sidebar_anchor: auv_driver::WindowPoint,
  memory: &ViewMemory,
  target: &PlaylistSelectTarget,
) -> Option<(ViewBounds, PlaylistReacquireSummary)> {
  let mut adapter =
    LiveSidebarReacquireAdapter::new(session, window, sidebar_bounds, inputs, sidebar_anchor);
  let reacquire_target = ReacquireTarget::LabelWithSection {
    label: target.label.clone(),
    section_hint: Some(target.section_kind.domain_kind().to_string()),
  };
  let outcome = reacquire(
    memory,
    reacquire_target,
    &mut adapter,
    &ReacquireConfig::default(),
  );
  let outcome_label_str = outcome_label(&outcome).to_string();
  match outcome {
    ReacquireOutcome::Reacquired {
      node,
      strategy_used,
      observation_count,
      ..
    } => Some((
      node.bounds,
      PlaylistReacquireSummary {
        outcome: outcome_label_str,
        strategy_used: Some(strategy_name(strategy_used).to_string()),
        observation_count,
        skipped_rescan_replay: true,
      },
    )),
    ReacquireOutcome::Stale { .. } | ReacquireOutcome::NotFound { .. } => None,
  }
}
