# Steam Library Automation Design

Date: 2026-06-09

Status: proposed API/CLI design. This records the agreed direction for a
future implementation slice; it does not approve implementing Steam store,
download, ownership, or UI automation surfaces in the same slice.

## Purpose

`auv-steam` should expose a narrow, grounded Steam library query surface for
agent-callable workflows. v0 is not a replacement for the Steam client or
SteamCMD. It is an AUV product CLI that can inspect the local Steam installed
library, return structured evidence, and leave clear extension points for
later owned-library, install, launch, and UI-backed slices.

The first slice is intentionally limited to installed games visible through
local Steam manifests. This keeps the result strongly grounded in files Steam
already wrote on the local macOS host instead of starting with brittle UI
automation or account-level assumptions.

## Existing Evidence

On macOS, Steam stores local client data under:

```text
~/Library/Application Support/Steam
```

The observed local machine has:

```text
/Applications/Steam.app
~/Library/Application Support/Steam/steamapps/libraryfolders.vdf
~/Library/Application Support/Steam/config/libraryfolders.vdf
~/Library/Application Support/Steam/steamapps/appmanifest_220200.acf
~/Library/Application Support/Steam/steamapps/appmanifest_2379780.acf
```

The app manifests contain the fields needed for installed-library v0, including
`appid`, `name`, `installdir`, `StateFlags`, `SizeOnDisk`, and update/download
metadata. Example observed values include:

```text
appid=2379780
name=Balatro
installdir=Balatro
StateFlags=4
```

Rust dependency research found `steamlocate` as the best fit for v0:

- crate: `steamlocate = "2.1.0"`
- purpose: locating Steam installation directories and game install manifests
- platforms: Windows, macOS, Linux
- license: MIT
- repository: `https://github.com/WilliamVenner/steamlocate-rs`
- maintenance signal checked on 2026-06-09: recent release/update in 2026,
  active enough for this narrow slice, and materially more domain-specific
  than a generic VDF parser.

`steamlocate` uses `keyvalues-serde` for Steam KeyValues/VDF parsing. AUV
should depend on `steamlocate` first and keep any direct VDF parser dependency
out of v0 unless a concrete field or diagnostic boundary requires it.

## Scope

v0 implements a local installed-library query:

```text
auv-steam library ls
```

Supported v0 combinations:

```text
status=installed + source=local  -> implemented through steamlocate
status=installed + source=auto   -> implemented as local
```

Deferred combinations:

```text
status=owned + source=web        -> deferred
status=owned + source=ui         -> deferred
status=all                       -> deferred until an owned-library source exists
```

The command may include name filtering. Name filtering is a query over the
installed-app result set, not an action-selection contract. Returning multiple
matches is valid for `library ls`; later launch/install commands must reject
ambiguous matches instead of silently choosing one.

## Non-Goals

The v0 slice does not implement:

- Steam store search, prices, tags, reviews, wishlist, purchase, or community
  pages.
- owned-but-not-installed library discovery.
- installing or downloading games.
- launching games.
- Steam login, 2FA, account, friend, chat, or family sharing flows.
- Steam UI scanning as the primary source of truth.
- Steam Web API integration.
- SteamCMD integration.

These are separate owner-approved slices because they introduce new evidence
sources, credentials, UI contracts, or verification requirements.

## Crate Shape

Create a product crate:

```text
crates/auv-steam/
  Cargo.toml
  src/
    lib.rs
    app.rs
    library.rs
    output.rs
    cli.rs
    bin/
      auv-steam.rs
```

`lib.rs` should declare modules and re-export only the public product API types
needed by callers. It should not become a catch-all implementation file.

`app.rs` owns the product-level facade:

```rust
pub struct Steam {
  library: SteamLibraryStore,
}
```

The facade should expose domain operations, not `steamlocate` internals:

```rust
impl Steam {
  pub fn locate() -> Result<Self, SteamError>;
  pub fn library_apps(&self, query: LibraryQuery) -> LibraryQueryResult;
}
```

`library.rs` owns the installed-library resolver. It wraps `steamlocate` and
maps third-party records into AUV-owned domain records such as
`SteamInstalledApp`, `SteamLibraryFolder`, `LibraryQuery`, and
`LibraryDiagnostic`.

Implementation should include this decision marker near the `steamlocate`
adapter:

```rust
// NOTICE(steam-library-manifest-parser): Steam library discovery is delegated
// to `steamlocate`, which already handles platform-specific Steam directory
// lookup and parses Steam KeyValues/VDF app manifests through `keyvalues-serde`.
// Keep AUV's layer focused on domain resolution, diagnostics, launch evidence,
// and verification instead of carrying a local VDF parser.
```

`output.rs` owns stable CLI JSON shapes and human summary rendering.

`cli.rs` owns argument parsing, output mode selection, exit code mapping, and
presentation. It should call the product API rather than reading manifests or
calling `steamlocate` directly.

## CLI Surface

The v0 command is:

```text
auv-steam library ls
  --name <query>
  --status installed|owned|all
  --source local|web|ui|auto
  --format summary|json
  --json-out <path>
```

Defaults:

```text
--status installed
--source auto
--format summary
```

There is no `--json` flag in v0. Steam follows the newer
`--format summary|json` style used by `auv-media-macos` while retaining
`--json-out` for agent artifact workflows.

Output mode precedence:

```text
--json-out <path> > --format json > --format summary
```

When `--json-out` succeeds, stdout should print a terse confirmation:

```text
json: <path>
```

## Query Semantics

`--name <query>` uses normalized contains matching:

- trim leading/trailing whitespace
- compare case-insensitively for ASCII
- collapse repeated whitespace for comparison
- do not fuzzy-match in v0
- return all matching apps

No match is a successful empty query result, not a runtime failure.

`--status installed` means local Steam manifests under discovered Steam library
folders. It does not mean the user's complete account-owned library.

`--status owned` means account-owned apps, including apps that may not be
installed locally. This is deferred because it needs an explicit source such as
Steam Web API, authenticated account data, local client cache research, or
Steam UI observation.

`--status all` is deferred until both installed and owned sources have an
accepted merge contract.

`--source auto` for `--status installed` resolves to `local_appmanifest` in v0.
It must not silently fall back to UI or web sources.

## Output Contract

`--format json` and `--json-out` produce a stable JSON object:

```json
{
  "command": "library.ls",
  "query": {
    "name": "bal",
    "status": "installed",
    "source": "auto"
  },
  "resolved_scope": {
    "status": "installed",
    "source": "local_appmanifest",
    "grounding": "strong"
  },
  "apps": [
    {
      "appid": 2379780,
      "name": "Balatro",
      "install_dir": "Balatro",
      "library_path": "/Users/neko/Library/Application Support/Steam",
      "manifest_path": "/Users/neko/Library/Application Support/Steam/steamapps/appmanifest_2379780.acf",
      "install_state": "installed",
      "source": "local_appmanifest",
      "grounding": "strong"
    }
  ],
  "diagnostics": []
}
```

Fields:

- `command`: stable command id, `library.ls`.
- `query`: user-requested query shape after defaults are applied.
- `resolved_scope`: actual source and grounding used by the resolver.
- `apps`: matching installed apps.
- `diagnostics`: structured warnings and unsupported-scope reports.

`appid` should be represented as a number in JSON when it fits the Rust type
used by `steamlocate`; the CLI may render it as text in summary output.

`install_state` is v0-normalized to `installed` for apps returned through local
manifests. Raw `state_flags` may be added later if an owner-approved slice
defines the stable interpretation and output name.

## Diagnostics And Errors

Use explicit diagnostic codes. Suggested v0 codes:

```text
steam_not_found
library_folder_unreadable
manifest_parse_failed
unsupported_library_status
unsupported_library_source
unsupported_library_scope
```

Exit code rules:

- Steam cannot be located: non-zero.
- `--status owned`: non-zero with `unsupported_library_status`.
- `--status all`: non-zero with `unsupported_library_scope`.
- `--source web` or `--source ui`: non-zero with `unsupported_library_source`.
- One library folder cannot be read: record diagnostic and continue if at
  least one folder can still be queried.
- One manifest cannot be parsed: record diagnostic and continue.
- `--name` returns no matches: exit zero with `apps: []`.

Unsupported statuses/sources should not silently degrade to installed/local.
This protects agents from treating a local-manifest answer as complete account
ownership data.

## Deferrals

Implementation must leave explicit markers at unsupported branches:

```rust
// TODO(steam-owned-library-v1): owned-but-not-installed discovery is deferred
// until an owner-approved slice chooses Steam Web/API, authenticated local
// cache, or UI observation as the evidence source.
```

```rust
// TODO(steam-library-all-v1): all-library merging is deferred until installed
// and owned sources define precedence, duplicate handling, and grounding
// reporting for shared appids.
```

```rust
// TODO(steam-ui-library-source-v1): Steam UI observation is deferred because
// v0 has a stronger local manifest source for installed games and no accepted
// UI parser contract for owned-library state.
```

Install and launch commands should also remain deferred in v0:

```rust
// TODO(steam-launch-v1): launching by resolved appid is deferred until the
// command defines launch method, verification evidence, and ambiguity handling.
```

```rust
// TODO(steam-install-v1): install requests are deferred until the appid source,
// ownership evidence, install trigger, and download verification contract are
// defined.
```

## Testing

Focused tests should cover:

- parsing local fixture `libraryfolders.vdf` and two `appmanifest_*.acf` files
  through the `steamlocate` adapter or a fixture-backed adapter seam;
- `--status installed --source local`;
- `--status installed --source auto` resolving to local;
- unsupported `owned`, `all`, `web`, and `ui` requests;
- name filtering with normalized contains matching;
- no-match returning success with empty `apps`;
- malformed manifest recording a diagnostic without dropping other apps;
- CLI output precedence where `--json-out` wins over `--format json`;
- human summary renders appid, name, install dir, and grounding without
  claiming complete account ownership.

Tests should not require Steam to be installed on the CI machine. Use fixtures
or a small adapter trait at the filesystem/third-party boundary; do not add a
mock-heavy dependency bag for internal helpers.

## Follow-Up Candidates

1. `library launch <name-or-appid>` using resolved installed appids and the
   official Steam launch mechanism, with process/window verification evidence.
2. `library install <appid>` using `steam://install/<appid>` after an ownership
   source exists.
3. Owned-library resolver through Steam Web API or another accepted source.
4. Steam UI parser for cases where local metadata and Web/API sources cannot
   answer the user-facing question.
5. Merge installed and owned sources into `--status all` with explicit source
   precedence and per-record grounding.
