# Apple Music Windows Command Reference

Date: 2026-06-26

Status: implemented.

## Summary

`auv-apple-music` is the Windows Apple Music command crate. It targets the
Microsoft Store/MSIX Apple Music app and uses `auv-driver-windows` for window
resolution, foreground input, capture, OCR, and UI Automation (UIA) inspection.

Apple Music Store installs should be launched by AppUserModelID, not by a
stable executable path. `open-window` now discovers registered Apple Music
Start app IDs with `Get-StartApps`, activates the first discovered ID via
`shell:AppsFolder\<AppUserModelID>`, and keeps known Apple Music AppUserModelID
shapes as offline fallbacks. The command records the chosen or failed launch
target in its JSON `steps` output.

## Command Surface

Run commands from the repository root with:

```text
cargo run -p auv-apple-music -- <command> [options]
```

All commands support human-readable output by default. Commands with `--json`
emit structured output for inspection or scripting.

| Command | Purpose |
| --- | --- |
| `open-window` | Ensure Apple Music has a visible top-level window. |
| `playback` | Read playback state, track title, and artist. |
| `search <query>` | Submit and verify a search query. |
| `search <query> --select <ANCHOR>` | Submit a search, select one unique UIA result, and verify navigation. |
| `transport play-pause` | Send the system play/pause media key. |
| `transport next` | Send the system next-track media key. |
| `transport previous` | Send the system previous-track media key. |

## `open-window`

```text
cargo run -p auv-apple-music -- open-window [--settle-ms <ms>] [--json]
```

Behavior:

1. Resolve an already visible Apple Music window.
2. If missing, discover launch targets through `Get-StartApps`.
3. Launch through `explorer.exe shell:AppsFolder\<AppUserModelID>`.
4. Poll until the Apple Music window appears or `--settle-ms` expires.

Options:

| Option | Default | Notes |
| --- | --- | --- |
| `--settle-ms <ms>` | `8000` | Window appearance timeout after launch. Use `0` to skip polling. |
| `--json` | off | Includes `resolve`, `discover-launch-target`, `launch`, and `wait` steps. |

The window resolver first matches process name `AppleMusic.exe`, then falls
back to a title containing `Apple Music`.

## `playback`

```text
cargo run -p auv-apple-music -- playback [--artifact-dir <dir>] [--json]
```

Behavior:

- Resolves the visible Apple Music window.
- Reads play/pause state from UIA transport button names.
- Reads title and artist from UIA when possible.
- Falls back to OCR on the bottom bar of a window capture when UIA metadata is
  not usable.

Options:

| Option | Default | Notes |
| --- | --- | --- |
| `--artifact-dir <dir>` | none | Saves the window capture PNG used for OCR/debugging. |
| `--json` | off | Emits `state`, `track_title`, `artist`, `metadata_source`, diagnostics, and optional artifact path. |

## `search`

```text
cargo run -p auv-apple-music -- search [options] <query>
```

Behavior:

1. Resolve and restore the Apple Music window.
2. Locate the UIA search edit.
3. Focus the search edit and submit the query through typed Windows input.
4. Verify the query through UIA, with OCR as a fallback.

Options:

| Option | Default | Notes |
| --- | --- | --- |
| `--settle-ms <ms>` | `300` | Delay after each input action. |
| `--verification-timeout-ms <ms>` | `5000` | Query verification timeout. |
| `--artifact-dir <dir>` | none | Saves final verification capture PNG. |
| `--json` | off | Emits window preparation, input action results, verification, and diagnostics. |

Search reports delivered input separately from semantic verification. A query
is not considered verified unless UIA or fallback OCR observes the normalized
query after submission.

## Search Result Selection

```text
cargo run -p auv-apple-music -- search <query> --select <ANCHOR> [options]
```

Behavior:

1. Runs `search <query>` and requires that search verification succeeds.
2. Finds one UIA result item whose accessible name contains `ANCHOR`.
3. Selects the result through typed Windows accessibility/input delivery.
4. Verifies that the view changed away from the original search result grid.

Extra option:

| Option | Default | Notes |
| --- | --- | --- |
| `--selection-timeout-ms <ms>` | `5000` | Timeout for finding a matching UIA result item. |

`ANCHOR` must uniquely identify one live result. Ambiguous anchors are rejected
rather than choosing an arbitrary row.

TODO(apple-music-search-candidate-ref): durable result-list artifacts and
stable `CandidateRef` consumption remain deferred; this command currently
selects one live UIA result by a unique accessible-name anchor.

## `transport`

```text
cargo run -p auv-apple-music -- transport [--settle-ms <ms>] [--json] <action>
```

Actions:

| Action | Delivered key |
| --- | --- |
| `play-pause` | `media_play_pause` |
| `next` | `media_next` |
| `previous` | `media_prev` |

Behavior:

- By default the library verifies that an Apple Music window exists before
  sending a media key, so the command does not accidentally control another
  active media app.
- The actual transport action is delivered as a system-wide Win32 media key via
  `auv-driver-windows`.
- Apple Music does not need to be foreground after the existence check; Windows
  routes media keys to the current media session owner.

Options:

| Option | Default | Notes |
| --- | --- | --- |
| `--settle-ms <ms>` | `150` | Delay after the media key press. |
| `--json` | off | Emits action, delivered key, and diagnostics. |

## Known Boundaries

- The command crate is Windows-only for live Apple Music control.
- Window launch is best-effort: Store/AppX registration is read from
  `Get-StartApps`, then fallback AppUserModelIDs are tried.
- Search result selection is live-UIA anchored, not a durable candidate-list
  replay contract.
- Transport commands use media keys, not coordinate clicks, to avoid
  multi-monitor coordinate drift.

## Validation

Focused validation after the AppUserModelID launch update:

```text
cargo fmt --check
cargo check -p auv-apple-music
cargo test -p auv-apple-music
git diff --check
```

Full workspace `cargo test` was also attempted after dependency download. It
failed in unrelated Minecraft remote-config tests:

- `minecraft::tests::training_job_launch_with_environment_uses_explicit_remote_config`
- `minecraft::tests::training_result_collection_with_environment_uses_explicit_remote_config`
