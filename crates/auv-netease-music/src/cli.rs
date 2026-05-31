// File: crates/auv-netease-music/src/cli.rs
use std::path::PathBuf;
use std::process::ExitCode;

use crate::output::{PlaylistEnvelope, collect_matches};
use crate::{Inputs, render_human_summary, run_live_scan};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum OutputMode {
  Human,
  Json,
  JsonFile(PathBuf),
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PlaylistCommand {
  pub inputs: Inputs,
  pub keyword: Option<String>,
  pub output: OutputMode,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Command {
  Playlist(PlaylistCommand),
  Help,
}

fn next(iter: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
  iter
    .next()
    .ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_pos(value: String, flag: &str) -> Result<usize, String> {
  let parsed: usize = value
    .parse()
    .map_err(|_| format!("{flag} expects a positive integer"))?;
  if parsed == 0 {
    return Err(format!("{flag} must be greater than 0"));
  }
  Ok(parsed)
}

fn parse_amount(value: String) -> Result<f64, String> {
  let parsed: f64 = value
    .parse()
    .map_err(|_| "--scroll-amount expects a number".to_string())?;
  if !parsed.is_finite() || parsed <= 0.0 {
    return Err("--scroll-amount must be greater than 0".to_string());
  }
  Ok(parsed)
}

pub(crate) fn parse_command(args: Vec<String>) -> Result<Command, String> {
  let mut iter = args.into_iter();
  let Some(sub) = iter.next() else {
    return Ok(Command::Help);
  };
  match sub.as_str() {
    "playlist" => parse_playlist(iter.collect()),
    "help" | "-h" | "--help" => Ok(Command::Help),
    other => Err(format!("unknown command {other:?}; try `playlist`")),
  }
}

fn parse_playlist(args: Vec<String>) -> Result<Command, String> {
  let mut inputs = Inputs::with_defaults();
  let mut keyword: Option<String> = None;
  let mut json = false;
  let mut json_out: Option<PathBuf> = None;
  let mut ls_verb_consumed = false;
  let mut iter = args.into_iter();
  while let Some(arg) = iter.next() {
    match arg.as_str() {
      "--json" => json = true,
      "--json-out" => json_out = Some(PathBuf::from(next(&mut iter, "--json-out")?)),
      "--app-id" => inputs.app_id = next(&mut iter, "--app-id")?,
      "--artifact-dir" => inputs.artifact_dir = PathBuf::from(next(&mut iter, "--artifact-dir")?),
      "--max-pages" => {
        inputs.max_pages = parse_pos(next(&mut iter, "--max-pages")?, "--max-pages")?
      }
      "--max-scrolls" => {
        inputs.max_scrolls = parse_pos(next(&mut iter, "--max-scrolls")?, "--max-scrolls")?
      }
      "--scroll-amount" => {
        inputs.scroll_amount = parse_amount(next(&mut iter, "--scroll-amount")?)?
      }
      "--sidebar-region" => {
        inputs.sidebar_region = Some(crate::parse_ratio_region(next(
          &mut iter,
          "--sidebar-region",
        )?)?)
      }
      other if other.starts_with("--") => return Err(format!("unknown flag {other}")),
      // A leading `ls` is accepted as a no-op "list" verb, so `playlist`,
      // `playlist ls`, `playlist <kw>`, and `playlist ls <kw>` all work.
      "ls" if keyword.is_none() && !ls_verb_consumed => ls_verb_consumed = true,
      other => {
        if keyword.is_some() {
          return Err(format!("unexpected extra argument {other:?}"));
        }
        keyword = Some(other.to_string());
      }
    }
  }
  let output = match json_out {
    Some(path) => OutputMode::JsonFile(path),
    None if json => OutputMode::Json,
    None => OutputMode::Human,
  };
  Ok(Command::Playlist(PlaylistCommand {
    inputs,
    keyword,
    output,
  }))
}

fn print_usage() {
  eprintln!(
    "auv-netease-music — NetEase Cloud Music CLI\n\
     \n\
     USAGE:\n\
     \x20 auv-netease-music playlist [ls] [<keyword>] [--json | --json-out <path>]\n\
     \x20   [--app-id <bundle>] [--artifact-dir <path>] [--max-pages <n>]\n\
     \x20   [--max-scrolls <n>] [--scroll-amount <f>]\n\
     \x20   [--sidebar-region x,y,width,height]\n\
     \n\
     Exit: 0 ok (even with 0 matches); 1 scan/IO failure; 2 usage error."
  );
}

/// Entry point for the `auv-netease-music` binary.
pub fn run() -> ExitCode {
  match parse_command(std::env::args().skip(1).collect()) {
    Ok(Command::Help) => {
      print_usage();
      ExitCode::SUCCESS
    }
    Ok(Command::Playlist(cmd)) => run_playlist(cmd),
    Err(error) => {
      eprintln!("error: {error}");
      ExitCode::from(2)
    }
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
  let matches = collect_matches(scan.projection(), cmd.keyword.as_deref());
  let item_count = collect_matches(scan.projection(), None).len();
  let envelope = PlaylistEnvelope {
    command: "playlist",
    query: cmd.keyword.clone(),
    item_count,
    match_count: matches.len(),
    matches,
    scan: &scan,
  };

  match &cmd.output {
    OutputMode::Human => {
      println!("{}", render_human_summary(&scan));
      ExitCode::SUCCESS
    }
    OutputMode::Json => match serde_json::to_string_pretty(&envelope) {
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
      let json = match serde_json::to_string_pretty(&envelope) {
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

#[cfg(test)]
mod tests {
  use super::*;

  fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
  }

  #[test]
  fn empty_args_is_help() {
    assert_eq!(parse_command(args(&[])).unwrap(), Command::Help);
  }

  #[test]
  fn playlist_without_keyword_uses_defaults_and_human_output() {
    let Command::Playlist(cmd) = parse_command(args(&["playlist"])).unwrap() else {
      panic!("expected playlist command");
    };
    assert_eq!(cmd.keyword, None);
    assert_eq!(cmd.output, OutputMode::Human);
    assert_eq!(cmd.inputs.app_id, crate::DEFAULT_APP_ID);
    assert_eq!(cmd.inputs.max_pages, 24);
  }

  #[test]
  fn playlist_keyword_and_json_flag() {
    let Command::Playlist(cmd) = parse_command(args(&["playlist", "daily", "--json"])).unwrap()
    else {
      panic!("expected playlist command");
    };
    assert_eq!(cmd.keyword.as_deref(), Some("daily"));
    assert_eq!(cmd.output, OutputMode::Json);
  }

  #[test]
  fn json_out_takes_precedence_over_json_flag() {
    let Command::Playlist(cmd) =
      parse_command(args(&["playlist", "--json", "--json-out", "/tmp/x.json"])).unwrap()
    else {
      panic!("expected playlist command");
    };
    assert_eq!(
      cmd.output,
      OutputMode::JsonFile(PathBuf::from("/tmp/x.json"))
    );
  }

  #[test]
  fn unknown_command_errors() {
    assert!(parse_command(args(&["bogus"])).is_err());
  }

  #[test]
  fn two_positionals_error() {
    assert!(parse_command(args(&["playlist", "a", "b"])).is_err());
  }

  #[test]
  fn ls_verb_lists_without_being_a_keyword() {
    let Command::Playlist(cmd) = parse_command(args(&["playlist", "ls"])).unwrap() else {
      panic!("expected playlist command");
    };
    assert_eq!(cmd.keyword, None);
  }

  #[test]
  fn ls_verb_then_keyword() {
    let Command::Playlist(cmd) = parse_command(args(&["playlist", "ls", "daily"])).unwrap() else {
      panic!("expected playlist command");
    };
    assert_eq!(cmd.keyword.as_deref(), Some("daily"));
  }

  #[test]
  fn artifact_dir_override() {
    let Command::Playlist(cmd) =
      parse_command(args(&["playlist", "--artifact-dir", "/tmp/foo"])).unwrap()
    else {
      panic!("expected playlist command");
    };
    assert_eq!(cmd.inputs.artifact_dir, PathBuf::from("/tmp/foo"));
  }
}
