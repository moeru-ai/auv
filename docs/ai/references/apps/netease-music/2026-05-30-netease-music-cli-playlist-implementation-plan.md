# NetEase Music CLI — `playlist` slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a compiled product CLI (`auv-netease-music`, alias `auv-wyy`) whose `playlist [<keyword>]` subcommand runs the existing NetEase sidebar scan and emits agent-callable JSON, by extracting the example's procedures into a reusable crate library — no behavior change to the scan.

**Architecture:** New workspace crate `crates/auv-netease-music/`. The whole of `examples/netease_playlist_ls.rs` moves into the crate library (visibility widened where the CLI needs it); the example becomes a thin wrapper. A `cli` module adds subcommand parsing, a keyword filter over the scan projection, a stable JSON output object, and exit codes. Two `[[bin]]` targets delegate to one `cli::run()`. The crate depends *down* the tree (`auv-driver`, `auv-driver-macos`) and **never** on the root `auv-cli` crate.

**Tech Stack:** Rust (edition 2024, rust-version 1.91), `serde`/`serde_json`, `image`, `auv-driver`, `auv-driver-macos`. Arg parsing is hand-rolled (matches the repo's existing style; no new `clap` dependency). macOS-gated live scan, same as the example.

**Scope:** `playlist` only. `song play` is a separate plan (it is welded to `auv-cli`'s run-recording machinery and needs a layering decision first). `playlist play` is deferred (net-new automation). See `docs/ai/references/apps/netease-music/2026-05-29-netease-music-cli-design.md`.

---

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` (root) | add `crates/auv-netease-music` to `[workspace].members` |
| `crates/auv-netease-music/Cargo.toml` | crate manifest, two `[[bin]]` targets, deps |
| `crates/auv-netease-music/src/lib.rs` | the moved scan procedures + IR types (from the example), visibility widened |
| `crates/auv-netease-music/src/output.rs` | `MatchRef`, `PlaylistJsonOutput`, `collect_matches` (pure, tested) |
| `crates/auv-netease-music/src/cli.rs` | argv → `Command`, `run()`/`run_playlist()`, usage, exit codes (parsing tested) |
| `crates/auv-netease-music/src/bin/auv-netease-music.rs` | thin `main` → `cli::run()` |
| `crates/auv-netease-music/src/bin/auv-wyy.rs` | thin `main` → `cli::run()` (alias binary) |
| `crates/auv-netease-music/examples/netease_playlist_ls.rs` | thin example over the lib (relocated from root `examples/`) |

---

## Task 1: Scaffold the crate

**Files:**
- Create: `crates/auv-netease-music/Cargo.toml`
- Create: `crates/auv-netease-music/src/lib.rs`
- Modify: `Cargo.toml:1-7` (workspace members)

- [ ] **Step 1: Create the crate manifest**

Create `crates/auv-netease-music/Cargo.toml`:

```toml
[package]
name = "auv-netease-music"
version.workspace = true
edition.workspace = true
publish.workspace = true
readme.workspace = true
license.workspace = true
authors.workspace = true
description.workspace = true
documentation.workspace = true
homepage.workspace = true
repository.workspace = true

[[bin]]
name = "auv-netease-music"
path = "src/bin/auv-netease-music.rs"

[[bin]]
name = "auv-wyy"
path = "src/bin/auv-wyy.rs"

[dependencies]
auv-driver = { path = "../auv-driver" }
serde.workspace = true
serde_json.workspace = true
image.workspace = true

[target.'cfg(target_os = "macos")'.dependencies]
auv-driver-macos = { path = "../auv-driver-macos" }
```

- [ ] **Step 2: Create a stub lib so the crate compiles**

Create `crates/auv-netease-music/src/lib.rs`:

```rust
//! NetEase Music product CLI library: sidebar playlist scan + agent-callable output.
```

- [ ] **Step 3: Register the crate in the workspace**

Edit `Cargo.toml` (root) `members` list:

```toml
[workspace]
members = [
  ".",
  "crates/auv-driver",
  "crates/auv-driver-macos",
  "crates/auv-netease-music",
  "crates/auv-overlay-macos",
]
```

- [ ] **Step 4: Verify the empty crate checks**

Run: `cargo check -p auv-netease-music`
Expected: PASS (no errors; crate has no code yet).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/auv-netease-music/Cargo.toml crates/auv-netease-music/src/lib.rs
git commit -m "feat(netease-music): scaffold product CLI crate

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: Move the scan procedures into the lib (behavior-preserving)

This is a faithful move of the existing example into the crate library. Do **not** rewrite logic; only relocate it, remove the binary glue, and widen visibility for the items the CLI consumes.

**Files:**
- Modify: `crates/auv-netease-music/src/lib.rs` (becomes the moved code)
- Source: `examples/netease_playlist_ls.rs` (entire file)

- [ ] **Step 1: Copy the example body into the lib**

Copy the full contents of `examples/netease_playlist_ls.rs` into `crates/auv-netease-music/src/lib.rs`, replacing the stub doc-comment, **except** `fn main()` (lines 370-375) and `fn run()` (lines 377-394). Those are binary glue and are reimplemented in `cli.rs`. Keep everything else verbatim, including the `#[cfg(test)] mod tests` module — those tests now run under this crate.

- [ ] **Step 2: Widen visibility for the items the CLI needs**

Apply these exact visibility changes to the moved code (everything else stays private):

```rust
// constants
pub const DEFAULT_APP_ID: &str = "com.netease.163music";
pub const DEFAULT_ARTIFACT_DIR: &str = "/tmp/auv-netease-playlist-ls-artifacts";

// CLI input type — all fields pub so cli.rs can build it
pub struct Inputs {
  pub app_id: String,
  pub json_out: Option<std::path::PathBuf>,
  pub artifact_dir: std::path::PathBuf,
  pub max_pages: usize,
  pub max_scrolls: usize,
  pub scroll_amount: f64,
  pub sidebar_region: Option<RatioRegion>,
  pub print_json: bool,
}

pub struct RatioRegion { /* keep existing fields, make them pub */ }

// projection types consumed by the keyword filter — structs + fields pub
pub struct PlaylistSidebarProjection { pub sections: Vec<SidebarSection> }
pub struct SidebarSection {
  pub id: String,
  pub kind: SidebarSectionKind,
  pub label: Option<String>,
  pub items: Vec<PlaylistSidebarItem>,
}
pub enum SidebarSectionKind { /* keep variants */ }
pub struct PlaylistSidebarItem {
  pub id: String,
  pub label: String,
  pub section_hint: Option<SidebarSectionKind>,
  pub confidence: Confidence,
  pub candidate_id: Option<String>,
  pub anchor_id: Option<String>,
}
pub enum Confidence { /* keep variants */ }

// the top scan artifact — only `projection` needs cross-module read access
pub struct PlaylistSidebarScan {
  /* keep all other fields private */
  pub(crate) projection: PlaylistSidebarProjection,
  /* ... */
}

// functions
pub fn run_live_scan(/* keep signature: &Inputs -> Result<PlaylistSidebarScan, String> */) { /* ... */ }
pub fn render_human_summary(scan: &PlaylistSidebarScan) -> String { /* ... */ }
pub fn normalize_identity(value: &str) -> String { /* ... */ }
pub(crate) fn parse_ratio_region(value: String) -> Result<RatioRegion, String> { /* ... */ }
```

Leave `RatioRegion`'s derive list and constructor as-is; only the field visibility changes. `PlaylistSidebarScan` keeps `#[derive(... Default ...)]`.

- [ ] **Step 3: Add a defaults constructor for `Inputs` and use it in `parse_inputs`**

Add this impl next to `Inputs`, and change `parse_inputs` (moved from lines 396-406) to start from it (DRY — the CLI reuses the same defaults):

```rust
impl Inputs {
  pub fn with_defaults() -> Self {
    Self {
      app_id: DEFAULT_APP_ID.to_string(),
      json_out: None,
      artifact_dir: std::path::PathBuf::from(DEFAULT_ARTIFACT_DIR),
      max_pages: 24,
      max_scrolls: 48,
      scroll_amount: 6.0,
      sidebar_region: None,
      print_json: false,
    }
  }
}
```

In `parse_inputs`, replace the inline `let mut inputs = Inputs { ... };` literal with:

```rust
  let mut inputs = Inputs::with_defaults();
```

- [ ] **Step 4: Verify the lib compiles and the moved tests pass**

Run: `cargo test -p auv-netease-music`
Expected: PASS — every `parse_inputs_*`, `parse_viewport_*`, `reconstruct_*`, `detect_*`, `scan_loop_*`, and `render_*` test that moved with the file passes unchanged. No test count regression vs the example.

- [ ] **Step 5: Commit**

```bash
git add crates/auv-netease-music/src/lib.rs
git commit -m "feat(netease-music): move playlist scan procedures into crate lib

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: Reduce the example to a thin wrapper over the lib

**Files:**
- Create: `crates/auv-netease-music/examples/netease_playlist_ls.rs`
- Delete: `examples/netease_playlist_ls.rs`

- [ ] **Step 1: Create the thin example in the crate**

Create `crates/auv-netease-music/examples/netease_playlist_ls.rs` (reproduces the original `main`/`run` behavior using the now-public lib API):

```rust
//! Thin demo over `auv_netease_music`. Prefer the `auv-wyy` / `auv-netease-music`
//! binaries for product use; this example exists for quick local runs.
use auv_netease_music::{Inputs, parse_inputs_public, render_human_summary, run_live_scan};

fn main() {
  if let Err(error) = run() {
    eprintln!("{error}");
    std::process::exit(1);
  }
}

fn run() -> Result<(), String> {
  let inputs: Inputs = parse_inputs_public(std::env::args().skip(1).collect())?;
  let scan = run_live_scan(&inputs)?;
  let json = serde_json::to_string_pretty(&scan).map_err(|error| error.to_string())?;
  if let Some(path) = &inputs.json_out {
    std::fs::write(path, &json)
      .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
  }
  if inputs.print_json {
    println!("{json}");
  } else {
    println!("{}", render_human_summary(&scan));
  }
  Ok(())
}
```

- [ ] **Step 2: Expose `parse_inputs` under a stable name for the example**

In `crates/auv-netease-music/src/lib.rs`, make the moved `parse_inputs` public via a thin re-export name (the example and any test use it; the bins use the richer `cli` parser instead):

```rust
/// Parse the legacy flat flag list (no subcommand). Used by the demo example.
pub fn parse_inputs_public(args: Vec<String>) -> Result<Inputs, String> {
  parse_inputs(args)
}
```

(Keep the original `parse_inputs` private; this wrapper is the public surface.)

- [ ] **Step 3: Delete the old root example**

Run: `git rm examples/netease_playlist_ls.rs`

- [ ] **Step 4: Verify the example builds**

Run: `cargo build -p auv-netease-music --example netease_playlist_ls`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/auv-netease-music/src/lib.rs crates/auv-netease-music/examples/netease_playlist_ls.rs
git commit -m "refactor(netease-music): relocate playlist example as thin lib wrapper

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: Keyword filter + JSON output (`output.rs`)

`collect_matches` is pure and testable without a driver — TDD it.

**Files:**
- Create: `crates/auv-netease-music/src/output.rs`
- Modify: `crates/auv-netease-music/src/lib.rs` (add `pub mod output;`)

- [ ] **Step 1: Declare the module**

Add to the top of `crates/auv-netease-music/src/lib.rs`:

```rust
pub mod output;
```

- [ ] **Step 2: Write the failing test**

Create `crates/auv-netease-music/src/output.rs` with the types, a stub `collect_matches`, and tests:

```rust
// File: crates/auv-netease-music/src/output.rs
use serde::Serialize;

use crate::{Confidence, PlaylistSidebarProjection, PlaylistSidebarScan, SidebarSectionKind};

/// One playlist item surfaced by the listing or keyword filter.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MatchRef {
  pub section_id: String,
  pub section_kind: SidebarSectionKind,
  pub item_id: String,
  pub label: String,
  pub anchor_id: Option<String>,
}

/// Agent-facing JSON output for the `playlist` command. Embeds the raw
/// scan artifact (which carries `schema_version` and `ScrollBoundarySummary`)
/// so an agent can distinguish "not found" from "scan not exhaustive".
#[derive(Clone, Debug, Serialize)]
pub struct PlaylistJsonOutput<'a> {
  pub command: &'static str,
  pub query: Option<String>,
  pub item_count: usize,
  pub match_count: usize,
  pub matches: Vec<MatchRef>,
  pub scan: &'a PlaylistSidebarScan,
}

/// Collect items whose normalized label contains the normalized keyword.
/// `keyword == None` returns every item (full listing).
pub fn collect_matches(
  projection: &PlaylistSidebarProjection,
  keyword: Option<&str>,
) -> Vec<MatchRef> {
  unimplemented!()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{PlaylistSidebarItem, SidebarSection};

  fn projection() -> PlaylistSidebarProjection {
    PlaylistSidebarProjection {
      sections: vec![SidebarSection {
        id: "sec-1".to_string(),
        kind: SidebarSectionKind::MyPlaylists,
        label: Some("我的歌单".to_string()),
        items: vec![
          PlaylistSidebarItem {
            id: "i1".to_string(),
            label: "Daily Mix".to_string(),
            section_hint: None,
            confidence: Confidence::High,
            candidate_id: None,
            anchor_id: Some("a1".to_string()),
          },
          PlaylistSidebarItem {
            id: "i2".to_string(),
            label: "Workout".to_string(),
            section_hint: None,
            confidence: Confidence::Low,
            candidate_id: None,
            anchor_id: None,
          },
        ],
      }],
    }
  }

  #[test]
  fn no_keyword_returns_all_items() {
    let matches = collect_matches(&projection(), None);
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].label, "Daily Mix");
    assert_eq!(matches[0].anchor_id.as_deref(), Some("a1"));
  }

  #[test]
  fn keyword_filters_case_and_whitespace_insensitively() {
    let matches = collect_matches(&projection(), Some("daily"));
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].item_id, "i1");
    assert_eq!(matches[0].section_kind, SidebarSectionKind::MyPlaylists);
  }

  #[test]
  fn keyword_without_match_returns_empty() {
    let matches = collect_matches(&projection(), Some("zzz"));
    assert!(matches.is_empty());
  }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `cargo test -p auv-netease-music output::tests`
Expected: FAIL — `collect_matches` panics with `not implemented`.

- [ ] **Step 4: Implement `collect_matches`**

Replace the `unimplemented!()` body:

```rust
pub fn collect_matches(
  projection: &PlaylistSidebarProjection,
  keyword: Option<&str>,
) -> Vec<MatchRef> {
  let needle = keyword.map(crate::normalize_identity);
  let mut out = Vec::new();
  for section in &projection.sections {
    for item in &section.items {
      if let Some(needle) = &needle {
        if !crate::normalize_identity(&item.label).contains(needle.as_str()) {
          continue;
        }
      }
      out.push(MatchRef {
        section_id: section.id.clone(),
        section_kind: section.kind,
        item_id: item.id.clone(),
        label: item.label.clone(),
        anchor_id: item.anchor_id.clone(),
      });
    }
  }
  out
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `cargo test -p auv-netease-music output::tests`
Expected: PASS (3 tests).

- [ ] **Step 6: Commit**

```bash
git add crates/auv-netease-music/src/lib.rs crates/auv-netease-music/src/output.rs
git commit -m "feat(netease-music): add keyword filter and JSON output

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: Subcommand parsing + dispatch (`cli.rs`)

`parse_command` is pure — TDD it. `run`/`run_playlist` wire it to the scan and output.

**Files:**
- Create: `crates/auv-netease-music/src/cli.rs`
- Modify: `crates/auv-netease-music/src/lib.rs` (add `pub mod cli;`)

- [ ] **Step 1: Declare the module**

Add to `crates/auv-netease-music/src/lib.rs`:

```rust
pub mod cli;
```

- [ ] **Step 2: Write the failing parser tests**

Create `crates/auv-netease-music/src/cli.rs`:

```rust
// File: crates/auv-netease-music/src/cli.rs
use std::path::PathBuf;
use std::process::ExitCode;

use crate::output::{PlaylistJsonOutput, collect_matches};
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
  iter.next().ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_pos(value: String, flag: &str) -> Result<usize, String> {
  let parsed: usize = value.parse().map_err(|_| format!("{flag} expects a positive integer"))?;
  if parsed == 0 {
    return Err(format!("{flag} must be greater than 0"));
  }
  Ok(parsed)
}

fn parse_amount(value: String) -> Result<f64, String> {
  let parsed: f64 = value.parse().map_err(|_| "--scroll-amount expects a number".to_string())?;
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
  let mut iter = args.into_iter();
  while let Some(arg) = iter.next() {
    match arg.as_str() {
      "--json" => json = true,
      "--json-out" => json_out = Some(PathBuf::from(next(&mut iter, "--json-out")?)),
      "--app-id" => inputs.app_id = next(&mut iter, "--app-id")?,
      "--max-pages" => inputs.max_pages = parse_pos(next(&mut iter, "--max-pages")?, "--max-pages")?,
      "--max-scrolls" => inputs.max_scrolls = parse_pos(next(&mut iter, "--max-scrolls")?, "--max-scrolls")?,
      "--scroll-amount" => inputs.scroll_amount = parse_amount(next(&mut iter, "--scroll-amount")?)?,
      "--sidebar-region" => inputs.sidebar_region = Some(crate::parse_ratio_region(next(&mut iter, "--sidebar-region")?)?),
      other if other.starts_with("--") => return Err(format!("unknown flag {other}")),
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
  Ok(Command::Playlist(PlaylistCommand { inputs, keyword, output }))
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
    let Command::Playlist(cmd) =
      parse_command(args(&["playlist", "daily", "--json"])).unwrap()
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
    assert_eq!(cmd.output, OutputMode::JsonFile(PathBuf::from("/tmp/x.json")));
  }

  #[test]
  fn unknown_command_errors() {
    assert!(parse_command(args(&["bogus"])).is_err());
  }

  #[test]
  fn two_positionals_error() {
    assert!(parse_command(args(&["playlist", "a", "b"])).is_err());
  }
}
```

- [ ] **Step 3: Run the parser tests to verify they fail to compile/pass**

Run: `cargo test -p auv-netease-music cli::tests`
Expected: FAIL — `run`/`run_playlist` not defined yet causes unused warnings only if referenced; the tests themselves should compile and PASS once parsing is complete. If they FAIL, fix `parse_*` until green. (At this point `run` is not implemented; that is Step 4.)

- [ ] **Step 4: Add `run` / `run_playlist` and usage**

Append to `crates/auv-netease-music/src/cli.rs`:

```rust
fn print_usage() {
  eprintln!(
    "auv-netease-music — NetEase Cloud Music CLI\n\
     \n\
     USAGE:\n\
     \x20 auv-wyy playlist [<keyword>] [--json | --json-out <path>]\n\
     \x20                  [--app-id <bundle>] [--max-pages <n>]\n\
     \x20                  [--max-scrolls <n>] [--scroll-amount <f>]\n\
     \x20                  [--sidebar-region x,y,width,height]\n\
     \n\
     Exit: 0 ok (even with 0 matches); 1 scan/IO failure; 2 usage error."
  );
}

/// Entry point shared by both binaries.
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
  let matches = collect_matches(&scan.projection, cmd.keyword.as_deref());
  let item_count = collect_matches(&scan.projection, None).len();
  let output = PlaylistJsonOutput {
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
```

- [ ] **Step 5: Run all crate tests**

Run: `cargo test -p auv-netease-music`
Expected: PASS — parser tests (6) + filter tests (3) + all moved scan tests.

- [ ] **Step 6: Commit**

```bash
git add crates/auv-netease-music/src/lib.rs crates/auv-netease-music/src/cli.rs
git commit -m "feat(netease-music): add playlist subcommand parsing and dispatch

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: Wire the two binaries

**Files:**
- Create: `crates/auv-netease-music/src/bin/auv-netease-music.rs`
- Create: `crates/auv-netease-music/src/bin/auv-wyy.rs`

- [ ] **Step 1: Create the primary binary**

Create `crates/auv-netease-music/src/bin/auv-netease-music.rs`:

```rust
use std::process::ExitCode;

fn main() -> ExitCode {
  auv_netease_music::cli::run()
}
```

- [ ] **Step 2: Create the alias binary**

Create `crates/auv-netease-music/src/bin/auv-wyy.rs`:

```rust
use std::process::ExitCode;

fn main() -> ExitCode {
  auv_netease_music::cli::run()
}
```

- [ ] **Step 3: Build both binaries**

Run: `cargo build -p auv-netease-music --bins`
Expected: PASS — produces `auv-netease-music` and `auv-wyy` under `target/debug/`.

- [ ] **Step 4: Smoke the usage path (no device needed)**

Run: `cargo run -p auv-netease-music --bin auv-wyy -- --help`
Expected: prints the usage block to stderr; exit code 0.

Run: `cargo run -p auv-netease-music --bin auv-wyy -- bogus; echo "exit=$?"`
Expected: `error: unknown command "bogus"; try \`playlist\`` and `exit=2`.

- [ ] **Step 5: Commit**

```bash
git add crates/auv-netease-music/src/bin/auv-netease-music.rs crates/auv-netease-music/src/bin/auv-wyy.rs
git commit -m "feat(netease-music): add auv-netease-music and auv-wyy binaries

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: Workspace validation

**Files:** none (verification only)

- [ ] **Step 1: Format check**

Run: `cargo fmt --check`
Expected: PASS (no diff). If it fails, run `cargo fmt` and re-commit.

- [ ] **Step 2: Workspace check**

Run: `cargo check --workspace`
Expected: PASS — new crate compiles; root `auv-cli` unaffected (the old example is gone, nothing references it).

- [ ] **Step 3: Workspace tests**

Run: `cargo test --workspace`
Expected: PASS — no regressions; new crate's parser/filter/scan tests included.

- [ ] **Step 4: Whitespace lint**

Run: `git diff --check`
Expected: no output.

- [ ] **Step 5: Confirm `auv-cli` has no dependency on the new crate and vice-versa**

Run: `cargo tree -p auv-netease-music -i auv-cli`
Expected: error / empty — `auv-netease-music` does **not** depend on `auv-cli`.

---

## Self-Review

- **Spec coverage:** crate placement (Task 1) ✓; extract procedures, examples thin (Tasks 2-3) ✓; `playlist [<keyword>]` with full-scan-always + matches (Tasks 4-5) ✓; JSON output embedding the `schema_version`-tagged scan + exit codes (Tasks 4-6) ✓; two real bins (Task 6) ✓; no `auv-cli`/`music.rs` dependency (Task 7 Step 5) ✓; `playlist play` / `song` excluded ✓.
- **Placeholders:** none — every code step is complete; extraction steps name exact items and line ranges.
- **Type consistency:** `collect_matches(&PlaylistSidebarProjection, Option<&str>)` used identically in `output.rs` and `cli.rs`; `Inputs::with_defaults()` defined in Task 2, used in Tasks 2/5; `PlaylistJsonOutput` fields match between definition (Task 4) and construction (Task 5); `cli::run() -> ExitCode` matches both bins (Task 6).

---

This plan is part of the convergence phase. It implements the `playlist` slice of `2026-05-29-netease-music-cli-design.md`. `song play` and `playlist play` are deferred to separate, owner-approved plans.
