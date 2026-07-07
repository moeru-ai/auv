pub mod cli;
pub mod driver;
pub mod search;

pub use driver::{MacosQqMusicDriver, OperationResult, QqMusicDriver};
pub use search::{
  DEFAULT_APP_ID, DEFAULT_SEARCH_SHORTCUT, DEFAULT_SETTLE_MS, SearchCommand, SearchCommandReport, SearchResultsAction, SearchResultsClick,
  SearchResultsSelect, SearchSubmit, run_search_command,
};
