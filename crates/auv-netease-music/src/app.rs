use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(target_os = "macos")]
use crate::Inputs;
#[cfg(target_os = "macos")]
use crate::run_live_scan;
use crate::views::player::PlayerView;
#[cfg(target_os = "macos")]
use crate::views::player::classify_bottom_playback_control_state;
#[cfg(target_os = "macos")]
use crate::views::screen;
use crate::views::screen::ScreenView;
use crate::views::sidebar::SidebarView;
#[cfg(target_os = "macos")]
use auv_driver::Capture;
#[cfg(target_os = "macos")]
use auv_driver::selector::{App, Window};
#[cfg(target_os = "macos")]
use auv_driver::{RatioRect, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObserveReuseMode {
  ReuseValidCache,
  ForceRefresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObserveScope {
  pub screen: bool,
  pub sidebar: bool,
  pub player: bool,
}

impl ObserveScope {
  pub fn all() -> Self {
    Self {
      screen: true,
      sidebar: true,
      player: true,
    }
  }
}

impl Default for ObserveScope {
  fn default() -> Self {
    Self::all()
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObserveOptions {
  pub reuse: ObserveReuseMode,
  pub scope: ObserveScope,
  pub cache_ttl: Duration,
}

impl Default for ObserveOptions {
  fn default() -> Self {
    Self {
      reuse: ObserveReuseMode::ReuseValidCache,
      scope: ObserveScope::all(),
      cache_ttl: Duration::from_millis(500),
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ObservationParts {
  pub screen: ScreenView,
  pub sidebar: SidebarView,
  pub player: PlayerView,
}

pub trait ObservationProvider {
  fn observe(&mut self, scope: ObserveScope) -> Result<ObservationParts, String>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct NeteaseCloudMusicObservation {
  generation: u64,
  observed_at_millis: u128,
  screen: ScreenView,
  sidebar: SidebarView,
  player: PlayerView,
}

impl NeteaseCloudMusicObservation {
  pub fn generation(&self) -> u64 {
    self.generation
  }

  pub fn observed_at_millis(&self) -> u128 {
    self.observed_at_millis
  }

  pub fn screen(&self) -> &ScreenView {
    &self.screen
  }

  pub fn sidebar(&self) -> &SidebarView {
    &self.sidebar
  }

  pub fn player(&self) -> &PlayerView {
    &self.player
  }
}

/// Product-level read API for NetEase Cloud Music observations.
///
/// TODO(netease-live-provider-v1): this first version still treats sidebar,
/// screen, and player as independently observed slices. Merging partial-scope
/// observations into one reconstruction generation needs an owner-approved
/// live observation contract.
pub struct NeteaseCloudMusic<P> {
  provider: P,
  cache: Option<CachedObservation>,
  next_generation: u64,
}

impl<P> NeteaseCloudMusic<P>
where
  P: ObservationProvider,
{
  pub fn new(provider: P) -> Self {
    Self {
      provider,
      cache: None,
      next_generation: 1,
    }
  }

  pub fn observe(&mut self, options: ObserveOptions) -> Result<&NeteaseCloudMusicObservation, String> {
    if self.can_reuse_cache(options) {
      return Ok(&self.cache.as_ref().expect("cache checked above").observation);
    }

    self.refresh(options.scope)
  }

  pub fn refresh(&mut self, scope: ObserveScope) -> Result<&NeteaseCloudMusicObservation, String> {
    let parts = self.provider.observe(scope)?;
    let observation = NeteaseCloudMusicObservation {
      generation: self.next_generation,
      observed_at_millis: observed_at_millis(),
      screen: parts.screen,
      sidebar: parts.sidebar,
      player: parts.player,
    };
    self.next_generation += 1;
    self.cache = Some(CachedObservation {
      created_at: Instant::now(),
      scope,
      observation,
    });

    Ok(&self.cache.as_ref().expect("cache was just written").observation)
  }

  pub fn invalidate_observation(&mut self) {
    self.cache = None;
  }

  fn can_reuse_cache(&self, options: ObserveOptions) -> bool {
    if options.reuse == ObserveReuseMode::ForceRefresh {
      return false;
    }

    let Some(cache) = &self.cache else {
      return false;
    };

    cache.scope == options.scope && cache.created_at.elapsed() <= options.cache_ttl
  }
}

#[cfg(target_os = "macos")]
impl NeteaseCloudMusic<LiveObservationProvider> {
  pub fn live(inputs: Inputs) -> Self {
    Self::new(LiveObservationProvider::new(inputs))
  }
}

#[cfg(target_os = "macos")]
pub struct LiveObservationProvider {
  inputs: Inputs,
}

#[cfg(target_os = "macos")]
impl LiveObservationProvider {
  pub fn new(inputs: Inputs) -> Self {
    Self { inputs }
  }
}

#[cfg(target_os = "macos")]
impl ObservationProvider for LiveObservationProvider {
  fn observe(&mut self, scope: ObserveScope) -> Result<ObservationParts, String> {
    let (screen, player) = if scope.screen || scope.player {
      self.observe_window(scope)?
    } else {
      (ScreenView::unknown(), PlayerView::unknown())
    };

    let sidebar = if scope.sidebar {
      let scan = run_live_scan(&self.inputs)?;
      SidebarView::from_projection(scan.projection().clone())
    } else {
      SidebarView::unknown()
    };

    Ok(ObservationParts {
      screen,
      sidebar,
      player,
    })
  }
}

#[cfg(target_os = "macos")]
impl LiveObservationProvider {
  fn observe_window(&self, scope: ObserveScope) -> Result<(ScreenView, PlayerView), String> {
    let session = auv_driver::open_local().map_err(|error| format!("live observation driver open failed: {error}"))?;
    let window = session
      .window()
      .resolve(Window::main_visible().owned_by(App::bundle(self.inputs.app_id.clone())))
      .map_err(|error| format!("live observation target window not found: {error}"))?;
    let capture = session.window().capture(&window).map_err(|error| format!("live observation window capture failed: {error}"))?;

    let screen = if scope.screen {
      let recognition = session
        .vision()
        .recognize_text_in_capture_with_options(&capture, RatioRect::new(0.0, 0.0, 1.0, 1.0), self.inputs.ocr_options.clone())
        .map_err(|error| format!("live observation full-window OCR failed: {error}"))?;
      let recognition = recognition_in_window_space(recognition, &capture);
      screen::classify_screen(&recognition, Size::new(window.frame.size.width, window.frame.size.height))
    } else {
      ScreenView::unknown()
    };

    let player = if scope.player {
      PlayerView::from_control_state(classify_bottom_playback_control_state(&capture.image))
    } else {
      PlayerView::unknown()
    };

    Ok((screen, player))
  }
}

#[cfg(target_os = "macos")]
fn recognition_in_window_space(
  mut recognition: auv_driver::vision::TextRecognition,
  capture: &Capture,
) -> auv_driver::vision::TextRecognition {
  for region in &mut recognition.regions {
    region.bounds.origin.x -= capture.bounds.origin.x;
    region.bounds.origin.y -= capture.bounds.origin.y;
  }
  recognition
}

struct CachedObservation {
  created_at: Instant,
  scope: ObserveScope,
  observation: NeteaseCloudMusicObservation,
}

fn observed_at_millis() -> u128 {
  SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::PlaybackControlState;
  use crate::views::player::PlayerView;
  use crate::views::screen::{ScreenState, ScreenView};
  use crate::views::sidebar::SidebarView;

  #[test]
  fn observe_reuses_valid_cache_without_calling_provider_again() {
    let mut app = NeteaseCloudMusic::new(FakeProvider::new());
    let options = ObserveOptions::default();

    let first_generation = app.observe(options).expect("observe").generation();
    let second_generation = app.observe(options).expect("observe").generation();

    assert_eq!(first_generation, second_generation);
    assert_eq!(app.provider.calls, 1);
  }

  #[test]
  fn force_refresh_calls_provider_and_advances_generation() {
    let mut app = NeteaseCloudMusic::new(FakeProvider::new());

    let first_generation = app.observe(ObserveOptions::default()).expect("observe").generation();
    let second_generation = app
      .observe(ObserveOptions {
        reuse: ObserveReuseMode::ForceRefresh,
        ..ObserveOptions::default()
      })
      .expect("observe")
      .generation();

    assert_eq!(first_generation + 1, second_generation);
    assert_eq!(app.provider.calls, 2);
  }

  #[test]
  fn invalidate_observation_forces_next_observe_to_refresh() {
    let mut app = NeteaseCloudMusic::new(FakeProvider::new());
    let options = ObserveOptions::default();

    let first_generation = app.observe(options).expect("observe").generation();
    app.invalidate_observation();
    let second_generation = app.observe(options).expect("observe").generation();

    assert_eq!(first_generation + 1, second_generation);
    assert_eq!(app.provider.calls, 2);
  }

  struct FakeProvider {
    calls: usize,
  }

  impl FakeProvider {
    fn new() -> Self {
      Self { calls: 0 }
    }
  }

  impl ObservationProvider for FakeProvider {
    fn observe(&mut self, _scope: ObserveScope) -> Result<ObservationParts, String> {
      self.calls += 1;
      Ok(ObservationParts {
        screen: ScreenView::for_tests(ScreenState::Default, None),
        sidebar: SidebarView::absent(),
        player: PlayerView::from_control_state(PlaybackControlState::PauseVisible),
      })
    }
  }
}
