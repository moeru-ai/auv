// File: crates/auv-netease-music/src/cli.rs
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::PathBuf;
use std::process::ExitCode;

use auv_driver::RatioRect;
use auv_driver::vision::TextRecognitionOptions;
use auv_media_macos::OutputFormat;
use clap::{Args, Parser, Subcommand};

use crate::output::build_playlist_json_output;
use crate::{
  DailyRecommendedPlayInputs, Inputs, PlaybackStatusInputs, PlaylistCategory, SongListInputs,
  run_daily_recommended_play, run_daily_recommended_songs_scan, run_live_scan,
  run_playback_status_probe,
};

pub(crate) fn positive_scroll_amount(raw: &str) -> Result<f64, String> {
  let parsed = raw
    .parse::<f64>()
    .map_err(|_| "expects a number".to_string())?;
  if !parsed.is_finite() || parsed <= 0.0 {
    return Err("must be greater than 0".to_string());
  }
  Ok(parsed)
}

pub(crate) fn zero_to_one(raw: &str) -> Result<f64, String> {
  let parsed = raw
    .parse::<f64>()
    .map_err(|_| "expects a number".to_string())?;
  if !parsed.is_finite() || !(0.0..=1.0).contains(&parsed) {
    return Err("must be between 0 and 1".to_string());
  }
  Ok(parsed)
}

pub(crate) fn split_csv(value: &str) -> Vec<String> {
  value
    .split(',')
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .map(ToOwned::to_owned)
    .collect()
}

pub(crate) fn push_trimmed(values: &mut Vec<String>, value: String) {
  let value = value.trim();
  if !value.is_empty() && !values.iter().any(|existing| existing == value) {
    values.push(value.to_string());
  }
}

pub(crate) fn push_csv(values: &mut Vec<String>, value: &str) {
  for part in split_csv(value) {
    push_trimmed(values, part);
  }
}

pub(crate) fn push_ocr_language(options: &mut TextRecognitionOptions, language: String) {
  let language = language.trim();
  if language.is_empty() {
    return;
  }
  let languages = options.recognition_languages.get_or_insert_with(Vec::new);
  if !languages.iter().any(|existing| existing == language) {
    languages.push(language.to_string());
  }
}

pub(crate) fn load_custom_words_file(
  values: &mut Vec<String>,
  path: PathBuf,
) -> Result<(), String> {
  let content = std::fs::read_to_string(&path)
    .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
  for line in content.lines() {
    let word = line.trim();
    if !word.is_empty() && !word.starts_with('#') {
      push_trimmed(values, word.to_string());
    }
  }
  Ok(())
}

pub(crate) fn parse_ratio_region(value: String) -> Result<RatioRect, String> {
  let parts = value
    .split(',')
    .map(str::trim)
    .map(|part| {
      part
        .parse::<f64>()
        .map_err(|_| "--sidebar-region expects x,y,width,height".to_string())
    })
    .collect::<Result<Vec<_>, _>>()?;

  if parts.len() != 4 {
    return Err("--sidebar-region expects x,y,width,height".to_string());
  }

  if parts.iter().any(|part| !part.is_finite()) {
    return Err("--sidebar-region expects finite x,y,width,height".to_string());
  }

  if parts[2] <= 0.0 || parts[3] <= 0.0 {
    return Err("--sidebar-region width and height must be greater than 0".to_string());
  }

  Ok(RatioRect::new(parts[0], parts[1], parts[2], parts[3]))
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum OutputMode {
  Human,
  Json,
  JsonFile(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaylistCommand {
  pub inputs: Inputs,
  pub query: Option<String>,
  pub output: OutputMode,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DailyRecommendedPlayCommand {
  pub inputs: DailyRecommendedPlayInputs,
  pub output: OutputMode,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaybackStatusCommand {
  pub inputs: PlaybackStatusInputs,
  pub output: OutputMode,
  pub wide: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SongsLsCommand {
  pub inputs: SongListInputs,
  pub target: SongsLsTarget,
  pub output: OutputMode,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SongsLsTarget {
  DailyRecommended,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NowPlayingCommand {
  pub output: OutputMode,
  /// Only report now-playing when this app owns the slot (NetEase by default).
  pub app_id: String,
}

/// A transport command, scoped to act only when `app_id` owns the now-playing
/// slot. Reuses `auv_media_macos::MediaCommand` rather than a local mirror.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ControlCommand {
  pub control: auv_media_macos::MediaCommand,
  pub app_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SeekCommand {
  pub seconds: f64,
  pub app_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
  PlaylistLs(PlaylistCommand),
  PlaylistPlayDailyRecommended(DailyRecommendedPlayCommand),
  PlaylistSongsLs(SongsLsCommand),
  PlaybackStatus(PlaybackStatusCommand),
  NowPlaying(NowPlayingCommand),
  Control(ControlCommand),
  Seek(SeekCommand),
}

#[derive(Clone, Debug, Parser)]
#[command(
  name = "auv-netease-music",
  disable_help_subcommand = true,
  about = "NetEase Cloud Music CLI"
)]
struct CliArgs {
  #[command(subcommand)]
  command: CliSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum CliSubcommand {
  /// Work with NetEase Cloud Music playlists.
  Playlist(PlaylistArgs),
  /// Experimental current playback probes.
  Playback(PlaybackArgs),
}

#[derive(Clone, Debug, Args)]
struct PlaybackArgs {
  #[command(subcommand)]
  command: PlaybackSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum PlaybackSubcommand {
  /// Open the current song detail view and read the source label.
  Status(PlaybackStatusArgs),
}

#[derive(Clone, Debug, Args)]
struct PlaybackStatusArgs {
  #[arg(long = "json")]
  json: bool,
  #[arg(long = "json-out")]
  json_out: Option<PathBuf>,
  #[arg(long = "app-id")]
  app_id: Option<String>,
  #[arg(long = "artifact-dir")]
  artifact_dir: Option<PathBuf>,
  #[arg(long = "settle-ms")]
  settle_ms: Option<u64>,
  #[arg(long = "wide", alias = "detailed")]
  wide: bool,
  #[arg(long = "hint-ocr-custom-word")]
  custom_words: Vec<String>,
  #[arg(long = "hint-ocr-custom-words")]
  custom_word_csvs: Vec<String>,
  #[arg(long = "hint-ocr-custom-words-file")]
  custom_word_files: Vec<PathBuf>,
  #[arg(long = "hint-ocr-language")]
  ocr_languages: Vec<String>,
  #[arg(long = "hint-ocr-languages")]
  ocr_language_csvs: Vec<String>,
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
}

#[derive(Clone, Debug, Args)]
struct NowPlayingArgs {
  /// Output format on stdout.
  #[arg(long = "format", value_enum, default_value_t = OutputFormat::Summary)]
  format: OutputFormat,
  #[arg(long = "json-out")]
  json_out: Option<PathBuf>,
  /// Only report now-playing when this app owns the slot (default: NetEase).
  #[arg(long = "app-id")]
  app_id: Option<String>,
}

#[derive(Clone, Debug, Args)]
struct ControlArgs {
  /// Only act when this app owns the now-playing slot (default: NetEase).
  #[arg(long = "app-id")]
  app_id: Option<String>,
}

#[derive(Clone, Debug, Args)]
struct SeekArgs {
  #[arg(value_name = "seconds")]
  seconds: f64,
  /// Only act when this app owns the now-playing slot (default: NetEase).
  #[arg(long = "app-id")]
  app_id: Option<String>,
}

#[derive(Clone, Debug, Args)]
struct PlaylistArgs {
  #[command(subcommand)]
  command: Option<PlaylistSubcommand>,
  #[command(flatten)]
  ls: PlaylistLsArgs,
}

#[derive(Clone, Debug, Subcommand)]
enum PlaylistSubcommand {
  /// List NetEase Cloud Music sidebar playlists.
  Ls(PlaylistLsArgs),
  /// Play a built-in playlist.
  Play(PlaylistPlayArgs),
  /// Scan songs from a playlist-like song table.
  Songs(PlaylistSongsArgs),
}

#[derive(Clone, Debug, Args)]
struct PlaylistSongsArgs {
  #[command(subcommand)]
  command: PlaylistSongsSubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum PlaylistSongsSubcommand {
  /// List songs from a supported song list.
  Ls(SongsLsArgs),
}

#[derive(Clone, Debug, Args)]
struct SongsLsArgs {
  #[arg(value_name = "daily-recommended")]
  target: String,
  #[arg(long = "json")]
  json: bool,
  #[arg(long = "json-out")]
  json_out: Option<PathBuf>,
  #[arg(long = "app-id")]
  app_id: Option<String>,
  #[arg(long = "artifact-dir")]
  artifact_dir: Option<PathBuf>,
  #[arg(long = "max-scrolls")]
  max_scrolls: Option<NonZeroUsize>,
  #[arg(long = "scroll-amount", value_parser = positive_scroll_amount)]
  scroll_amount: Option<f64>,
  #[arg(long = "scroll-settle-ms")]
  scroll_settle_ms: Option<u64>,
  #[arg(long = "hint-ocr-custom-word")]
  custom_words: Vec<String>,
  #[arg(long = "hint-ocr-custom-words")]
  custom_word_csvs: Vec<String>,
  #[arg(long = "hint-ocr-custom-words-file")]
  custom_word_files: Vec<PathBuf>,
  #[arg(long = "hint-ocr-language")]
  ocr_languages: Vec<String>,
  #[arg(long = "hint-ocr-languages")]
  ocr_language_csvs: Vec<String>,
}

#[derive(Clone, Debug, Args)]
struct PlaylistPlayArgs {
  #[command(subcommand)]
  command: PlaylistPlaySubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum PlaylistPlaySubcommand {
  /// Open Daily Recommended and press Play All.
  #[command(name = "daily-recommended")]
  DailyRecommended(DailyRecommendedArgs),
}

#[derive(Clone, Debug, Args)]
struct DailyRecommendedArgs {
  #[arg(long = "json")]
  json: bool,
  #[arg(long = "json-out")]
  json_out: Option<PathBuf>,
  #[arg(long = "app-id")]
  app_id: Option<String>,
  #[arg(long = "artifact-dir")]
  artifact_dir: Option<PathBuf>,
  #[arg(long = "max-top-scrolls")]
  max_top_scrolls: Option<NonZeroUsize>,
  #[arg(long = "top-scroll-amount", value_parser = positive_scroll_amount)]
  top_scroll_amount: Option<f64>,
  #[arg(long = "settle-ms")]
  settle_ms: Option<NonZeroU64>,
  #[arg(long = "play-icon-template")]
  play_icon_template: Option<PathBuf>,
  #[arg(long = "play-icon-threshold", value_parser = zero_to_one)]
  play_icon_threshold: Option<f64>,
  #[arg(long = "hint-ocr-custom-word")]
  custom_words: Vec<String>,
  #[arg(long = "hint-ocr-custom-words")]
  custom_word_csvs: Vec<String>,
  #[arg(long = "hint-ocr-custom-words-file")]
  custom_word_files: Vec<PathBuf>,
  #[arg(long = "hint-ocr-language")]
  ocr_languages: Vec<String>,
  #[arg(long = "hint-ocr-languages")]
  ocr_language_csvs: Vec<String>,
}

#[derive(Clone, Debug, Args)]
struct PlaylistLsArgs {
  #[arg(value_name = "ls|keyword")]
  first: Option<String>,
  #[arg(value_name = "keyword")]
  second: Option<String>,
  #[arg(long = "category")]
  category: Option<PlaylistCategory>,
  #[arg(long = "filter")]
  filter: Option<String>,
  #[arg(long = "json")]
  json: bool,
  #[arg(long = "json-out")]
  json_out: Option<PathBuf>,
  #[arg(long = "app-id")]
  app_id: Option<String>,
  #[arg(long = "artifact-dir")]
  artifact_dir: Option<PathBuf>,
  #[arg(long = "max-scrolls")]
  max_scrolls: Option<NonZeroUsize>,
  #[arg(long = "scroll-amount", value_parser = positive_scroll_amount)]
  scroll_amount: Option<f64>,
  #[arg(long = "scroll-settle-ms")]
  scroll_settle_ms: Option<u64>,
  #[arg(long = "sidebar-region")]
  sidebar_region: Option<String>,
  #[arg(long = "hint-ocr-custom-word")]
  custom_words: Vec<String>,
  #[arg(long = "hint-ocr-custom-words")]
  custom_word_csvs: Vec<String>,
  #[arg(long = "hint-ocr-custom-words-file")]
  custom_word_files: Vec<PathBuf>,
  #[arg(long = "hint-ocr-language")]
  ocr_languages: Vec<String>,
  #[arg(long = "hint-ocr-languages")]
  ocr_language_csvs: Vec<String>,
}

fn command_from_args(parsed: CliArgs) -> Result<Command, String> {
  match parsed.command {
    CliSubcommand::Playlist(args) => parse_playlist(args),
    CliSubcommand::Playback(args) => parse_playback(args),
  }
}

fn parse_playback(args: PlaybackArgs) -> Result<Command, String> {
  match args.command {
    PlaybackSubcommand::Status(args) => parse_playback_status(args).map(Command::PlaybackStatus),
    CliSubcommand::NowPlaying(args) => parse_now_playing(args),
    CliSubcommand::Play(args) => Ok(control(auv_media_macos::MediaCommand::Play, args)),
    CliSubcommand::Pause(args) => Ok(control(auv_media_macos::MediaCommand::Pause, args)),
    CliSubcommand::Toggle(args) => Ok(control(
      auv_media_macos::MediaCommand::TogglePlayPause,
      args,
    )),
    CliSubcommand::Next(args) => Ok(control(auv_media_macos::MediaCommand::NextTrack, args)),
    CliSubcommand::Previous(args) => {
      Ok(control(auv_media_macos::MediaCommand::PreviousTrack, args))
    }
    CliSubcommand::Seek(args) => parse_seek(args),
  }
}

/// Resolve an optional `--app-id` to the NetEase default when omitted.
fn resolve_app_id(app_id: Option<String>) -> String {
  app_id.unwrap_or_else(|| crate::DEFAULT_APP_ID.to_string())
}

fn control(control: auv_media_macos::MediaCommand, args: ControlArgs) -> Command {
  Command::Control(ControlCommand {
    control,
    app_id: resolve_app_id(args.app_id),
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
    return Err(
      "seek position must be a non-negative finite number of seconds within the representable range"
        .to_string(),
    );
  }
  Ok(Command::Seek(SeekCommand {
    seconds: args.seconds,
    app_id: resolve_app_id(args.app_id),
  }))
}

fn parse_playlist(args: PlaylistArgs) -> Result<Command, String> {
  match args.command {
    Some(PlaylistSubcommand::Ls(ls)) => parse_playlist_ls(ls).map(Command::PlaylistLs),
    Some(PlaylistSubcommand::Play(play)) => parse_playlist_play(play),
    Some(PlaylistSubcommand::Songs(songs)) => parse_playlist_songs(songs),
    None => parse_playlist_ls(args.ls).map(Command::PlaylistLs),
  }
}

fn parse_playlist_ls(args: PlaylistLsArgs) -> Result<PlaylistCommand, String> {
  let mut inputs = Inputs::with_defaults();
  let query = match (args.first.as_deref(), args.second.as_deref()) {
    (None, None) => None,
    (Some("ls"), None) => None,
    (Some("ls"), Some(keyword)) => Some(keyword.to_string()),
    (Some(keyword), None) => Some(keyword.to_string()),
    (Some(_), Some(extra)) => return Err(format!("unexpected extra argument {extra:?}")),
    (None, Some(_)) => unreachable!("clap fills positional arguments in order"),
  };

  if let Some(app_id) = args.app_id {
    inputs.app_id = app_id;
  }
  if let Some(artifact_dir) = args.artifact_dir {
    inputs.artifact_dir = artifact_dir;
  }
  if let Some(max_scrolls) = args.max_scrolls {
    inputs.max_scrolls = max_scrolls.get();
  }
  if let Some(scroll_amount) = args.scroll_amount {
    inputs.scroll_amount = scroll_amount;
  }
  if let Some(scroll_settle_ms) = args.scroll_settle_ms {
    inputs.scroll_settle_ms = scroll_settle_ms;
  }
  if let Some(category) = args.category {
    inputs.category = category;
  }
  if let Some(sidebar_region) = args.sidebar_region {
    inputs.sidebar_region = Some(parse_ratio_region(sidebar_region)?);
  }
  for word in args.custom_words {
    push_trimmed(&mut inputs.ocr_options.custom_words, word);
  }
  for csv in args.custom_word_csvs {
    push_csv(&mut inputs.ocr_options.custom_words, &csv);
  }
  for path in args.custom_word_files {
    load_custom_words_file(&mut inputs.ocr_options.custom_words, path)?;
  }
  for language in args.ocr_languages {
    push_ocr_language(&mut inputs.ocr_options, language);
  }
  for csv in args.ocr_language_csvs {
    for language in split_csv(&csv) {
      push_ocr_language(&mut inputs.ocr_options, language);
    }
  }
  let query = args.filter.or(query);
  let output = match args.json_out {
    Some(path) => OutputMode::JsonFile(path),
    None if args.json => OutputMode::Json,
    None => OutputMode::Human,
  };
  Ok(PlaylistCommand {
    inputs,
    query,
    output,
  })
}

fn parse_playlist_play(args: PlaylistPlayArgs) -> Result<Command, String> {
  match args.command {
    PlaylistPlaySubcommand::DailyRecommended(args) => {
      parse_daily_recommended(args).map(Command::PlaylistPlayDailyRecommended)
    }
  }
}

fn parse_playlist_songs(args: PlaylistSongsArgs) -> Result<Command, String> {
  match args.command {
    PlaylistSongsSubcommand::Ls(args) => parse_songs_ls(args).map(Command::PlaylistSongsLs),
  }
}

fn parse_songs_ls(args: SongsLsArgs) -> Result<SongsLsCommand, String> {
  let target = match args.target.as_str() {
    "daily-recommended" => SongsLsTarget::DailyRecommended,
    other => {
      return Err(format!(
        "unsupported songs ls target {other:?}; expected \"daily-recommended\""
      ));
    }
  };
  let mut inputs = SongListInputs::with_defaults();
  if let Some(app_id) = args.app_id {
    inputs.app_id = app_id;
  }
  if let Some(artifact_dir) = args.artifact_dir {
    inputs.artifact_dir = artifact_dir;
  }
  if let Some(max_scrolls) = args.max_scrolls {
    inputs.max_scrolls = max_scrolls.get();
  }
  if let Some(scroll_amount) = args.scroll_amount {
    inputs.scroll_amount = scroll_amount;
  }
  if let Some(scroll_settle_ms) = args.scroll_settle_ms {
    inputs.scroll_settle_ms = scroll_settle_ms;
  }
  for word in args.custom_words {
    push_trimmed(&mut inputs.ocr_options.custom_words, word);
  }
  for csv in args.custom_word_csvs {
    push_csv(&mut inputs.ocr_options.custom_words, &csv);
  }
  for path in args.custom_word_files {
    load_custom_words_file(&mut inputs.ocr_options.custom_words, path)?;
  }
  for language in args.ocr_languages {
    push_ocr_language(&mut inputs.ocr_options, language);
  }
  for csv in args.ocr_language_csvs {
    for language in split_csv(&csv) {
      push_ocr_language(&mut inputs.ocr_options, language);
    }
  }
  let output = match args.json_out {
    Some(path) => OutputMode::JsonFile(path),
    None if args.json => OutputMode::Json,
    None => OutputMode::Human,
  };
  Ok(SongsLsCommand {
    inputs,
    target,
    output,
  })
}

fn parse_daily_recommended(
  args: DailyRecommendedArgs,
) -> Result<DailyRecommendedPlayCommand, String> {
  let mut inputs = DailyRecommendedPlayInputs::with_defaults();
  if let Some(app_id) = args.app_id {
    inputs.app_id = app_id;
  }
  if let Some(artifact_dir) = args.artifact_dir {
    inputs.artifact_dir = artifact_dir;
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
  for word in args.custom_words {
    push_trimmed(&mut inputs.ocr_options.custom_words, word);
  }
  for csv in args.custom_word_csvs {
    push_csv(&mut inputs.ocr_options.custom_words, &csv);
  }
  for path in args.custom_word_files {
    load_custom_words_file(&mut inputs.ocr_options.custom_words, path)?;
  }
  for language in args.ocr_languages {
    push_ocr_language(&mut inputs.ocr_options, language);
  }
  for csv in args.ocr_language_csvs {
    for language in split_csv(&csv) {
      push_ocr_language(&mut inputs.ocr_options, language);
    }
  }

  let output = match args.json_out {
    Some(path) => OutputMode::JsonFile(path),
    None if args.json => OutputMode::Json,
    None => OutputMode::Human,
  };
  Ok(DailyRecommendedPlayCommand { inputs, output })
}

fn parse_playback_status(args: PlaybackStatusArgs) -> Result<PlaybackStatusCommand, String> {
  let mut inputs = PlaybackStatusInputs::with_defaults();
  if let Some(app_id) = args.app_id {
    inputs.app_id = app_id;
  }
  if let Some(artifact_dir) = args.artifact_dir {
    inputs.artifact_dir = artifact_dir;
  }
  if let Some(settle_ms) = args.settle_ms {
    inputs.settle_ms = settle_ms;
  }
  for word in args.custom_words {
    push_trimmed(&mut inputs.ocr_options.custom_words, word);
  }
  for csv in args.custom_word_csvs {
    push_csv(&mut inputs.ocr_options.custom_words, &csv);
  }
  for path in args.custom_word_files {
    load_custom_words_file(&mut inputs.ocr_options.custom_words, path)?;
  }
  for language in args.ocr_languages {
    push_ocr_language(&mut inputs.ocr_options, language);
  }
  for csv in args.ocr_language_csvs {
    for language in split_csv(&csv) {
      push_ocr_language(&mut inputs.ocr_options, language);
    }
  }

  let output = match args.json_out {
    Some(path) => OutputMode::JsonFile(path),
    None if args.json => OutputMode::Json,
    None => OutputMode::Human,
  };
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

  match command_from_args(parsed) {
    Ok(Command::PlaylistLs(cmd)) => run_playlist(cmd),
    Ok(Command::PlaylistPlayDailyRecommended(cmd)) => run_daily_recommended(cmd),
    Ok(Command::PlaylistSongsLs(cmd)) => run_songs_ls(cmd),
    Ok(Command::PlaybackStatus(cmd)) => run_playback_status(cmd),
    Ok(Command::NowPlaying(cmd)) => run_now_playing(cmd),
    Ok(Command::Control(cmd)) => run_control(cmd),
    Ok(Command::Seek(cmd)) => run_seek(cmd),
    Err(error) => {
      if error.starts_with("error:") {
        eprint!("{error}");
      } else {
        eprintln!("error: {error}");
      }
      ExitCode::from(2)
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::path::{Path, PathBuf};
  use std::time::{SystemTime, UNIX_EPOCH};

  fn playlist_args() -> PlaylistArgs {
    PlaylistArgs {
      command: None,
      ls: playlist_ls_args(),
    }
  }

  fn playlist_ls_args() -> PlaylistLsArgs {
    PlaylistLsArgs {
      first: None,
      second: None,
      category: None,
      filter: None,
      json: false,
      json_out: None,
      app_id: None,
      artifact_dir: None,
      max_scrolls: None,
      scroll_amount: None,
      scroll_settle_ms: None,
      sidebar_region: None,
      custom_words: Vec::new(),
      custom_word_csvs: Vec::new(),
      custom_word_files: Vec::new(),
      ocr_languages: Vec::new(),
      ocr_language_csvs: Vec::new(),
    }
  }

  fn parse_playlist_command(argv: &[&str]) -> PlaylistCommand {
    let parsed = CliArgs::try_parse_from(argv).expect("CLI args should parse");
    match command_from_args(parsed).expect("playlist command should parse") {
      Command::PlaylistLs(command) => command,
      other => panic!("expected playlist ls command, got {other:?}"),
    }
  }

  fn parse_daily_recommended_command(argv: &[&str]) -> DailyRecommendedPlayCommand {
    let parsed = CliArgs::try_parse_from(argv).expect("CLI args should parse");
    match command_from_args(parsed).expect("daily recommended command should parse") {
      Command::PlaylistPlayDailyRecommended(command) => command,
      other => panic!("expected daily recommended command, got {other:?}"),
    }
  }

  fn parse_songs_ls_command(argv: &[&str]) -> SongsLsCommand {
    let parsed = CliArgs::try_parse_from(argv).expect("CLI args should parse");
    match command_from_args(parsed).expect("songs ls command should parse") {
      Command::PlaylistSongsLs(command) => command,
      other => panic!("expected songs ls command, got {other:?}"),
    }
  }

  fn parse_playback_status_command(argv: &[&str]) -> PlaybackStatusCommand {
    let parsed = CliArgs::try_parse_from(argv).expect("CLI args should parse");
    match command_from_args(parsed).expect("playback status command should parse") {
      Command::PlaybackStatus(command) => command,
      other => panic!("expected playback status command, got {other:?}"),
    }
  }

  struct TempWordsFile {
    path: PathBuf,
  }

  impl TempWordsFile {
    fn new(contents: &str) -> Self {
      let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();
      let path = std::env::temp_dir().join(format!(
        "auv-netease-cli-custom-words-{}-{unique}.txt",
        std::process::id()
      ));
      std::fs::write(&path, contents).expect("temp custom words file should be writable");
      Self { path }
    }

    fn path(&self) -> &Path {
      &self.path
    }
  }

  impl Drop for TempWordsFile {
    fn drop(&mut self) {
      let _ = std::fs::remove_file(&self.path);
    }
  }

  #[test]
  fn parse_playlist_without_positional_or_filter_leaves_query_empty() {
    let command = match parse_playlist(playlist_args()).expect("playlist args should parse") {
      Command::PlaylistLs(command) => command,
      other => panic!("expected playlist ls command, got {other:?}"),
    };

    assert_eq!(command.query, None);
    assert_eq!(command.output, OutputMode::Human);
  }

  #[test]
  fn parse_playlist_uses_positional_keyword_as_query() {
    let mut args = playlist_args();
    args.ls.first = Some("daily".to_string());

    let command = match parse_playlist(args).expect("playlist args should parse") {
      Command::PlaylistLs(command) => command,
      other => panic!("expected playlist ls command, got {other:?}"),
    };

    assert_eq!(command.query.as_deref(), Some("daily"));
  }

  #[test]
  fn parse_playlist_prefers_explicit_filter_over_positional_keyword() {
    let mut args = playlist_args();
    args.ls.first = Some("daily".to_string());
    args.ls.filter = Some("liked".to_string());

    let command = match parse_playlist(args).expect("playlist args should parse") {
      Command::PlaylistLs(command) => command,
      other => panic!("expected playlist ls command, got {other:?}"),
    };

    assert_eq!(command.query.as_deref(), Some("liked"));
  }

  #[test]
  fn clap_playlist_ls_leaves_query_empty() {
    let command = parse_playlist_command(&["auv-netease-music", "playlist", "ls"]);

    assert_eq!(command.query, None);
    assert_eq!(command.output, OutputMode::Human);
  }

  #[test]
  fn clap_playlist_ls_keyword_sets_query() {
    let command = parse_playlist_command(&["auv-netease-music", "playlist", "ls", "daily"]);

    assert_eq!(command.query.as_deref(), Some("daily"));
  }

  #[test]
  fn clap_playlist_legacy_keyword_still_sets_query() {
    let command = parse_playlist_command(&["auv-netease-music", "playlist", "daily"]);

    assert_eq!(command.query.as_deref(), Some("daily"));
  }

  #[test]
  fn clap_playlist_prefers_json_out_over_json_flag() {
    let command = parse_playlist_command(&[
      "auv-netease-music",
      "playlist",
      "ls",
      "--json",
      "--json-out",
      "/tmp/playlists.json",
    ]);

    assert_eq!(
      command.output,
      OutputMode::JsonFile(PathBuf::from("/tmp/playlists.json"))
    );
  }

  #[test]
  fn clap_playlist_maps_flags_into_inputs() {
    let command = parse_playlist_command(&[
      "auv-netease-music",
      "playlist",
      "ls",
      "--category",
      "favorite",
      "--app-id",
      "com.example.Player",
      "--artifact-dir",
      "/tmp/netease-artifacts",
      "--max-scrolls",
      "9",
      "--scroll-amount",
      "512",
      "--scroll-settle-ms",
      "750",
      "--sidebar-region",
      "0.1,0.2,0.3,0.4",
    ]);

    assert_eq!(command.inputs.category, PlaylistCategory::Favorite);
    assert_eq!(command.inputs.app_id, "com.example.Player");
    assert_eq!(
      command.inputs.artifact_dir,
      PathBuf::from("/tmp/netease-artifacts")
    );
    assert_eq!(command.inputs.max_scrolls, 9);
    assert_eq!(command.inputs.scroll_amount, 512.0);
    assert_eq!(command.inputs.scroll_settle_ms, 750);
    assert_eq!(
      command.inputs.sidebar_region,
      Some(parse_ratio_region("0.1,0.2,0.3,0.4".to_string()).expect("region should parse"))
    );
  }

  #[test]
  fn clap_playlist_allows_zero_scroll_settle_for_fast_collection() {
    let command = parse_playlist_command(&[
      "auv-netease-music",
      "playlist",
      "ls",
      "--scroll-settle-ms",
      "0",
    ]);

    assert_eq!(command.inputs.scroll_settle_ms, 0);
  }

  #[test]
  fn clap_playlist_collects_ocr_hint_flags() {
    let custom_words = TempWordsFile::new(
      r#"
        # comment
        Gamma
        Delta
        Alpha
      "#,
    );
    let custom_words_path = custom_words.path().to_string_lossy().into_owned();

    let command = parse_playlist_command(&[
      "auv-netease-music",
      "playlist",
      "ls",
      "--hint-ocr-custom-word",
      " Alpha ",
      "--hint-ocr-custom-word",
      "Alpha",
      "--hint-ocr-custom-words",
      "Beta, Gamma",
      "--hint-ocr-custom-words-file",
      custom_words_path.as_str(),
      "--hint-ocr-language",
      " zh-Hans ",
      "--hint-ocr-language",
      "zh-Hans",
      "--hint-ocr-languages",
      "en-US, ja-JP",
    ]);

    assert_eq!(
      command.inputs.ocr_options.custom_words,
      vec![
        "Alpha".to_string(),
        "Beta".to_string(),
        "Gamma".to_string(),
        "Delta".to_string(),
      ]
    );
    assert_eq!(
      command.inputs.ocr_options.recognition_languages,
      Some(vec![
        "zh-Hans".to_string(),
        "en-US".to_string(),
        "ja-JP".to_string(),
      ])
    );
  }

  #[test]
  fn clap_playlist_play_daily_recommended_maps_flags() {
    let command = parse_daily_recommended_command(&[
      "auv-netease-music",
      "playlist",
      "play",
      "daily-recommended",
      "--json",
      "--artifact-dir",
      "/tmp/netease-daily",
      "--play-icon-template",
      "/tmp/play.png",
    ]);

    assert_eq!(command.output, OutputMode::Json);
    assert_eq!(
      command.inputs.artifact_dir,
      PathBuf::from("/tmp/netease-daily")
    );
    assert_eq!(
      command.inputs.play_icon_template,
      Some(PathBuf::from("/tmp/play.png"))
    );
  }

  #[test]
  fn clap_playlist_songs_ls_daily_recommended_maps_flags() {
    let command = parse_songs_ls_command(&[
      "auv-netease-music",
      "playlist",
      "songs",
      "ls",
      "daily-recommended",
      "--json-out",
      "/tmp/songs.json",
      "--max-scrolls",
      "42",
      "--scroll-settle-ms",
      "0",
    ]);

    assert_eq!(command.target, SongsLsTarget::DailyRecommended);
    assert_eq!(
      command.output,
      OutputMode::JsonFile(PathBuf::from("/tmp/songs.json"))
    );
    assert_eq!(command.inputs.max_scrolls, 42);
    assert_eq!(command.inputs.scroll_settle_ms, 0);
  }

  #[test]
  fn clap_playback_status_maps_flags() {
    let command = parse_playback_status_command(&[
      "auv-netease-music",
      "playback",
      "status",
      "--json-out",
      "/tmp/playback-status.json",
      "--settle-ms",
      "250",
      "--wide",
    ]);

    assert_eq!(
      command.output,
      OutputMode::JsonFile(PathBuf::from("/tmp/playback-status.json"))
    );
    assert_eq!(command.inputs.settle_ms, 250);
    assert!(command.wide);
  }

  #[test]
  fn clap_playback_status_accepts_detailed_alias_for_wide_output() {
    let command =
      parse_playback_status_command(&["auv-netease-music", "playback", "status", "--detailed"]);

    assert!(command.wide);
  }

  #[test]
  fn playlist_songs_ls_rejects_unknown_target() {
    let parsed =
      CliArgs::try_parse_from(["auv-netease-music", "playlist", "songs", "ls", "current"])
        .expect("unknown target is rejected by semantic parser");
    let error = command_from_args(parsed).expect_err("unknown target should fail");

    assert_eq!(
      error,
      "unsupported songs ls target \"current\"; expected \"daily-recommended\""
    );
  }

  #[test]
  fn parse_daily_recommended_rejects_invalid_icon_threshold() {
    let error = CliArgs::try_parse_from([
      "auv-netease-music",
      "playlist",
      "play",
      "daily-recommended",
      "--play-icon-threshold",
      "1.2",
    ])
    .expect_err("threshold should fail clap parsing");

    assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
  }

  #[test]
  fn clap_playlist_rejects_extra_positional_argument() {
    let parsed = CliArgs::try_parse_from(["auv-netease-music", "playlist", "ls", "daily", "extra"])
      .expect("nested ls accepts positionals before semantic parsing");
    let error = command_from_args(parsed).expect_err("extra positional argument should fail");

    assert_eq!(error, "unexpected extra argument \"extra\"");
  }

  fn seek_args(seconds: f64) -> SeekArgs {
    SeekArgs {
      seconds,
      app_id: None,
    }
  }

  #[test]
  fn parse_seek_accepts_normal_value() {
    let command = parse_seek(seek_args(12.5)).expect("normal seek seconds should parse");
    match command {
      Command::Seek(cmd) => assert_eq!(cmd.seconds, 12.5),
      other => panic!("expected Command::Seek, got {other:?}"),
    }
  }

  #[test]
  fn parse_seek_rejects_negative() {
    parse_seek(seek_args(-1.0)).expect_err("negative seek seconds must be rejected");
  }

  #[test]
  fn parse_seek_rejects_nan_and_infinity() {
    parse_seek(seek_args(f64::NAN)).expect_err("NaN seek seconds must be rejected");
    parse_seek(seek_args(f64::INFINITY)).expect_err("infinity seek seconds must be rejected");
  }

  #[test]
  fn parse_seek_rejects_overflow_past_duration_max() {
    // `Duration::from_secs_f64` panics on values above `Duration::MAX`
    // (~1.84e19 seconds). The pre-fix parse_seek did not check overflow,
    // so a 1e20 input would have hit that panic during run_seek's Duration
    // construction.
    parse_seek(seek_args(1e20)).expect_err("overflow seek seconds must be rejected");
  }
}

fn run_playlist(cmd: PlaylistCommand) -> ExitCode {
  let scan = match run_live_scan(&cmd.inputs) {
    Ok(scan) => scan,
    Err(error) => {
      eprintln!("scan failed: {error}");
      return ExitCode::from(1);
    }
  };
  let output = build_playlist_json_output(&scan, cmd.query.as_deref());

  match &cmd.output {
    OutputMode::Human => {
      println!("{}", scan.to_human_readable());
      ExitCode::SUCCESS
    }
    OutputMode::Json => match serde_json::to_string_pretty(&output) {
      Ok(json) => {
        println!("{json}");
        ExitCode::SUCCESS
      }
      Err(error) => {
        eprintln!("encode failed: {error}");
        ExitCode::from(1)
      }
    },
    OutputMode::JsonFile(path) => {
      let json = match serde_json::to_string_pretty(&output) {
        Ok(json) => json,
        Err(error) => {
          eprintln!("encode failed: {error}");
          return ExitCode::from(1);
        }
      };
      if let Err(error) = std::fs::write(path, json) {
        eprintln!("failed to write {}: {error}", path.display());
        return ExitCode::from(1);
      }
      ExitCode::SUCCESS
    }
  }
}

#[cfg(target_os = "macos")]
fn run_now_playing(cmd: NowPlayingCommand) -> ExitCode {
  let state = match auv_media_macos::now_playing() {
    Ok(state) => state,
    Err(error) => {
      eprintln!("now-playing read failed: {error}");
      return ExitCode::from(1);
    }
  };
  // Scope to NetEase (or the requested --app-id): when another app owns the
  // slot, report the idle state rather than that app's track.
  let state = if state.source_bundle_id.as_deref() == Some(cmd.app_id.as_str()) {
    state
  } else {
    auv_media_macos::NowPlayingState::default()
  };
  // netease's output omits the like fields (NetEase never reports them).
  let output = crate::output::build_now_playing_output(&state);

  match &cmd.output {
    OutputMode::Human => {
      println!("{}", auv_media_macos::output::render_human_summary(&state));
      ExitCode::SUCCESS
    }
    OutputMode::Json => match serde_json::to_string_pretty(&output) {
      Ok(json) => {
        println!("{json}");
        ExitCode::SUCCESS
      }
      Err(error) => {
        eprintln!("encode failed: {error}");
        ExitCode::from(1)
      }
    },
    OutputMode::JsonFile(path) => {
      let json = match serde_json::to_string_pretty(&output) {
        Ok(json) => json,
        Err(error) => {
          eprintln!("encode failed: {error}");
          return ExitCode::from(1);
        }
      };
      if let Err(error) = std::fs::write(path, json) {
        eprintln!("failed to write {}: {error}", path.display());
        return ExitCode::from(1);
      }
      ExitCode::SUCCESS
    }
  }
}

#[cfg(not(target_os = "macos"))]
fn run_now_playing(_cmd: NowPlayingCommand) -> ExitCode {
  eprintln!("now-playing is only available on macOS");
  ExitCode::from(1)
}

/// Require that `app_id` currently owns the now-playing slot before acting on
/// it. Returns `Err(exit_code)` (with a message) when it does not, so controls
/// never act on some other app that happens to be playing.
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

#[cfg(not(target_os = "macos"))]
fn run_control(_cmd: ControlCommand) -> ExitCode {
  eprintln!("media controls are only available on macOS");
  ExitCode::from(1)
}

#[cfg(target_os = "macos")]
fn run_seek(cmd: SeekCommand) -> ExitCode {
  if let Err(code) = require_owner(&cmd.app_id) {
    return code;
  }
  // Defense-in-depth: parse_seek already rejects out-of-range seconds, but
  // a direct SeekCommand construction (tests, future callers) could still
  // reach run_seek with overflow/NaN. `try_from_secs_f64` avoids the panic
  // path inside `Duration::from_secs_f64`.
  let duration = match std::time::Duration::try_from_secs_f64(cmd.seconds) {
    Ok(duration) => duration,
    Err(_) => {
      eprintln!(
        "seek failed: seek position must be a non-negative finite number of seconds within the representable range"
      );
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

  match &cmd.output {
    OutputMode::Human => {
      println!("{}", result.to_human_readable());
      ExitCode::SUCCESS
    }
    OutputMode::Json => match serde_json::to_string_pretty(&result) {
      Ok(json) => {
        println!("{json}");
        ExitCode::SUCCESS
      }
      Err(error) => {
        eprintln!("encode failed: {error}");
        ExitCode::from(1)
      }
    },
    OutputMode::JsonFile(path) => {
      let json = match serde_json::to_string_pretty(&result) {
        Ok(json) => json,
        Err(error) => {
          eprintln!("encode failed: {error}");
          return ExitCode::from(1);
        }
      };
      if let Err(error) = std::fs::write(path, json) {
        eprintln!("failed to write {}: {error}", path.display());
        return ExitCode::from(1);
      }
      ExitCode::SUCCESS
    }
  }
}

fn run_playback_status(cmd: PlaybackStatusCommand) -> ExitCode {
  let result = match run_playback_status_probe(&cmd.inputs) {
    Ok(result) => result,
    Err(error) => {
      eprintln!("playback status probe failed: {error}");
      return ExitCode::from(1);
    }
  };

  match &cmd.output {
    OutputMode::Human => {
      println!("{}", result.to_human_readable(cmd.wide));
      ExitCode::SUCCESS
    }
    OutputMode::Json => match serde_json::to_string_pretty(&result.to_json()) {
      Ok(json) => {
        println!("{json}");
        ExitCode::SUCCESS
      }
      Err(error) => {
        eprintln!("encode failed: {error}");
        ExitCode::from(1)
      }
    },
    OutputMode::JsonFile(path) => {
      let json = match serde_json::to_string_pretty(&result.to_json()) {
        Ok(json) => json,
        Err(error) => {
          eprintln!("encode failed: {error}");
          return ExitCode::from(1);
        }
      };
      if let Err(error) = std::fs::write(path, json) {
        eprintln!("failed to write {}: {error}", path.display());
        return ExitCode::from(1);
      }
      ExitCode::SUCCESS
    }
  }
}

fn run_songs_ls(cmd: SongsLsCommand) -> ExitCode {
  let result = match cmd.target {
    SongsLsTarget::DailyRecommended => match run_daily_recommended_songs_scan(&cmd.inputs) {
      Ok(result) => result,
      Err(error) => {
        eprintln!("songs ls failed: {error}");
        return ExitCode::from(1);
      }
    },
  };

  match &cmd.output {
    OutputMode::Human => {
      println!("NetEase song list scan");
      println!("target: {}", result.target);
      println!("items: {}", result.items.len());
      println!("observations: {}", result.observations.len());
      if result.known_limits.is_empty() {
        println!("known_limits: (none)");
      } else {
        println!("known_limits:");
        for limit in &result.known_limits {
          println!("  - {limit}");
        }
      }
      ExitCode::SUCCESS
    }
    OutputMode::Json => match serde_json::to_string_pretty(&result) {
      Ok(json) => {
        println!("{json}");
        ExitCode::SUCCESS
      }
      Err(error) => {
        eprintln!("encode failed: {error}");
        ExitCode::from(1)
      }
    },
    OutputMode::JsonFile(path) => {
      let json = match serde_json::to_string_pretty(&result) {
        Ok(json) => json,
        Err(error) => {
          eprintln!("encode failed: {error}");
          return ExitCode::from(1);
        }
      };
      if let Err(error) = std::fs::write(path, json) {
        eprintln!("failed to write {}: {error}", path.display());
        return ExitCode::from(1);
      }
      ExitCode::SUCCESS
    }
  }
}
