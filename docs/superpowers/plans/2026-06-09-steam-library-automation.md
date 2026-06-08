# Steam Library Automation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build `auv-steam library ls`, a product CLI that lists locally installed Steam apps from strongly grounded local Steam manifests.

**Architecture:** Add a new `crates/auv-steam` product crate. The crate owns AUV domain/query/output types, delegates Steam directory and appmanifest discovery to `steamlocate`, and keeps unsupported owned/web/ui/all library sources explicit. CLI parsing and output rendering call the product API rather than reading Steam files directly.

**Tech Stack:** Rust 2024, `clap`, `serde`, `serde_json`, `thiserror`, `steamlocate`.

---

## Baseline

This plan was written in the isolated worktree:

```text
/Users/neko/Git/github.com/moeru-ai/auv/.worktrees/steam-library-automation
```

Baseline verification before writing the plan:

```bash
cargo build
cargo test
```

Expected current baseline: both pass.

## File Structure

Create these files:

| File | Responsibility |
| --- | --- |
| `crates/auv-steam/Cargo.toml` | Product crate manifest and `auv-steam` binary target. |
| `crates/auv-steam/src/lib.rs` | Module declarations and public re-exports only. |
| `crates/auv-steam/src/app.rs` | Product facade `Steam` and app-level query method. |
| `crates/auv-steam/src/library.rs` | Domain records, query semantics, diagnostics, and `steamlocate` adapter. |
| `crates/auv-steam/src/output.rs` | Stable JSON output shape and human summary rendering. |
| `crates/auv-steam/src/cli.rs` | Clap argv parsing, output mode selection, exit codes. |
| `crates/auv-steam/src/bin/auv-steam.rs` | Thin binary entry point. |
| `crates/auv-steam/tests/fixtures.rs` | Fixture-backed integration tests for the query layer. |
| `crates/auv-steam/tests/cli.rs` | CLI parse/output mode tests. |

Modify:

| File | Change |
| --- | --- |
| `Cargo.toml` | Add `crates/auv-steam` to workspace members. |

Do not modify NetEase, Balatro, media, driver, or root runtime modules in this slice.

## Task 1: Scaffold `auv-steam`

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/auv-steam/Cargo.toml`
- Create: `crates/auv-steam/src/lib.rs`
- Create: `crates/auv-steam/src/bin/auv-steam.rs`
- Create: `crates/auv-steam/src/app.rs`
- Create: `crates/auv-steam/src/library.rs`
- Create: `crates/auv-steam/src/output.rs`
- Create: `crates/auv-steam/src/cli.rs`

- [ ] **Step 1: Add the workspace member**

Edit root `Cargo.toml` and add `crates/auv-steam` to `[workspace].members` near the other product crates:

```toml
[workspace]
members = [
  ".",
  "crates/auv-driver",
  "crates/auv-driver-macos",
  "crates/auv-game-balatro",
  "crates/auv-inference-common",
  "crates/auv-inference-ultralytics",
  "crates/auv-media-macos",
  "crates/auv-netease-music",
  "crates/auv-overlay-macos",
  "crates/auv-steam",
  "crates/auv-view",
]
```

- [ ] **Step 2: Create the crate manifest**

Create `crates/auv-steam/Cargo.toml`:

```toml
[package]
name = "auv-steam"
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
name = "auv-steam"
path = "src/bin/auv-steam.rs"

[dependencies]
clap = { version = "4.5", features = ["derive"] }
serde.workspace = true
serde_json.workspace = true
steamlocate = "2.1.0"
thiserror.workspace = true
```

- [ ] **Step 3: Create module shell**

Create `crates/auv-steam/src/lib.rs`:

```rust
//! Steam product CLI library: local installed-library queries.

pub mod app;
pub mod cli;
pub mod library;
pub mod output;

pub use app::Steam;
pub use library::{
  Grounding, LibraryDiagnostic, LibraryDiagnosticSeverity, LibraryQuery, LibraryQueryResult,
  LibrarySource, LibraryStatus, ResolvedLibraryScope, SteamInstalledApp,
};
```

- [ ] **Step 4: Create thin binary**

Create `crates/auv-steam/src/bin/auv-steam.rs`:

```rust
use std::process::ExitCode;

fn main() -> ExitCode {
  auv_steam::cli::run()
}
```

- [ ] **Step 5: Create temporary compile stubs**

Create `crates/auv-steam/src/library.rs`:

```rust
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryStatus {
  #[default]
  Installed,
  Owned,
  All,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySource {
  #[default]
  Auto,
  Local,
  Web,
  Ui,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Grounding {
  Strong,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LibraryDiagnosticSeverity {
  Warning,
  Error,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct LibraryQuery {
  pub name: Option<String>,
  pub status: LibraryStatus,
  pub source: LibrarySource,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolvedLibraryScope {
  pub status: LibraryStatus,
  pub source: String,
  pub grounding: Grounding,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct SteamInstalledApp {
  pub appid: u32,
  pub name: String,
  pub install_dir: String,
  pub library_path: String,
  pub manifest_path: String,
  pub install_state: String,
  pub source: String,
  pub grounding: Grounding,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LibraryDiagnostic {
  pub severity: LibraryDiagnosticSeverity,
  pub code: String,
  pub message: String,
  pub path: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LibraryQueryResult {
  pub query: LibraryQuery,
  pub resolved_scope: ResolvedLibraryScope,
  pub apps: Vec<SteamInstalledApp>,
  pub diagnostics: Vec<LibraryDiagnostic>,
}

#[derive(Debug, Error)]
pub enum SteamError {
  #[error("Steam could not be located")]
  NotFound,
}
```

Create `crates/auv-steam/src/app.rs`:

```rust
use crate::library::{LibraryQuery, LibraryQueryResult, SteamError};

pub struct Steam;

impl Steam {
  pub fn locate() -> Result<Self, SteamError> {
    Ok(Self)
  }

  pub fn library_apps(&self, _query: LibraryQuery) -> Result<LibraryQueryResult, SteamError> {
    Err(SteamError::NotFound)
  }
}
```

Create `crates/auv-steam/src/output.rs`:

```rust
use serde::Serialize;

use crate::library::{LibraryQuery, LibraryQueryResult, ResolvedLibraryScope, SteamInstalledApp};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LibraryLsJsonOutput<'a> {
  pub command: &'static str,
  pub query: &'a LibraryQuery,
  pub resolved_scope: &'a ResolvedLibraryScope,
  pub apps: &'a [SteamInstalledApp],
  pub diagnostics: &'a [crate::library::LibraryDiagnostic],
}

pub fn build_library_ls_json_output(result: &LibraryQueryResult) -> LibraryLsJsonOutput<'_> {
  LibraryLsJsonOutput {
    command: "library.ls",
    query: &result.query,
    resolved_scope: &result.resolved_scope,
    apps: &result.apps,
    diagnostics: &result.diagnostics,
  }
}

pub fn render_library_summary(result: &LibraryQueryResult) -> String {
  format!("Steam installed library apps: {}", result.apps.len())
}
```

Create `crates/auv-steam/src/cli.rs`:

```rust
use std::process::ExitCode;

pub fn run() -> ExitCode {
  ExitCode::SUCCESS
}
```

- [ ] **Step 6: Verify scaffold builds**

Run:

```bash
cargo check -p auv-steam
```

Expected: PASS.

- [ ] **Step 7: Commit scaffold**

```bash
git add Cargo.toml crates/auv-steam
git commit -m "feat(auv-steam): scaffold product crate"
```

## Task 2: Define Library Query Semantics

**Files:**
- Modify: `crates/auv-steam/src/library.rs`
- Test: unit tests in `crates/auv-steam/src/library.rs`

- [ ] **Step 1: Write tests for supported and unsupported scope resolution**

Append to `crates/auv-steam/src/library.rs`:

```rust
#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn installed_auto_resolves_to_local_appmanifest() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Auto,
    };

    let scope = resolve_scope(&query).expect("installed auto should resolve");

    assert_eq!(
      scope,
      ResolvedLibraryScope {
        status: LibraryStatus::Installed,
        source: "local_appmanifest".to_string(),
        grounding: Grounding::Strong,
      }
    );
  }

  #[test]
  fn installed_local_resolves_to_local_appmanifest() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    };

    let scope = resolve_scope(&query).expect("installed local should resolve");

    assert_eq!(scope.source, "local_appmanifest");
    assert_eq!(scope.grounding, Grounding::Strong);
  }

  #[test]
  fn owned_status_is_explicitly_unsupported_in_v0() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Owned,
      source: LibrarySource::Auto,
    };

    let diagnostic = resolve_scope(&query).expect_err("owned should be deferred");

    assert_eq!(diagnostic.code, "unsupported_library_status");
    assert_eq!(diagnostic.severity, LibraryDiagnosticSeverity::Error);
    assert!(diagnostic.message.contains("owned"));
  }

  #[test]
  fn all_status_is_explicitly_unsupported_in_v0() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::All,
      source: LibrarySource::Auto,
    };

    let diagnostic = resolve_scope(&query).expect_err("all should be deferred");

    assert_eq!(diagnostic.code, "unsupported_library_scope");
    assert!(diagnostic.message.contains("all"));
  }

  #[test]
  fn web_and_ui_sources_are_explicitly_unsupported_in_v0() {
    for source in [LibrarySource::Web, LibrarySource::Ui] {
      let query = LibraryQuery {
        name: None,
        status: LibraryStatus::Installed,
        source,
      };

      let diagnostic = resolve_scope(&query).expect_err("web/ui should be deferred");

      assert_eq!(diagnostic.code, "unsupported_library_source");
      assert!(diagnostic.message.contains(source.as_str()));
    }
  }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p auv-steam library::tests::installed_auto_resolves_to_local_appmanifest -- --nocapture
```

Expected: FAIL because `resolve_scope` and `LibrarySource::as_str` are not implemented.

- [ ] **Step 3: Implement scope resolution and deferral markers**

Add this implementation to `crates/auv-steam/src/library.rs` before the test module:

```rust
impl LibrarySource {
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Auto => "auto",
      Self::Local => "local",
      Self::Web => "web",
      Self::Ui => "ui",
    }
  }
}

impl LibraryDiagnostic {
  fn error(
    code: impl Into<String>,
    message: impl Into<String>,
    path: Option<String>,
  ) -> Self {
    Self {
      severity: LibraryDiagnosticSeverity::Error,
      code: code.into(),
      message: message.into(),
      path,
    }
  }

  fn warning(
    code: impl Into<String>,
    message: impl Into<String>,
    path: Option<String>,
  ) -> Self {
    Self {
      severity: LibraryDiagnosticSeverity::Warning,
      code: code.into(),
      message: message.into(),
      path,
    }
  }
}

pub fn resolve_scope(query: &LibraryQuery) -> Result<ResolvedLibraryScope, LibraryDiagnostic> {
  match query.status {
    LibraryStatus::Installed => {}
    LibraryStatus::Owned => {
      // TODO(steam-owned-library-v1): owned-but-not-installed discovery is
      // deferred until an owner-approved slice chooses Steam Web/API,
      // authenticated local cache, or UI observation as the evidence source.
      return Err(LibraryDiagnostic::error(
        "unsupported_library_status",
        "owned Steam library status is not implemented in v0; use --status installed",
        None,
      ));
    }
    LibraryStatus::All => {
      // TODO(steam-library-all-v1): all-library merging is deferred until
      // installed and owned sources define precedence, duplicate handling, and
      // grounding reporting for shared appids.
      return Err(LibraryDiagnostic::error(
        "unsupported_library_scope",
        "all Steam library status is not implemented in v0; use --status installed",
        None,
      ));
    }
  }

  match query.source {
    LibrarySource::Auto | LibrarySource::Local => Ok(ResolvedLibraryScope {
      status: LibraryStatus::Installed,
      source: "local_appmanifest".to_string(),
      grounding: Grounding::Strong,
    }),
    LibrarySource::Web => {
      // TODO(steam-owned-library-v1): web-backed ownership is deferred until an
      // owner-approved slice defines credentials, profile visibility, and
      // account-library evidence.
      Err(LibraryDiagnostic::error(
        "unsupported_library_source",
        "Steam web library source is not implemented in v0; use --source local",
        None,
      ))
    }
    LibrarySource::Ui => {
      // TODO(steam-ui-library-source-v1): Steam UI observation is deferred
      // because v0 has a stronger local manifest source for installed games and
      // no accepted UI parser contract for owned-library state.
      Err(LibraryDiagnostic::error(
        "unsupported_library_source",
        "Steam UI library source is not implemented in v0; use --source local",
        None,
      ))
    }
  }
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p auv-steam library::tests:: -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit query semantics**

```bash
git add crates/auv-steam/src/library.rs
git commit -m "feat(auv-steam): define library query scope"
```

## Task 3: Add Fixture-Backed Installed App Filtering

**Files:**
- Modify: `crates/auv-steam/src/library.rs`

- [ ] **Step 1: Write tests for normalized name filtering and result shape**

Append these tests inside the existing `#[cfg(test)] mod tests` in `crates/auv-steam/src/library.rs`:

```rust
  fn fake_app(appid: u32, name: &str) -> SteamInstalledApp {
    SteamInstalledApp {
      appid,
      name: name.to_string(),
      install_dir: name.to_string(),
      library_path: "/tmp/Steam".to_string(),
      manifest_path: format!("/tmp/Steam/steamapps/appmanifest_{appid}.acf"),
      install_state: "installed".to_string(),
      source: "local_appmanifest".to_string(),
      grounding: Grounding::Strong,
    }
  }

  #[test]
  fn query_apps_without_name_returns_all_apps() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    };
    let apps = vec![fake_app(2379780, "Balatro"), fake_app(220200, "Kerbal Space Program")];

    let result = query_installed_apps(query, apps, Vec::new()).expect("query should succeed");

    assert_eq!(result.apps.len(), 2);
    assert_eq!(result.resolved_scope.source, "local_appmanifest");
    assert!(result.diagnostics.is_empty());
  }

  #[test]
  fn query_apps_filters_by_case_insensitive_contains_match() {
    let query = LibraryQuery {
      name: Some("BAL".to_string()),
      status: LibraryStatus::Installed,
      source: LibrarySource::Auto,
    };
    let apps = vec![fake_app(2379780, "Balatro"), fake_app(220200, "Kerbal Space Program")];

    let result = query_installed_apps(query, apps, Vec::new()).expect("query should succeed");

    assert_eq!(result.apps.len(), 1);
    assert_eq!(result.apps[0].appid, 2379780);
  }

  #[test]
  fn query_apps_collapses_repeated_query_whitespace() {
    let query = LibraryQuery {
      name: Some("space   program".to_string()),
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    };
    let apps = vec![fake_app(2379780, "Balatro"), fake_app(220200, "Kerbal Space Program")];

    let result = query_installed_apps(query, apps, Vec::new()).expect("query should succeed");

    assert_eq!(result.apps.len(), 1);
    assert_eq!(result.apps[0].name, "Kerbal Space Program");
  }

  #[test]
  fn query_apps_no_match_is_successful_empty_result() {
    let query = LibraryQuery {
      name: Some("missing".to_string()),
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    };
    let apps = vec![fake_app(2379780, "Balatro")];

    let result = query_installed_apps(query, apps, Vec::new()).expect("query should succeed");

    assert!(result.apps.is_empty());
    assert!(result.diagnostics.is_empty());
  }

  #[test]
  fn query_apps_preserves_warning_diagnostics() {
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    };
    let diagnostics = vec![LibraryDiagnostic::warning(
      "manifest_parse_failed",
      "failed to parse appmanifest_1.acf",
      Some("/tmp/Steam/steamapps/appmanifest_1.acf".to_string()),
    )];

    let result = query_installed_apps(query, vec![fake_app(2379780, "Balatro")], diagnostics)
      .expect("query should succeed");

    assert_eq!(result.apps.len(), 1);
    assert_eq!(result.diagnostics[0].code, "manifest_parse_failed");
  }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p auv-steam library::tests::query_apps_filters_by_case_insensitive_contains_match -- --nocapture
```

Expected: FAIL because `query_installed_apps` is not implemented.

- [ ] **Step 3: Implement query filtering**

Add this implementation before the test module:

```rust
pub fn query_installed_apps(
  query: LibraryQuery,
  apps: Vec<SteamInstalledApp>,
  diagnostics: Vec<LibraryDiagnostic>,
) -> Result<LibraryQueryResult, LibraryDiagnostic> {
  let resolved_scope = resolve_scope(&query)?;
  let normalized_name = query.name.as_deref().map(normalize_match_text);
  let apps = apps
    .into_iter()
    .filter(|app| {
      normalized_name
        .as_ref()
        .is_none_or(|needle| normalize_match_text(&app.name).contains(needle))
    })
    .collect();

  Ok(LibraryQueryResult {
    query,
    resolved_scope,
    apps,
    diagnostics,
  })
}

fn normalize_match_text(value: &str) -> String {
  value
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
    .to_ascii_lowercase()
}
```

- [ ] **Step 4: Run focused tests**

Run:

```bash
cargo test -p auv-steam library::tests::query_apps -- --nocapture
```

Expected: PASS for all `query_apps_*` tests.

- [ ] **Step 5: Commit filtering**

```bash
git add crates/auv-steam/src/library.rs
git commit -m "feat(auv-steam): filter installed library apps"
```

## Task 4: Add `steamlocate` Adapter

**Files:**
- Modify: `crates/auv-steam/src/library.rs`
- Modify: `crates/auv-steam/src/app.rs`

- [ ] **Step 1: Add a source trait and fake-source test**

Append this test in `crates/auv-steam/src/library.rs`:

```rust
  #[derive(Default)]
  struct FakeInstalledAppSource {
    apps: Vec<SteamInstalledApp>,
    diagnostics: Vec<LibraryDiagnostic>,
  }

  impl InstalledAppSource for FakeInstalledAppSource {
    fn installed_apps(&self) -> Result<InstalledAppRead, SteamError> {
      Ok(InstalledAppRead {
        apps: self.apps.clone(),
        diagnostics: self.diagnostics.clone(),
      })
    }
  }

  #[test]
  fn store_reads_source_and_applies_query() {
    let source = FakeInstalledAppSource {
      apps: vec![fake_app(2379780, "Balatro"), fake_app(220200, "Kerbal Space Program")],
      diagnostics: Vec::new(),
    };
    let store = SteamLibraryStore::new(source);
    let query = LibraryQuery {
      name: Some("bal".to_string()),
      status: LibraryStatus::Installed,
      source: LibrarySource::Auto,
    };

    let result = store.query(query).expect("query should succeed");

    assert_eq!(result.apps.len(), 1);
    assert_eq!(result.apps[0].appid, 2379780);
  }

  #[test]
  fn store_rejects_unsupported_scope_before_reading_source() {
    let source = FakeInstalledAppSource {
      apps: vec![fake_app(2379780, "Balatro")],
      diagnostics: Vec::new(),
    };
    let store = SteamLibraryStore::new(source);
    let query = LibraryQuery {
      name: None,
      status: LibraryStatus::Owned,
      source: LibrarySource::Auto,
    };

    let diagnostic = store.query(query).expect_err("owned is unsupported");

    assert_eq!(diagnostic.code, "unsupported_library_status");
  }
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cargo test -p auv-steam library::tests::store_reads_source_and_applies_query -- --nocapture
```

Expected: FAIL because `SteamLibraryStore`, `InstalledAppSource`, and `InstalledAppRead` are not implemented.

- [ ] **Step 3: Implement the source boundary**

Add this implementation before the test module:

```rust
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct InstalledAppRead {
  pub apps: Vec<SteamInstalledApp>,
  pub diagnostics: Vec<LibraryDiagnostic>,
}

pub trait InstalledAppSource {
  fn installed_apps(&self) -> Result<InstalledAppRead, SteamError>;
}

pub struct SteamLibraryStore<S> {
  source: S,
}

impl<S> SteamLibraryStore<S>
where
  S: InstalledAppSource,
{
  pub fn new(source: S) -> Self {
    Self { source }
  }

  pub fn query(&self, query: LibraryQuery) -> Result<LibraryQueryResult, LibraryDiagnostic> {
    resolve_scope(&query)?;
    let read = self.source.installed_apps().map_err(|error| {
      LibraryDiagnostic::error("steam_not_found", error.to_string(), None)
    })?;
    query_installed_apps(query, read.apps, read.diagnostics)
  }
}
```

- [ ] **Step 4: Implement `steamlocate` production source**

Add this implementation below `SteamLibraryStore`:

```rust
pub struct SteamlocateSource {
  steam_dir: steamlocate::SteamDir,
}

impl SteamlocateSource {
  pub fn locate() -> Result<Self, SteamError> {
    let steam_dir = steamlocate::SteamDir::locate().map_err(|_| SteamError::NotFound)?;
    Ok(Self { steam_dir })
  }
}

impl InstalledAppSource for SteamlocateSource {
  fn installed_apps(&self) -> Result<InstalledAppRead, SteamError> {
    // NOTICE(steam-library-manifest-parser): Steam library discovery is
    // delegated to `steamlocate`, which already handles platform-specific Steam
    // directory lookup and parses Steam KeyValues/VDF app manifests through
    // `keyvalues-serde`. Keep AUV's layer focused on domain resolution,
    // diagnostics, launch evidence, and verification instead of carrying a
    // local VDF parser.
    let mut apps = Vec::new();
    let mut diagnostics = Vec::new();

    for library in self.steam_dir.libraries().map_err(|_| SteamError::NotFound)? {
      let library = match library {
        Ok(library) => library,
        Err(error) => {
          diagnostics.push(LibraryDiagnostic::warning(
            "library_folder_unreadable",
            format!("failed to read Steam library folder: {error}"),
            None,
          ));
          continue;
        }
      };

      for app in library.apps() {
        let app = match app {
          Ok(app) => app,
          Err(error) => {
            diagnostics.push(LibraryDiagnostic::warning(
              "manifest_parse_failed",
              format!("failed to parse Steam app manifest: {error}"),
              Some(library.path().display().to_string()),
            ));
            continue;
          }
        };
        let manifest_path = library
          .path()
          .join("steamapps")
          .join(format!("appmanifest_{}.acf", app.app_id));
        apps.push(SteamInstalledApp {
          appid: app.app_id,
          name: app.name.unwrap_or_else(|| format!("Steam App {}", app.app_id)),
          install_dir: app.install_dir,
          library_path: library.path().display().to_string(),
          manifest_path: manifest_path.display().to_string(),
          install_state: "installed".to_string(),
          source: "local_appmanifest".to_string(),
          grounding: Grounding::Strong,
        });
      }
    }

    Ok(InstalledAppRead { apps, diagnostics })
  }
}
```

- [ ] **Step 5: Wire the product facade**

Replace `crates/auv-steam/src/app.rs` with:

```rust
use crate::library::{
  LibraryDiagnostic, LibraryQuery, LibraryQueryResult, SteamLibraryStore, SteamError,
  SteamlocateSource,
};

pub struct Steam {
  library: SteamLibraryStore<SteamlocateSource>,
}

impl Steam {
  pub fn locate() -> Result<Self, SteamError> {
    Ok(Self {
      library: SteamLibraryStore::new(SteamlocateSource::locate()?),
    })
  }

  pub fn library_apps(&self, query: LibraryQuery) -> Result<LibraryQueryResult, LibraryDiagnostic> {
    self.library.query(query)
  }
}
```

- [ ] **Step 6: Run focused tests and check adapter compile**

Run:

```bash
cargo test -p auv-steam library::tests::store_ -- --nocapture
cargo check -p auv-steam
```

Expected: PASS. If compile fails on `steamlocate` API names, fix only the `SteamlocateSource` adapter.

- [ ] **Step 7: Commit adapter**

```bash
git add crates/auv-steam/src/library.rs crates/auv-steam/src/app.rs
git commit -m "feat(auv-steam): read installed apps through steamlocate"
```

## Task 5: Implement Output Rendering

**Files:**
- Modify: `crates/auv-steam/src/output.rs`

- [ ] **Step 1: Write output tests**

Replace `crates/auv-steam/src/output.rs` tests with:

```rust
#[cfg(test)]
mod tests {
  use super::*;
  use crate::library::{
    Grounding, LibraryQuery, LibraryQueryResult, LibrarySource, LibraryStatus,
    ResolvedLibraryScope, SteamInstalledApp,
  };

  fn result() -> LibraryQueryResult {
    LibraryQueryResult {
      query: LibraryQuery {
        name: Some("bal".to_string()),
        status: LibraryStatus::Installed,
        source: LibrarySource::Auto,
      },
      resolved_scope: ResolvedLibraryScope {
        status: LibraryStatus::Installed,
        source: "local_appmanifest".to_string(),
        grounding: Grounding::Strong,
      },
      apps: vec![SteamInstalledApp {
        appid: 2379780,
        name: "Balatro".to_string(),
        install_dir: "Balatro".to_string(),
        library_path: "/tmp/Steam".to_string(),
        manifest_path: "/tmp/Steam/steamapps/appmanifest_2379780.acf".to_string(),
        install_state: "installed".to_string(),
        source: "local_appmanifest".to_string(),
        grounding: Grounding::Strong,
      }],
      diagnostics: Vec::new(),
    }
  }

  #[test]
  fn json_output_uses_stable_command_id() {
    let result = result();

    let value = serde_json::to_value(build_library_ls_json_output(&result)).expect("json");

    assert_eq!(value["command"], "library.ls");
    assert_eq!(value["query"]["name"], "bal");
    assert_eq!(value["resolved_scope"]["source"], "local_appmanifest");
    assert_eq!(value["apps"][0]["appid"], 2379780);
  }

  #[test]
  fn summary_lists_appid_name_and_grounding() {
    assert_eq!(
      render_library_summary(&result()),
      "APPID    NAME     INSTALL DIR  SOURCE             GROUNDING\n2379780  Balatro  Balatro      local_appmanifest  strong"
    );
  }
}
```

- [ ] **Step 2: Run tests to verify summary fails**

Run:

```bash
cargo test -p auv-steam output::tests::summary_lists_appid_name_and_grounding -- --nocapture
```

Expected: FAIL because current summary only prints a count.

- [ ] **Step 3: Implement rendering**

Replace `render_library_summary` in `crates/auv-steam/src/output.rs` with:

```rust
pub fn render_library_summary(result: &LibraryQueryResult) -> String {
  let mut rows = Vec::new();
  rows.push(format!(
    "{:<8}  {:<7}  {:<11}  {:<17}  {}",
    "APPID", "NAME", "INSTALL DIR", "SOURCE", "GROUNDING"
  ));
  for app in &result.apps {
    rows.push(format!(
      "{:<8}  {:<7}  {:<11}  {:<17}  {}",
      app.appid,
      app.name,
      app.install_dir,
      app.source,
      grounding_label(app.grounding)
    ));
  }
  if result.apps.is_empty() {
    rows.push("(no matching installed Steam apps)".to_string());
  }
  rows.join("\n")
}

fn grounding_label(grounding: crate::library::Grounding) -> &'static str {
  match grounding {
    crate::library::Grounding::Strong => "strong",
  }
}
```

- [ ] **Step 4: Run output tests**

Run:

```bash
cargo test -p auv-steam output::tests:: -- --nocapture
```

Expected: PASS.

- [ ] **Step 5: Commit output rendering**

```bash
git add crates/auv-steam/src/output.rs
git commit -m "feat(auv-steam): render library output"
```

## Task 6: Implement CLI Parsing And Dispatch

**Files:**
- Modify: `crates/auv-steam/src/cli.rs`
- Test: `crates/auv-steam/src/cli.rs`

- [ ] **Step 1: Write CLI parse tests**

Replace `crates/auv-steam/src/cli.rs` with parser skeleton plus tests:

```rust
use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::library::{LibraryQuery, LibrarySource, LibraryStatus};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
  #[default]
  Summary,
  Json,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OutputMode {
  Summary,
  Json,
  JsonFile(PathBuf),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LibraryLsCommand {
  pub query: LibraryQuery,
  pub output: OutputMode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Command {
  LibraryLs(LibraryLsCommand),
}

#[derive(Clone, Debug, Parser)]
#[command(name = "auv-steam")]
struct CliArgs {
  #[command(subcommand)]
  command: CliCommand,
}

#[derive(Clone, Debug, Subcommand)]
enum CliCommand {
  Library(LibraryArgs),
}

#[derive(Clone, Debug, Args)]
struct LibraryArgs {
  #[command(subcommand)]
  command: LibrarySubcommand,
}

#[derive(Clone, Debug, Subcommand)]
enum LibrarySubcommand {
  Ls(LibraryLsArgs),
}

#[derive(Clone, Debug, Args)]
struct LibraryLsArgs {
  #[arg(long)]
  name: Option<String>,
  #[arg(long, value_enum, default_value_t = LibraryStatus::Installed)]
  status: LibraryStatus,
  #[arg(long, value_enum, default_value_t = LibrarySource::Auto)]
  source: LibrarySource,
  #[arg(long, value_enum, default_value_t)]
  format: OutputFormat,
  #[arg(long = "json-out", value_name = "PATH")]
  json_out: Option<PathBuf>,
}

fn command_from_args(args: CliArgs) -> Command {
  match args.command {
    CliCommand::Library(args) => match args.command {
      LibrarySubcommand::Ls(args) => Command::LibraryLs(parse_library_ls(args)),
    },
  }
}

fn parse_library_ls(args: LibraryLsArgs) -> LibraryLsCommand {
  let output = match args.json_out {
    Some(path) => OutputMode::JsonFile(path),
    None if args.format == OutputFormat::Json => OutputMode::Json,
    None => OutputMode::Summary,
  };
  LibraryLsCommand {
    query: LibraryQuery {
      name: args.name,
      status: args.status,
      source: args.source,
    },
    output,
  }
}

pub fn run() -> ExitCode {
  ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
  use super::*;
  use clap::Parser;

  fn parse(argv: &[&str]) -> LibraryLsCommand {
    let parsed = CliArgs::try_parse_from(argv).expect("argv should parse");
    match command_from_args(parsed) {
      Command::LibraryLs(command) => command,
    }
  }

  #[test]
  fn library_ls_defaults_to_installed_auto_summary() {
    let command = parse(&["auv-steam", "library", "ls"]);

    assert_eq!(command.query.status, LibraryStatus::Installed);
    assert_eq!(command.query.source, LibrarySource::Auto);
    assert_eq!(command.output, OutputMode::Summary);
  }

  #[test]
  fn library_ls_maps_filters() {
    let command = parse(&[
      "auv-steam",
      "library",
      "ls",
      "--name",
      "bal",
      "--status",
      "installed",
      "--source",
      "local",
      "--format",
      "json",
    ]);

    assert_eq!(command.query.name.as_deref(), Some("bal"));
    assert_eq!(command.query.status, LibraryStatus::Installed);
    assert_eq!(command.query.source, LibrarySource::Local);
    assert_eq!(command.output, OutputMode::Json);
  }

  #[test]
  fn json_out_takes_precedence_over_format_json() {
    let command = parse(&[
      "auv-steam",
      "library",
      "ls",
      "--format",
      "json",
      "--json-out",
      "/tmp/library.json",
    ]);

    assert_eq!(command.output, OutputMode::JsonFile(PathBuf::from("/tmp/library.json")));
  }
}
```

- [ ] **Step 2: Run parse tests**

Run:

```bash
cargo test -p auv-steam cli::tests:: -- --nocapture
```

Expected: PASS after adding clap `ValueEnum` derives in Task 6 step 3 if missing.

- [ ] **Step 3: Add `ValueEnum` derives for query enums**

Modify imports and derives in `crates/auv-steam/src/library.rs`:

```rust
use clap::ValueEnum;
```

Change `LibraryStatus` derive:

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
```

Change `LibrarySource` derive:

```rust
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize, ValueEnum)]
```

- [ ] **Step 4: Implement CLI dispatch**

Replace `run()` and add helpers in `crates/auv-steam/src/cli.rs`:

```rust
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
    Command::LibraryLs(command) => run_library_ls(command),
  }
}

fn run_library_ls(command: LibraryLsCommand) -> ExitCode {
  let steam = match crate::Steam::locate() {
    Ok(steam) => steam,
    Err(error) => {
      eprintln!("{error}");
      return ExitCode::FAILURE;
    }
  };
  let result = match steam.library_apps(command.query) {
    Ok(result) => result,
    Err(diagnostic) => {
      eprintln!("{}: {}", diagnostic.code, diagnostic.message);
      return ExitCode::FAILURE;
    }
  };

  match emit_output(&command.output, &result) {
    Ok(()) => ExitCode::SUCCESS,
    Err(message) => {
      eprintln!("{message}");
      ExitCode::FAILURE
    }
  }
}

fn emit_output(
  mode: &OutputMode,
  result: &crate::library::LibraryQueryResult,
) -> Result<(), String> {
  match mode {
    OutputMode::Summary => {
      println!("{}", crate::output::render_library_summary(result));
      Ok(())
    }
    OutputMode::Json => {
      let json = serde_json::to_string_pretty(&crate::output::build_library_ls_json_output(result))
        .map_err(|error| format!("failed to encode library JSON: {error}"))?;
      println!("{json}");
      Ok(())
    }
    OutputMode::JsonFile(path) => {
      let json = serde_json::to_string_pretty(&crate::output::build_library_ls_json_output(result))
        .map_err(|error| format!("failed to encode library JSON: {error}"))?;
      std::fs::write(path, format!("{json}\n"))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
      println!("json: {}", path.display());
      Ok(())
    }
  }
}
```

- [ ] **Step 5: Run CLI tests and check**

Run:

```bash
cargo test -p auv-steam cli::tests:: -- --nocapture
cargo check -p auv-steam
```

Expected: PASS.

- [ ] **Step 6: Commit CLI**

```bash
git add crates/auv-steam/src/cli.rs crates/auv-steam/src/library.rs
git commit -m "feat(auv-steam): add library ls cli"
```

## Task 7: Add Integration Fixture Tests

**Files:**
- Create: `crates/auv-steam/tests/fixtures.rs`

- [ ] **Step 1: Add fixture-backed integration tests**

Create `crates/auv-steam/tests/fixtures.rs`:

```rust
use auv_steam::library::{
  Grounding, InstalledAppRead, InstalledAppSource, LibraryDiagnostic, LibraryQuery, LibrarySource,
  LibraryStatus, SteamError, SteamInstalledApp, SteamLibraryStore,
};

#[derive(Clone)]
struct FixtureSource {
  read: InstalledAppRead,
}

impl InstalledAppSource for FixtureSource {
  fn installed_apps(&self) -> Result<InstalledAppRead, SteamError> {
    Ok(self.read.clone())
  }
}

fn app(appid: u32, name: &str) -> SteamInstalledApp {
  SteamInstalledApp {
    appid,
    name: name.to_string(),
    install_dir: name.to_string(),
    library_path: "/fixture/Steam".to_string(),
    manifest_path: format!("/fixture/Steam/steamapps/appmanifest_{appid}.acf"),
    install_state: "installed".to_string(),
    source: "local_appmanifest".to_string(),
    grounding: Grounding::Strong,
  }
}

#[test]
fn fixture_source_lists_installed_apps_without_real_steam() {
  let store = SteamLibraryStore::new(FixtureSource {
    read: InstalledAppRead {
      apps: vec![app(2379780, "Balatro"), app(220200, "Kerbal Space Program")],
      diagnostics: Vec::new(),
    },
  });

  let result = store
    .query(LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Auto,
    })
    .expect("installed auto should query fixture");

  assert_eq!(result.apps.len(), 2);
  assert_eq!(result.resolved_scope.source, "local_appmanifest");
}

#[test]
fn fixture_source_preserves_manifest_warning() {
  let store = SteamLibraryStore::new(FixtureSource {
    read: InstalledAppRead {
      apps: vec![app(2379780, "Balatro")],
      diagnostics: vec![LibraryDiagnostic {
        severity: auv_steam::library::LibraryDiagnosticSeverity::Warning,
        code: "manifest_parse_failed".to_string(),
        message: "failed to parse fixture manifest".to_string(),
        path: Some("/fixture/Steam/steamapps/appmanifest_1.acf".to_string()),
      }],
    },
  });

  let result = store
    .query(LibraryQuery {
      name: None,
      status: LibraryStatus::Installed,
      source: LibrarySource::Local,
    })
    .expect("warnings should not fail query");

  assert_eq!(result.apps.len(), 1);
  assert_eq!(result.diagnostics[0].code, "manifest_parse_failed");
}
```

- [ ] **Step 2: Run integration tests**

Run:

```bash
cargo test -p auv-steam --test fixtures -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Commit integration tests**

```bash
git add crates/auv-steam/tests/fixtures.rs
git commit -m "test(auv-steam): cover fixture-backed library queries"
```

## Task 8: Final Verification

**Files:**
- No new files unless fixes are required.

- [ ] **Step 1: Format**

Run:

```bash
cargo fmt
```

Expected: command exits 0.

- [ ] **Step 2: Check formatting**

Run:

```bash
cargo fmt --check
```

Expected: PASS.

- [ ] **Step 3: Run crate tests**

Run:

```bash
cargo test -p auv-steam
```

Expected: PASS.

- [ ] **Step 4: Run workspace tests**

Run:

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 5: Check whitespace**

Run:

```bash
git diff --check
```

Expected: no output.

- [ ] **Step 6: Smoke CLI help**

Run:

```bash
cargo run -p auv-steam -- library ls --help
```

Expected output includes:

```text
--status <STATUS>
--source <SOURCE>
--format <FORMAT>
--json-out <PATH>
```

- [ ] **Step 7: Smoke local query when Steam is installed**

Run this only on a machine with Steam installed:

```bash
cargo run -p auv-steam -- library ls --format json
```

Expected on the current macOS development machine: JSON contains installed apps such as `Balatro` if the local Steam manifests remain present. If Steam is absent, the command should exit non-zero with a clear `Steam could not be located` error.

- [ ] **Step 8: Commit verification fixes if needed**

If formatting or small compile fixes were required, commit them:

```bash
git add Cargo.toml Cargo.lock crates/auv-steam
git commit -m "chore(auv-steam): finalize library query"
```

Skip this commit if no files changed after Task 7.

## Self-Review Checklist

- Spec coverage:
  - local installed-library query: Task 4 and Task 6
  - `steamlocate` dependency and `keyvalues-serde` notice: Task 4
  - `--format summary|json` and `--json-out`: Task 6
  - unsupported owned/web/ui/all: Task 2 and Task 6
  - normalized name contains matching: Task 3
  - output contract: Task 5
  - tests without real Steam: Task 7
- No `--json` flag is introduced.
- No Steam store, download, install, launch, login, Web API, or UI automation is implemented.
- Unsupported sources fail explicitly and do not silently degrade.
