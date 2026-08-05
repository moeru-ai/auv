// File: supported/apps/auv-netease-music/src/cli.rs
mod input;
mod presentation;

use std::num::{NonZeroU64, NonZeroUsize};
use std::path::PathBuf;
use std::process::ExitCode;

use auv_cli_common::outputs::cli::CliOutput;
use auv_cli_common::outputs::formats::table::TableOptions;
use auv_media_macos::OutputFormat;
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::output::{PlaylistOutput, render_song_list_human};
use crate::{
  Confidence, DailyRecommendedPlayInputs, Inputs, OpenWindowInputs, PlaybackStatusInputs, PlaylistCategory, run_daily_recommended_play,
  run_daily_recommended_songs_scan, run_live_scan, run_live_scan_until_query, run_open_window, run_playback_status_probe,
};
use input::{AppTargetArgs, OcrHintArgs, ScrollArgs, parse_ratio_region, positive_scroll_amount, zero_to_one};
pub(crate) use presentation::OutputMode;
use presentation::{OutputArgs, emit};

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum PlaylistOutputFormat {
  Json,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum PlaylistConfidenceArg {
  High,
  Medium,
  Low,
}

impl PlaylistConfidenceArg {
  fn into_confidence(self) -> Confidence {
    match self {
      Self::High => Confidence::High,
      Self::Medium => Confidence::Medium,
      Self::Low => Confidence::Low,
    }
  }
}

#[derive(Clone, Debug, PartialEq)]
struct PlaylistOutputOptions {
  mode: OutputMode,
  detail: bool,
  min_confidence: Option<Confidence>,
}

#[derive(Clone, Debug, PartialEq)]
struct PlaylistCommand {
  inputs: Inputs,
  query: Option<String>,
  output: PlaylistOutputOptions,
}

#[derive(Clone, Debug, PartialEq)]
struct PlaylistSelectCommand {
  inputs: Inputs,
  query: String,
  output: OutputMode,
}

#[derive(Clone, Debug, PartialEq)]
struct PlaylistPlayCommand {
  inputs: Inputs,
  query: String,
  output: OutputMode,
}

#[derive(Clone, Debug, PartialEq)]
struct DailyRecommendedPlayCommand {
  inputs: DailyRecommendedPlayInputs,
  output: OutputMode,
}

#[derive(Clone, Debug, PartialEq)]
struct PlaybackStatusCommand {
  inputs: PlaybackStatusInputs,
  output: OutputMode,
  wide: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct SongsLsCommand {
  inputs: Inputs,
  output: OutputMode,
}

#[derive(Clone, Debug, PartialEq)]
struct NowPlayingCommand {
  output: OutputMode,
  /// Only report now-playing when this app owns the slot (NetEase by default).
  app_id: String,
}

#[derive(Clone, Debug, PartialEq)]
struct OpenWindowCommand {
  inputs: OpenWindowInputs,
  output: OutputMode,
}

/// A transport command, scoped to act only when `app_id` owns the now-playing
/// slot. Reuses `auv_media_macos::MediaCommand` rather than a local mirror.
#[derive(Clone, Debug, PartialEq)]
struct ControlCommand {
  control: auv_media_macos::MediaCommand,
  app_id: String,
}

#[derive(Clone, Debug, PartialEq)]
struct SeekCommand {
  seconds: f64,
  app_id: String,
}

#[derive(Clone, Debug, PartialEq)]
enum Command {
  OpenWindow(OpenWindowCommand),
  PlaylistLs(PlaylistCommand),
  PlaylistSelect(PlaylistSelectCommand),
  PlaylistPlay(PlaylistPlayCommand),
  PlaylistPlayDailyRecommended(DailyRecommendedPlayCommand),
  PlaylistSongsLs(SongsLsCommand),
  NowPlaying(NowPlayingCommand),
  Control(ControlCommand),
  Seek(SeekCommand),
  PlaybackStatus(PlaybackStatusCommand),
}

#[derive(Clone, Debug, Parser)]
#[command(
  name = "auv-netease-music",
  version,
  disable_help_subcommand = true,
  about = "Inspect and control NetEase Cloud Music through AUV",
  long_about = "Inspect and control NetEase Cloud Music through AUV.\n\nThe CLI exposes typed operations for playlist discovery and playback, song-list scanning, now-playing inspection, and transport control. Human-readable output is the default; use --json or --json-out on commands that expose structured results.",
  after_long_help = "Examples:\n  # List playlists detected in the NetEase sidebar\n  auv-netease-music playlist ls\n\n  # Find a playlist\n  auv-netease-music playlist ls \"Trance vol.2\" --json\n\n  # Scan songs from the Daily Recommendations view\n  auv-netease-music playlist songs ls\n\n  # Read the system now-playing state when NetEase owns it\n  auv-netease-music now-playing"
)]
struct CliArgs {
  /// Directory containing the run store used by AUV tracing.
  #[arg(long = "store-root", global = true)]
  store_root: Option<PathBuf>,
  #[command(subcommand)]
  command: CliSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum CliSubcommand {
  /// Ensure NetEase Cloud Music is running and its window is visible.
  ///
  /// Launches the application when needed, resolves its main window, and
  /// reports the resolved application and window metadata.
  OpenWindow(OpenWindowArgs),
  /// Discover, open, play, and inspect NetEase playlists.
  Playlist(PlaylistArgs),
  /// Read the system now-playing state (via the macOS media API).
  #[command(name = "now-playing")]
  NowPlaying(NowPlayingArgs),
  /// Start playback (only when NetEase owns the now-playing slot).
  Play(ControlArgs),
  /// Pause (only when NetEase owns the now-playing slot).
  Pause(ControlArgs),
  /// Toggle play/pause (only when NetEase owns the now-playing slot).
  Toggle(ControlArgs),
  /// Skip to the next track (only when NetEase owns the now-playing slot).
  Next(ControlArgs),
  /// Return to the previous track (only when NetEase owns the now-playing slot).
  Previous(ControlArgs),
  /// Seek to a position in seconds (only when NetEase owns the now-playing slot).
  Seek(SeekArgs),
  /// Inspect playback information visible inside the NetEase application.
  Playback(PlaybackArgs),
}

#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  auv-netease-music open-window\n  auv-netease-music open-window --json\n  auv-netease-music open-window --exe 'C:\\\\Program Files\\\\NetEase\\\\cloudmusic.exe'"
)]
struct OpenWindowArgs {
  /// How long to wait for the window to appear after launch.
  #[arg(long = "settle-ms", default_value_t = 8_000)]
  settle_ms: u64,
  /// Explicit path to cloudmusic.exe.
  #[arg(long = "exe")]
  executable: Option<PathBuf>,
  /// Windows process name used to resolve the app window.
  #[arg(long = "process-name")]
  process_name: Option<String>,
  /// Localized window-title fallback.
  #[arg(long = "window-title")]
  window_title: Option<String>,
  /// Output the structured launch result as JSON.
  #[arg(long)]
  json: bool,
}

#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  auv-netease-music now-playing\n  auv-netease-music now-playing --format json\n  auv-netease-music now-playing --format json --json-out now-playing.json"
)]
struct NowPlayingArgs {
  /// Output format on stdout.
  #[arg(long = "format", value_enum, default_value_t = OutputFormat::Summary)]
  format: OutputFormat,
  /// Write the now-playing result as JSON to this file.
  #[arg(long = "json-out")]
  json_out: Option<PathBuf>,
  /// Only report now-playing when this app owns the slot (default: NetEase).
  #[arg(long = "app-id")]
  app_id: Option<String>,
}

#[derive(Clone, Debug, Args)]
struct ControlArgs {
  /// Only act when this app owns the now-playing slot (default: NetEase).
  #[command(flatten)]
  app: AppTargetArgs,
}

#[derive(Clone, Debug, Args)]
struct SeekArgs {
  /// Absolute playback position in seconds.
  #[arg(value_name = "SECONDS")]
  seconds: f64,
  /// Only act when this app owns the now-playing slot (default: NetEase).
  #[command(flatten)]
  app: AppTargetArgs,
}

#[derive(Clone, Debug, Args)]
struct PlaybackArgs {
  #[command(subcommand)]
  command: PlaybackSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum PlaybackSubcommand {
  /// Open the current song detail view and inspect its playback metadata.
  Status(PlaybackStatusArgs),
}

#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  auv-netease-music playback status\n  auv-netease-music playback status --wide\n  auv-netease-music playback status --json"
)]
struct PlaybackStatusArgs {
  #[command(flatten)]
  output: OutputArgs,
  #[command(flatten)]
  app: AppTargetArgs,
  /// Delay in milliseconds after opening the song detail view.
  #[arg(long = "settle-ms")]
  settle_ms: Option<u64>,
  /// Include the full set of observed playback fields.
  #[arg(long = "wide", alias = "detailed")]
  wide: bool,
  #[command(flatten)]
  ocr: OcrHintArgs,
}

#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  auv-netease-music playlist ls\n  auv-netease-music playlist ls \"Trance vol.2\" --json\n  auv-netease-music playlist songs ls"
)]
struct PlaylistArgs {
  #[command(subcommand)]
  command: PlaylistSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum PlaylistSubcommand {
  /// Scan and list playlists visible in the NetEase sidebar.
  Ls(PlaylistLsArgs),
  /// Open a sidebar playlist without starting playback.
  Select(PlaylistSelectArgs),
  /// Open a playlist and start playback.
  Play(PlaylistPlayArgs),
  /// Inspect songs shown in supported NetEase list views.
  Songs(PlaylistSongsArgs),
}

#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  # Scan the Daily Recommendations song table\n  auv-netease-music playlist songs ls\n\n  # Save the complete structured result\n  auv-netease-music playlist songs ls --json-out songs.json"
)]
struct PlaylistSongsArgs {
  #[command(subcommand)]
  command: PlaylistSongsSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum PlaylistSongsSubcommand {
  /// Scan and list songs from the Daily Recommendations view.
  Ls(SongsLsArgs),
}

#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  auv-netease-music playlist songs ls\n  auv-netease-music playlist songs ls --json\n  auv-netease-music playlist songs ls --max-scrolls 20 --json-out songs.json"
)]
struct SongsLsArgs {
  #[command(flatten)]
  output: OutputArgs,
  #[command(flatten)]
  app: AppTargetArgs,
  #[command(flatten)]
  scroll: ScrollArgs,
  #[command(flatten)]
  ocr: OcrHintArgs,
}

#[derive(Clone, Debug, Args)]
#[command(
  override_usage = "auv-netease-music playlist play <QUERY> [OPTIONS]\n       auv-netease-music playlist play daily-recommended [OPTIONS]",
  after_long_help = "Examples:\n  # Find and play a playlist by name\n  auv-netease-music playlist play \"Trance vol.2\"\n\n  # Open Daily Recommendations and start all songs\n  auv-netease-music playlist play daily-recommended"
)]
struct PlaylistPlayArgs {
  /// Playlist query or `daily-recommended`.
  #[arg(value_name = "QUERY")]
  target: Option<String>,
  #[command(flatten)]
  output: OutputArgs,
  #[command(flatten)]
  app: AppTargetArgs,
  #[command(flatten)]
  scroll: ScrollArgs,
  /// Normalized sidebar rectangle as x,y,width,height.
  #[arg(long = "sidebar-region", value_parser = parse_ratio_region)]
  sidebar_region: Option<auv_driver::RatioRect>,
  /// Maximum upward scroll steps used only by `daily-recommended`.
  #[arg(long = "max-top-scrolls")]
  max_top_scrolls: Option<NonZeroUsize>,
  /// Upward scroll distance per step used only by `daily-recommended`.
  #[arg(long = "top-scroll-amount", value_parser = positive_scroll_amount)]
  top_scroll_amount: Option<f64>,
  /// UI settle delay in milliseconds used only by `daily-recommended`.
  #[arg(long = "settle-ms")]
  settle_ms: Option<NonZeroU64>,
  /// PNG template used to verify the playing-state icon for `daily-recommended`.
  #[arg(long = "play-icon-template")]
  play_icon_template: Option<PathBuf>,
  /// Template-match threshold from 0 to 1 for `--play-icon-template`.
  #[arg(long = "play-icon-threshold", value_parser = zero_to_one)]
  play_icon_threshold: Option<f64>,
  #[command(flatten)]
  ocr: OcrHintArgs,
}

#[derive(Clone, Debug, Args)]
#[command(
  after_long_help = "Examples:\n  # List all observed playlists as a table\n  auv-netease-music playlist ls\n\n  # Stop after resolving a playlist keyword and emit structured output\n  auv-netease-music playlist ls \"Trance vol.2\" --json\n\n  # Inspect scan evidence and diagnostics\n  auv-netease-music playlist ls --detail"
)]
struct PlaylistLsArgs {
  /// Optional playlist keyword. When present, scanning continues until the keyword is found or the list boundary is reached.
  #[arg(value_name = "KEYWORD")]
  keyword: Option<String>,
  /// Restrict collection to all, created, or favorite playlist sections.
  #[arg(long = "category")]
  category: Option<PlaylistCategory>,
  /// Playlist keyword alias for callers that prefer an explicit option.
  #[arg(long = "filter")]
  filter: Option<String>,
  #[command(flatten)]
  output: OutputArgs,
  /// Output-format alias; `json` is equivalent to `--json`.
  #[arg(long = "format", value_enum)]
  format: Option<PlaylistOutputFormat>,
  /// Include per-playlist evidence and diagnostics.
  #[arg(long = "detail")]
  detail: bool,
  /// Hide playlist matches below this confidence level.
  #[arg(long = "min-confidence", value_enum)]
  min_confidence: Option<PlaylistConfidenceArg>,
  #[command(flatten)]
  app: AppTargetArgs,
  #[command(flatten)]
  scroll: ScrollArgs,
  /// Normalized sidebar rectangle as x,y,width,height.
  #[arg(long = "sidebar-region", value_parser = parse_ratio_region)]
  sidebar_region: Option<auv_driver::RatioRect>,
  #[command(flatten)]
  ocr: OcrHintArgs,
}

#[derive(Clone, Debug, Args)]
#[command(after_long_help = "Examples:\n  # Resolve and open a playlist by name\n  auv-netease-music playlist select \"Trance vol.2\"")]
struct PlaylistSelectArgs {
  /// Playlist name or substring to resolve in the sidebar.
  #[arg(value_name = "QUERY")]
  query: String,
  #[command(flatten)]
  output: OutputArgs,
  #[command(flatten)]
  app: AppTargetArgs,
  #[command(flatten)]
  scroll: ScrollArgs,
  /// Normalized sidebar rectangle as x,y,width,height.
  #[arg(long = "sidebar-region", value_parser = parse_ratio_region)]
  sidebar_region: Option<auv_driver::RatioRect>,
  #[command(flatten)]
  ocr: OcrHintArgs,
}

fn command_from_args(parsed: CliArgs) -> Result<Command, String> {
  match parsed.command {
    CliSubcommand::OpenWindow(args) => Ok(Command::OpenWindow(parse_open_window(args))),
    CliSubcommand::Playlist(args) => parse_playlist(args),
    CliSubcommand::NowPlaying(args) => parse_now_playing(args),
    CliSubcommand::Play(args) => Ok(control(auv_media_macos::MediaCommand::Play, args)),
    CliSubcommand::Pause(args) => Ok(control(auv_media_macos::MediaCommand::Pause, args)),
    CliSubcommand::Toggle(args) => Ok(control(auv_media_macos::MediaCommand::TogglePlayPause, args)),
    CliSubcommand::Next(args) => Ok(control(auv_media_macos::MediaCommand::NextTrack, args)),
    CliSubcommand::Previous(args) => Ok(control(auv_media_macos::MediaCommand::PreviousTrack, args)),
    CliSubcommand::Seek(args) => parse_seek(args),
    CliSubcommand::Playback(args) => match args.command {
      PlaybackSubcommand::Status(args) => parse_playback_status(args).map(Command::PlaybackStatus),
    },
  }
}

fn parse_open_window(args: OpenWindowArgs) -> OpenWindowCommand {
  let mut inputs = OpenWindowInputs::default();
  inputs.settle_ms = args.settle_ms;
  inputs.executable = args.executable;
  if let Some(process_name) = args.process_name {
    inputs.resolve.process_name = process_name;
  }
  if let Some(window_title) = args.window_title {
    inputs.resolve.title = window_title;
  }
  OpenWindowCommand {
    inputs,
    output: if args.json {
      OutputMode::Json
    } else {
      OutputMode::Human
    },
  }
}

/// Resolve an optional `--app-id` to the NetEase default when omitted.
fn resolve_app_id(app_id: Option<String>) -> String {
  app_id.unwrap_or_else(|| crate::DEFAULT_APP_ID.to_string())
}

fn control(control: auv_media_macos::MediaCommand, args: ControlArgs) -> Command {
  Command::Control(ControlCommand {
    control,
    app_id: resolve_app_id(args.app.app_id),
  })
}

fn parse_now_playing(args: NowPlayingArgs) -> Result<Command, String> {
  let output = match args.json_out {
    Some(path) => OutputMode::JsonFile(path),
    None => match args.format {
      OutputFormat::Json => OutputMode::Json,
      OutputFormat::Summary => OutputMode::Human,
    },
  };
  Ok(Command::NowPlaying(NowPlayingCommand {
    output,
    app_id: resolve_app_id(args.app_id),
  }))
}

fn parse_seek(args: SeekArgs) -> Result<Command, String> {
  // `Duration::try_from_secs_f64` rejects NaN, infinity, negative, and
  // values past `Duration::MAX`. The old check missed the overflow case;
  // `Duration::from_secs_f64` would have panicked on inputs like `1e20`.
  if std::time::Duration::try_from_secs_f64(args.seconds).is_err() {
    return Err("seek position must be a non-negative finite number of seconds within the representable range".to_string());
  }
  Ok(Command::Seek(SeekCommand {
    seconds: args.seconds,
    app_id: resolve_app_id(args.app.app_id),
  }))
}

fn parse_playlist(args: PlaylistArgs) -> Result<Command, String> {
  match args.command {
    PlaylistSubcommand::Ls(ls) => parse_playlist_ls(ls).map(Command::PlaylistLs),
    PlaylistSubcommand::Select(select) => parse_playlist_select(select).map(Command::PlaylistSelect),
    PlaylistSubcommand::Play(play) => parse_playlist_play(play),
    PlaylistSubcommand::Songs(songs) => match songs.command {
      PlaylistSongsSubcommand::Ls(args) => parse_songs_ls(args).map(Command::PlaylistSongsLs),
    },
  }
}

fn parse_playlist_ls(args: PlaylistLsArgs) -> Result<PlaylistCommand, String> {
  let mut inputs = Inputs::with_defaults();
  let query = args.keyword;

  if let Some(app_id) = args.app.app_id {
    inputs.app_id = app_id;
  }
  if let Some(max_scrolls) = args.scroll.max_scrolls {
    inputs.max_scrolls = max_scrolls.get();
  }
  if let Some(scroll_amount) = args.scroll.scroll_amount {
    inputs.scroll_amount = scroll_amount;
  }
  if let Some(scroll_settle_ms) = args.scroll.scroll_settle_ms {
    inputs.scroll_settle_ms = scroll_settle_ms;
  }
  if let Some(category) = args.category {
    inputs.category = category;
  }
  inputs.sidebar_region = args.sidebar_region;
  args.ocr.apply(&mut inputs.ocr_options)?;
  let query = args.filter.or(query);
  let mode = args.output.mode_with_json_alias(args.format == Some(PlaylistOutputFormat::Json));
  let output = PlaylistOutputOptions {
    mode,
    detail: args.detail,
    min_confidence: args.min_confidence.map(PlaylistConfidenceArg::into_confidence),
  };
  Ok(PlaylistCommand {
    inputs,
    query,
    output,
  })
}

fn parse_playlist_select(args: PlaylistSelectArgs) -> Result<PlaylistSelectCommand, String> {
  let mut inputs = Inputs::with_defaults();
  if let Some(app_id) = args.app.app_id {
    inputs.app_id = app_id;
  }
  if let Some(max_scrolls) = args.scroll.max_scrolls {
    inputs.max_scrolls = max_scrolls.get();
  }
  if let Some(scroll_amount) = args.scroll.scroll_amount {
    inputs.scroll_amount = scroll_amount;
  }
  if let Some(scroll_settle_ms) = args.scroll.scroll_settle_ms {
    inputs.scroll_settle_ms = scroll_settle_ms;
  }
  inputs.sidebar_region = args.sidebar_region;
  args.ocr.apply(&mut inputs.ocr_options)?;
  let output = args.output.mode();
  Ok(PlaylistSelectCommand {
    inputs,
    query: args.query,
    output,
  })
}

fn parse_playlist_play(args: PlaylistPlayArgs) -> Result<Command, String> {
  // TODO(playlist-play-command-shape): daily-recommended still shares one
  // option surface with scanned-playlist playback. Splitting that public
  // command path is deferred from this help-contract slice until the owner
  // approves the replacement command hierarchy.
  if args.target.as_deref() == Some("daily-recommended") {
    return parse_daily_recommended(args).map(Command::PlaylistPlayDailyRecommended);
  }

  parse_playlist_play_query(args).map(Command::PlaylistPlay)
}

fn parse_playlist_play_query(args: PlaylistPlayArgs) -> Result<PlaylistPlayCommand, String> {
  let query = match args.target.as_deref() {
    Some(query) if query.trim().is_empty() => {
      return Err("playlist play query must not be empty".to_string());
    }
    Some(query) => query.to_string(),
    None => {
      return Err("playlist play requires a query or daily-recommended".to_string());
    }
  };
  let mut inputs = Inputs::with_defaults();
  if let Some(app_id) = args.app.app_id {
    inputs.app_id = app_id;
  }
  if let Some(max_scrolls) = args.scroll.max_scrolls {
    inputs.max_scrolls = max_scrolls.get();
  }
  if let Some(scroll_amount) = args.scroll.scroll_amount {
    inputs.scroll_amount = scroll_amount;
  }
  if let Some(scroll_settle_ms) = args.scroll.scroll_settle_ms {
    inputs.scroll_settle_ms = scroll_settle_ms;
  }
  inputs.sidebar_region = args.sidebar_region;
  args.ocr.apply(&mut inputs.ocr_options)?;
  let output = args.output.mode();
  Ok(PlaylistPlayCommand {
    inputs,
    query,
    output,
  })
}

fn parse_songs_ls(args: SongsLsArgs) -> Result<SongsLsCommand, String> {
  let mut inputs = Inputs::with_defaults();
  inputs.scroll_amount = 520.0;
  if let Some(app_id) = args.app.app_id {
    inputs.app_id = app_id;
  }
  if let Some(max_scrolls) = args.scroll.max_scrolls {
    inputs.max_scrolls = max_scrolls.get();
  }
  if let Some(scroll_amount) = args.scroll.scroll_amount {
    inputs.scroll_amount = scroll_amount;
  }
  if let Some(scroll_settle_ms) = args.scroll.scroll_settle_ms {
    inputs.scroll_settle_ms = scroll_settle_ms;
  }
  args.ocr.apply(&mut inputs.ocr_options)?;
  let output = args.output.mode();
  Ok(SongsLsCommand { inputs, output })
}

fn parse_daily_recommended(args: PlaylistPlayArgs) -> Result<DailyRecommendedPlayCommand, String> {
  let mut inputs = DailyRecommendedPlayInputs::with_defaults();
  if let Some(app_id) = args.app.app_id {
    inputs.app_id = app_id;
  }
  if let Some(max_top_scrolls) = args.max_top_scrolls {
    inputs.max_top_scrolls = max_top_scrolls.get();
  }
  if let Some(top_scroll_amount) = args.top_scroll_amount {
    inputs.top_scroll_amount = top_scroll_amount;
  }
  if let Some(settle_ms) = args.settle_ms {
    inputs.settle_ms = settle_ms.get();
  }
  inputs.play_icon_template = args.play_icon_template;
  if let Some(threshold) = args.play_icon_threshold {
    inputs.play_icon_threshold = threshold;
  }
  args.ocr.apply(&mut inputs.ocr_options)?;
  let output = args.output.mode();
  Ok(DailyRecommendedPlayCommand { inputs, output })
}

fn parse_playback_status(args: PlaybackStatusArgs) -> Result<PlaybackStatusCommand, String> {
  let mut inputs = PlaybackStatusInputs::with_defaults();
  if let Some(app_id) = args.app.app_id {
    inputs.app_id = app_id;
  }
  if let Some(settle_ms) = args.settle_ms {
    inputs.settle_ms = settle_ms;
  }
  args.ocr.apply(&mut inputs.ocr_options)?;
  let output = args.output.mode();
  Ok(PlaybackStatusCommand {
    inputs,
    output,
    wide: args.wide,
  })
}

/// Entry point for the `auv-netease-music` binary.
pub fn run() -> ExitCode {
  let parsed = match CliArgs::try_parse_from(std::env::args()) {
    Ok(parsed) => parsed,
    Err(error) => {
      let exit_code = error.exit_code();
      let _ = error.print();
      return match u8::try_from(exit_code) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => ExitCode::from(code),
        Err(_) => ExitCode::from(2),
      };
    }
  };
  let store_root = parsed.store_root.clone();

  let command = match command_from_args(parsed) {
    Ok(command) => command,
    Err(error) => {
      if error.starts_with("error:") {
        eprint!("{error}");
      } else {
        eprintln!("error: {error}");
      }
      return ExitCode::from(2);
    }
  };

  let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
    Ok(runtime) => runtime,
    Err(error) => {
      eprintln!("error: failed to initialize the NetEase command runtime: {error}");
      return ExitCode::from(1);
    }
  };

  // The same command frontend serves daemon-free local operations and tonic
  // clients over Unix sockets. A Tokio reactor is therefore part of the
  // frontend boundary even when the selected command happens to stay local.
  runtime.block_on(async move {
    let Some(store_root) = store_root else {
      return execute_command(command).await;
    };
    let store = match auv_tracing::FileTracingStore::open(&store_root) {
      Ok(store) => std::sync::Arc::new(store),
      Err(error) => {
        eprintln!("error: failed to open tracing store {}: {error}", store_root.display());
        return ExitCode::from(1);
      }
    };
    let dispatch = match auv_tracing::configure().tracing_store(store).build() {
      Ok(dispatch) => dispatch,
      Err(error) => {
        eprintln!("error: failed to configure tracing store: {error}");
        return ExitCode::from(1);
      }
    };
    let root = auv_tracing::dispatcher::with_default(&dispatch, || auv_tracing::Context::root(auv_tracing::RunId::new()));
    let future = root.in_scope(|| execute_command(command));
    let exit = root.instrument(future).await;
    if let Err(error) = dispatch.flush().await {
      eprintln!("warning: run instrumentation flush failed: {error}");
    }
    exit
  })
}

async fn execute_command(command: Command) -> ExitCode {
  match command {
    Command::OpenWindow(cmd) => run_open_window_command(cmd),
    Command::PlaylistLs(cmd) => run_playlist(cmd),
    Command::PlaylistSelect(cmd) => run_playlist_select_command(cmd),
    Command::PlaylistPlay(cmd) => run_playlist_play_command(cmd),
    Command::PlaylistPlayDailyRecommended(cmd) => run_daily_recommended(cmd),
    Command::PlaylistSongsLs(cmd) => run_songs_ls(cmd),
    Command::NowPlaying(cmd) => run_now_playing(cmd).await,
    Command::Control(cmd) => run_control(cmd),
    Command::Seek(cmd) => run_seek(cmd),
    Command::PlaybackStatus(cmd) => run_playback_status(cmd),
  }
}

fn run_playlist(cmd: PlaylistCommand) -> ExitCode {
  let scan = match cmd.query.as_deref() {
    Some(query) => run_live_scan_until_query(&cmd.inputs, query),
    None => run_live_scan(&cmd.inputs),
  };
  let scan = match scan {
    Ok(scan) => scan,
    Err(error) => {
      eprintln!("scan failed: {error}");
      return ExitCode::from(1);
    }
  };

  crate::telemetry::json_artifact("auv.netease.playlist_sidebar_scan", &scan);

  let output = PlaylistOutput::new(&scan, cmd.query.as_deref(), cmd.output.min_confidence, cmd.output.detail);
  let json = output.to_json();

  emit(&cmd.output.mode, &json, || output.to_human(TableOptions::default()))
}

fn run_open_window_command(cmd: OpenWindowCommand) -> ExitCode {
  let result = match run_open_window(&cmd.inputs) {
    Ok(result) => result,
    Err(error) => {
      eprintln!("open-window failed: {error}");
      return ExitCode::from(1);
    }
  };

  let success = result.window_found;
  let exit = emit(&cmd.output, &result, || {
    let mut lines = vec![format!(
      "window: {}",
      if result.window_found {
        "visible"
      } else {
        "not found"
      }
    )];
    if let Some(title) = &result.window_title {
      lines.push(format!("title: {title}"));
    }
    lines.join("\n")
  });
  if exit == ExitCode::SUCCESS && !success {
    ExitCode::from(1)
  } else {
    exit
  }
}

fn run_playlist_select_command(cmd: PlaylistSelectCommand) -> ExitCode {
  // NOTICE(netease-run-artifact-reuse-retired): caller-supplied scan artifacts
  // were retired with the app-local RunStore reader. Reintroduce reuse only
  // through an owner-approved shared runtime consumer contract.
  let result = match crate::commands::playlist::run_playlist_select(&cmd.inputs, &cmd.query) {
    Ok(result) => result,
    Err(error) => {
      eprintln!("playlist select failed: {error}");
      return ExitCode::from(1);
    }
  };

  crate::telemetry::json_artifact("auv.netease.playlist_select_result", &result);

  emit(&cmd.output, &result, || result.to_human_readable().to_string())
}

fn run_playlist_play_command(cmd: PlaylistPlayCommand) -> ExitCode {
  let result = crate::commands::playlist::run_playlist_play(&cmd.inputs, &cmd.query);
  let result = match result {
    Ok(result) => result,
    Err(error) => {
      eprintln!("playlist play failed: {error}");
      return ExitCode::from(1);
    }
  };

  emit(&cmd.output, &result, || result.to_human_readable().to_string())
}

#[cfg(target_os = "macos")]
async fn run_now_playing(cmd: NowPlayingCommand) -> ExitCode {
  let state = match inherited_remote_context() {
    Some(context) => remote_now_playing(context, &cmd.app_id).await,
    None => auv_media_macos::now_playing().map_err(|error| error.to_string()),
  };
  let state = match state {
    Ok(state) => state,
    Err(error) => {
      eprintln!("now-playing read failed: {error}");
      return ExitCode::from(1);
    }
  };
  let (state, output) = crate::output::now_playing_for_app(state, &cmd.app_id);

  emit(&cmd.output, &output, || auv_media_macos::output::render_human_summary(&state))
}

#[cfg(not(target_os = "macos"))]
async fn run_now_playing(_cmd: NowPlayingCommand) -> ExitCode {
  eprintln!("now-playing is only available on macOS");
  ExitCode::from(1)
}

#[cfg(target_os = "macos")]
fn inherited_remote_context() -> Option<auv::AuvContext> {
  let context = auv::AuvContext::from_env().ok()?;
  // A root invocation without Device/Run selection remains the ordinary local
  // plugin path. Selecting a Device creates or attaches a Run before exec.
  context.run_id.as_ref()?;
  Some(context)
}

#[cfg(target_os = "macos")]
async fn remote_now_playing(context: auv::AuvContext, app_id: &str) -> Result<auv_media_macos::NowPlayingState, String> {
  let auv = auv::Client::from_context(context).await.map_err(|error| error.to_string())?;
  let run = auv.run(Default::default()).await.map_err(|error| format!("resolve inherited NetEase Run failed: {error}"))?;
  let runner = run
    .runner(auv::client::RunnerOptions {
      runner_class: "auv.app.netease_music".parse().expect("the NetEase RunnerClass ID is valid"),
      ..Default::default()
    })
    .await
    .map_err(|error| format!("construct NetEase Runner route failed: {error}"))?;
  let transport = runner.extension_transport().map_err(|error| error.to_string())?;
  let mut service = crate::api::v1::netease_music_service_client::NeteaseMusicServiceClient::new(transport);
  let result = service
    .get_now_playing(crate::api::v1::GetNowPlayingRequest {
      application_bundle_id: Some(app_id.to_string()),
    })
    .await
    .map(tonic::Response::into_inner)
    .map_err(|status| format!("NetEase Runner GetNowPlaying failed: {status}"));
  let response = result?;
  Ok(auv_media_macos::NowPlayingState {
    present: response.present,
    source_bundle_id: response.source_bundle_id,
    title: response.title,
    artist: response.artist,
    album: response.album,
    duration_seconds: response.duration_seconds,
    elapsed_seconds: response.elapsed_seconds,
    playback_rate: response.playback_rate,
    is_playing: response.is_playing,
    content_item_id: response.content_item_id,
    supports_like: response.supports_like,
    is_liked: response.is_liked,
  })
}

#[cfg(target_os = "macos")]
fn require_owner(app_id: &str) -> Result<(), ExitCode> {
  let state = match auv_media_macos::now_playing() {
    Ok(state) => state,
    Err(error) => {
      eprintln!("now-playing read failed: {error}");
      return Err(ExitCode::from(1));
    }
  };
  if state.source_bundle_id.as_deref() == Some(app_id) {
    return Ok(());
  }
  let current = match state.source_bundle_id.as_deref() {
    Some(other) => format!(" (current: {other})"),
    None => " (nothing playing)".to_string(),
  };
  eprintln!("skipped: {app_id} is not the current now-playing app{current}");
  Err(ExitCode::from(1))
}

#[cfg(target_os = "macos")]
fn run_control(cmd: ControlCommand) -> ExitCode {
  if let Err(code) = require_owner(&cmd.app_id) {
    return code;
  }
  match auv_media_macos::send_command(cmd.control) {
    Ok(()) => {
      println!("ok: {}", cmd.control.label());
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("control failed: {error}");
      ExitCode::from(1)
    }
  }
}

#[cfg(target_os = "windows")]
fn run_control(cmd: ControlCommand) -> ExitCode {
  use crate::{TransportAction, TransportInputs, run_transport_action};
  use auv_media_macos::MediaCommand;

  let action = match cmd.control {
    MediaCommand::TogglePlayPause => TransportAction::PlayPause,
    MediaCommand::NextTrack => TransportAction::Next,
    MediaCommand::PreviousTrack => TransportAction::Previous,
    MediaCommand::Play | MediaCommand::Pause => {
      // TODO(netease-windows-idempotent-playback): separate play and pause
      // require a reliable UIA-observed current state; add them only after a
      // live player-state selector is owner-approved and covered by a smoke.
      eprintln!("{} is not available through the Windows UIA slice; use `toggle`", cmd.control.label());
      return ExitCode::from(1);
    }
  };
  match run_transport_action(&TransportInputs::new(action)) {
    Ok(result) => {
      println!("ok: {} via {:?} control={:?} path={}", result.action, result.delivery.selected_path, result.control_name, result.node_path);
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("control failed: {error}");
      ExitCode::from(1)
    }
  }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn run_control(_cmd: ControlCommand) -> ExitCode {
  eprintln!("media controls are only available on macOS and Windows");
  ExitCode::from(1)
}

#[cfg(target_os = "macos")]
fn run_seek(cmd: SeekCommand) -> ExitCode {
  if let Err(code) = require_owner(&cmd.app_id) {
    return code;
  }
  let duration = match std::time::Duration::try_from_secs_f64(cmd.seconds) {
    Ok(duration) => duration,
    Err(_) => {
      eprintln!("seek failed: seek position must be a non-negative finite number of seconds within the representable range");
      return ExitCode::from(1);
    }
  };
  match auv_media_macos::seek(duration) {
    Ok(()) => {
      println!("ok: seek {}s", cmd.seconds);
      ExitCode::SUCCESS
    }
    Err(error) => {
      eprintln!("seek failed: {error}");
      ExitCode::from(1)
    }
  }
}

#[cfg(not(target_os = "macos"))]
fn run_seek(_cmd: SeekCommand) -> ExitCode {
  eprintln!("media controls are only available on macOS");
  ExitCode::from(1)
}

fn run_daily_recommended(cmd: DailyRecommendedPlayCommand) -> ExitCode {
  let result = match run_daily_recommended_play(&cmd.inputs) {
    Ok(result) => result,
    Err(error) => {
      eprintln!("play daily-recommended failed: {error}");
      return ExitCode::from(1);
    }
  };

  emit(&cmd.output, &result, || result.to_human_readable().to_string())
}

fn run_playback_status(cmd: PlaybackStatusCommand) -> ExitCode {
  let result = match run_playback_status_probe(&cmd.inputs) {
    Ok(result) => result,
    Err(error) => {
      eprintln!("playback status probe failed: {error}");
      return ExitCode::from(1);
    }
  };

  let json = result.to_json();
  emit(&cmd.output, &json, || result.to_human_readable(cmd.wide).to_string())
}

fn run_songs_ls(cmd: SongsLsCommand) -> ExitCode {
  let result = match run_daily_recommended_songs_scan(&cmd.inputs) {
    Ok(result) => result,
    Err(error) => {
      eprintln!("songs ls failed: {error}");
      return ExitCode::from(1);
    }
  };

  emit(&cmd.output, &result, || render_song_list_human(&result))
}
