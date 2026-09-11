use crate::{Inputs, ScrollDirection, SongListItem, SongListObservation, SongListScanResult};
use auv_driver::vision::TextRecognition;
use auv_driver::{Size, WindowPoint};
use auv_view::{
  BoundaryConfidence, ParserDiagnostic, ScanAppContext, ScanWindowContext, ScrollBoundarySummary, ViewBounds, ViewRegionRecord,
};

#[cfg(target_os = "macos")]
use auv_driver::{InputPolicy, LocalDriverSession, Scroll, ScrollOptions, WindowInput as _};
#[cfg(target_os = "macos")]
use image::RgbaImage;
#[cfg(target_os = "macos")]
use std::collections::HashSet;

#[cfg(target_os = "macos")]
pub(crate) fn scan_open_song_list(
  session: LocalDriverSession,
  window: auv_driver::Window,
  inputs: &Inputs,
  target: &str,
  region_name: &str,
  diagnostics: Vec<ParserDiagnostic>,
  known_limits: Vec<String>,
) -> Result<SongListScanResult, String> {
  let app = ScanAppContext {
    app_id: window.app_bundle_id.clone().or_else(|| Some(inputs.app_id.clone())),
    name: window.app_name.clone(),
    version: None,
  };
  let window_context = ScanWindowContext {
    id: Some(window.reference.id.clone()),
    title: window.title.clone(),
    bounds: Some(ViewBounds::new(0.0, 0.0, window.frame.size.width, window.frame.size.height)),
  };
  let region_bounds = song_list_bounds(Size::new(window.frame.size.width, window.frame.size.height));
  let song_list_region = ViewRegionRecord {
    id: None,
    name: Some(region_name.to_string()),
    bounds: Some(region_bounds),
    coordinate_space: Some("window".to_string()),
  };
  let mut scanner = SongListScanner::new(session, window, inputs, region_bounds, diagnostics, known_limits);
  scanner.seek_boundary(ScrollDirection::Up)?;
  scanner.scan_down()?;
  Ok(scanner.finish(app, window_context, song_list_region, target))
}

fn song_list_bounds(window_size: Size) -> ViewBounds {
  // NOTICE(netease-song-leading-columns): the current desktop layout places
  // row indexes near 22% and titles near 27% of the window width. A 20% left
  // edge preserves both anchors while remaining right of the sidebar. Revisit
  // this ratio when a typed song-list view owns stable column/header bounds.
  let x = window_size.width * 0.20;
  let y = window_size.height * 0.23;
  let bottom = crate::view_parsers::sidebar::region::playlist_sidebar_bottom(window_size);
  ViewBounds::new(x, y, window_size.width - x - 24.0, (bottom - y).max(1.0))
}

fn parse_song_list_rows(observation_index: usize, bounds: ViewBounds, recognition: &TextRecognition) -> Vec<SongListItem> {
  let mut index_regions = recognition
    .regions
    .iter()
    .filter(|region| {
      let text = region.text.trim();
      let center_y = region.bounds.origin.y + region.bounds.size.height * 0.5;
      text.len() <= 3
        && text.chars().all(|ch| ch.is_ascii_digit())
        && auv_view::viewport_contains_center(bounds, recognized_bounds(&region.bounds))
        && center_y >= bounds.y + 36.0
    })
    .collect::<Vec<_>>();
  index_regions.sort_by(|left, right| left.bounds.origin.y.partial_cmp(&right.bounds.origin.y).unwrap_or(std::cmp::Ordering::Equal));

  let mut rows = Vec::new();
  for index_region in index_regions {
    let index = index_region.text.trim().parse::<u32>().ok();
    let row_center_y = index_region.bounds.origin.y + index_region.bounds.size.height * 0.5;
    let row_top = row_center_y - 32.0;
    let row_bottom = row_center_y + 32.0;
    let mut parts = recognition
      .regions
      .iter()
      .filter(|region| {
        let text = region.text.trim();
        let center_y = region.bounds.origin.y + region.bounds.size.height * 0.5;
        !text.is_empty()
          && center_y >= row_top
          && center_y <= row_bottom
          && auv_view::viewport_contains_center(bounds, recognized_bounds(&region.bounds))
          && region.bounds.origin.x > index_region.bounds.origin.x + 16.0
      })
      .collect::<Vec<_>>();
    parts.sort_by(|left, right| left.bounds.origin.x.partial_cmp(&right.bounds.origin.x).unwrap_or(std::cmp::Ordering::Equal));
    let title = parts
      .iter()
      .find(|region| {
        let x = region.bounds.origin.x;
        x > bounds.x + 60.0 && x < bounds.x + bounds.width * 0.52
      })
      .map(|region| region.text.trim().to_string());
    let row_text = parts.iter().map(|region| region.text.trim()).filter(|text| !text.is_empty()).collect::<Vec<_>>().join(" | ");
    let Some(title) = title.filter(|title| !title.is_empty()) else {
      continue;
    };
    rows.push(SongListItem {
      id: format!("song.obs{observation_index}.{}", index.map(|value| value.to_string()).unwrap_or_else(|| auv_view::slug(&title))),
      index,
      title,
      row_text,
      bounds: Some(ViewBounds::new(bounds.x, row_top, bounds.width, row_bottom - row_top)),
    });
  }
  rows
}

fn song_item_key(row: &SongListItem) -> String {
  row.index.map(|index| format!("index:{index}")).unwrap_or_else(|| format!("text:{}", auv_view::normalize_identity(&row.row_text)))
}

fn recognized_bounds(bounds: &auv_driver::Rect) -> ViewBounds {
  ViewBounds::new(bounds.origin.x, bounds.origin.y, bounds.size.width, bounds.size.height)
}

#[cfg(target_os = "macos")]
struct SongListScanner<'a> {
  session: LocalDriverSession,
  window: auv_driver::Window,
  inputs: &'a Inputs,
  region_bounds: ViewBounds,
  observations: Vec<SongListObservation>,
  items: Vec<SongListItem>,
  seen_items: HashSet<String>,
  boundary: ScrollBoundarySummary,
  pending_scroll_delivery_path: Option<String>,
  previous_crop: Option<RgbaImage>,
  motion_policy: crate::scroll::policies::detection_motion::MotionDetectionPolicy,
  diagnostics: Vec<ParserDiagnostic>,
  known_limits: Vec<String>,
}

#[cfg(target_os = "macos")]
impl<'a> SongListScanner<'a> {
  fn new(
    session: LocalDriverSession,
    window: auv_driver::Window,
    inputs: &'a Inputs,
    region_bounds: ViewBounds,
    diagnostics: Vec<ParserDiagnostic>,
    known_limits: Vec<String>,
  ) -> Self {
    Self {
      session,
      window,
      inputs,
      region_bounds,
      observations: Vec::new(),
      items: Vec::new(),
      seen_items: HashSet::new(),
      boundary: ScrollBoundarySummary::default(),
      pending_scroll_delivery_path: None,
      previous_crop: None,
      motion_policy: crate::scroll::policies::detection_motion::MotionDetectionPolicy::default(),
      diagnostics,
      known_limits,
    }
  }

  fn finish(self, app: ScanAppContext, window: ScanWindowContext, song_list_region: ViewRegionRecord, target: &str) -> SongListScanResult {
    SongListScanResult {
      command: "playlist.songs.ls".to_string(),
      target: target.to_string(),
      app,
      window,
      song_list_region,
      items: self.items,
      observations: self.observations,
      boundary: self.boundary,
      diagnostics: self.diagnostics,
      known_limits: self.known_limits,
    }
  }

  fn seek_boundary(&mut self, direction: ScrollDirection) -> Result<(), String> {
    self.pending_scroll_delivery_path = None;
    self.previous_crop = Some(self.capture_region_crop()?);
    let delta = match direction {
      ScrollDirection::Up => self.inputs.scroll_amount * crate::LIVE_TOP_SEEK_SCROLL_DELTA_MULTIPLIER,
      ScrollDirection::Down => -self.inputs.scroll_amount * crate::LIVE_TOP_SEEK_SCROLL_DELTA_MULTIPLIER,
    };
    let mut no_motion_confirmations = 0usize;
    for _ in 0..crate::LIVE_TOP_SEEK_MAX_SCROLL_INPUTS {
      self.scroll_region(delta, std::time::Duration::ZERO)?;
      std::thread::sleep(std::time::Duration::from_millis(crate::LIVE_FAST_SEEK_SAMPLE_INTERVAL_MS));
      let crop = self.capture_region_crop()?;
      if let Some(previous) = self.previous_crop.as_ref() {
        let motion = self.motion_policy.compare(previous, &crop);
        if motion.no_motion && crate::view_parsers::sidebar::successful_scroll_delivery_path(self.pending_scroll_delivery_path.as_deref()) {
          no_motion_confirmations += 1;
          if no_motion_confirmations >= 2 {
            match direction {
              ScrollDirection::Up => self.boundary.top = BoundaryConfidence::Likely,
              ScrollDirection::Down => self.boundary.bottom = BoundaryConfidence::Likely,
            }
            self.pending_scroll_delivery_path = None;
            self.previous_crop = Some(crop);
            return Ok(());
          }
        } else {
          no_motion_confirmations = 0;
        }
      }
      self.previous_crop = Some(crop);
      self.pending_scroll_delivery_path = None;
    }
    self.known_limits.push(format!(
      "song list {:?} seek stopped after max_scrolls={} without boundary confirmation",
      direction,
      crate::LIVE_TOP_SEEK_MAX_SCROLL_INPUTS
    ));
    Ok(())
  }

  fn scan_down(&mut self) -> Result<(), String> {
    self.pending_scroll_delivery_path = None;
    self.previous_crop = None;
    let mut consecutive_no_new = 0usize;
    let mut consecutive_no_motion = 0usize;
    for _ in 0..=self.inputs.max_scrolls {
      let observation = self.observe_page(self.observations.len())?;
      let introduced_new = self.record_items(&observation.rows);
      if introduced_new {
        consecutive_no_new = 0;
      } else if observation.incoming_scroll_delivery_path.is_some() {
        consecutive_no_new += 1;
      }
      if observation.scroll_motion.as_ref().is_some_and(|motion| motion.no_motion) {
        consecutive_no_motion += 1;
      } else {
        consecutive_no_motion = 0;
      }
      self.observations.push(observation);

      if consecutive_no_new >= 2 || consecutive_no_motion >= 2 {
        self.seek_boundary(ScrollDirection::Down)?;
        let final_observation = self.observe_page(self.observations.len())?;
        self.record_items(&final_observation.rows);
        self.observations.push(final_observation);
        return Ok(());
      }
      if self.observations.len() > self.inputs.max_scrolls {
        self.known_limits.push(format!("song list scan stopped after max_scrolls={}", self.inputs.max_scrolls));
        return Ok(());
      }
      self.scroll_region(-self.inputs.scroll_amount, std::time::Duration::from_millis(self.inputs.scroll_settle_ms))?;
    }
    Ok(())
  }

  fn record_items(&mut self, rows: &[SongListItem]) -> bool {
    let mut introduced_new = false;
    for row in rows {
      if self.seen_items.insert(song_item_key(row)) {
        self.items.push(row.clone());
        introduced_new = true;
      }
    }
    introduced_new
  }

  fn observe_page(&mut self, observation_index: usize) -> Result<SongListObservation, String> {
    auv_tracing::in_span!("auv.netease.song_list.observe", || {
      let capture = self.session.window().capture(&self.window).map_err(|error| format!("song list capture failed: {error}"))?;
      crate::telemetry::png_artifact("auv.netease.song_list.capture", &capture.image);
      let recognition = self
        .session
        .vision()
        .recognize_text_in_capture_with_options(
          &capture,
          crate::bounds_to_ratio(self.region_bounds, &capture),
          self.inputs.ocr_options.clone(),
        )
        .map_err(|error| format!("song list OCR failed: {error}"))?;
      let recognition = crate::recognition_in_window_space(recognition, &capture);
      let crop = crate::crop_image(&capture.image, self.region_bounds, capture.scale_factor);
      let incoming_scroll_delivery_path = self.pending_scroll_delivery_path.take();
      let scroll_motion =
        incoming_scroll_delivery_path.as_ref().and(self.previous_crop.as_ref()).map(|previous| self.motion_policy.compare(previous, &crop));
      self.previous_crop = Some(crop);
      Ok(SongListObservation {
        observation_index,
        incoming_scroll_delivery_path,
        scroll_motion,
        rows: parse_song_list_rows(observation_index, self.region_bounds, &recognition),
      })
    })
  }

  fn capture_region_crop(&mut self) -> Result<RgbaImage, String> {
    let capture = self.session.window().capture(&self.window).map_err(|error| format!("song list seek capture failed: {error}"))?;
    Ok(crate::crop_image(&capture.image, self.region_bounds, capture.scale_factor))
  }

  fn scroll_region(&mut self, vertical_delta: f64, settle: std::time::Duration) -> Result<(), String> {
    let point =
      WindowPoint::new(self.region_bounds.x + self.region_bounds.width * 0.5, self.region_bounds.y + self.region_bounds.height * 0.65);
    let result = self
      .session
      .window()
      .scroll(
        &self.window,
        point,
        Scroll::new(0.0, vertical_delta),
        ScrollOptions {
          policy: InputPolicy::BackgroundPreferred,
          settle,
          ..ScrollOptions::default()
        },
      )
      .map_err(|error| format!("song list scroll failed: {error}"))?;
    self.pending_scroll_delivery_path = Some(result.selected_path.as_str().to_string());
    Ok(())
  }
}

#[cfg(test)]
#[path = "song_test.rs"]
mod tests;
