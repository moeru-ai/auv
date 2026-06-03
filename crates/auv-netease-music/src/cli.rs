// File: crates/auv-netease-music/src/cli.rs
use std::num::{NonZeroU64, NonZeroUsize};
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

use crate::output::build_playlist_json_output;
use crate::{
  DailyRecommendedPlayInputs, Inputs, PlaylistCategory, run_daily_recommended_play, run_live_scan,
};

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
pub(crate) enum Command {
  PlaylistLs(PlaylistCommand),
  PlaylistPlayDailyRecommended(DailyRecommendedPlayCommand),
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
  #[arg(long = "top-scroll-amount", value_parser = crate::positive_scroll_amount)]
  top_scroll_amount: Option<f64>,
  #[arg(long = "settle-ms")]
  settle_ms: Option<NonZeroU64>,
  #[arg(long = "play-icon-template")]
  play_icon_template: Option<PathBuf>,
  #[arg(long = "play-icon-threshold", value_parser = crate::zero_to_one)]
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
  #[arg(long = "scroll-amount", value_parser = crate::positive_scroll_amount)]
  scroll_amount: Option<f64>,
  #[arg(long = "scroll-settle-ms")]
  scroll_settle_ms: Option<NonZeroU64>,
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
  }
}

fn parse_playlist(args: PlaylistArgs) -> Result<Command, String> {
  match args.command {
    Some(PlaylistSubcommand::Ls(ls)) => parse_playlist_ls(ls).map(Command::PlaylistLs),
    Some(PlaylistSubcommand::Play(play)) => parse_playlist_play(play),
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
    inputs.scroll_settle_ms = scroll_settle_ms.get();
  }
  if let Some(category) = args.category {
    inputs.category = category;
  }
  if let Some(sidebar_region) = args.sidebar_region {
    inputs.sidebar_region = Some(crate::parse_ratio_region(sidebar_region)?);
  }
  for word in args.custom_words {
    crate::push_trimmed(&mut inputs.ocr_options.custom_words, word);
  }
  for csv in args.custom_word_csvs {
    crate::push_csv(&mut inputs.ocr_options.custom_words, &csv);
  }
  for path in args.custom_word_files {
    crate::load_custom_words_file(&mut inputs.ocr_options.custom_words, path)?;
  }
  for language in args.ocr_languages {
    crate::push_ocr_language(&mut inputs.ocr_options, language);
  }
  for csv in args.ocr_language_csvs {
    for language in crate::split_csv(&csv) {
      crate::push_ocr_language(&mut inputs.ocr_options, language);
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
    crate::push_trimmed(&mut inputs.ocr_options.custom_words, word);
  }
  for csv in args.custom_word_csvs {
    crate::push_csv(&mut inputs.ocr_options.custom_words, &csv);
  }
  for path in args.custom_word_files {
    crate::load_custom_words_file(&mut inputs.ocr_options.custom_words, path)?;
  }
  for language in args.ocr_languages {
    crate::push_ocr_language(&mut inputs.ocr_options, language);
  }
  for csv in args.ocr_language_csvs {
    for language in crate::split_csv(&csv) {
      crate::push_ocr_language(&mut inputs.ocr_options, language);
    }
  }

  let output = match args.json_out {
    Some(path) => OutputMode::JsonFile(path),
    None if args.json => OutputMode::Json,
    None => OutputMode::Human,
  };
  Ok(DailyRecommendedPlayCommand { inputs, output })
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
      Some(crate::parse_ratio_region("0.1,0.2,0.3,0.4".to_string()).expect("region should parse"))
    );
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
      println!("{}", scan.human_summary());
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
      println!("{}", result.human_summary());
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
