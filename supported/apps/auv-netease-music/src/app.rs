use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(target_os = "macos")]
use crate::Inputs;
#[cfg(target_os = "macos")]
use crate::run_live_scan;
use crate::views::main::MainView;
use crate::views::player::PlayerView;
#[cfg(target_os = "macos")]
use crate::views::player::classify_bottom_playback_control_state;
#[cfg(target_os = "macos")]
use crate::views::screen;
use crate::views::screen::ScreenView;
use crate::views::sidebar::SidebarView;
use crate::{SongListScanResult, models::SongSource};
#[cfg(target_os = "macos")]
use auv_driver::Capture;
#[cfg(target_os = "macos")]
use auv_driver::selector::{App, Window};
#[cfg(target_os = "macos")]
use auv_driver::{RatioRect, Size};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewReuse {
  ReuseValidCache,
  Fresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewScope {
  pub screen: bool,
  pub main: bool,
  pub sidebar: bool,
  pub player: bool,
}

impl ViewScope {
  pub fn all() -> Self {
    Self {
      screen: true,
      main: true,
      sidebar: true,
      player: true,
    }
  }
}

impl Default for ViewScope {
  fn default() -> Self {
    Self::all()
  }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ViewRead {
  pub reuse: ViewReuse,
  pub scope: ViewScope,
  pub cache_ttl: Duration,
}

impl ViewRead {
  pub fn fresh() -> Self {
    Self {
      reuse: ViewReuse::Fresh,
      ..Self::default()
    }
  }
}

impl Default for ViewRead {
  fn default() -> Self {
    Self {
      reuse: ViewReuse::ReuseValidCache,
      scope: ViewScope::all(),
      cache_ttl: Duration::from_millis(500),
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewParts {
  pub screen: ScreenView,
  pub main: MainView,
  pub sidebar: SidebarView,
  pub player: PlayerView,
}

pub trait ViewProvider {
  fn read_views(&mut self, scope: ViewScope) -> Result<ViewParts, String>;
}

pub trait SongSourceProvider {
  fn songs_from(&mut self, source: SongSource) -> Result<SongListScanResult, String>;
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppViews {
  generation: u64,
  read_at_millis: u128,
  screen: ScreenView,
  main: MainView,
  sidebar: SidebarView,
  player: PlayerView,
}

impl AppViews {
  pub fn generation(&self) -> u64 {
    self.generation
  }

  pub fn read_at_millis(&self) -> u128 {
    self.read_at_millis
  }

  pub fn screen(&self) -> &ScreenView {
    &self.screen
  }

  pub fn main(&self) -> &MainView {
    &self.main
  }

  pub fn recommended(&self) -> Option<&crate::views::recommended::RecommendedView> {
    self.main.recommended()
  }

  pub fn sidebar(&self) -> &SidebarView {
    &self.sidebar
  }

  pub fn player(&self) -> &PlayerView {
    &self.player
  }
}

/// Product-level read interface for NetEase Cloud Music views.
///
/// TODO(netease-live-provider-v1): this first version still reads sidebar,
/// screen/main, and player as independent slices. Merge them into one capture
/// generation when the existing sidebar scan can consume the shared window
/// capture without weakening its scroll semantics.
pub struct NeteaseCloudMusic<P> {
  provider: P,
  cache: Option<CachedViews>,
  next_generation: u64,
}

impl<P> NeteaseCloudMusic<P>
where
  P: ViewProvider,
{
  pub fn from_provider(provider: P) -> Self {
    Self {
      provider,
      cache: None,
      next_generation: 1,
    }
  }

  pub fn views(&mut self, read: ViewRead) -> Result<&AppViews, String> {
    if self.can_reuse_cache(read) {
      return Ok(&self.cache.as_ref().expect("cache checked above").views);
    }

    self.refresh_views(read.scope)
  }

  pub fn refresh_views(&mut self, scope: ViewScope) -> Result<&AppViews, String> {
    let parts = self.provider.read_views(scope)?;
    let views = AppViews {
      generation: self.next_generation,
      read_at_millis: read_at_millis(),
      screen: parts.screen,
      main: parts.main,
      sidebar: parts.sidebar,
      player: parts.player,
    };
    self.next_generation += 1;
    self.cache = Some(CachedViews {
      created_at: Instant::now(),
      scope,
      views,
    });

    Ok(&self.cache.as_ref().expect("cache was just written").views)
  }

  pub fn invalidate_views(&mut self) {
    self.cache = None;
  }

  fn can_reuse_cache(&self, read: ViewRead) -> bool {
    if read.reuse == ViewReuse::Fresh {
      return false;
    }

    let Some(cache) = &self.cache else {
      return false;
    };

    cache.scope == read.scope && cache.created_at.elapsed() <= read.cache_ttl
  }
}

impl<P> NeteaseCloudMusic<P>
where
  P: ViewProvider + SongSourceProvider,
{
  /// Open the app-owned operation surface for Daily Recommended.
  ///
  /// This short path does not flatten the UI model: the live provider still
  /// resolves Recommended -> featured entries -> Daily Recommended before it
  /// opens the detail view. Callers that need to inspect that hierarchy can
  /// read it through [`Self::views`].
  pub fn daily_recommended(&mut self) -> DailyRecommended<'_, P> {
    DailyRecommended { app: self }
  }

  pub fn songs_from(&mut self, source: impl Into<SongSource>) -> Result<SongListScanResult, String> {
    self.invalidate_views();
    let result = self.provider.songs_from(source.into());
    self.invalidate_views();
    result
  }
}

/// Operations rooted at NetEase's Daily Recommended source.
pub struct DailyRecommended<'app, P> {
  app: &'app mut NeteaseCloudMusic<P>,
}

impl<P> DailyRecommended<'_, P>
where
  P: ViewProvider + SongSourceProvider,
{
  pub fn songs(self) -> Result<SongListScanResult, String> {
    self.app.songs_from(crate::models::DailyRecommendedRef)
  }
}

#[cfg(target_os = "macos")]
impl NeteaseCloudMusic<LiveViewProvider> {
  pub fn new() -> Self {
    Self::with_inputs(Inputs::with_defaults())
  }

  pub fn with_inputs(inputs: Inputs) -> Self {
    Self::from_provider(LiveViewProvider::new(inputs))
  }

  /// Configured live constructor retained for callers that already use the
  /// app crate's earlier naming.
  pub fn live(inputs: Inputs) -> Self {
    Self::with_inputs(inputs)
  }
}

#[cfg(target_os = "macos")]
pub struct LiveViewProvider {
  inputs: Inputs,
}

#[cfg(target_os = "macos")]
impl LiveViewProvider {
  pub fn new(inputs: Inputs) -> Self {
    Self { inputs }
  }
}

#[cfg(target_os = "macos")]
impl ViewProvider for LiveViewProvider {
  fn read_views(&mut self, scope: ViewScope) -> Result<ViewParts, String> {
    let (screen, main, player) = if scope.screen || scope.main || scope.player {
      self.observe_window(scope)?
    } else {
      (ScreenView::unknown(), MainView::Unknown, PlayerView::unknown())
    };

    let sidebar = if scope.sidebar {
      let scan = run_live_scan(&self.inputs)?;
      SidebarView::from_projection(scan.projection().clone())
    } else {
      SidebarView::unknown()
    };

    Ok(ViewParts {
      screen,
      main,
      sidebar,
      player,
    })
  }
}

#[cfg(target_os = "macos")]
impl SongSourceProvider for LiveViewProvider {
  fn songs_from(&mut self, source: SongSource) -> Result<SongListScanResult, String> {
    match source {
      SongSource::DailyRecommended(_) => crate::commands::daily_recommended::scan_daily_recommended_songs(&self.inputs),
      SongSource::Playlist(reference) => crate::commands::playlist::scan_playlist_songs(&self.inputs, &reference),
    }
  }
}

#[cfg(target_os = "macos")]
pub fn run_songs_scan(inputs: &crate::Inputs, source: impl Into<SongSource>) -> Result<SongListScanResult, String> {
  let mut app = NeteaseCloudMusic::with_inputs(inputs.clone());
  app.songs_from(source)
}

#[cfg(not(target_os = "macos"))]
pub fn run_songs_scan(_inputs: &crate::Inputs, _source: impl Into<SongSource>) -> Result<SongListScanResult, String> {
  Err("live NetEase song scan is only supported on macOS".to_string())
}

#[cfg(target_os = "macos")]
impl LiveViewProvider {
  fn observe_window(&self, scope: ViewScope) -> Result<(ScreenView, MainView, PlayerView), String> {
    let session = auv_driver::open_local().map_err(|error| format!("live observation driver open failed: {error}"))?;
    let window = session
      .window()
      .resolve(Window::main_visible().owned_by(App::bundle(self.inputs.app_id.clone())))
      .map_err(|error| format!("live observation target window not found: {error}"))?;
    let capture = session.window().capture(&window).map_err(|error| format!("live observation window capture failed: {error}"))?;

    let (screen, main) = if scope.screen || scope.main {
      let recognition = session
        .vision()
        .recognize_text_in_capture_with_options(&capture, RatioRect::new(0.0, 0.0, 1.0, 1.0), self.inputs.ocr_options.clone())
        .map_err(|error| format!("live observation full-window OCR failed: {error}"))?;
      let recognition = recognition_in_window_space(recognition, &capture);
      let window_size = Size::new(window.frame.size.width, window.frame.size.height);
      let screen = if scope.screen {
        screen::classify_screen(&recognition, window_size)
      } else {
        ScreenView::unknown()
      };
      let main = if scope.main {
        MainView::parse(&recognition, window_size)
      } else {
        MainView::Unknown
      };
      (screen, main)
    } else {
      (ScreenView::unknown(), MainView::Unknown)
    };

    let player = if scope.player {
      PlayerView::from_control_state(classify_bottom_playback_control_state(&capture.image))
    } else {
      PlayerView::unknown()
    };

    Ok((screen, main, player))
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

struct CachedViews {
  created_at: Instant,
  scope: ViewScope,
  views: AppViews,
}

fn read_at_millis() -> u128 {
  SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis()
}
