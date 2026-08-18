---
inclusion: fileMatch
fileMatchPattern: ['**/*.rs', '**/Cargo.toml', 'Cargo.toml', 'rustfmt.toml']
---

# Rust: RustRover MCP + Cargo (AUV)

When this rule applies, treat RustRover MCP as the **required** structure layer and terminal Cargo as the **required** validation authority.

## Before editing Rust

1. Confirm RustRover MCP (`user-rustrover`) is available; read tool schemas before calling.
2. Locate symbols with MCP first:
   - `search_symbol` → `get_symbol_info`
   - `analyze_calls` when changing public APIs, traits, or call sites
   - `search_file` for crate/test/fixture paths
3. Do **not** start with plain `grep`/`rg` for structural Rust work (types, traits, methods, callers).
4. Plain text search is still OK for: docs, fixtures, JSON artifacts, logs, comments, literal CLI strings — after semantic lookup fails or target is non-code.

## While editing

- Prefer `rename_refactoring` for symbol renames across the workspace.
- Optional: `reformat_file` on touched files (helper only; not proof of correctness).
- Optional: `get_file_problems` / `build_project` for fast IDE feedback.

## After editing Rust (required)

Run from repo root before claiming done:

```sh
cargo fmt --check
cargo check
cargo test   # or a narrow filter when the slice is localized
git diff --check
```

If CLI/MCP/command catalog behavior changed, also run:

```sh
cargo run --quiet -- list-commands
```

## MCP unavailable

If RustRover MCP is down or not indexed:

1. Say so explicitly in the summary.
2. Fall back to `read` + minimal `rg` only for the assigned slice.
3. Still run the full Cargo validation block above — no skip.

## Non-goals

- RustRover build/diagnostics do **not** replace `cargo check` / `cargo test`.
- Do not bypass Cargo validation because IDE feedback looked clean.
