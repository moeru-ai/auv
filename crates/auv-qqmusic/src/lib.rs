pub mod cli;
pub mod driver;
pub mod search;

mod tracing {
  pub(super) fn search<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.qqmusic.search", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(super) fn search_result_select<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.qqmusic.search_result.select", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }

  pub(super) fn search_result_click<T>(operation: impl FnOnce() -> T) -> T {
    #[cfg(feature = "tracing")]
    return auv_tracing::in_span!("auv.qqmusic.search_result.click", operation);
    #[cfg(not(feature = "tracing"))]
    operation()
  }
}

pub use driver::{MacosQqMusicDriver, QqMusicDriver};
pub use search::{
  DEFAULT_APP_ID, DEFAULT_SEARCH_SHORTCUT, DEFAULT_SETTLE_MS, SearchAction, SearchActionResult, SearchCommand, SearchCommandReport,
  SearchResultsAction, SearchResultsClick, SearchResultsSelect, SearchSubmit, run_search_command,
};
